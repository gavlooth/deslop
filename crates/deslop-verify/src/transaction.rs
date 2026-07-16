use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use deslop_core::revision_guard;
use deslop_protocol::{SharedWorkOrder, WorkOrderSubject, WorkOrderVerification};
use deslop_recipes::{ExpectedGraphDelta, TransformationCandidate};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

use crate::{
    AtomicCommitReceipt, AtomicFailureInjection, EvidenceDecision, EvidenceKind, EvidenceOutcome,
    PreChangeCharacterization, RecipeDemotionRecord, RecipeDemotionStore, UndoState,
    VerificationCheck, VerificationCheckKind, VerificationDisposition, VerificationEvidence,
    VerifierExecutionPolicy, VerifierFailure, VerifierFailureKind, VerifierPlan,
    VerifierPlanStatus, VerifierStage, commit_atomic_sources_with_injection, evaluate_evidence,
    restore_committed_transaction,
};

pub const VERIFICATION_TRANSACTION_SCHEMA: &str = "deslop.verification-transaction/1";

#[derive(Debug, Clone)]
pub struct VerificationTransactionOptions {
    pub root: PathBuf,
    pub undo_root: PathBuf,
    pub demotion_journal: PathBuf,
    pub patch_authored_sequence: u64,
    pub authorize_write: bool,
    pub atomic_failure: Option<AtomicFailureInjection>,
}

