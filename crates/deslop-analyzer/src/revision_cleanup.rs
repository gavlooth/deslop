//! Revision-aware, intent-neutral cleanup comparison.
//!
//! This module compares exact analyzer snapshots.  It deliberately treats a snapshot as
//! evidence about source and findings only: it never infers why a change was made.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use deslop_core::{FileReport, Finding, Lang};
use serde::{Deserialize, Serialize};

use crate::{AnalyzerConfig, AnalyzerConfigSnapshot, ScanContext};

pub const REVISION_CLEANUP_SCHEMA: &str = "deslop.revision-cleanup/1";
pub const REVISION_CLEANUP_CONTEXT_SCHEMA: &str = "deslop.revision-cleanup-context/1";

/// How source bytes were materialized.  The materialization label is part of comparability;
/// callers must not silently compare a VCS extraction with a directory overlay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum SnapshotMaterialization {
    Directory,
    Vcs { revision: String },
}

/// Effective inputs which must agree before a delta has meaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionContext {
    pub schema: String,
    pub analyzer: AnalyzerConfigSnapshot,
    pub grammar: BTreeMap<PathBuf, String>,
    pub scope: Vec<PathBuf>,
    pub build_context: String,
    pub materialization: SnapshotMaterialization,
}

/// One exact source file in a revision snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionSource {
    pub path: PathBuf,
    pub lang: Lang,
    pub bytes_hash: String,
    pub text: String,
}

/// A finding plus its revision-aware attribution.  Removed findings have no target finding;
/// introduced findings have no base finding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributedFinding {
    pub finding: Option<Finding>,
    pub counterpart: Option<Finding>,
    pub disposition: FindingDisposition,
    pub identity: SourceIdentity,
    pub evidence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingDisposition {
    Introduced,
    Inherited,
    Removed,
    Moved,
    Uncertain,
}

/// Stable source identity used when attributing findings across revisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub path: PathBuf,
    pub region_hash: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub occurrence: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Incomparability {
    pub reasons: Vec<IncomparabilityReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum IncomparabilityReason {
    Schema {
        base: String,
        target: String,
    },
    AnalyzerConfig,
    Grammar {
        path: PathBuf,
        base: String,
        target: String,
    },
    Scope {
        base: Vec<PathBuf>,
        target: Vec<PathBuf>,
    },
    BuildContext {
        base: String,
        target: String,
    },
    Materialization {
        base: SnapshotMaterialization,
        target: SnapshotMaterialization,
    },
}

/// An analyzer projection and its exact source materialization, detached from filesystem paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionSnapshot {
    pub schema: String,
    pub label: String,
    pub context: RevisionContext,
    pub sources: Vec<RevisionSource>,
    pub reports: Vec<FileReport>,
}

impl RevisionSnapshot {
    /// Construct a comparable snapshot from the existing scan pipeline.  `scope` and
    /// `build_context` are caller-owned declarations and are retained verbatim in the contract.
    pub fn from_scan_context(
        label: impl Into<String>,
        scope: Vec<PathBuf>,
        build_context: impl Into<String>,
        materialization: SnapshotMaterialization,
        config: AnalyzerConfigSnapshot,
        scan: ScanContext,
    ) -> Self {
        Self::from_scan_context_at(
            None,
            label,
            scope,
            build_context,
            materialization,
            config,
            scan,
        )
    }

