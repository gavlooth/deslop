//! Target-bound revision-aware cleanup proposal integration.
//!
//! The analyzer owns attribution; this module only adapts target findings into the existing
//! validated finding work-order and shared work-order flow.  It does not execute commands or
//! grant imported history authority.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use deslop_analyzer::AnalyzerConfig;
use deslop_analyzer::revision_cleanup::{
    FindingDisposition, RevisionComparison, compare_paths_with_scope,
};
use deslop_core::{Finding, Span};
use serde::Serialize;

use crate::{SharedWorkOrder, WorkOrder, propose_work_orders_with_exclusions};

pub const REVISION_CLEANUP_PROPOSAL_SCHEMA: &str = "deslop.revision-cleanup-proposal/1";

/// A target-bound proposal batch.  The comparison remains embedded so an incomparable history
/// cannot be mistaken for a clean/no-op comparison.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionCleanupProposalBatch {
    pub schema: String,
    pub task_requirements: String,
    pub comparison: RevisionComparison,
    pub proposals: Vec<SharedWorkOrder>,
    pub affected_identities: Vec<String>,
    pub declared_read_set: Vec<String>,
    pub preconditions: Vec<String>,
    pub verification_plan: Vec<String>,
}

/// Compare base and target, then adapt target-bound existing findings into validated shared work
/// orders.  The target scan is performed through the same proposal implementation used by the
/// ordinary CLI/MCP flow, so context IDs, revision guards, and read resources remain canonical.
pub fn revision_cleanup_proposals(
    base: &Path,
    target: &Path,
    scope: &[PathBuf],
    config: AnalyzerConfig,
    task_requirements: impl Into<String>,
) -> Result<RevisionCleanupProposalBatch> {
    let task_requirements = task_requirements.into();
    if task_requirements.trim().is_empty() {
        bail!("task requirements must not be empty");
    }
    let comparison = compare_paths_with_scope(base, target, scope, config.clone())?;
    let batch = propose_work_orders_with_exclusions(target, scope, config.clone(), &[])?;
    let canonical_target = target.canonicalize()?;
    ensure_target_scan_matches(&comparison, &batch, &canonical_target, &config)?;
    let incomparable = !comparison.comparable;
    let mut proposals = Vec::new();
    let mut affected = BTreeSet::new();
    let mut reads = BTreeSet::new();
    let mut preconditions = BTreeSet::new();
    let mut verification = BTreeSet::new();

    for order in batch.work_orders {
        if !incomparable && !order_is_attributed(&order, &comparison, &canonical_target) {
            continue;
        }
        let mut order = order;
        order.instruction = format!(
            "Task requirements: {}\n\n{}",
            task_requirements, order.instruction
        );
        let target_order = order.clone();
        let shared = SharedWorkOrder::from_finding_order(target_order)?;
        affected.insert(format!(
            "{}:{}-{}:{}-{}",
            order.path.display(),
            order.region.start_line,
            order.region.end_line,
            order.region.start_byte,
            order.region.end_byte
        ));
        for source in &batch.context.sources {
            reads.insert(format!(
                "source:{}:{}",
                source.path.display(),
                source.revision_guard
            ));
        }
        for resource in shared
            .access()
            .reads
            .iter()
            .chain(shared.access().requires.iter())
        {
            reads.insert(format!("{:?}:{}", resource.kind, resource.identity));
        }
        for finding in &order.findings {
            if let Some(precondition) = &finding.precondition {
                preconditions.insert(precondition.clone());
            }
        }
        verification.insert(
            "re-run declared checks against the exact target revision and full read set"
                .to_string(),
        );
        verification.insert(
            "protect tests, checks, error handling, and public contracts during review".to_string(),
        );
        proposals.push(shared);
    }

    if incomparable {
        preconditions.insert(
            "base and target contexts are incomparable; do not interpret attribution as a delta"
                .to_string(),
        );
    }
    if proposals.is_empty() && comparison.comparable {
        verification.insert(
            "no introduced or uncertain target finding was eligible for proposal".to_string(),
        );
    }
    Ok(RevisionCleanupProposalBatch {
        schema: REVISION_CLEANUP_PROPOSAL_SCHEMA.to_string(),
        task_requirements,
        comparison,
        proposals,
        affected_identities: affected.into_iter().collect(),
        declared_read_set: reads.into_iter().collect(),
        preconditions: preconditions.into_iter().collect(),
        verification_plan: verification.into_iter().collect(),
    })
}

