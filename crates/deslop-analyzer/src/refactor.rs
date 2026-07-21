//! Refactor-defect accumulation detection over a revision window.
//!
//! Design: `docs/REFACTOR_DEFECT_ACCUMULATION.md`. Phase 1 shipped review-only
//! `owner-moved-consumer-stale` and `producer-verifier-schema-drift` over a
//! two-revision window, built on [`ContractChangeHistory`] facts extracted
//! from exact `ProjectAnalysis` snapshots. Phase 2 adds the adoption
//! surfaces:
//!
//! - `accepted-config-inert`: a formerly live config key loses its final
//!   behavioral read while the acceptance surface (a defaults literal) still
//!   carries it — the doc's two historical cases, including a live
//!   replacement key;
//! - `test-oracle-lag`: a migration fired while an unchanged test still
//!   exercises the former representation. The finding states what remains
//!   unproved; it never claims the implementation is wrong;
//! - `adoption-chain-incomplete` summaries: when several families share one
//!   owner migration, one summary is emitted in
//!   [`RefactorRiskReport::summaries`] — never in `findings` — so baselines
//!   and severity counts do not double-count;
//! - multi-revision windows: persistence (revisions survived, independent
//!   edits) and co-change triage inputs computed from contract fingerprints
//!   across the window.
//!
//! All findings are `NeverAuto`: the analysis diagnoses an incomplete
//! contract migration and suggests a verification; it never proposes a
//! rewrite.
//!
//! Detection discipline (Phase 1, unchanged):
//! - an owner change requires a function whose contract-token set both lost
//!   and gained tokens between the revisions (a pure removal is a deletion,
//!   not a migration);
//! - a removed token only counts as a *former representation* when it is
//!   still referenced somewhere in the after revision — otherwise the change
//!   is a rewrite or rename, which must not fire (rename/move negative
//!   cases);
//! - a stale consumer must be matched by name and have an *unchanged* token
//!   set still containing the removed token. A consumer that moved anywhere
//!   else — including to a compatibility adapter — does not fire.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use deslop_core::SafetyClass;
use deslop_core::refactor_defect::{
    CapabilityLevel, ContractEdgeKind, ContractNodeRef, ContractRole, ContractStep, CoverageGap,
    EntityMatchEvidence, EntityMatchKind, EvidenceItem, FactProvider, OwnerMigration, Persistence,
    REFACTOR_DEFECT_SCHEMA, RefactorDefect, RevisionPair, rule_names,
};
use deslop_parse::{
    ContractChangeHistory, ContractFunction, DiscoveryPolicy, FactCoverage, FileContracts,
    ProjectAnalysis, ProjectSnapshotPlanner, ProjectSnapshotRequest, RepositorySpec,
    RevisionContracts, RootSpec, ScopeSpec,
};
use serde::Serialize;

/// Wire schema identifier for a refactor-risk report over one revision window.
pub const REFACTOR_RISK_SCHEMA: &str = "deslop.refactor-risk/1";

/// The output of one refactor-risk comparison over an ordered window.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RefactorRiskReport {
    pub schema: String,
    /// First revision label (== `revisions[0]`).
    pub before: String,
    /// Last revision label.
    pub after: String,
    /// All revision labels in window order, oldest first.
    pub revisions: Vec<String>,
    pub coverage: FactCoverage,
    pub coverage_reasons: Vec<String>,
    /// Detector-family findings, one per detected condition.
    pub findings: Vec<RefactorDefect>,
    /// `adoption-chain-incomplete` summaries. Kept out of `findings` so
    /// baselines and severity counts never double-count the underlying
    /// findings.
    pub summaries: Vec<RefactorDefect>,
}

/// Compare two directory snapshots (`--from`, `--to`) and report
/// refactor-defect findings.
pub fn refactor_risk_paths(from: &Path, to: &Path) -> Result<RefactorRiskReport> {
    refactor_risk_window_paths(&[from.to_path_buf(), to.to_path_buf()])
}

/// Compare an ordered window of directory snapshots (`--from`, `--to`,
/// `--then ...`) and report refactor-defect findings.
pub fn refactor_risk_window_paths(roots: &[PathBuf]) -> Result<RefactorRiskReport> {
    let mut revisions = Vec::with_capacity(roots.len());
    for root in roots {
        revisions.push((root.display().to_string(), analysis_for(root)?));
    }
    analyze_refactor_window(revisions)
}

/// Compare two exact analyses under caller-supplied revision labels.
pub fn analyze_refactor_risk(
    before: (String, Arc<ProjectAnalysis>),
    after: (String, Arc<ProjectAnalysis>),
) -> Result<RefactorRiskReport> {
    analyze_refactor_window(vec![before, after])
}

/// Compare an ordered window of exact analyses under caller-supplied labels.
///
/// Detection runs over each adjacent pair; persistence and co-change triage
/// inputs are then tracked across the whole window. Two-revision windows
/// behave exactly like Phase 1 and carry the explicit
/// accumulation-not-analyzed coverage gap.
pub fn analyze_refactor_window(
    revisions: Vec<(String, Arc<ProjectAnalysis>)>,
) -> Result<RefactorRiskReport> {
    if revisions.len() < 2 {
        bail!(
            "refactor-risk requires at least two revisions (got {})",
            revisions.len()
        );
    }
    let labels: Vec<String> = revisions
        .iter()
        .map(|(label, _)| label.clone())
        .collect();
    let history = ContractChangeHistory::from_analyses(&revisions)
        .context("build contract change history")?;
    let mut drafts = Vec::new();
    for index in 0..history.revisions.len() - 1 {
        detect_pair(
            index,
            &labels,
            &history.revisions[index],
            &history.revisions[index + 1],
            &mut drafts,
        );
    }
    for draft in &mut drafts {
        track_persistence(&history, draft);
    }
    let window = history.revisions.len();
    let findings: Vec<RefactorDefect> = drafts
        .iter()
        .map(|draft| build_finding(draft, window))
        .collect();
    let summaries = build_summaries(&drafts, window);
    Ok(RefactorRiskReport {
        schema: REFACTOR_RISK_SCHEMA.to_string(),
        before: labels.first().cloned().unwrap_or_default(),
        after: labels.last().cloned().unwrap_or_default(),
        revisions: labels,
        coverage: history.coverage,
        coverage_reasons: history.reasons,
        findings,
        summaries,
    })
}