    /// Variant used by directory/VCS adapters whose scan paths may be absolute.
    pub fn from_scan_context_at(
        root: Option<&Path>,
        label: impl Into<String>,
        scope: Vec<PathBuf>,
        build_context: impl Into<String>,
        materialization: SnapshotMaterialization,
        config: AnalyzerConfigSnapshot,
        scan: ScanContext,
    ) -> Self {
        let normalize = |path: &Path| normalize_path(root, path);
        let mut grammar = BTreeMap::new();
        for file in scan.analysis.files() {
            let g = file.grammar();
            grammar.insert(
                normalize(&scan.analysis.snapshot().root().join(&file.key().path)),
                format!(
                    "lang={:?};dialect={};selector={};grammar={};version={};parser-build={}",
                    g.lang(),
                    g.dialect(),
                    g.selector(),
                    g.grammar_id(),
                    g.grammar_version(),
                    g.parser_build()
                ),
            );
        }
        let lang_by_path = scan
            .reports
            .iter()
            .map(|report| (normalize(&report.path), report.lang))
            .collect::<BTreeMap<_, _>>();
        let mut sources = scan
            .input_contents
            .into_iter()
            .map(|(path, text)| {
                let path = normalize(&path);
                RevisionSource {
                    lang: lang_by_path.get(&path).copied().unwrap_or(Lang::Generic),
                    bytes_hash: hash_bytes(text.as_bytes()),
                    path,
                    text,
                }
            })
            .collect::<Vec<_>>();
        sources.sort_by(|a, b| a.path.cmp(&b.path));
        let mut reports = scan.reports;
        for report in &mut reports {
            report.path = normalize(&report.path);
            for finding in &mut report.findings {
                finding.path = normalize(&finding.path);
            }
        }
        let mut scope = scope
            .into_iter()
            .map(|path| normalize(&path))
            .collect::<Vec<_>>();
        scope.sort();
        scope.dedup();
        Self {
            schema: REVISION_CLEANUP_SCHEMA.to_string(),
            label: label.into(),
            context: RevisionContext {
                schema: REVISION_CLEANUP_CONTEXT_SCHEMA.to_string(),
                analyzer: config,
                grammar,
                scope,
                build_context: build_context.into(),
                materialization,
            },
            sources,
            reports,
        }
    }

    pub fn source(&self, path: &Path) -> Option<&RevisionSource> {
        self.sources.iter().find(|source| source.path == path)
    }
}

fn normalize_path(root: Option<&Path>, path: &Path) -> PathBuf {
    if let Some(root) = root
        && path.is_absolute()
        && let Ok(relative) = path.strip_prefix(root)
    {
        return relative.to_path_buf();
    }
    path.to_path_buf()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionComparison {
    pub schema: String,
    pub base: String,
    pub target: String,
    pub comparable: bool,
    pub incomparability: Option<Incomparability>,
    pub target_sources: BTreeMap<PathBuf, String>,
    pub findings: Vec<AttributedFinding>,
    pub introduced: usize,
    pub inherited: usize,
    pub removed: usize,
    pub moved: usize,
    pub uncertain: usize,
}

/// Scan two materializations with the same effective config and compare their exact projections.
pub fn compare_paths(
    base: &Path,
    target: &Path,
    config: AnalyzerConfig,
) -> Result<RevisionComparison> {
    compare_paths_with_scope(base, target, &[PathBuf::from(".")], config)
}

/// Scoped form used by integrations that must bind comparison and proposal scans to one scope.
pub fn compare_paths_with_scope(
    base: &Path,
    target: &Path,
    scope: &[PathBuf],
    config: AnalyzerConfig,
) -> Result<RevisionComparison> {
    let scope = if scope.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        scope.to_vec()
    };
    let base_root = base.canonicalize()?;
    let target_root = target.canonicalize()?;
    let base_paths = scope_paths(&base_root, &scope);
    let target_paths = scope_paths(&target_root, &scope);
    let base_scan = crate::scan_paths_with_context(&base_paths, config.clone())?;
    let target_scan = crate::scan_paths_with_context(&target_paths, config.clone())?;
    let cfg = config.snapshot();
    let base_snapshot = RevisionSnapshot::from_scan_context_at(
        Some(&base_root),
        "base",
        scope.clone(),
        "deslop-analyzer-scanner/1",
        SnapshotMaterialization::Directory,
        cfg.clone(),
        base_scan,
    );
    let target_snapshot = RevisionSnapshot::from_scan_context_at(
        Some(&target_root),
        "target",
        scope,
        "deslop-analyzer-scanner/1",
        SnapshotMaterialization::Directory,
        cfg,
        target_scan,
    );
    compare_snapshots(&base_snapshot, &target_snapshot)
}

fn scope_paths(root: &Path, scope: &[PathBuf]) -> Vec<PathBuf> {
    scope
        .iter()
        .map(|path| {
            if path == Path::new(".") {
                root.to_path_buf()
            } else {
                root.join(path)
            }
        })
        .collect()
}

