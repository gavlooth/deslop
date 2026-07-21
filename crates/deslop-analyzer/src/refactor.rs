//! Refactor-defect accumulation detection over a revision window.
//!
//! Design: `docs/REFACTOR_DEFECT_ACCUMULATION.md`. This is the Phase 1 slice:
//! review-only `owner-moved-consumer-stale` and
//! `producer-verifier-schema-drift` over a two-revision window, built on
//! [`ContractChangeHistory`] facts extracted from exact `ProjectAnalysis`
//! snapshots. All findings are `NeverAuto`: the analysis diagnoses an
//! incomplete contract migration and suggests a verification; it never
//! proposes a rewrite.
//!
//! Detection discipline:
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
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use deslop_core::SafetyClass;
use deslop_core::refactor_defect::{
    CapabilityLevel, ContractEdgeKind, ContractNodeRef, ContractRole, ContractStep, CoverageGap,
    EntityMatchEvidence, EntityMatchKind, EvidenceItem, FactProvider, OwnerMigration, Persistence,
    REFACTOR_DEFECT_SCHEMA, RefactorDefect, RevisionPair, rule_names,
};
use deslop_parse::{
    ContractChangeHistory, ContractFunction, DiscoveryPolicy, FactCoverage, FileContracts,
    ProjectAnalysis, ProjectSnapshotPlanner, ProjectSnapshotRequest, RepositorySpec, RootSpec,
    ScopeSpec,
};
use serde::Serialize;

/// Wire schema identifier for a refactor-risk report over one revision pair.
pub const REFACTOR_RISK_SCHEMA: &str = "deslop.refactor-risk/1";

/// The output of one refactor-risk comparison.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RefactorRiskReport {
    pub schema: String,
    pub before: String,
    pub after: String,
    pub coverage: FactCoverage,
    pub coverage_reasons: Vec<String>,
    pub findings: Vec<RefactorDefect>,
}

/// Compare two directory snapshots (`--from`, `--to`) and report
/// refactor-defect findings.
pub fn refactor_risk_paths(from: &Path, to: &Path) -> Result<RefactorRiskReport> {
    let before = analysis_for(from)?;
    let after = analysis_for(to)?;
    analyze_refactor_risk(
        (from.display().to_string(), before),
        (to.display().to_string(), after),
    )
}