/// Build an exact analysis from one on-disk snapshot directory, mirroring
/// the planner flow used by `scan_paths_with_context` and `deslop-graph`.
/// The root is pinned to the directory itself so file paths are comparable
/// across the two revisions (`before/dir/x.py` and `after/dir/x.py` must
/// identify the same contract file).
fn analysis_for(root: &Path) -> Result<Arc<ProjectAnalysis>> {
    let invocation_base = std::env::current_dir().context("resolve invocation base")?;
    let planner = ProjectSnapshotPlanner::resolve(ProjectSnapshotRequest {
        invocation_base,
        root: RootSpec::Explicit(root.to_path_buf()),
        repository: RepositorySpec::Auto,
        scope: ScopeSpec::Requested(vec![root.to_path_buf()]),
        discovery: DiscoveryPolicy::LegacyRespectIgnore,
    })?;
    let built = planner.build()?;
    ProjectAnalysis::build(built.snapshot)
}

/// A contract-token domain. Detector families are one structural query over
/// different domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenDomain {
    /// Callee/attribute reference tokens.
    References,
    /// String-literal (schema field) tokens.
    Literals,
    /// Config-key read tokens.
    ConfigKeys,
}

impl TokenDomain {
    fn tokens<'f>(self, function: &'f ContractFunction) -> &'f BTreeSet<String> {
        match self {
            Self::References => &function.references,
            Self::Literals => &function.literals,
            Self::ConfigKeys => &function.config_keys,
        }
    }

    /// Module-level tokens of this domain (tokens outside any function).
    fn module_tokens<'f>(self, file: &'f FileContracts) -> Vec<&'f str> {
        match self {
            Self::References => Vec::new(),
            Self::Literals => file.module_literals.keys().map(String::as_str).collect(),
            Self::ConfigKeys => file
                .module_config_keys
                .keys()
                .map(String::as_str)
                .collect(),
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::References => "reference",
            Self::Literals => "schema field",
            Self::ConfigKeys => "config key",
        }
    }
}

/// The two migration detector families shipped in Phase 1. Both are the same
/// structural query — a changed token set with an unchanged dependent still
/// holding a removed token — over different token domains and roles.
#[derive(Debug, Clone, Copy)]
enum Family {
    OwnerMovedConsumerStale,
    ProducerVerifierSchemaDrift,
}

impl Family {
    fn rule(self) -> &'static str {
        match self {
            Self::OwnerMovedConsumerStale => rule_names::OWNER_MOVED_CONSUMER_STALE,
            Self::ProducerVerifierSchemaDrift => rule_names::PRODUCER_VERIFIER_SCHEMA_DRIFT,
        }
    }

    fn domain(self) -> TokenDomain {
        match self {
            Self::OwnerMovedConsumerStale => TokenDomain::References,
            Self::ProducerVerifierSchemaDrift => TokenDomain::Literals,
        }
    }

    fn roles(self) -> (ContractRole, ContractRole) {
        match self {
            Self::OwnerMovedConsumerStale => (ContractRole::Owner, ContractRole::Consumer),
            Self::ProducerVerifierSchemaDrift => (ContractRole::Producer, ContractRole::Verifier),
        }
    }

    fn edges(self) -> (ContractEdgeKind, ContractEdgeKind) {
        match self {
            Self::OwnerMovedConsumerStale => {
                (ContractEdgeKind::Produces, ContractEdgeKind::Consumes)
            }
            Self::ProducerVerifierSchemaDrift => {
                (ContractEdgeKind::Produces, ContractEdgeKind::Verifies)
            }
        }
    }

    fn suggested_verification(
        self,
        owner: &str,
        consumer: &str,
        added: &BTreeSet<String>,
    ) -> String {
        let added = added.iter().map(String::as_str).collect::<Vec<_>>().join(", ");
        match self {
            Self::OwnerMovedConsumerStale => format!(
                "decide whether `{consumer}` should follow `{owner}` to the new owner ({added}), \
                 or pin the old representation behind a tested compatibility invariant"
            ),
            Self::ProducerVerifierSchemaDrift => format!(
                "reconcile `{consumer}` with `{owner}`'s new schema ({added}): update the \
                 verifier's required fields or restore the removed ones"
            ),
        }
    }
}

/// The contract node a stale condition is attached to: a function, or a
/// module-level token (acceptance surfaces such as defaults dicts live
/// outside functions).
#[derive(Debug, Clone)]
enum Holder {
    Function { path: PathBuf, function: ContractFunction },
    Module { path: PathBuf, span: deslop_core::Span, token: String },
}

impl Holder {
    fn path(&self) -> &Path {
        match self {
            Self::Function { path, .. } | Self::Module { path, .. } => path,
        }
    }

    fn display_name(&self) -> String {
        match self {
            Self::Function { function, .. } => function.name.clone(),
            Self::Module { token, .. } => format!("module level ({token})"),
        }
    }

    /// Identity used to detect edits across revisions.
    fn identity(&self) -> String {
        match self {
            Self::Function { function, .. } => function.fingerprint.clone(),
            Self::Module { span, .. } => format!("{span:?}"),
        }
    }

    fn node_ref(&self, role: ContractRole) -> ContractNodeRef {
        let (span, fingerprint) = match self {
            Self::Function { function, .. } => (function.span, function.fingerprint.clone()),
            Self::Module { span, token, .. } => (*span, format!("module-token:{token}")),
        };
        ContractNodeRef {
            role,
            path: self.path().to_path_buf(),
            span,
            fingerprint,
            provider: FactProvider::TreeSitter,
            capability: CapabilityLevel::Complete,
        }
    }
}

/// One detected condition before it is assembled into a `RefactorDefect`.
/// Drafts keep the function-level facts persistence tracking needs; the
/// finding payload itself does not carry names outside `detail` strings.
#[derive(Debug, Clone)]
struct FindingDraft {
    rule: &'static str,
    pair_index: usize,
    before_label: String,
    after_label: String,
    /// Owner file path (config-inert findings use the holder's path).
    path: PathBuf,
    owner_before: Option<ContractFunction>,
    owner_after: Option<ContractFunction>,
    dependent: Holder,
    domain: TokenDomain,
    /// Removed tokens the dependent still holds (config-inert: the key).
    stale: BTreeSet<String>,
    /// Removed tokens that survive somewhere in the after revision.
    surviving: BTreeSet<String>,
    /// Tokens the owner gained (config-inert: live replacement keys).
    added: BTreeSet<String>,
    note: Option<String>,
    persistence: Persistence,
}