/// Compare two exact snapshots.  Incomparable snapshots return a valid result with no
/// attribution delta; callers must display the reasons rather than treating it as zero change.
pub fn compare_snapshots(
    base: &RevisionSnapshot,
    target: &RevisionSnapshot,
) -> Result<RevisionComparison> {
    validate_snapshot(base)?;
    validate_snapshot(target)?;
    let reasons = comparability_reasons(base, target);
    if !reasons.is_empty() {
        return Ok(RevisionComparison {
            schema: REVISION_CLEANUP_SCHEMA.to_string(),
            base: base.label.clone(),
            target: target.label.clone(),
            comparable: false,
            incomparability: Some(Incomparability { reasons }),
            target_sources: source_hashes(target),
            findings: Vec::new(),
            introduced: 0,
            inherited: 0,
            removed: 0,
            moved: 0,
            uncertain: 0,
        });
    }
    let mut base_findings = base
        .reports
        .iter()
        .flat_map(|report| report.findings.iter())
        .collect::<Vec<_>>();
    let target_findings = target
        .reports
        .iter()
        .flat_map(|report| report.findings.iter())
        .collect::<Vec<_>>();
    base_findings.sort_by(finding_order);
    let mut used = BTreeSet::new();
    let mut output = Vec::new();
    for target_finding in target_findings.iter().copied() {
        let identity = identity_for(target, target_finding);
        let candidates = base_findings
            .iter()
            .enumerate()
            .filter(|(index, finding)| {
                !used.contains(index)
                    && same_content_identity(base, finding, target, target_finding)
            })
            .collect::<Vec<_>>();
        let same_path = candidates
            .iter()
            .copied()
            .filter(|(_, finding)| finding.path == target_finding.path)
            .collect::<Vec<_>>();
        let all_matches = base_findings
            .iter()
            .filter(|finding| same_content_identity(base, finding, target, target_finding))
            .count();
        let duplicate_target = target_findings
            .iter()
            .filter(|finding| same_content_identity(target, finding, target, target_finding))
            .count()
            > 1;
        let (disposition, counterpart, evidence) =
            if (all_matches > 1 || duplicate_target) && !candidates.is_empty() {
                let (index, finding) = candidates[0];
                used.insert(index);
                (
                    FindingDisposition::Uncertain,
                    Some((*finding).clone()),
                    "duplicate identical regions make source attribution ambiguous".to_string(),
                )
            } else if same_path.len() == 1 {
                let (index, finding) = same_path[0];
                used.insert(index);
                (
                    FindingDisposition::Inherited,
                    Some((*finding).clone()),
                    "exact source bytes and source identity persisted".to_string(),
                )
            } else if candidates.len() == 1 {
                let (index, finding) = candidates[0];
                used.insert(index);
                (
                    FindingDisposition::Moved,
                    Some((*finding).clone()),
                    "same finding content moved to a different source path".to_string(),
                )
            } else if candidates.len() > 1 {
                let (index, finding) = candidates[0];
                used.insert(index);
                (
                    FindingDisposition::Uncertain,
                    Some((*finding).clone()),
                    "duplicate identical regions make source attribution ambiguous".to_string(),
                )
            } else {
                (
                    FindingDisposition::Introduced,
                    None,
                    "no matching base finding with stable rule and source bytes".to_string(),
                )
            };
        output.push(AttributedFinding {
            finding: Some(target_finding.clone()),
            counterpart,
            disposition,
            identity,
            evidence,
        });
    }
    for (index, finding) in base_findings.iter().enumerate() {
        if !used.contains(&index) {
            output.push(AttributedFinding {
                finding: None,
                counterpart: Some((*finding).clone()),
                disposition: FindingDisposition::Removed,
                identity: identity_for(base, finding),
                evidence: "base finding has no matching target finding".to_string(),
            });
        }
    }
    output.sort_by(|a, b| {
        a.identity
            .path
            .cmp(&b.identity.path)
            .then(a.identity.start_byte.cmp(&b.identity.start_byte))
            .then(a.identity.region_hash.cmp(&b.identity.region_hash))
    });
    let mut occurrences = BTreeMap::<(PathBuf, String), usize>::new();
    for attributed in &mut output {
        let key = (
            attributed.identity.path.clone(),
            attributed.identity.region_hash.clone(),
        );
        let occurrence = occurrences.entry(key).or_default();
        attributed.identity.occurrence = *occurrence;
        *occurrence += 1;
    }
    let mut comparison = RevisionComparison {
        schema: REVISION_CLEANUP_SCHEMA.to_string(),
        base: base.label.clone(),
        target: target.label.clone(),
        comparable: true,
        incomparability: None,
        target_sources: source_hashes(target),
        findings: output,
        introduced: 0,
        inherited: 0,
        removed: 0,
        moved: 0,
        uncertain: 0,
    };
    for finding in &comparison.findings {
        match finding.disposition {
            FindingDisposition::Introduced => comparison.introduced += 1,
            FindingDisposition::Inherited => comparison.inherited += 1,
            FindingDisposition::Removed => comparison.removed += 1,
            FindingDisposition::Moved => comparison.moved += 1,
            FindingDisposition::Uncertain => comparison.uncertain += 1,
        }
    }
    Ok(comparison)
}