impl VerificationTransactionOptions {
    pub fn controlled(root: PathBuf, patch_authored_sequence: u64) -> Self {
        Self {
            root,
            undo_root: PathBuf::from(".deslop/undo"),
            demotion_journal: PathBuf::from(".deslop/negative-memory/recipes.jsonl"),
            patch_authored_sequence,
            authorize_write: false,
            atomic_failure: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GraphReanalysisPhase {
    Patched,
    Formatted,
    Live,
    Rollback,
}

pub trait VerificationRuntime {
    fn format(
        &mut self,
        staged_root: &Path,
        order: &SharedWorkOrder,
        check: &VerificationCheck,
        policy: &VerifierExecutionPolicy,
    ) -> std::result::Result<VerificationEvidence, VerifierFailure>;

    fn reanalyze_graph_delta(
        &mut self,
        root: &Path,
        order: &SharedWorkOrder,
        phase: GraphReanalysisPhase,
        policy: &VerifierExecutionPolicy,
    ) -> std::result::Result<ExpectedGraphDelta, VerifierFailure>;

    fn run_check(
        &mut self,
        staged_root: &Path,
        order: &SharedWorkOrder,
        check: &VerificationCheck,
        policy: &VerifierExecutionPolicy,
    ) -> std::result::Result<VerificationEvidence, VerifierFailure>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationTransactionStatus {
    Applied,
    VerifiedReviewOnly,
    Rejected,
    RolledBack,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationTransactionReport {
    pub schema: String,
    pub work_order: String,
    pub verifier_plan: String,
    pub status: VerificationTransactionStatus,
    pub evidence: Option<EvidenceDecision>,
    pub failures: Vec<VerifierFailure>,
    pub written: Vec<PathBuf>,
    pub undo_manifest: Option<PathBuf>,
    pub demotion: Option<String>,
    pub rollback_verified: bool,
    pub residual_uncertainty: Vec<String>,
}

pub fn execute_verification_transaction(
    order: &SharedWorkOrder,
    plan: &VerifierPlan,
    characterization: Option<&PreChangeCharacterization>,
    options: &VerificationTransactionOptions,
    runtime: &mut impl VerificationRuntime,
) -> Result<VerificationTransactionReport> {
    order.validate()?;
    plan.validate()?;
    validate_options(options)?;
    if plan.work_order() != order.id().as_str()
        || plan.status() != VerifierPlanStatus::Ready
        || order.provenance().project_snapshot.as_deref() != Some(plan.snapshot())
    {
        bail!("verification transaction requires a current ready plan for the exact work order");
    }
    let candidate = transformation_candidate(order)?;
    let root = options.root.canonicalize().with_context(|| {
        format!(
            "failed to resolve verification root {}",
            options.root.display()
        )
    })?;
    let recipe_id = candidate.recipe().id().as_str();
    let store = RecipeDemotionStore::load(&root, &options.demotion_journal)?;
    if store.is_demoted(recipe_id) {
        return Ok(rejected_report(
            order,
            plan,
            failure(
                VerifierStage::Demotion,
                VerifierFailureKind::Counterexample,
                None,
                "recipe is demoted by an unresolved counterexample",
                false,
            ),
        ));
    }

    let expected_sources = read_exact_sources(&root, candidate)?;
    let replacements = build_replacements(candidate, &expected_sources)?;
    validate_patch_budget(order, &expected_sources, &replacements)?;
    let staged = TempDir::new().context("failed to create M7 staged workspace")?;
    crate::copy_project_for_check(&root, staged.path())?;
    write_source_map(staged.path(), &replacements)?;

    let expected_delta = candidate.expected_delta();
    let mut evidence = Vec::new();
    let mut failures = Vec::new();
    if let Err(failure) = compare_graph_delta(
        runtime,
        staged.path(),
        order,
        GraphReanalysisPhase::Patched,
        expected_delta,
        plan.policy(),
    ) {
        failures.push(failure);
    }

    let before_format = snapshot_workspace(staged.path())?;
    for check in plan
        .checks()
        .iter()
        .filter(|check| check.kind == VerificationCheckKind::Format)
    {
        match runtime.format(staged.path(), order, check, plan.policy()) {
            Ok(observation) => evidence.push(observation),
            Err(failure) => failures.push(failure),
        }
    }
    let formatted_sources = read_paths(staged.path(), replacements.keys())?;
    if let Some(failure) = validate_format_scope(
        order,
        staged.path(),
        &before_format,
        &replacements,
        &formatted_sources,
    )? {
        failures.push(failure);
    }
    if let Err(failure) = compare_graph_delta(
        runtime,
        staged.path(),
        order,
        GraphReanalysisPhase::Formatted,
        expected_delta,
        plan.policy(),
    ) {
        failures.push(failure);
    }

    for check in plan.checks().iter().filter(|check| {
        !matches!(
            check.kind,
            VerificationCheckKind::Format | VerificationCheckKind::GraphDelta
        )
    }) {
        match runtime.run_check(staged.path(), order, check, plan.policy()) {
            Ok(observation) => evidence.push(observation),
            Err(failure) => failures.push(failure),
        }
    }
    for check in plan
        .checks()
        .iter()
        .filter(|check| check.kind == VerificationCheckKind::GraphDelta)
    {
        evidence.push(VerificationEvidence::new(
            check.id.clone(),
            EvidenceKind::GraphDelta,
            plan.snapshot(),
            graph_delta_artifact(expected_delta)?,
            if failures
                .iter()
                .any(|failure| failure.kind == VerifierFailureKind::GraphDeltaMismatch)
            {
                EvidenceOutcome::Failed
            } else {
                EvidenceOutcome::Passed
            },
            "patched and formatted graph deltas compared with the exact recipe contract",
        )?);
    }

    if !failures.is_empty() {
        let demotion = demote_for_counterexample(
            &root,
            options,
            candidate,
            plan,
            &formatted_sources,
            &failures,
        )?;
        let mut report = rejected_report(order, plan, failures.remove(0));
        report.failures.extend(failures);
        report.demotion = demotion;
        return Ok(report);
    }

    let decision = evaluate_evidence(
        order,
        plan,
        evidence,
        characterization,
        options.patch_authored_sequence,
    )?;
    if decision.disposition == VerificationDisposition::Rejected {
        let demotion = demote_for_failed_evidence(
            &root,
            options,
            candidate,
            plan,
            &formatted_sources,
            &decision,
        )?;
        return Ok(VerificationTransactionReport {
            schema: VERIFICATION_TRANSACTION_SCHEMA.into(),
            work_order: order.id().as_str().into(),
            verifier_plan: plan.id().into(),
            status: VerificationTransactionStatus::Rejected,
            residual_uncertainty: decision.residual_uncertainty.clone(),
            evidence: Some(decision),
            failures: Vec::new(),
            written: Vec::new(),
            undo_manifest: None,
            demotion,
            rollback_verified: true,
        });
    }
    if decision.disposition != VerificationDisposition::Automatic || !options.authorize_write {
        let mut uncertainty = decision.residual_uncertainty.clone();
        if !options.authorize_write {
            uncertainty
                .push("write authority was not granted for this verified transaction".into());
        }
        uncertainty.sort();
        uncertainty.dedup();
        return Ok(VerificationTransactionReport {
            schema: VERIFICATION_TRANSACTION_SCHEMA.into(),
            work_order: order.id().as_str().into(),
            verifier_plan: plan.id().into(),
            status: VerificationTransactionStatus::VerifiedReviewOnly,
            evidence: Some(decision),
            failures: Vec::new(),
            written: Vec::new(),
            undo_manifest: None,
            demotion: None,
            rollback_verified: true,
            residual_uncertainty: uncertainty,
        });
    }

    let receipt = match commit_atomic_sources_with_injection(
        &root,
        &options.undo_root,
        &expected_sources,
        &formatted_sources,
        options.atomic_failure,
    ) {
        Ok(receipt) => receipt,
        Err(error) => {
            let crash = options
                .atomic_failure
                .is_some_and(|injection| injection.mode == crate::AtomicFailureMode::Crash);
            return Ok(VerificationTransactionReport {
                schema: VERIFICATION_TRANSACTION_SCHEMA.into(),
                work_order: order.id().as_str().into(),
                verifier_plan: plan.id().into(),
                status: if crash {
                    VerificationTransactionStatus::RecoveryRequired
                } else {
                    VerificationTransactionStatus::RolledBack
                },
                evidence: Some(decision),
                failures: vec![failure(
                    VerifierStage::Commit,
                    if crash {
                        VerifierFailureKind::Crash
                    } else {
                        VerifierFailureKind::PartialWrite
                    },
                    None,
                    error.to_string(),
                    crash,
                )],
                written: Vec::new(),
                undo_manifest: None,
                demotion: None,
                rollback_verified: !crash && sources_equal(&root, &expected_sources)?,
                residual_uncertainty: Vec::new(),
            });
        }
    };

    match compare_graph_delta(
        runtime,
        &root,
        order,
        GraphReanalysisPhase::Live,
        expected_delta,
        plan.policy(),
    ) {
        Ok(()) => applied_report(order, plan, decision, receipt),
        Err(live_failure) => {
            restore_committed_transaction(&root, &receipt.manifest)?;
            let rollback_verified = sources_equal(&root, &expected_sources)?
                && compare_graph_delta(
                    runtime,
                    &root,
                    order,
                    GraphReanalysisPhase::Rollback,
                    &ExpectedGraphDelta {
                        changes: Vec::new(),
                    },
                    plan.policy(),
                )
                .is_ok();
            let demotion = demote_for_counterexample(
                &root,
                options,
                candidate,
                plan,
                &formatted_sources,
                std::slice::from_ref(&live_failure),
            )?;
            Ok(VerificationTransactionReport {
                schema: VERIFICATION_TRANSACTION_SCHEMA.into(),
                work_order: order.id().as_str().into(),
                verifier_plan: plan.id().into(),
                status: VerificationTransactionStatus::RolledBack,
                evidence: Some(decision),
                failures: vec![live_failure],
                written: Vec::new(),
                undo_manifest: Some(receipt.manifest),
                demotion,
                rollback_verified,
                residual_uncertainty: Vec::new(),
            })
        }
    }
}

fn transformation_candidate(order: &SharedWorkOrder) -> Result<&TransformationCandidate> {
    match order.subject() {
        WorkOrderSubject::Transformation { candidate } => Ok(candidate),
        WorkOrderSubject::FindingProposal { .. } => {
            bail!("M7 authoritative transaction requires a transformation candidate")
        }
    }
}

fn read_exact_sources(
    root: &Path,
    candidate: &TransformationCandidate,
) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let paths = candidate
        .edits()
        .iter()
        .map(|edit| edit.target.file().path.clone())
        .collect::<BTreeSet<_>>();
    read_paths(root, paths.iter())
}

fn read_paths<'a>(
    root: &Path,
    paths: impl IntoIterator<Item = &'a PathBuf>,
) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    paths
        .into_iter()
        .map(|path| {
            validate_relative_path(path)?;
            Ok((
                path.clone(),
                fs::read(root.join(path))
                    .with_context(|| format!("failed to read {}", path.display()))?,
            ))
        })
        .collect()
}

fn build_replacements(
    candidate: &TransformationCandidate,
    originals: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut edits = BTreeMap::<PathBuf, Vec<_>>::new();
    for edit in candidate.edits() {
        edits
            .entry(edit.target.file().path.clone())
            .or_default()
            .push(edit);
    }
    let mut replacements = BTreeMap::new();
    for (path, mut file_edits) in edits {
        file_edits.sort_by_key(|edit| (edit.span.start_byte, edit.span.end_byte));
        if file_edits
            .windows(2)
            .any(|pair| pair[0].span.end_byte > pair[1].span.start_byte)
        {
            bail!("candidate has overlapping edits for {}", path.display());
        }
        let original = std::str::from_utf8(&originals[&path])?;
        let mut replacement = original.to_string();
        for edit in file_edits.into_iter().rev() {
            let before = original
                .get(edit.span.start_byte..edit.span.end_byte)
                .context("candidate edit is outside source bytes")?;
            if before != edit.before
                || revision_guard(&path, edit.span, before) != edit.revision_guard
            {
                bail!("stale candidate revision guard for {}", path.display());
            }
            replacement.replace_range(edit.span.start_byte..edit.span.end_byte, &edit.after);
        }
        replacements.insert(path, replacement.into_bytes());
    }
    Ok(replacements)
}

fn validate_patch_budget(
    order: &SharedWorkOrder,
    originals: &BTreeMap<PathBuf, Vec<u8>>,
    replacements: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<()> {
    let budget = order.patch_budget();
    let removed = originals
        .iter()
        .map(|(path, bytes)| bytes.len().saturating_sub(replacements[path].len()))
        .sum::<usize>();
    let added = replacements
        .iter()
        .map(|(path, bytes)| bytes.len().saturating_sub(originals[path].len()))
        .sum::<usize>();
    if replacements.len() > budget.maximum_files
        || order
            .subject()
            .as_transformation()
            .is_some_and(|candidate| candidate.edits().len() > budget.maximum_edits)
        || removed > budget.maximum_removed_bytes
        || added > budget.maximum_added_bytes
    {
        bail!("candidate exceeds its exact work-order patch budget");
    }
    Ok(())
}

fn write_source_map(root: &Path, sources: &BTreeMap<PathBuf, Vec<u8>>) -> Result<()> {
    for (path, bytes) in sources {
        fs::write(root.join(path), bytes)?;
    }
    Ok(())
}

fn compare_graph_delta(
    runtime: &mut impl VerificationRuntime,
    root: &Path,
    order: &SharedWorkOrder,
    phase: GraphReanalysisPhase,
    expected: &ExpectedGraphDelta,
    policy: &VerifierExecutionPolicy,
) -> std::result::Result<(), VerifierFailure> {
    let actual = runtime.reanalyze_graph_delta(root, order, phase, policy)?;
    if &actual != expected {
        return Err(failure(
            VerifierStage::GraphDelta,
            VerifierFailureKind::GraphDeltaMismatch,
            None,
            format!("{phase:?} graph delta differs from the exact recipe contract"),
            false,
        ));
    }
    Ok(())
}

fn validate_format_scope(
    order: &SharedWorkOrder,
    root: &Path,
    before_workspace: &BTreeMap<PathBuf, Vec<u8>>,
    preformatted: &BTreeMap<PathBuf, Vec<u8>>,
    formatted: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<Option<VerifierFailure>> {
    let (protect_bytes, protect_files) = match order.verification() {
        WorkOrderVerification::Transformation {
            protect_undeclared_bytes,
            protect_undeclared_files,
            ..
        } => (*protect_undeclared_bytes, *protect_undeclared_files),
        WorkOrderVerification::FindingProposal { .. } => (true, true),
    };
    if protect_bytes && preformatted != formatted {
        return Ok(Some(failure(
            VerifierStage::Format,
            VerifierFailureKind::FormatChangedSemantics,
            None,
            "formatter changed bytes outside the exact staged candidate",
            false,
        )));
    }
    if protect_files {
        let after = snapshot_workspace(root)?;
        let declared = preformatted.keys().collect::<BTreeSet<_>>();
        for (path, bytes) in before_workspace {
            if !declared.contains(path) && after.get(path) != Some(bytes) {
                return Ok(Some(failure(
                    VerifierStage::Format,
                    VerifierFailureKind::FilesystemViolation,
                    None,
                    format!("formatter changed undeclared file {}", path.display()),
                    false,
                )));
            }
        }
        if after
            .keys()
            .any(|path| !declared.contains(path) && !before_workspace.contains_key(path))
        {
            return Ok(Some(failure(
                VerifierStage::Format,
                VerifierFailureKind::FilesystemViolation,
                None,
                "formatter created an undeclared file",
                false,
            )));
        }
    }
    Ok(None)
}

fn snapshot_workspace(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut snapshot = BTreeMap::new();
    for entry in ignore::WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(name.as_ref(), ".deslop" | ".git" | ".jj" | "target")
        })
        .build()
    {
        let entry = entry?;
        if entry.file_type().is_some_and(|kind| kind.is_file()) {
            snapshot.insert(
                entry.path().strip_prefix(root)?.to_path_buf(),
                fs::read(entry.path())?,
            );
        }
    }
    Ok(snapshot)
}

fn demote_for_counterexample(
    root: &Path,
    options: &VerificationTransactionOptions,
    candidate: &TransformationCandidate,
    plan: &VerifierPlan,
    sources: &BTreeMap<PathBuf, Vec<u8>>,
    failures: &[VerifierFailure],
) -> Result<Option<String>> {
    let Some(failure) = failures.iter().find(|failure| {
        matches!(
            failure.kind,
            VerifierFailureKind::Counterexample
                | VerifierFailureKind::FormatChangedSemantics
                | VerifierFailureKind::GraphDeltaMismatch
        )
    }) else {
        return Ok(None);
    };
    let bytes = counterexample_bytes(sources);
    let record = RecipeDemotionRecord::counterexample(
        candidate.recipe().id().as_str(),
        candidate.id().as_str(),
        plan.snapshot(),
        failure.stage,
        failure.kind,
        &bytes,
        &failure.detail,
    )?;
    let id = record.id.clone();
    RecipeDemotionStore::append(root, &options.demotion_journal, record)?;
    Ok(Some(id))
}

fn demote_for_failed_evidence(
    root: &Path,
    options: &VerificationTransactionOptions,
    candidate: &TransformationCandidate,
    plan: &VerifierPlan,
    sources: &BTreeMap<PathBuf, Vec<u8>>,
    decision: &EvidenceDecision,
) -> Result<Option<String>> {
    let Some(observation) = decision.evidence.iter().find(|observation| {
        observation.outcome == EvidenceOutcome::Failed
            && matches!(
                observation.kind,
                EvidenceKind::TargetedTest
                    | EvidenceKind::Characterization
                    | EvidenceKind::Differential
                    | EvidenceKind::Mutation
            )
    }) else {
        return Ok(None);
    };
    let bytes = counterexample_bytes(sources);
    let kind = format!("{:?}", observation.kind).to_lowercase();
    let record = RecipeDemotionRecord::counterexample(
        candidate.recipe().id().as_str(),
        candidate.id().as_str(),
        plan.snapshot(),
        VerifierStage::Command,
        VerifierFailureKind::Counterexample,
        &bytes,
        format!(
            "{} evidence `{}` failed: {}",
            kind, observation.check, observation.detail
        ),
    )?;
    let id = record.id.clone();
    RecipeDemotionStore::append(root, &options.demotion_journal, record)?;
    Ok(Some(id))
}

fn counterexample_bytes(sources: &BTreeMap<PathBuf, Vec<u8>>) -> Vec<u8> {
    sources
        .iter()
        .flat_map(|(path, bytes)| {
            path.to_string_lossy()
                .as_bytes()
                .iter()
                .copied()
                .chain(std::iter::once(0))
                .chain(bytes.iter().copied())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn graph_delta_artifact(delta: &ExpectedGraphDelta) -> Result<String> {
    let payload = serde_json::to_vec(delta)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"deslop observed graph delta v1\0");
    hasher.update(&payload);
    Ok(format!("gd1_{}", hasher.finalize().to_hex()))
}

fn applied_report(
    order: &SharedWorkOrder,
    plan: &VerifierPlan,
    decision: EvidenceDecision,
    receipt: AtomicCommitReceipt,
) -> Result<VerificationTransactionReport> {
    if receipt.state != UndoState::Committed {
        bail!("atomic commit receipt is not committed");
    }
    Ok(VerificationTransactionReport {
        schema: VERIFICATION_TRANSACTION_SCHEMA.into(),
        work_order: order.id().as_str().into(),
        verifier_plan: plan.id().into(),
        status: VerificationTransactionStatus::Applied,
        evidence: Some(decision),
        failures: Vec::new(),
        written: receipt.written,
        undo_manifest: Some(receipt.manifest),
        demotion: None,
        rollback_verified: true,
        residual_uncertainty: Vec::new(),
    })
}

fn rejected_report(
    order: &SharedWorkOrder,
    plan: &VerifierPlan,
    failure: VerifierFailure,
) -> VerificationTransactionReport {
    VerificationTransactionReport {
        schema: VERIFICATION_TRANSACTION_SCHEMA.into(),
        work_order: order.id().as_str().into(),
        verifier_plan: plan.id().into(),
        status: VerificationTransactionStatus::Rejected,
        evidence: None,
        failures: vec![failure],
        written: Vec::new(),
        undo_manifest: None,
        demotion: None,
        rollback_verified: true,
        residual_uncertainty: Vec::new(),
    }
}

fn failure(
    stage: VerifierStage,
    kind: VerifierFailureKind,
    check: Option<String>,
    detail: impl Into<String>,
    retryable: bool,
) -> VerifierFailure {
    VerifierFailure {
        stage,
        kind,
        check,
        detail: detail.into(),
        retryable,
    }
}

fn validate_options(options: &VerificationTransactionOptions) -> Result<()> {
    if options.patch_authored_sequence == 0 {
        bail!("patch authored sequence must be nonzero");
    }
    validate_relative_path(&options.undo_root)?;
    validate_relative_path(&options.demotion_journal)
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("verification transaction path must stay relative to the project root");
    }
    Ok(())
}

fn sources_equal(root: &Path, expected: &BTreeMap<PathBuf, Vec<u8>>) -> Result<bool> {
    for (path, bytes) in expected {
        if fs::read(root.join(path)).ok().as_deref() != Some(bytes.as_slice()) {
            return Ok(false);
        }
    }
    Ok(true)
}

trait TransformationSubjectExt {
    fn as_transformation(&self) -> Option<&TransformationCandidate>;
}

impl TransformationSubjectExt for WorkOrderSubject {
    fn as_transformation(&self) -> Option<&TransformationCandidate> {
        match self {
            WorkOrderSubject::Transformation { candidate } => Some(candidate),
            WorkOrderSubject::FindingProposal { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AtomicFailureMode, AtomicFailurePoint, AuthorityObservation, AuthorityProvider,
        AuthorityRequirement, AuthorityState, CheckSelectionMode, NetworkPolicy,
        VerificationCatalog, VerifierExecutionPolicy, VerifierPlan,
    };
    use deslop_protocol::SharedWorkOrder;
    use deslop_recipes::detect_rust_recipes;

    struct RecordedRuntime {
        delta: ExpectedGraphDelta,
        fail_kind: Option<VerifierFailureKind>,
        fail_phase: Option<GraphReanalysisPhase>,
    }

    impl VerificationRuntime for RecordedRuntime {
        fn format(
            &mut self,
            _root: &Path,
            order: &SharedWorkOrder,
            check: &VerificationCheck,
            _policy: &VerifierExecutionPolicy,
        ) -> std::result::Result<VerificationEvidence, VerifierFailure> {
            if self.fail_kind == Some(VerifierFailureKind::FormatChangedSemantics) {
                return Err(failure(
                    VerifierStage::Format,
                    VerifierFailureKind::FormatChangedSemantics,
                    Some(check.id.clone()),
                    "injected formatting failure",
                    false,
                ));
            }
            VerificationEvidence::new(
                check.id.clone(),
                check.kind.into(),
                order.provenance().project_snapshot.clone().unwrap(),
                "formatter-artifact",
                EvidenceOutcome::Passed,
                "formatter passed",
            )
            .map_err(|error| {
                failure(
                    VerifierStage::Format,
                    VerifierFailureKind::InvalidInput,
                    None,
                    error.to_string(),
                    false,
                )
            })
        }

        fn reanalyze_graph_delta(
            &mut self,
            _root: &Path,
            _order: &SharedWorkOrder,
            phase: GraphReanalysisPhase,
            _policy: &VerifierExecutionPolicy,
        ) -> std::result::Result<ExpectedGraphDelta, VerifierFailure> {
            if self.fail_phase == Some(phase) {
                return Ok(ExpectedGraphDelta {
                    changes: Vec::new(),
                });
            }
            if phase == GraphReanalysisPhase::Rollback {
                return Ok(ExpectedGraphDelta {
                    changes: Vec::new(),
                });
            }
            Ok(self.delta.clone())
        }

        fn run_check(
            &mut self,
            _root: &Path,
            order: &SharedWorkOrder,
            check: &VerificationCheck,
            _policy: &VerifierExecutionPolicy,
        ) -> std::result::Result<VerificationEvidence, VerifierFailure> {
            if self.fail_kind == Some(VerifierFailureKind::Timeout)
                && check.kind == VerificationCheckKind::TargetedTest
            {
                return Err(failure(
                    VerifierStage::Command,
                    VerifierFailureKind::Timeout,
                    Some(check.id.clone()),
                    "injected timeout",
                    true,
                ));
            }
            if matches!(
                self.fail_kind,
                Some(VerifierFailureKind::CommandFailed | VerifierFailureKind::Crash)
            ) && check.kind == VerificationCheckKind::Build
            {
                let kind = self.fail_kind.unwrap();
                return Err(failure(
                    VerifierStage::Command,
                    kind,
                    Some(check.id.clone()),
                    format!("injected {kind:?}"),
                    kind == VerifierFailureKind::Crash,
                ));
            }
            if self.fail_kind == Some(VerifierFailureKind::Counterexample)
                && check.kind == VerificationCheckKind::Differential
            {
                return VerificationEvidence::new(
                    check.id.clone(),
                    check.kind.into(),
                    order.provenance().project_snapshot.clone().unwrap(),
                    "differential-counterexample",
                    EvidenceOutcome::Failed,
                    "before/after behavior differs",
                )
                .map_err(|error| {
                    failure(
                        VerifierStage::Command,
                        VerifierFailureKind::InvalidInput,
                        None,
                        error.to_string(),
                        false,
                    )
                });
            }
            VerificationEvidence::new(
                check.id.clone(),
                check.kind.into(),
                order.provenance().project_snapshot.clone().unwrap(),
                format!("artifact-{}", check.id),
                EvidenceOutcome::Passed,
                "recorded check passed",
            )
            .map_err(|error| {
                failure(
                    VerifierStage::Command,
                    VerifierFailureKind::InvalidInput,
                    None,
                    error.to_string(),
                    false,
                )
            })
        }
    }

    fn fixture() -> (TempDir, SharedWorkOrder, VerifierPlan, ExpectedGraphDelta) {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("fixture.rs"), "fn run() { return; 1; }\n").unwrap();
        let candidate = detect_rust_recipes(root.path(), &[PathBuf::from("fixture.rs")])
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let delta = candidate.expected_delta().clone();
        let order = SharedWorkOrder::from_candidate(candidate).unwrap();
        let snapshot = order.provenance().project_snapshot.clone().unwrap();
        let resource = order.access().writes[0].clone();
        let requirement = AuthorityRequirement {
            key: "compiler-precondition".into(),
            accepted_providers: vec![AuthorityProvider::Compiler],
        };
        let kinds = [
            ("build", VerificationCheckKind::Build),
            ("coverage", VerificationCheckKind::Coverage),
            ("differential", VerificationCheckKind::Differential),
            ("format", VerificationCheckKind::Format),
            ("lint", VerificationCheckKind::Lint),
            ("mutation", VerificationCheckKind::Mutation),
            ("targeted-test", VerificationCheckKind::TargetedTest),
            ("type", VerificationCheckKind::Type),
        ];
        let checks = kinds
            .into_iter()
            .map(|(id, kind)| VerificationCheck {
                id: id.into(),
                kind,
                command: Some("recorded".into()),
                covers: vec![resource.clone()],
                dependencies: Vec::new(),
                authority: vec![requirement.clone()],
                always_required: false,
            })
            .collect();
        let plan = VerifierPlan::build(
            &order,
            VerificationCatalog {
                snapshot: snapshot.clone(),
                impact_coverage_complete: true,
                checks,
            },
            vec![AuthorityObservation {
                key: requirement.key,
                provider: AuthorityProvider::Compiler,
                snapshot,
                artifact: "compiler-artifact".into(),
                state: AuthorityState::Proven,
                detail: "compiler precondition passed".into(),
            }],
            VerifierExecutionPolicy::hermetic_workspace(),
        )
        .unwrap();
        assert_eq!(plan.selection(), CheckSelectionMode::ImpactCone);
        (root, order, plan, delta)
    }

    fn options(root: &Path) -> VerificationTransactionOptions {
        let mut options = VerificationTransactionOptions::controlled(root.to_path_buf(), 2);
        options.authorize_write = true;
        options
    }

    #[test]
    fn full_evidence_applies_once_with_durable_undo() {
        let (root, order, plan, delta) = fixture();
        let report = execute_verification_transaction(
            &order,
            &plan,
            None,
            &options(root.path()),
            &mut RecordedRuntime {
                delta,
                fail_kind: None,
                fail_phase: None,
            },
        )
        .unwrap();
        assert_eq!(report.status, VerificationTransactionStatus::Applied);
        assert!(report.undo_manifest.unwrap().is_file());
        assert!(
            !fs::read_to_string(root.path().join("fixture.rs"))
                .unwrap()
                .contains("1;")
        );
    }

    #[test]
    fn graph_counterexample_rejects_demotes_and_never_writes() {
        let (root, order, plan, delta) = fixture();
        let original = fs::read(root.path().join("fixture.rs")).unwrap();
        let report = execute_verification_transaction(
            &order,
            &plan,
            None,
            &options(root.path()),
            &mut RecordedRuntime {
                delta,
                fail_kind: None,
                fail_phase: Some(GraphReanalysisPhase::Formatted),
            },
        )
        .unwrap();
        assert_eq!(report.status, VerificationTransactionStatus::Rejected);
        assert!(report.demotion.is_some());
        assert_eq!(fs::read(root.path().join("fixture.rs")).unwrap(), original);
        let candidate = transformation_candidate(&order).unwrap();
        assert!(
            RecipeDemotionStore::load(root.path(), &options(root.path()).demotion_journal)
                .unwrap()
                .is_demoted(candidate.recipe().id().as_str())
        );
    }

    #[test]
    fn timeout_and_partial_write_failures_restore_exact_bytes() {
        let (root, order, plan, delta) = fixture();
        let original = fs::read(root.path().join("fixture.rs")).unwrap();
        let report = execute_verification_transaction(
            &order,
            &plan,
            None,
            &options(root.path()),
            &mut RecordedRuntime {
                delta: delta.clone(),
                fail_kind: Some(VerifierFailureKind::Timeout),
                fail_phase: None,
            },
        )
        .unwrap();
        assert_eq!(report.status, VerificationTransactionStatus::Rejected);
        assert_eq!(fs::read(root.path().join("fixture.rs")).unwrap(), original);

        let (root, order, plan, delta) = fixture();
        let original = fs::read(root.path().join("fixture.rs")).unwrap();
        let mut options = options(root.path());
        options.atomic_failure = Some(AtomicFailureInjection {
            point: AtomicFailurePoint::AfterRename(0),
            mode: AtomicFailureMode::Error,
        });
        let report = execute_verification_transaction(
            &order,
            &plan,
            None,
            &options,
            &mut RecordedRuntime {
                delta,
                fail_kind: None,
                fail_phase: None,
            },
        )
        .unwrap();
        assert_eq!(report.status, VerificationTransactionStatus::RolledBack);
        assert!(report.rollback_verified);
        assert_eq!(fs::read(root.path().join("fixture.rs")).unwrap(), original);
    }

    #[test]
    fn command_crash_and_format_failures_never_write() {
        for kind in [
            VerifierFailureKind::CommandFailed,
            VerifierFailureKind::Crash,
            VerifierFailureKind::FormatChangedSemantics,
        ] {
            let (root, order, plan, delta) = fixture();
            let original = fs::read(root.path().join("fixture.rs")).unwrap();
            let report = execute_verification_transaction(
                &order,
                &plan,
                None,
                &options(root.path()),
                &mut RecordedRuntime {
                    delta,
                    fail_kind: Some(kind),
                    fail_phase: None,
                },
            )
            .unwrap();
            assert_eq!(report.status, VerificationTransactionStatus::Rejected);
            assert_eq!(fs::read(root.path().join("fixture.rs")).unwrap(), original);
            assert!(report.failures.iter().any(|failure| failure.kind == kind));
            assert_eq!(
                report.demotion.is_some(),
                kind == VerifierFailureKind::FormatChangedSemantics
            );
        }
    }

    #[test]
    fn failed_differential_evidence_immediately_demotes() {
        let (root, order, plan, delta) = fixture();
        let original = fs::read(root.path().join("fixture.rs")).unwrap();
        let report = execute_verification_transaction(
            &order,
            &plan,
            None,
            &options(root.path()),
            &mut RecordedRuntime {
                delta,
                fail_kind: Some(VerifierFailureKind::Counterexample),
                fail_phase: None,
            },
        )
        .unwrap();
        assert_eq!(report.status, VerificationTransactionStatus::Rejected);
        assert!(report.demotion.is_some());
        assert_eq!(fs::read(root.path().join("fixture.rs")).unwrap(), original);
    }

    #[test]
    fn simulated_crash_requires_and_supports_deterministic_recovery() {
        let (root, order, plan, delta) = fixture();
        let original = fs::read(root.path().join("fixture.rs")).unwrap();
        let mut options = options(root.path());
        options.atomic_failure = Some(AtomicFailureInjection {
            point: AtomicFailurePoint::AfterRename(0),
            mode: AtomicFailureMode::Crash,
        });
        let report = execute_verification_transaction(
            &order,
            &plan,
            None,
            &options,
            &mut RecordedRuntime {
                delta,
                fail_kind: None,
                fail_phase: None,
            },
        )
        .unwrap();
        assert_eq!(
            report.status,
            VerificationTransactionStatus::RecoveryRequired
        );
        let recovered =
            crate::recover_incomplete_transactions(root.path(), &options.undo_root).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(fs::read(root.path().join("fixture.rs")).unwrap(), original);
    }

    #[test]
    fn live_graph_delta_failure_restores_reanalyzes_and_demotes() {
        let (root, order, plan, delta) = fixture();
        let original = fs::read(root.path().join("fixture.rs")).unwrap();
        let report = execute_verification_transaction(
            &order,
            &plan,
            None,
            &options(root.path()),
            &mut RecordedRuntime {
                delta,
                fail_kind: None,
                fail_phase: Some(GraphReanalysisPhase::Live),
            },
        )
        .unwrap();
        assert_eq!(report.status, VerificationTransactionStatus::RolledBack);
        assert!(report.rollback_verified);
        assert!(report.demotion.is_some());
        assert_eq!(fs::read(root.path().join("fixture.rs")).unwrap(), original);
    }

    #[test]
    fn execution_policy_is_bound_and_network_defaults_denied() {
        let (_root, _order, plan, _delta) = fixture();
        assert_eq!(plan.policy().network, NetworkPolicy::Denied);
    }
}