impl FindingDraft {
    fn owner_name(&self) -> Option<&str> {
        self.owner_after.as_ref().map(|owner| owner.name.as_str())
    }
}

/// Index a file's functions by name.
fn functions_by_name(file: &FileContracts) -> BTreeMap<&str, &ContractFunction> {
    file.functions
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect()
}

/// Run all detectors over one adjacent revision pair.
fn detect_pair(
    pair_index: usize,
    labels: &[String],
    rev_before: &RevisionContracts,
    rev_after: &RevisionContracts,
    drafts: &mut Vec<FindingDraft>,
) {
    let pair_start = drafts.len();
    for file_before in &rev_before.files {
        let Some(file_after) = rev_after
            .files
            .iter()
            .find(|file| file.path == file_before.path)
        else {
            continue;
        };
        detect_family(
            Family::OwnerMovedConsumerStale,
            pair_index,
            labels,
            file_before,
            file_after,
            drafts,
        );
        detect_family(
            Family::ProducerVerifierSchemaDrift,
            pair_index,
            labels,
            file_before,
            file_after,
            drafts,
        );
    }
    detect_config_inert(pair_index, labels, rev_before, rev_after, drafts);
    detect_test_oracle_lag(pair_index, labels, rev_before, rev_after, pair_start, drafts);
}

/// Run one migration detector family over one matched file pair.
fn detect_family(
    family: Family,
    pair_index: usize,
    labels: &[String],
    file_before: &FileContracts,
    file_after: &FileContracts,
    drafts: &mut Vec<FindingDraft>,
) {
    let domain = family.domain();
    let before_by_name = functions_by_name(file_before);
    let after_by_name = functions_by_name(file_after);
    let global_after: BTreeSet<&str> = file_after
        .functions
        .iter()
        .flat_map(|function| domain.tokens(function).iter().map(String::as_str))
        .chain(domain.module_tokens(file_after))
        .collect();

    for (name, owner_before) in &before_by_name {
        let Some(owner_after) = after_by_name.get(name) else {
            continue;
        };
        let tokens_before = domain.tokens(owner_before);
        let tokens_after = domain.tokens(owner_after);
        let removed: BTreeSet<String> = tokens_before
            .iter()
            .filter(|token| !tokens_after.contains(*token))
            .cloned()
            .collect();
        let added: BTreeSet<String> = tokens_after
            .iter()
            .filter(|token| !tokens_before.contains(*token))
            .cloned()
            .collect();
        // A migration both loses and gains tokens; pure removals or additions
        // are deletions/rewrites, not owner moves.
        if removed.is_empty() || added.is_empty() {
            continue;
        }
        // The former representation must survive somewhere in the after
        // revision; otherwise this is a rename or rewrite, not a stale
        // consumer (rename/move negative cases).
        let surviving: BTreeSet<String> = removed
            .iter()
            .filter(|token| global_after.contains(token.as_str()))
            .cloned()
            .collect();
        if surviving.is_empty() {
            continue;
        }
        // Stale dependents: matched by name, token set unchanged, still
        // holding at least one surviving removed token.
        for (dependent_name, dependent_before) in &before_by_name {
            if dependent_name == name {
                continue;
            }
            let Some(dependent_after) = after_by_name.get(dependent_name) else {
                continue;
            };
            if domain.tokens(dependent_before) != domain.tokens(dependent_after) {
                continue;
            }
            let stale: BTreeSet<String> = surviving
                .iter()
                .filter(|token| domain.tokens(dependent_before).contains(*token))
                .cloned()
                .collect();
            if stale.is_empty() {
                continue;
            }
            drafts.push(FindingDraft {
                rule: family.rule(),
                pair_index,
                before_label: labels[pair_index].clone(),
                after_label: labels[pair_index + 1].clone(),
                path: file_before.path.clone(),
                owner_before: Some((*owner_before).clone()),
                owner_after: Some((*owner_after).clone()),
                dependent: Holder::Function {
                    path: file_after.path.clone(),
                    function: (*dependent_after).clone(),
                },
                domain,
                stale,
                surviving: surviving.clone(),
                added: added.clone(),
                note: None,
                persistence: Persistence {
                    revisions: 1,
                    independent_edits: 0,
                },
            });
        }
    }
}