fn validate_snapshot(snapshot: &RevisionSnapshot) -> Result<()> {
    if snapshot.schema != REVISION_CLEANUP_SCHEMA {
        bail!(
            "unsupported revision cleanup snapshot schema `{}`",
            snapshot.schema
        );
    }
    if snapshot.context.schema != REVISION_CLEANUP_CONTEXT_SCHEMA {
        bail!(
            "unsupported revision cleanup context schema `{}`",
            snapshot.context.schema
        );
    }
    for source in &snapshot.sources {
        if source.path.as_os_str().is_empty() || source.path.is_absolute() {
            bail!("snapshot source path must be root-relative");
        }
        if source.bytes_hash != hash_bytes(source.text.as_bytes()) {
            bail!("snapshot source hash does not match exact source bytes");
        }
    }
    for report in &snapshot.reports {
        for finding in &report.findings {
            let source = snapshot
                .source(&finding.path)
                .ok_or_else(|| anyhow::anyhow!("finding source is absent from exact snapshot"))?;
            if finding.span.start_byte > finding.span.end_byte
                || finding.span.end_byte > source.text.len()
                || !source.text.is_char_boundary(finding.span.start_byte)
                || !source.text.is_char_boundary(finding.span.end_byte)
            {
                bail!("finding span is outside exact UTF-8 source bytes");
            }
        }
    }
    Ok(())
}

fn comparability_reasons(
    base: &RevisionSnapshot,
    target: &RevisionSnapshot,
) -> Vec<IncomparabilityReason> {
    let mut reasons = Vec::new();
    if base.context.schema != target.context.schema {
        reasons.push(IncomparabilityReason::Schema {
            base: base.context.schema.clone(),
            target: target.context.schema.clone(),
        });
    }
    if base.context.analyzer != target.context.analyzer {
        reasons.push(IncomparabilityReason::AnalyzerConfig);
    }
    if base.context.scope != target.context.scope {
        reasons.push(IncomparabilityReason::Scope {
            base: base.context.scope.clone(),
            target: target.context.scope.clone(),
        });
    }
    if base.context.build_context != target.context.build_context {
        reasons.push(IncomparabilityReason::BuildContext {
            base: base.context.build_context.clone(),
            target: target.context.build_context.clone(),
        });
    }
    if !same_materialization_kind(
        &base.context.materialization,
        &target.context.materialization,
    ) {
        reasons.push(IncomparabilityReason::Materialization {
            base: base.context.materialization.clone(),
            target: target.context.materialization.clone(),
        });
    }
    let paths = base
        .context
        .grammar
        .keys()
        .chain(target.context.grammar.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for path in paths {
        if let (Some(left), Some(right)) = (
            base.context.grammar.get(&path),
            target.context.grammar.get(&path),
        ) && left != right
        {
            reasons.push(IncomparabilityReason::Grammar {
                path,
                base: left.clone(),
                target: right.clone(),
            });
        }
    }
    reasons
}
fn same_materialization_kind(
    left: &SnapshotMaterialization,
    right: &SnapshotMaterialization,
) -> bool {
    matches!(
        (left, right),
        (
            SnapshotMaterialization::Directory,
            SnapshotMaterialization::Directory
        ) | (
            SnapshotMaterialization::Vcs { .. },
            SnapshotMaterialization::Vcs { .. }
        )
    )
}

fn finding_order(left: &&Finding, right: &&Finding) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.span.start_byte.cmp(&right.span.start_byte))
        .then(left.span.end_byte.cmp(&right.span.end_byte))
        .then(left.rule.cmp(&right.rule))
        .then(left.fingerprint.cmp(&right.fingerprint))
}

fn same_content_identity(
    base: &RevisionSnapshot,
    left: &Finding,
    target: &RevisionSnapshot,
    right: &Finding,
) -> bool {
    left.rule == right.rule
        && left.detected_by == right.detected_by
        && region_hash(base, left) == region_hash(target, right)
}