/// Compare two exact analyses under caller-supplied revision labels.
pub fn analyze_refactor_risk(
    before: (String, Arc<ProjectAnalysis>),
    after: (String, Arc<ProjectAnalysis>),
) -> Result<RefactorRiskReport> {
    let before_label = before.0.clone();
    let after_label = after.0.clone();
    let history = ContractChangeHistory::from_analyses(&[before, after])
        .context("build contract change history")?;
    let revisions = &history.revisions;
    let (rev_before, rev_after) = (&revisions[0], &revisions[1]);
    let mut findings = Vec::new();
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
            &before_label,
            &after_label,
            file_before,
            file_after,
            &mut findings,
        );
        detect_family(
            Family::ProducerVerifierSchemaDrift,
            &before_label,
            &after_label,
            file_before,
            file_after,
            &mut findings,
        );
    }
    Ok(RefactorRiskReport {
        schema: REFACTOR_RISK_SCHEMA.to_string(),
        before: before_label,
        after: after_label,
        coverage: history.coverage,
        coverage_reasons: history.reasons,
        findings,
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

/// The two detector families shipped in this slice. Both are the same
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

    fn tokens<'f>(self, function: &'f ContractFunction) -> &'f BTreeSet<String> {
        match self {
            Self::OwnerMovedConsumerStale => &function.references,
            Self::ProducerVerifierSchemaDrift => &function.literals,
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

    fn token_domain(self) -> &'static str {
        match self {
            Self::OwnerMovedConsumerStale => "reference",
            Self::ProducerVerifierSchemaDrift => "schema field",
        }
    }

    fn suggested_verification(
        self,
        owner: &str,
        consumer: &str,
        added: &BTreeSet<&str>,
    ) -> String {
        let added = added.iter().copied().collect::<Vec<_>>().join(", ");
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

/// Index a file's functions by name.
fn functions_by_name(file: &FileContracts) -> BTreeMap<&str, &ContractFunction> {
    file.functions
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect()
}

/// Run one detector family over one matched file pair.
fn detect_family(
    family: Family,
    before_label: &str,
    after_label: &str,
    file_before: &FileContracts,
    file_after: &FileContracts,
    findings: &mut Vec<RefactorDefect>,
) {
    let before_by_name = functions_by_name(file_before);
    let after_by_name = functions_by_name(file_after);
    let global_after: BTreeSet<&str> = file_after
        .functions
        .iter()
        .flat_map(|function| family.tokens(function).iter().map(String::as_str))
        .collect();

    for (name, owner_before) in &before_by_name {
        let Some(owner_after) = after_by_name.get(name) else {
            continue;
        };
        let tokens_before = family.tokens(owner_before);
        let tokens_after = family.tokens(owner_after);
        let removed: BTreeSet<&str> = tokens_before
            .iter()
            .map(String::as_str)
            .filter(|token| !tokens_after.contains(*token))
            .collect();
        let added: BTreeSet<&str> = tokens_after
            .iter()
            .map(String::as_str)
            .filter(|token| !tokens_before.contains(*token))
            .collect();
        // A migration both loses and gains tokens; pure removals or additions
        // are deletions/rewrites, not owner moves.
        if removed.is_empty() || added.is_empty() {
            continue;
        }
        // The former representation must survive somewhere in the after
        // revision; otherwise this is a rename or rewrite, not a stale
        // consumer (rename/move negative cases).
        let surviving: BTreeSet<&str> = removed
            .iter()
            .copied()
            .filter(|token| global_after.contains(token))
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
            if family.tokens(dependent_before) != family.tokens(dependent_after) {
                continue;
            }
            let stale: BTreeSet<&str> = surviving
                .iter()
                .copied()
                .filter(|token| family.tokens(dependent_before).contains(*token))
                .collect();
            if stale.is_empty() {
                continue;
            }
            findings.push(build_finding(
                family,
                before_label,
                after_label,
                &file_before.path,
                owner_before,
                owner_after,
                dependent_before,
                dependent_after,
                &stale,
                &added,
            ));
        }
    }
}

/// Assemble one `deslop.refactor-defect/1` finding. Panics are impossible by
/// construction, but every finding is still validated against the core
/// invariants before it leaves the analyzer.
fn build_finding(
    family: Family,
    before_label: &str,
    after_label: &str,
    path: &Path,
    owner_before: &ContractFunction,
    owner_after: &ContractFunction,
    _dependent_before: &ContractFunction,
    dependent_after: &ContractFunction,
    stale: &BTreeSet<&str>,
    added: &BTreeSet<&str>,
) -> RefactorDefect {
    let (owner_role, dependent_role) = family.roles();
    let (owner_edge, dependent_edge) = family.edges();
    let node_ref = |role: ContractRole, function: &ContractFunction| ContractNodeRef {
        role,
        path: path.to_path_buf(),
        span: function.span,
        fingerprint: function.fingerprint.clone(),
        provider: FactProvider::TreeSitter,
        capability: CapabilityLevel::Complete,
    };
    let stale_list = stale.iter().copied().collect::<Vec<_>>().join(", ");
    let added_list = added.iter().copied().collect::<Vec<_>>().join(", ");
    let domain = family.token_domain();
    let finding = RefactorDefect {
        schema: REFACTOR_DEFECT_SCHEMA.to_string(),
        rule: family.rule().to_string(),
        revisions: RevisionPair {
            before: before_label.to_string(),
            after: after_label.to_string(),
        },
        owner: Some(OwnerMigration {
            before: node_ref(owner_role, owner_before),
            after: node_ref(owner_role, owner_after),
            match_evidence: vec![EntityMatchEvidence {
                kind: EntityMatchKind::PathSpan,
                detail: format!(
                    "function `{}` matched by name within {}",
                    owner_before.name,
                    path.display()
                ),
            }],
        }),
        stale_edges: stale
            .iter()
            .map(|token| ContractStep {
                edge: dependent_edge,
                node: node_ref(dependent_role, dependent_after),
                detail: format!(
                    "`{}` still {domain}s `{token}` from the former owner",
                    dependent_after.name
                ),
            })
            .collect(),
        causal_path: vec![
            ContractStep {
                edge: owner_edge,
                node: node_ref(owner_role, owner_after),
                detail: format!(
                    "`{}` moved from {stale_list} to {added_list}",
                    owner_after.name
                ),
            },
            ContractStep {
                edge: dependent_edge,
                node: node_ref(dependent_role, dependent_after),
                detail: format!(
                    "`{}` is unchanged and still {domain}s {stale_list}",
                    dependent_after.name
                ),
            },
        ],
        evidence: vec![
            EvidenceItem {
                provider: FactProvider::TreeSitter,
                detail: format!(
                    "`{}` {domain} set changed between revisions (removed {stale_list}, added {added_list})",
                    owner_after.name
                ),
                node: Some(node_ref(owner_role, owner_after)),
            },
            EvidenceItem {
                provider: FactProvider::TreeSitter,
                detail: format!(
                    "`{}` {domain} set is unchanged and contains {stale_list}",
                    dependent_after.name
                ),
                node: Some(node_ref(dependent_role, dependent_after)),
            },
        ],
        counter_evidence: Vec::new(),
        coverage_gaps: vec![CoverageGap {
            provider: FactProvider::VcsHistory,
            capability: CapabilityLevel::Unknown,
            reason: "two-revision window; accumulation, rename/move history, and co-change \
                     evidence not analyzed"
                .to_string(),
        }],
        persistence: Persistence {
            revisions: 1,
            independent_edits: 0,
        },
        priority_inputs: BTreeMap::from([
            ("owner-change".to_string(), 1),
            ("stale-edges".to_string(), stale.len() as i64),
        ]),
        safety: SafetyClass::NeverAuto,
        suggested_verification: family.suggested_verification(
            &owner_after.name,
            &dependent_after.name,
            added,
        ),
    };
    debug_assert!(finding.validate().is_ok());
    finding
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
}