/// `accepted-config-inert`: a config key read in the before revision has no
/// read anywhere in the after revision, yet the acceptance surface (a
/// literal, often a defaults dict) still carries it. A key that vanished
/// from reads *and* literals is a clean removal and must not fire; a key
/// still read is live and must not fire. Unknown dynamic uses suppress
/// promotion rather than being treated as inert — they are simply outside
/// the extracted domains.
fn detect_config_inert(
    pair_index: usize,
    labels: &[String],
    rev_before: &RevisionContracts,
    rev_after: &RevisionContracts,
    drafts: &mut Vec<FindingDraft>,
) {
    let reads = |rev: &RevisionContracts| -> BTreeSet<String> {
        rev.files
            .iter()
            .flat_map(|file| {
                file.functions
                    .iter()
                    .flat_map(|function| function.config_keys.iter().cloned())
                    .chain(file.module_config_keys.keys().cloned())
            })
            .collect()
    };
    let before_reads = reads(rev_before);
    let after_reads = reads(rev_after);
    let replacements: BTreeSet<String> = after_reads
        .difference(&before_reads)
        .cloned()
        .collect();

    for key in before_reads.difference(&after_reads) {
        // The acceptance surface in the after revision: any function literal
        // or module-level literal still carrying the key.
        let mut holders: Vec<Holder> = Vec::new();
        for file in &rev_after.files {
            for function in &file.functions {
                if function.literals.contains(key) {
                    holders.push(Holder::Function {
                        path: file.path.clone(),
                        function: function.clone(),
                    });
                }
            }
            if let Some(span) = file.module_literals.get(key) {
                holders.push(Holder::Module {
                    path: file.path.clone(),
                    span: *span,
                    token: key.clone(),
                });
            }
        }
        if holders.is_empty() {
            continue;
        }
        let note = if replacements.is_empty() {
            None
        } else {
            Some(format!(
                "replacement key(s) live in the after revision: {}",
                replacements
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        };
        let dependent = holders
            .into_iter()
            .next()
            .expect("holders is non-empty");
        drafts.push(FindingDraft {
            rule: rule_names::ACCEPTED_CONFIG_INERT,
            pair_index,
            before_label: labels[pair_index].clone(),
            after_label: labels[pair_index + 1].clone(),
            path: dependent.path().to_path_buf(),
            owner_before: None,
            owner_after: None,
            dependent,
            domain: TokenDomain::ConfigKeys,
            stale: BTreeSet::from([key.clone()]),
            surviving: BTreeSet::from([key.clone()]),
            added: replacements.clone(),
            note,
            persistence: Persistence {
                revisions: 1,
                independent_edits: 0,
            },
        });
    }
}

/// Whether a function looks like a test: its name starts with `test_`, or
/// its file is a `test_*`/`*_test` module or lives under a `test`/`tests`
/// directory. Classification evidence only.
fn is_test_function(path: &Path, name: &str) -> bool {
    if name.starts_with("test_") {
        return true;
    }
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if stem.starts_with("test_") || stem.ends_with("_test") {
        return true;
    }
    path.components().any(|component| {
        let text = component.as_os_str().to_str().unwrap_or_default();
        text == "test" || text == "tests"
    })
}

/// `test-oracle-lag`: a migration fired in this pair while an unchanged test
/// still exercises the former representation. Reports what remains unproved
/// (no oracle covers the new owner); a test that changed in the same revision
/// is counter-evidence and does not fire.
fn detect_test_oracle_lag(
    pair_index: usize,
    labels: &[String],
    rev_before: &RevisionContracts,
    rev_after: &RevisionContracts,
    pair_start: usize,
    drafts: &mut Vec<FindingDraft>,
) {
    // Migrations detected in this pair so far (owned copies: drafts grows below).
    let migrations: Vec<FindingDraft> = drafts[pair_start..]
        .iter()
        .filter(|draft| {
            draft.rule == rule_names::OWNER_MOVED_CONSUMER_STALE
                || draft.rule == rule_names::PRODUCER_VERIFIER_SCHEMA_DRIFT
        })
        .cloned()
        .collect();
    for migration in migrations {
        for file in &rev_after.files {
            for function in &file.functions {
                if !is_test_function(&file.path, &function.name) {
                    continue;
                }
                // Unchanged since the before revision (exact bytes).
                let unchanged = rev_before
                    .files
                    .iter()
                    .find(|candidate| candidate.path == file.path)
                    .and_then(|candidate| {
                        candidate
                            .functions
                            .iter()
                            .find(|before_fn| before_fn.name == function.name)
                    })
                    .is_some_and(|before_fn| before_fn.fingerprint == function.fingerprint);
                if !unchanged {
                    continue;
                }
                let stale: BTreeSet<String> = migration
                    .surviving
                    .iter()
                    .filter(|token| migration.domain.tokens(function).contains(*token))
                    .cloned()
                    .collect();
                if stale.is_empty() {
                    continue;
                }
                drafts.push(FindingDraft {
                    rule: rule_names::TEST_ORACLE_LAG,
                    pair_index,
                    before_label: labels[pair_index].clone(),
                    after_label: labels[pair_index + 1].clone(),
                    path: migration.path.clone(),
                    owner_before: migration.owner_before.clone(),
                    owner_after: migration.owner_after.clone(),
                    dependent: Holder::Function {
                        path: file.path.clone(),
                        function: function.clone(),
                    },
                    domain: migration.domain,
                    stale,
                    surviving: migration.surviving.clone(),
                    added: migration.added.clone(),
                    note: None,
                    persistence: Persistence {
                        revisions: 1,
                        independent_edits: 0,
                    },
                });
            }
        }
    }
}

/// The holder in one revision matching `holder` by path and name/token.
fn locate(rev: &RevisionContracts, holder: &Holder) -> Option<Holder> {
    match holder {
        Holder::Function { path, function } => rev
            .files
            .iter()
            .find(|file| file.path == *path)?
            .functions
            .iter()
            .find(|candidate| candidate.name == function.name)
            .map(|candidate| Holder::Function {
                path: path.clone(),
                function: candidate.clone(),
            }),
        Holder::Module { path, token, .. } => rev
            .files
            .iter()
            .find(|file| file.path == *path)
            .and_then(|file| {
                file.module_literals.get(token).map(|span| Holder::Module {
                    path: path.clone(),
                    span: *span,
                    token: token.clone(),
                })
            }),
    }
}

/// Whether the holder's identity changed between two revisions. Appearing or
/// disappearing counts as a change.
fn changed_between(before: &RevisionContracts, after: &RevisionContracts, holder: &Holder) -> bool {
    match (locate(before, holder), locate(after, holder)) {
        (Some(left), Some(right)) => left.identity() != right.identity(),
        (None, None) => false,
        _ => true,
    }
}

/// Whether the draft's stale condition still holds in one revision.
fn stale_holds(draft: &FindingDraft, rev: &RevisionContracts) -> bool {
    if draft.rule == rule_names::ACCEPTED_CONFIG_INERT {
        let Some(key) = draft.stale.iter().next() else {
            return false;
        };
        let accepted = rev.files.iter().any(|file| {
            file.functions
                .iter()
                .any(|function| function.literals.contains(key))
                || file.module_literals.contains_key(key)
        });
        let read = rev.files.iter().any(|file| {
            file.functions
                .iter()
                .any(|function| function.config_keys.contains(key))
                || file.module_config_keys.contains_key(key)
        });
        return accepted && !read;
    }
    let Some(holder) = locate(rev, &draft.dependent) else {
        return false;
    };
    match &holder {
        Holder::Function { function, .. } => draft
            .stale
            .iter()
            .any(|token| draft.domain.tokens(function).contains(token)),
        Holder::Module { token, .. } => draft.stale.contains(token),
    }
}

/// Fill in `draft.persistence` by tracking the stale condition across the
/// revisions after the pair that detected it: how many revisions the stale
/// edge survives, and how often the owner and the stale dependent were
/// edited independently (co-change triage evidence from contract
/// fingerprints).
fn track_persistence(history: &ContractChangeHistory, draft: &mut FindingDraft) {
    let window = history.revisions.len();
    let mut survived = 1_u64;
    let mut independent = 0_u64;
    for index in draft.pair_index + 1..window.saturating_sub(1) {
        let rev = &history.revisions[index + 1];
        if !stale_holds(draft, rev) {
            break;
        }
        survived += 1;
        let dependent_changed = changed_between(&history.revisions[index], rev, &draft.dependent);
        let owner_changed = match &draft.owner_after {
            Some(owner) => {
                let holder = Holder::Function {
                    path: draft.path.clone(),
                    function: owner.clone(),
                };
                changed_between(&history.revisions[index], rev, &holder)
            }
            None => false,
        };
        if dependent_changed ^ owner_changed {
            independent += 1;
        }
    }
    draft.persistence = Persistence {
        revisions: survived,
        independent_edits: independent,
    };
}

/// The adoption-chain stage a rule's dependent represents, for summary
/// findings.
fn chain_stage(rule: &str) -> Option<(ContractEdgeKind, ContractRole)> {
    match rule {
        rule_names::OWNER_MOVED_CONSUMER_STALE => {
            Some((ContractEdgeKind::Consumes, ContractRole::Consumer))
        }
        rule_names::PRODUCER_VERIFIER_SCHEMA_DRIFT => {
            Some((ContractEdgeKind::Verifies, ContractRole::Verifier))
        }
        rule_names::TEST_ORACLE_LAG => {
            Some((ContractEdgeKind::Exercises, ContractRole::TestEntryPoint))
        }
        _ => None,
    }
}

/// The two-revision coverage gap: accumulation evidence requires a longer
/// window.
fn window_gap() -> CoverageGap {
    CoverageGap {
        provider: FactProvider::VcsHistory,
        capability: CapabilityLevel::Unknown,
        reason: "two-revision window; accumulation, rename/move history, and co-change \
                 evidence not analyzed"
            .to_string(),
    }
}

/// Assemble one `deslop.refactor-defect/1` finding from a draft. Every
/// finding is validated against the core invariants before it leaves the
/// analyzer.
fn build_finding(draft: &FindingDraft, window: usize) -> RefactorDefect {
    let mut coverage_gaps = Vec::new();
    if window == 2 {
        coverage_gaps.push(window_gap());
    }
    let stale_list = draft.stale.iter().map(String::as_str).collect::<Vec<_>>().join(", ");
    let added_list = draft.added.iter().map(String::as_str).collect::<Vec<_>>().join(", ");
    let domain = draft.domain.noun();
    let dependent_name = draft.dependent.display_name();

    let owner = draft
        .owner_before
        .as_ref()
        .zip(draft.owner_after.as_ref())
        .map(|(before, after)| OwnerMigration {
            before: ContractNodeRef {
                role: owner_role(draft.rule),
                path: draft.path.clone(),
                span: before.span,
                fingerprint: before.fingerprint.clone(),
                provider: FactProvider::TreeSitter,
                capability: CapabilityLevel::Complete,
            },
            after: ContractNodeRef {
                role: owner_role(draft.rule),
                path: draft.path.clone(),
                span: after.span,
                fingerprint: after.fingerprint.clone(),
                provider: FactProvider::TreeSitter,
                capability: CapabilityLevel::Complete,
            },
            match_evidence: vec![EntityMatchEvidence {
                kind: EntityMatchKind::PathSpan,
                detail: format!(
                    "function `{}` matched by name within {}",
                    after.name,
                    draft.path.display()
                ),
            }],
        });

    let (stale_edges, causal_path, evidence, suggested_verification, mut priority_inputs) =
        match draft.rule {
            rule_names::OWNER_MOVED_CONSUMER_STALE
            | rule_names::PRODUCER_VERIFIER_SCHEMA_DRIFT => {
                let family = if draft.rule == rule_names::OWNER_MOVED_CONSUMER_STALE {
                    Family::OwnerMovedConsumerStale
                } else {
                    Family::ProducerVerifierSchemaDrift
                };
                let (owner_role, dependent_role) = family.roles();
                let (owner_edge, dependent_edge) = family.edges();
                let owner_after = draft.owner_after.as_ref().expect("migration draft has owner");
                let owner_node = ContractNodeRef {
                    role: owner_role,
                    path: draft.path.clone(),
                    span: owner_after.span,
                    fingerprint: owner_after.fingerprint.clone(),
                    provider: FactProvider::TreeSitter,
                    capability: CapabilityLevel::Complete,
                };
                let dependent_node = draft.dependent.node_ref(dependent_role);
                (
                    draft
                        .stale
                        .iter()
                        .map(|token| ContractStep {
                            edge: dependent_edge,
                            node: dependent_node.clone(),
                            detail: format!(
                                "`{dependent_name}` still {domain}s `{token}` from the former owner"
                            ),
                        })
                        .collect::<Vec<_>>(),
                    vec![
                        ContractStep {
                            edge: owner_edge,
                            node: owner_node.clone(),
                            detail: format!(
                                "`{}` moved from {stale_list} to {added_list}",
                                owner_after.name
                            ),
                        },
                        ContractStep {
                            edge: dependent_edge,
                            node: dependent_node.clone(),
                            detail: format!(
                                "`{dependent_name}` is unchanged and still {domain}s {stale_list}"
                            ),
                        },
                    ],
                    vec![
                        EvidenceItem {
                            provider: FactProvider::TreeSitter,
                            detail: format!(
                                "`{}` {domain} set changed between revisions (removed {stale_list}, added {added_list})",
                                owner_after.name
                            ),
                            node: Some(owner_node),
                        },
                        EvidenceItem {
                            provider: FactProvider::TreeSitter,
                            detail: format!(
                                "`{dependent_name}` {domain} set is unchanged and contains {stale_list}"
                            ),
                            node: Some(dependent_node),
                        },
                    ],
                    family.suggested_verification(
                        &owner_after.name,
                        &dependent_name,
                        &draft.added,
                    ),
                    BTreeMap::from([
                        ("owner-change".to_string(), 1),
                        ("stale-edges".to_string(), draft.stale.len() as i64),
                    ]),
                )
            }
            rule_names::TEST_ORACLE_LAG => {
                let owner_after = draft.owner_after.as_ref().expect("oracle draft has owner");
                let test_node = draft.dependent.node_ref(ContractRole::TestEntryPoint);
                (
                    draft
                        .stale
                        .iter()
                        .map(|token| ContractStep {
                            edge: ContractEdgeKind::Exercises,
                            node: test_node.clone(),
                            detail: format!(
                                "test `{dependent_name}` is unchanged and still exercises `{token}` \
                                 from the former owner"
                            ),
                        })
                        .collect::<Vec<_>>(),
                    vec![
                        ContractStep {
                            edge: ContractEdgeKind::Produces,
                            node: draft.dependent.node_ref(ContractRole::Owner),
                            detail: format!(
                                "`{}` moved from {stale_list} to {added_list}",
                                owner_after.name
                            ),
                        },
                        ContractStep {
                            edge: ContractEdgeKind::Exercises,
                            node: test_node.clone(),
                            detail: format!(
                                "`{dependent_name}` is unchanged through the migration and still \
                                 {domain}s {stale_list}"
                            ),
                        },
                    ],
                    vec![
                        EvidenceItem {
                            provider: FactProvider::TreeSitter,
                            detail: format!(
                                "production contract changed while test `{dependent_name}` kept \
                                 identical bytes and still {domain}s {stale_list}"
                            ),
                            node: Some(test_node),
                        },
                    ],
                    format!(
                        "no oracle covers `{}`'s new representation ({added_list}): update \
                         `{dependent_name}` to exercise it, or pin the former representation as \
                         an explicit compatibility test",
                        owner_after.name
                    ),
                    BTreeMap::from([
                        ("owner-change".to_string(), 1),
                        ("stale-edges".to_string(), draft.stale.len() as i64),
                        ("missing-oracle".to_string(), 1),
                    ]),
                )
            }
            rule_names::ACCEPTED_CONFIG_INERT => {
                let key = draft.stale.iter().next().expect("config draft has a key");
                let holder_node = draft.dependent.node_ref(ContractRole::ConfigParameter);
                let mut verification = format!(
                    "remove `{key}` from the acceptance surface (`{dependent_name}`) or restore a \
                     behavioral consumer"
                );
                if !draft.added.is_empty() {
                    verification.push_str(&format!(
                        "; replacement key(s) {added_list} are live"
                    ));
                }
                let mut evidence = vec![
                    EvidenceItem {
                        provider: FactProvider::TreeSitter,
                        detail: format!(
                            "config key `{key}` was read in revision {} and has no config read in \
                             revision {}",
                            draft.before_label, draft.after_label
                        ),
                        node: None,
                    },
                    EvidenceItem {
                        provider: FactProvider::TreeSitter,
                        detail: format!(
                            "`{dependent_name}` still carries `{key}` in its literal surface"
                        ),
                        node: Some(holder_node.clone()),
                    },
                ];
                if let Some(note) = &draft.note {
                    evidence.push(EvidenceItem {
                        provider: FactProvider::TreeSitter,
                        detail: note.clone(),
                        node: None,
                    });
                }
                (
                    vec![ContractStep {
                        edge: ContractEdgeKind::Configures,
                        node: holder_node.clone(),
                        detail: format!(
                            "`{dependent_name}` still accepts config key `{key}`, but nothing in \
                             revision {} reads it",
                            draft.after_label
                        ),
                    }],
                    vec![
                        ContractStep {
                            edge: ContractEdgeKind::Reads,
                            node: holder_node.clone(),
                            detail: format!(
                                "revision {} read `{key}`; revision {} does not",
                                draft.before_label, draft.after_label
                            ),
                        },
                        ContractStep {
                            edge: ContractEdgeKind::Configures,
                            node: holder_node,
                            detail: format!(
                                "the acceptance surface still carries `{key}`"
                            ),
                        },
                    ],
                    evidence,
                    verification,
                    BTreeMap::from([("stale-edges".to_string(), 1)]),
                )
            }
            _ => unreachable!("unknown draft rule {}", draft.rule),
        };

    priority_inputs.insert("persistence".to_string(), draft.persistence.revisions as i64);
    priority_inputs.insert(
        "independent-churn".to_string(),
        draft.persistence.independent_edits as i64,
    );

    let finding = RefactorDefect {
        schema: REFACTOR_DEFECT_SCHEMA.to_string(),
        rule: draft.rule.to_string(),
        revisions: RevisionPair {
            before: draft.before_label.clone(),
            after: draft.after_label.clone(),
        },
        owner,
        stale_edges,
        causal_path,
        evidence,
        counter_evidence: Vec::new(),
        coverage_gaps,
        persistence: draft.persistence,
        priority_inputs,
        safety: SafetyClass::NeverAuto,
        suggested_verification,
    };
    debug_assert!(finding.validate().is_ok());
    finding
}

/// The owner role for a rule's owner migration node.
fn owner_role(rule: &str) -> ContractRole {
    match rule {
        rule_names::PRODUCER_VERIFIER_SCHEMA_DRIFT => ContractRole::Producer,
        _ => ContractRole::Owner,
    }
}

/// Emit one `adoption-chain-incomplete` summary per owner migration shared
/// by at least two detector families. Summaries present the missing chain
/// stages; they travel in [`RefactorRiskReport::summaries`] so they never
/// duplicate the underlying findings in baselines or severity counts.
fn build_summaries(drafts: &[FindingDraft], window: usize) -> Vec<RefactorDefect> {
    let mut groups: BTreeMap<(usize, String, PathBuf), Vec<&FindingDraft>> = BTreeMap::new();
    for draft in drafts {
        let Some(owner_name) = draft.owner_name() else {
            continue;
        };
        groups
            .entry((draft.pair_index, owner_name.to_string(), draft.path.clone()))
            .or_default()
            .push(draft);
    }
    let mut summaries = Vec::new();
    for ((_pair_index, owner_name, path), members) in groups {
        let rules: BTreeSet<&str> = members.iter().map(|member| member.rule).collect();
        if rules.len() < 2 {
            continue;
        }
        let first = members[0];
        let owner_before = first.owner_before.as_ref().expect("summary owner");
        let owner_after = first.owner_after.as_ref().expect("summary owner");
        let role = owner_role(first.rule);
        let owner_node = |function: &ContractFunction| ContractNodeRef {
            role,
            path: path.clone(),
            span: function.span,
            fingerprint: function.fingerprint.clone(),
            provider: FactProvider::TreeSitter,
            capability: CapabilityLevel::Complete,
        };
        let mut stale_edges = Vec::new();
        for rule in &rules {
            let Some((edge, stage_role)) = chain_stage(rule) else {
                continue;
            };
            let member = members
                .iter()
                .find(|member| member.rule == *rule)
                .expect("rule came from a member");
            stale_edges.push(ContractStep {
                edge,
                node: member.dependent.node_ref(stage_role),
                detail: format!(
                    "chain stage `{rule}` incomplete: `{}` is still bound to the former owner",
                    member.dependent.display_name()
                ),
            });
        }
        let mut coverage_gaps = Vec::new();
        if window == 2 {
            coverage_gaps.push(window_gap());
        }
        let persistence = members
            .iter()
            .map(|member| member.persistence)
            .max_by_key(|persistence| persistence.revisions)
            .unwrap_or(Persistence {
                revisions: 1,
                independent_edits: 0,
            });
        let summary = RefactorDefect {
            schema: REFACTOR_DEFECT_SCHEMA.to_string(),
            rule: rule_names::ADOPTION_CHAIN_INCOMPLETE.to_string(),
            revisions: RevisionPair {
                before: first.before_label.clone(),
                after: first.after_label.clone(),
            },
            owner: Some(OwnerMigration {
                before: owner_node(owner_before),
                after: owner_node(owner_after),
                match_evidence: vec![EntityMatchEvidence {
                    kind: EntityMatchKind::PathSpan,
                    detail: format!(
                        "function `{owner_name}` matched by name within {}",
                        path.display()
                    ),
                }],
            }),
            stale_edges,
            causal_path: vec![ContractStep {
                edge: ContractEdgeKind::Produces,
                node: owner_node(owner_after),
                detail: format!(
                    "`{owner_name}` changed owners; {} adoption-chain stages did not follow",
                    rules.len()
                ),
            }],
            evidence: vec![EvidenceItem {
                provider: FactProvider::TreeSitter,
                detail: format!(
                    "summarizes {} findings across {} detector families over owner \
                     `{owner_name}`; emitted once and excluded from finding counts",
                    members.len(),
                    rules.len()
                ),
                node: Some(owner_node(owner_after)),
            }],
            counter_evidence: Vec::new(),
            coverage_gaps,
            persistence,
            priority_inputs: BTreeMap::from([
                ("families".to_string(), rules.len() as i64),
                (
                    "stale-edges".to_string(),
                    members.iter().map(|member| member.stale.len() as i64).sum(),
                ),
                ("persistence".to_string(), persistence.revisions as i64),
                (
                    "independent-churn".to_string(),
                    persistence.independent_edits as i64,
                ),
            ]),
            safety: SafetyClass::NeverAuto,
            suggested_verification: format!(
                "complete the adoption chain for `{owner_name}`: reconcile {}",
                rules
                    .iter()
                    .map(|rule| format!("`{rule}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        debug_assert!(summary.validate().is_ok());
        summaries.push(summary);
    }
    summaries
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_parse::{ProjectSnapshotBuilder, RepositoryId};

    fn analysis(files: &[(&str, &[u8])]) -> Arc<ProjectAnalysis> {
        let temp = tempfile::tempdir().unwrap();
        let mut builder = ProjectSnapshotBuilder::new(
            temp.path(),
            RepositoryId::explicit("refactor-test").unwrap(),
        )
        .unwrap();
        for (path, source) in files {
            builder = builder.with_overlay(path, source.to_vec()).unwrap();
        }
        ProjectAnalysis::build(builder.build().unwrap()).unwrap()
    }

    fn compare(before: &[(&str, &[u8])], after: &[(&str, &[u8])]) -> RefactorRiskReport {
        analyze_refactor_risk(
            ("before".to_string(), analysis(before)),
            ("after".to_string(), analysis(after)),
        )
        .unwrap()
    }

    const SCORER_BEFORE: &[u8] = br#"class Scorer:
    def __init__(self, model):
        self.model = model

    def decide(self, candidates):
        return max(candidates, key=lambda c: self.model.raw_score(c))

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;

    #[test]
    fn owner_moved_consumer_stale_fires() {
        let after = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;
        let report = compare(&[("scoring.py", SCORER_BEFORE)], &[("scoring.py", after)]);
        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert_eq!(finding.rule, rule_names::OWNER_MOVED_CONSUMER_STALE);
        assert_eq!(finding.safety, SafetyClass::NeverAuto);
        finding.validate().unwrap();
        assert_eq!(finding.stale_edges.len(), 1);
        assert!(finding.stale_edges[0].detail.contains("model.raw_score"));
        assert!(!finding.coverage_gaps.is_empty());
    }

    #[test]
    fn full_adoption_does_not_fire() {
        let after = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.posterior.committed_score(candidate)
"#;
        let report = compare(&[("scoring.py", SCORER_BEFORE)], &[("scoring.py", after)]);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn pure_rename_does_not_fire() {
        let before = br#"def decide(candidates):
    return max(candidates)


def rank(candidates):
    return decide(candidates)
"#;
        let after = br#"def choose(candidates):
    return max(candidates)


def rank(candidates):
    return choose(candidates)
"#;
        let report = compare(&[("scoring.py", before)], &[("scoring.py", after)]);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn compatibility_adapter_does_not_fire() {
        let after = br#"class ScoreAdapter:
    def __init__(self, posterior):
        self.posterior = posterior

    def legacy_score(self, candidate):
        return self.posterior.committed_score(candidate)


class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.adapter.legacy_score(candidate)
"#;
        let report = compare(&[("scoring.py", SCORER_BEFORE)], &[("scoring.py", after)]);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn producer_verifier_schema_drift_fires() {
        let before = br#"def build_manifest(run):
    return {"run_id": run.id, "epochs": run.epochs, "metric": run.final_loss}


def validate_manifest(manifest):
    required = {"run_id", "epochs", "metric"}
    return required <= manifest.keys()
"#;
        let after = br#"def build_manifest(run):
    return {"run_id": run.id, "epochs": run.epochs, "seed": run.seed, "metrics": {"loss": run.final_loss}}


def validate_manifest(manifest):
    required = {"run_id", "epochs", "metric"}
    return required <= manifest.keys()
"#;
        let report = compare(&[("manifest.py", before)], &[("manifest.py", after)]);
        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert_eq!(finding.rule, rule_names::PRODUCER_VERIFIER_SCHEMA_DRIFT);
        finding.validate().unwrap();
        assert!(finding.stale_edges[0].detail.contains("metric"));
    }

    #[test]
    fn coherent_schema_migration_does_not_fire() {
        let before = br#"def build_manifest(run):
    return {"run_id": run.id, "metric": run.final_loss}


def validate_manifest(manifest):
    required = {"run_id", "metric"}
    return required <= manifest.keys()
"#;
        let after = br#"def build_manifest(run):
    return {"run_id": run.id, "metrics": {"loss": run.final_loss}}


def validate_manifest(manifest):
    required = {"run_id", "metrics"}
    return required <= manifest.keys()
"#;
        let report = compare(&[("manifest.py", before)], &[("manifest.py", after)]);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn julia_owner_moved_consumer_stale_fires() {
        let before = b"decide(model, candidates) = argmax(c -> raw_score(model, c), candidates)\n\npublic_score(model, c) = raw_score(model, c)\n";
        let after = b"decide(posterior, candidates) = commit(posterior, candidates)\n\npublic_score(model, c) = raw_score(model, c)\n";
        let report = compare(&[("scoring.jl", before)], &[("scoring.jl", after)]);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule, rule_names::OWNER_MOVED_CONSUMER_STALE);
    }

    const CONFIG_BEFORE: &[u8] = br#"import os

DEFAULTS = {"THRESHOLD": 0.5}


def load_config():
    config = dict(DEFAULTS)
    config["THRESHOLD"] = os.environ["THRESHOLD"]
    return config
"#;

    #[test]
    fn accepted_config_inert_fires_when_retired_key_stays_accepted() {
        let after = br#"import os

DEFAULTS = {"THRESHOLD": 0.5}


def load_config():
    config = dict(DEFAULTS)
    config["THRESHOLD"] = os.environ["THRESHOLD_V2"]
    return config
"#;
        let report = compare(&[("config.py", CONFIG_BEFORE)], &[("config.py", after)]);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::ACCEPTED_CONFIG_INERT)
            .expect("accepted-config-inert should fire");
        finding.validate().unwrap();
        assert!(finding.owner.is_none());
        assert!(finding.stale_edges[0].detail.contains("THRESHOLD"));
        assert!(
            finding
                .evidence
                .iter()
                .any(|item| item.detail.contains("THRESHOLD_V2")),
            "replacement key should be noted: {:?}",
            finding.evidence
        );
        assert!(finding.suggested_verification.contains("THRESHOLD_V2"));
    }

    #[test]
    fn clean_config_removal_does_not_fire() {
        let after = br#"import os

DEFAULTS = {}


def load_config():
    return dict(DEFAULTS)
"#;
        let report = compare(&[("config.py", CONFIG_BEFORE)], &[("config.py", after)]);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.rule != rule_names::ACCEPTED_CONFIG_INERT),
            "clean removal must not fire: {:?}",
            report.findings
        );
    }

    #[test]
    fn module_level_acceptance_fires_config_inert() {
        let before = br#"import os

LIMIT = os.environ["LIMIT"]
"#;
        let after = br#"DEFAULTS = {"LIMIT": 10}
"#;
        let report = compare(&[("settings.py", before)], &[("settings.py", after)]);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::ACCEPTED_CONFIG_INERT)
            .expect("module-level acceptance should fire");
        finding.validate().unwrap();
        assert!(finding.stale_edges[0].detail.contains("LIMIT"));
    }

    #[test]
    fn test_oracle_lag_fires_for_unchanged_test() {
        let test = br#"from scoring import Scorer


def test_public_score(scorer, candidate):
    assert scorer.public_score(candidate) == model.raw_score(candidate)
"#;
        let after = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;
        let report = compare(
            &[("scoring.py", SCORER_BEFORE), ("test_scoring.py", test)],
            &[("scoring.py", after), ("test_scoring.py", test)],
        );
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::TEST_ORACLE_LAG)
            .expect("test-oracle-lag should fire");
        finding.validate().unwrap();
        assert!(finding.suggested_verification.contains("no oracle"));
        assert_eq!(report.summaries.len(), 1);
        assert_eq!(
            report.summaries[0].rule,
            rule_names::ADOPTION_CHAIN_INCOMPLETE
        );
        report.summaries[0].validate().unwrap();
    }

    #[test]
    fn updated_test_does_not_fire_oracle_lag() {
        let test_before = br#"def test_public_score():
    assert public_score(c) == model.raw_score(c)
"#;
        let test_after = br#"def test_public_score():
    assert public_score(c) == posterior.committed_score(c)
"#;
        let after = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;
        let report = compare(
            &[("scoring.py", SCORER_BEFORE), ("test_scoring.py", test_before)],
            &[("scoring.py", after), ("test_scoring.py", test_after)],
        );
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.rule != rule_names::TEST_ORACLE_LAG),
            "a coherently updated test is counter-evidence: {:?}",
            report.findings
        );
    }

    #[test]
    fn persistence_tracks_stale_edge_across_window() {
        let rev2 = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;
        // rev3 edits the owner alone (independent churn) while the stale
        // consumer is untouched.
        let rev3 = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        committed = self.posterior.commit(candidates)
        return committed

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;
        let report = analyze_refactor_window(vec![
            ("rev1".to_string(), analysis(&[("scoring.py", SCORER_BEFORE)])),
            ("rev2".to_string(), analysis(&[("scoring.py", rev2)])),
            ("rev3".to_string(), analysis(&[("scoring.py", rev3)])),
        ])
        .unwrap();
        assert_eq!(report.revisions, vec!["rev1", "rev2", "rev3"]);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::OWNER_MOVED_CONSUMER_STALE)
            .expect("migration should fire on rev1->rev2");
        assert_eq!(finding.persistence.revisions, 2);
        assert_eq!(finding.persistence.independent_edits, 1);
        assert_eq!(finding.priority_inputs["persistence"], 2);
        assert_eq!(finding.priority_inputs["independent-churn"], 1);
        // A window longer than two revisions analyzes accumulation, so the
        // two-revision coverage gap must be gone.
        assert!(finding.coverage_gaps.is_empty());
    }

    #[test]
    fn repaired_edge_stops_persistence() {
        let rev2 = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;
        let rev3 = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.posterior.committed_score(candidate)
"#;
        let report = analyze_refactor_window(vec![
            ("rev1".to_string(), analysis(&[("scoring.py", SCORER_BEFORE)])),
            ("rev2".to_string(), analysis(&[("scoring.py", rev2)])),
            ("rev3".to_string(), analysis(&[("scoring.py", rev3)])),
        ])
        .unwrap();
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::OWNER_MOVED_CONSUMER_STALE)
            .expect("migration should fire on rev1->rev2");
        assert_eq!(finding.persistence.revisions, 1);
        assert_eq!(finding.persistence.independent_edits, 0);
    }

    #[test]
    fn window_requires_two_revisions() {
        let report = analyze_refactor_window(vec![(
            "only".to_string(),
            analysis(&[("scoring.py", SCORER_BEFORE)]),
        )]);
        assert!(report.is_err());
    }
}