fn ensure_target_scan_matches(
    comparison: &RevisionComparison,
    batch: &crate::ProposalBatch,
    target: &Path,
    config: &AnalyzerConfig,
) -> Result<()> {
    let mut expected_config = config.snapshot();
    crate::normalize_analyzer_paths(target, &mut expected_config)?;
    crate::canonicalize_analyzer(&mut expected_config);
    if batch.context.analyzer != expected_config {
        bail!("target proposal scan analyzer config drifted from comparison");
    }
    let actual = batch
        .analysis
        .files()
        .map(|file| {
            (
                normalize_target_path(
                    target,
                    &batch.analysis.snapshot().root().join(&file.key().path),
                ),
                blake3::hash(file.source()).to_hex().to_string(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if actual != comparison.target_sources {
        let differing: Vec<_> = comparison
            .target_sources
            .keys()
            .chain(actual.keys())
            .filter(|path| comparison.target_sources.get(*path) != actual.get(*path))
            .take(8)
            .map(|path| (path, comparison.target_sources.get(path), actual.get(path)))
            .collect();
        bail!("target proposal scan source bytes drifted from comparison: {differing:?}");
    }
    Ok(())
}

fn normalize_target_path(target: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(target)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

fn order_is_attributed(order: &WorkOrder, comparison: &RevisionComparison, root: &Path) -> bool {
    comparison.findings.iter().any(|attributed| {
        matches!(
            attributed.disposition,
            FindingDisposition::Introduced | FindingDisposition::Uncertain
        ) && attributed
            .finding
            .as_ref()
            .is_some_and(|finding| finding_matches_order(finding, order, root))
    })
}

fn finding_matches_order(finding: &Finding, order: &WorkOrder, root: &Path) -> bool {
    finding.path == normalize_target_path(root, &order.path)
        && spans_overlap(
            finding.span,
            Span::new(
                order.region.start_line,
                order.region.end_line,
                order.region.start_byte,
                order.region.end_byte,
            ),
        )
}

fn spans_overlap(left: Span, right: Span) -> bool {
    left.start_byte < right.end_byte && right.start_byte < left.end_byte
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn nested_target() -> Result<tempfile::TempDir> {
        let target = tempfile::tempdir()?;
        fs::create_dir(target.path().join("src"))?;
        fs::write(
            target.path().join("src/value.rs"),
            "fn value() -> i32 {\n    return 1;\n}\n",
        )?;
        Ok(target)
    }

    #[test]
    fn target_scan_rejects_source_drift_in_nested_scope() -> Result<()> {
        let target = nested_target()?;
        let root = target.path().canonicalize()?;
        let scope = [PathBuf::from("src")];
        let config = AnalyzerConfig::default();
        let comparison = compare_paths_with_scope(&root, &root, &scope, config.clone())?;
        let batch = propose_work_orders_with_exclusions(&root, &scope, config.clone(), &[])?;
        ensure_target_scan_matches(&comparison, &batch, &root, &config)?;

        fs::write(
            root.join("src/value.rs"),
            "fn value() -> i32 {\n    return 2;\n}\n",
        )?;
        let changed = propose_work_orders_with_exclusions(&root, &scope, config.clone(), &[])?;
        ensure_target_scan_matches(&comparison, &changed, &root, &config)
            .expect_err("a later target scan must not authorize stale comparison bytes");
        Ok(())
    }

    #[test]
    fn target_scan_rejects_config_drift_even_with_identical_sources() -> Result<()> {
        let target = nested_target()?;
        let root = target.path().canonicalize()?;
        let scope = [PathBuf::from("src")];
        let config = AnalyzerConfig::default();
        let comparison = compare_paths_with_scope(&root, &root, &scope, config.clone())?;
        let batch = propose_work_orders_with_exclusions(&root, &scope, config.clone(), &[])?;
        ensure_target_scan_matches(&comparison, &batch, &root, &config)?;

        let changed_config = AnalyzerConfig {
            min_duplication_tokens: config.min_duplication_tokens + 1,
            ..config.clone()
        };
        let changed = propose_work_orders_with_exclusions(&root, &scope, changed_config, &[])?;
        ensure_target_scan_matches(&comparison, &changed, &root, &config)
            .expect_err("unchanged source bytes must not bypass analyzer config guards");
        Ok(())
    }
}