fn identity_for(snapshot: &RevisionSnapshot, finding: &Finding) -> SourceIdentity {
    SourceIdentity {
        path: finding.path.clone(),
        region_hash: region_hash(snapshot, finding),
        start_byte: finding.span.start_byte,
        end_byte: finding.span.end_byte,
        occurrence: 0,
    }
}

fn region_hash(snapshot: &RevisionSnapshot, finding: &Finding) -> String {
    snapshot
        .source(&finding.path)
        .and_then(|source| {
            source
                .text
                .as_bytes()
                .get(finding.span.start_byte..finding.span.end_byte)
        })
        .map(hash_bytes)
        .unwrap_or_else(|| hash_bytes(finding.message.as_bytes()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
fn source_hashes(snapshot: &RevisionSnapshot) -> BTreeMap<PathBuf, String> {
    snapshot
        .sources
        .iter()
        .map(|source| (source.path.clone(), source.bytes_hash.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_core::{AnalysisProvenance, DetectedBy, SafetyClass, Severity, Span};

    fn fixture(label: &str, text: &str, findings: Vec<Finding>) -> RevisionSnapshot {
        RevisionSnapshot {
            schema: REVISION_CLEANUP_SCHEMA.to_string(),
            label: label.to_string(),
            context: RevisionContext {
                schema: REVISION_CLEANUP_CONTEXT_SCHEMA.to_string(),
                analyzer: AnalyzerConfig::default().snapshot(),
                grammar: BTreeMap::new(),
                scope: vec![PathBuf::from(".")],
                build_context: "test-build/1".to_string(),
                materialization: SnapshotMaterialization::Directory,
            },
            sources: vec![RevisionSource {
                path: PathBuf::from("unicode-λ.rs"),
                lang: Lang::Rust,
                bytes_hash: hash_bytes(text.as_bytes()),
                text: text.to_string(),
            }],
            reports: vec![FileReport {
                path: PathBuf::from("unicode-λ.rs"),
                lang: Lang::Rust,
                analysis: AnalysisProvenance::complete(),
                findings,
            }],
        }
    }

    fn finding(path: &str, start: usize, end: usize, rule: &str) -> Finding {
        Finding {
            path: PathBuf::from(path),
            span: Span::new(1, 1, start, end),
            rule: rule.to_string(),
            severity: Severity::Minor,
            safety: SafetyClass::RiskySuggest,
            detected_by: DetectedBy::Duplication,
            message: "review cleanup hypothesis".to_string(),
            suggestion: "review".to_string(),
            precondition: None,
            edit: None,
            fingerprint: "fixture".to_string(),
        }
    }

    #[test]
    fn unchanged_bytes_are_inherited_not_introduced() {
        let base = fixture(
            "base",
            "fn λ() {}\n",
            vec![finding("unicode-λ.rs", 0, 8, "duplicate-code")],
        );
        let target = fixture(
            "target",
            "fn λ() {}\n",
            vec![finding("unicode-λ.rs", 0, 8, "duplicate-code")],
        );
        let comparison = compare_snapshots(&base, &target).unwrap();
        assert!(comparison.comparable);
        assert_eq!(comparison.introduced, 0);
        assert_eq!(comparison.inherited, 1);
    }

    #[test]
    fn context_drift_is_explicitly_incomparable() {
        let base = fixture("base", "fn λ() {}\n", Vec::new());
        let mut target = fixture("target", "fn λ() {}\n", Vec::new());
        target.context.build_context = "different-build/1".to_string();
        let comparison = compare_snapshots(&base, &target).unwrap();
        assert!(!comparison.comparable);
        assert!(comparison.incomparability.is_some());
        assert!(comparison.findings.is_empty());
    }

    #[test]
    fn duplicate_identical_regions_are_uncertain() {
        let base = fixture(
            "base",
            "fn λ() {}\nfn λ() {}\n",
            vec![
                finding("unicode-λ.rs", 0, 8, "duplicate-code"),
                finding("unicode-λ.rs", 11, 19, "duplicate-code"),
            ],
        );
        let target = fixture(
            "target",
            "fn λ() {}\nfn λ() {}\n",
            vec![finding("unicode-λ.rs", 0, 8, "duplicate-code")],
        );
        let comparison = compare_snapshots(&base, &target).unwrap();
        assert_eq!(comparison.uncertain, 1);
    }
}
