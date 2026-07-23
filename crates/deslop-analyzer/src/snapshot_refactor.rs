//! Snapshot-native contract pathology detection.
//!
//! This module analyzes one exact `ProjectAnalysis`. It reports present-state
//! contract splits and invariant gaps; it never manufactures a historical
//! transition, persistence count, or old/new direction.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use deslop_core::SafetyClass;
use deslop_core::refactor_defect::{
    CapabilityLevel, ContractEdgeKind, ContractNodeRef, ContractRole, ContractStep, CoverageGap,
    EvidenceItem, FactProvider,
};
use deslop_core::snapshot_pathology::{
    SNAPSHOT_PATHOLOGY_SCHEMA, SNAPSHOT_REFACTOR_RISK_SCHEMA, SnapshotPathology, rule_names,
};
use deslop_parse::{
    ContractFunction, ContractSnapshot, DiscoveryPolicy, FactCoverage, FileContracts,
    ProjectAnalysis, ProjectSnapshotPlanner, ProjectSnapshotRequest, RepositorySpec, RootSpec,
    ScopeSpec,
};
use serde::Serialize;

use crate::sibling_gate::{GateAnchor, sibling_gate_asymmetries};

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRefactorRiskReport {
    pub schema: String,
    pub snapshot: String,
    pub coverage: FactCoverage,
    pub coverage_reasons: Vec<String>,
    pub findings: Vec<SnapshotPathology>,
    pub summaries: Vec<SnapshotPathology>,
}

pub fn to_file_reports(report: &SnapshotRefactorRiskReport) -> Vec<deslop_core::FileReport> {
    let mut by_path: BTreeMap<PathBuf, Vec<deslop_core::Finding>> = BTreeMap::new();
    for pathology in &report.findings {
        let finding = pathology.to_finding();
        by_path
            .entry(finding.path.clone())
            .or_default()
            .push(finding);
    }
    by_path
        .into_iter()
        .map(|(path, findings)| deslop_core::FileReport {
            lang: deslop_lang::detect_lang(&path),
            path,
            analysis: deslop_core::AnalysisProvenance::complete(),
            findings,
        })
        .collect()
}

/// Analyze current paths without consulting a VCS/history provider.
pub fn snapshot_refactor_risk_paths(paths: &[PathBuf]) -> Result<SnapshotRefactorRiskReport> {
    let invocation_base = std::env::current_dir().context("resolve snapshot invocation base")?;
    let mut requested = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    // A single directory is a self-contained snapshot root. This keeps its
    // presentation paths relative to that input and prevents an enclosing
    // repository's `tests/` component from reclassifying fixture/project
    // sources as test code. Multiple roots retain the normal auto-root join.
    let root = if requested.len() == 1 {
        let absolute = if requested[0].is_absolute() {
            requested[0].clone()
        } else {
            invocation_base.join(&requested[0])
        };
        if absolute.is_dir() {
            requested = vec![absolute.clone()];
            RootSpec::Explicit(absolute)
        } else {
            RootSpec::Auto
        }
    } else {
        RootSpec::Auto
    };
    let planner = ProjectSnapshotPlanner::resolve(ProjectSnapshotRequest {
        invocation_base,
        root,
        repository: RepositorySpec::Auto,
        scope: ScopeSpec::Requested(requested),
        discovery: DiscoveryPolicy::LegacyRespectIgnore,
    })?;
    let built = planner.build()?;
    let analysis = ProjectAnalysis::build(built.snapshot)?;
    analyze_snapshot_refactor("current", analysis)
}

pub fn analyze_snapshot_refactor(
    label: impl Into<String>,
    analysis: Arc<ProjectAnalysis>,
) -> Result<SnapshotRefactorRiskReport> {
    let label = label.into();
    let snapshot = ContractSnapshot::from_analysis(label.clone(), &analysis)
        .context("extract current contract snapshot")?;
    let index = SnapshotIndex::new(&snapshot.files);
    let mut findings = Vec::new();
    let mut reasons = snapshot.reasons.clone();
    if index.dynamic_access {
        reasons.push(
            "current snapshot contains dynamic/string-addressed access; absence of a contract edge is unknown"
                .to_string(),
        );
    }

    detect_contract_splits(&label, &index, &mut findings);
    detect_schema_mismatch(&label, &index, &mut findings);
    detect_config_without_reach(&label, &index, &mut findings);
    detect_lossy_confidence(&label, &index, &mut findings);
    detect_repeated_work(&label, &index, &mut findings);
    detect_test_dimension_gap(&label, &index, &mut findings);
    let revision = snapshot.revision_contracts();
    detect_sibling_gate_asymmetry(&label, &revision, &mut findings);

    findings.sort_by(|left, right| {
        (&left.rule, left.pathology_identity()).cmp(&(&right.rule, right.pathology_identity()))
    });
    findings.dedup_by(|left, right| left.pathology_identity() == right.pathology_identity());
    for finding in &findings {
        finding
            .validate()
            .map_err(|error| anyhow::anyhow!("invalid snapshot pathology: {error}"))?;
    }
    let summaries = build_summaries(&label, &findings);
    let coverage = if reasons.is_empty() {
        FactCoverage::Complete
    } else {
        FactCoverage::Partial
    };
    Ok(SnapshotRefactorRiskReport {
        schema: SNAPSHOT_REFACTOR_RISK_SCHEMA.to_string(),
        snapshot: label,
        coverage,
        coverage_reasons: reasons,
        findings,
        summaries,
    })
}

struct IndexedFunction<'a> {
    file: &'a FileContracts,
    function: &'a ContractFunction,
}

struct SnapshotIndex<'a> {
    functions: Vec<IndexedFunction<'a>>,
    by_name: BTreeMap<&'a str, Vec<usize>>,
    dynamic_access: bool,
}

impl<'a> SnapshotIndex<'a> {
    fn new(files: &'a [FileContracts]) -> Self {
        let mut functions = Vec::new();
        let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        let mut dynamic_access = false;
        for file in files {
            for function in &file.functions {
                let index = functions.len();
                by_name
                    .entry(function.name.as_str())
                    .or_default()
                    .push(index);
                dynamic_access |= function.references.iter().any(|token| {
                    matches!(
                        token_leaf(token),
                        "getattr" | "eval" | "invoke" | "getfield"
                    )
                });
                functions.push(IndexedFunction { file, function });
            }
        }
        Self {
            functions,
            by_name,
            dynamic_access,
        }
    }

    /// Current reference closure, preserving terminal tokens that do not
    /// name an extracted function. Same-named candidates are unioned and
    /// therefore suppress rather than manufacture a split.
    fn terminal_references(&self, start: usize) -> BTreeSet<String> {
        let mut terminals = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut queue = vec![start];
        while let Some(index) = queue.pop() {
            if !seen.insert(index) {
                continue;
            }
            for token in &self.functions[index].function.references {
                let leaf = token_leaf(token);
                if let Some(candidates) = self.by_name.get(leaf) {
                    for candidate in candidates {
                        if *candidate != index {
                            queue.push(*candidate);
                        }
                    }
                } else if !is_plumbing_reference(token) {
                    terminals.insert(token.clone());
                }
            }
        }
        terminals
    }
}

fn token_leaf(token: &str) -> &str {
    token.rsplit('.').next().unwrap_or(token)
}

fn token_root(token: &str) -> &str {
    token.split('.').next().unwrap_or(token)
}

fn is_plumbing_reference(token: &str) -> bool {
    let leaf = token_leaf(token).to_ascii_lowercase();
    matches!(
        leaf.as_str(),
        "return"
            | "dict"
            | "list"
            | "set"
            | "tuple"
            | "len"
            | "max"
            | "min"
            | "range"
            | "enumerate"
            | "zip"
            | "keys"
            | "values"
            | "items"
            | "contains"
            | "emit"
            | "emit_v2"
            | "combine"
    ) || matches!(
        token_root(token).to_ascii_lowercase().as_str(),
        "metrics" | "metric" | "logger" | "log" | "status" | "health" | "registry"
    )
}

fn same_mechanism(left: &BTreeSet<String>, right: &BTreeSet<String>) -> bool {
    left.iter().any(|a| {
        right
            .iter()
            .any(|b| a == b || token_root(a) == token_root(b) || token_leaf(a) == token_leaf(b))
    })
}

fn is_test(path: &std::path::Path, function: &ContractFunction) -> bool {
    function.name.starts_with("test_")
        || path.components().any(|component| {
            matches!(
                component.as_os_str().to_str().unwrap_or_default(),
                "test" | "tests"
            )
        })
        || path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.starts_with("test_") || stem.ends_with("_test"))
}

fn is_behavior(function: &ContractFunction) -> bool {
    let name = function.name.to_ascii_lowercase();
    [
        "decide", "step", "train", "run", "resume", "execute", "apply", "process",
    ]
    .iter()
    .any(|part| {
        name == *part
            || name.starts_with(&format!("{part}_"))
            || name.ends_with(&format!("_{part}"))
    })
}

fn is_gate(function: &ContractFunction) -> bool {
    let name = function.name.to_ascii_lowercase();
    function.assertions > 0
        || ["check", "verify", "validate"].iter().any(|part| {
            name == *part
                || name.starts_with(&format!("{part}_"))
                || name.ends_with(&format!("_{part}"))
        })
}

fn is_telemetry(function: &ContractFunction) -> bool {
    function.references.iter().any(|token| {
        matches!(
            token_root(token).to_ascii_lowercase().as_str(),
            "metrics" | "metric" | "statsd" | "prometheus" | "telemetry" | "logger" | "log"
        )
    })
}

fn is_identity_publisher(function: &ContractFunction) -> bool {
    function.references.iter().any(|token| {
        matches!(
            token_root(token).to_ascii_lowercase().as_str(),
            "status" | "health" | "heartbeat" | "watchdog" | "registry"
        )
    })
}

fn is_public_consumer(function: &ContractFunction) -> bool {
    let name = function.name.to_ascii_lowercase();
    (name.contains("public") || name.contains("score") || name.contains("result"))
        && !is_gate(function)
}

fn node(
    indexed: &IndexedFunction<'_>,
    role: ContractRole,
    capability: CapabilityLevel,
) -> ContractNodeRef {
    ContractNodeRef {
        role,
        path: indexed.file.path.clone(),
        span: indexed.function.span,
        fingerprint: indexed.function.fingerprint.clone(),
        provider: FactProvider::TreeSitter,
        capability,
    }
}

fn syntax_gap(reason: impl Into<String>) -> CoverageGap {
    CoverageGap {
        provider: FactProvider::TreeSitter,
        capability: CapabilityLevel::Partial,
        reason: reason.into(),
    }
}

fn pathology(
    snapshot: &str,
    rule: &'static str,
    anchors: Vec<ContractNodeRef>,
    steps: Vec<ContractStep>,
    evidence: Vec<EvidenceItem>,
    gaps: Vec<CoverageGap>,
    suggestion: impl Into<String>,
) -> SnapshotPathology {
    let mut priority_inputs = BTreeMap::new();
    priority_inputs.insert("current-contract-split".to_string(), 1);
    priority_inputs.insert("boundary-distance".to_string(), steps.len() as i64);
    SnapshotPathology {
        schema: SNAPSHOT_PATHOLOGY_SCHEMA.to_string(),
        rule: rule.to_string(),
        family: rule.to_string(),
        snapshot: snapshot.to_string(),
        anchors,
        conflicting_edges: steps.clone(),
        causal_path: steps,
        evidence,
        counter_evidence: Vec::new(),
        coverage_gaps: gaps,
        priority_inputs,
        safety: SafetyClass::NeverAuto,
        suggested_verification: suggestion.into(),
    }
}

fn split_step(indexed: &IndexedFunction<'_>, role: ContractRole, detail: String) -> ContractStep {
    ContractStep {
        edge: match role {
            ContractRole::Verifier => ContractEdgeKind::Verifies,
            ContractRole::TelemetrySurface => ContractEdgeKind::Observes,
            ContractRole::RuntimeIdentity => ContractEdgeKind::Publishes,
            _ => ContractEdgeKind::Consumes,
        },
        node: node(indexed, role, CapabilityLevel::Partial),
        token: None,
        detail,
    }
}

fn gate_node(anchor: GateAnchor<'_>) -> ContractNodeRef {
    ContractNodeRef {
        role: ContractRole::Verifier,
        path: anchor.path.to_path_buf(),
        span: anchor.function.span,
        fingerprint: anchor.function.fingerprint.clone(),
        provider: FactProvider::TreeSitter,
        capability: CapabilityLevel::Partial,
    }
}

fn detect_sibling_gate_asymmetry(
    snapshot: &str,
    revision: &deslop_parse::RevisionContracts,
    findings: &mut Vec<SnapshotPathology>,
) {
    for asymmetry in sibling_gate_asymmetries(revision) {
        let left_node = gate_node(asymmetry.left);
        let right_node = gate_node(asymmetry.right);
        let shared = asymmetry
            .shared_identifiers
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let detail = format!(
            "fail-loud gates `{}` and `{}` share [{}], but {}",
            asymmetry.left.function.name,
            asymmetry.right.function.name,
            shared,
            asymmetry.detail()
        );
        findings.push(pathology(
            snapshot,
            rule_names::SIBLING_ADMISSION_GUARDS_ASYMMETRIC,
            vec![left_node.clone(), right_node.clone()],
            vec![
                ContractStep {
                    edge: ContractEdgeKind::Verifies,
                    node: left_node.clone(),
                    token: asymmetry.shared_identifiers.iter().next().cloned(),
                    detail: format!(
                        "`{}` is a fail-loud admission gate over [{}]",
                        asymmetry.left.function.name, shared
                    ),
                },
                ContractStep {
                    edge: ContractEdgeKind::Verifies,
                    node: right_node.clone(),
                    token: asymmetry.shared_identifiers.iter().next().cloned(),
                    detail,
                },
            ],
            vec![EvidenceItem {
                provider: FactProvider::TreeSitter,
                detail: format!(
                    "{} shared domain identifier(s) survive type-like, callable, \
                     popularity, and overlap bounds",
                    asymmetry.shared_identifiers.len()
                ),
                node: Some(right_node),
            }],
            vec![syntax_gap(
                "guard features and unique-callee closure are syntactic; runtime admission equivalence and field aliasing are not proved",
            )],
            "exercise both sibling gates with identical missing, zero-observation/NaN, non-finite, boundary, and valid payloads; centralize the predicate or document intentional asymmetry",
        ));
    }
}

fn detect_contract_splits(
    snapshot: &str,
    index: &SnapshotIndex<'_>,
    findings: &mut Vec<SnapshotPathology>,
) {
    if index.dynamic_access {
        return;
    }
    for (behavior_index, behavior) in index.functions.iter().enumerate() {
        if !is_behavior(behavior.function) || is_test(&behavior.file.path, behavior.function) {
            continue;
        }
        let governing = index.terminal_references(behavior_index);
        if governing.is_empty() {
            continue;
        }
        for (surface_index, surface) in index.functions.iter().enumerate() {
            if surface_index == behavior_index
                || surface.file.path != behavior.file.path
                || is_test(&surface.file.path, surface.function)
            {
                continue;
            }
            let (rule, role, label, suggestion) = if is_gate(surface.function) {
                (
                    rule_names::MECHANISM_GATE_CONTRACT_SPLIT,
                    ContractRole::Verifier,
                    "gate",
                    "exercise the governing behavior and gate together; bind the gate to the same mechanism or document the invariant",
                )
            } else if is_telemetry(surface.function) {
                (
                    rule_names::TELEMETRY_CLAIM_UNBOUND,
                    ContractRole::TelemetrySurface,
                    "telemetry",
                    "trace the reported claim to the governing behavior under a representative run",
                )
            } else if is_identity_publisher(surface.function) {
                (
                    rule_names::PUBLISHED_IDENTITY_NOT_LIVE,
                    ContractRole::RuntimeIdentity,
                    "published identity",
                    "compare the published identity with the identity governing resume/runtime behavior",
                )
            } else if is_public_consumer(surface.function) {
                (
                    rule_names::OWNER_CONSUMER_CONTRACT_SPLIT,
                    ContractRole::Consumer,
                    "consumer",
                    "compare the governing decision and exposed consumer over the same inputs",
                )
            } else {
                continue;
            };
            let observed = index.terminal_references(surface_index);
            if observed.is_empty() || same_mechanism(&governing, &observed) {
                continue;
            }
            let detail = format!(
                "current behavior `{}` reaches [{}] while {label} `{}` reaches [{}]",
                behavior.function.name,
                governing.iter().cloned().collect::<Vec<_>>().join(", "),
                surface.function.name,
                observed.iter().cloned().collect::<Vec<_>>().join(", ")
            );
            findings.push(pathology(
                snapshot,
                rule,
                vec![
                    node(behavior, ContractRole::Owner, CapabilityLevel::Partial),
                    node(surface, role, CapabilityLevel::Partial),
                ],
                vec![split_step(surface, role, detail)],
                vec![EvidenceItem {
                    provider: FactProvider::TreeSitter,
                    detail: "two current syntactic reference closures terminate at disjoint mechanism tokens"
                        .to_string(),
                    node: Some(node(surface, role, CapabilityLevel::Partial)),
                }],
                vec![syntax_gap(
                    "reference closures are syntax candidates; symbol resolution and runtime equivalence are not proved",
                )],
                suggestion,
            ));
        }
    }
}

fn schema_subject(name: &str, prefixes: &[&str]) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    prefixes
        .iter()
        .find_map(|prefix| lower.strip_prefix(prefix).map(ToOwned::to_owned))
}

fn schema_tokens(function: &ContractFunction) -> BTreeSet<String> {
    function
        .literals
        .iter()
        .filter(|token| {
            !token.is_empty()
                && token.len() <= 80
                && token
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
        })
        .cloned()
        .collect()
}

fn detect_schema_mismatch(
    snapshot: &str,
    index: &SnapshotIndex<'_>,
    findings: &mut Vec<SnapshotPathology>,
) {
    const PRODUCERS: &[&str] = &["build_", "create_", "write_", "serialize_", "emit_"];
    const VERIFIERS: &[&str] = &["verify_", "validate_", "read_", "check_"];
    for producer in &index.functions {
        let Some(subject) = schema_subject(&producer.function.name, PRODUCERS) else {
            continue;
        };
        let produced = schema_tokens(producer.function);
        if produced.is_empty() {
            continue;
        }
        for verifier in &index.functions {
            if verifier.file.path != producer.file.path
                || schema_subject(&verifier.function.name, VERIFIERS).as_deref()
                    != Some(subject.as_str())
            {
                continue;
            }
            let checked = schema_tokens(verifier.function);
            if checked.is_empty() || produced == checked {
                continue;
            }
            let only_producer: Vec<_> = produced.difference(&checked).cloned().collect();
            let only_verifier: Vec<_> = checked.difference(&produced).cloned().collect();
            let detail = format!(
                "current producer `{}` and verifier `{}` disagree for `{subject}` (producer-only [{}], verifier-only [{}])",
                producer.function.name,
                verifier.function.name,
                only_producer.join(", "),
                only_verifier.join(", ")
            );
            let step = ContractStep {
                edge: ContractEdgeKind::Verifies,
                node: node(verifier, ContractRole::Verifier, CapabilityLevel::Partial),
                token: only_verifier
                    .first()
                    .cloned()
                    .or_else(|| only_producer.first().cloned()),
                detail,
            };
            findings.push(pathology(
                snapshot,
                rule_names::PRODUCER_VERIFIER_SCHEMA_MISMATCH,
                vec![
                    node(producer, ContractRole::Producer, CapabilityLevel::Partial),
                    node(verifier, ContractRole::Verifier, CapabilityLevel::Partial),
                ],
                vec![step],
                Vec::new(),
                vec![syntax_gap(
                    "identifier-shaped literals are schema candidates; units and field semantics require typed provider evidence",
                )],
                "round-trip a produced artifact through the verifier and assert every produced/required field and unit",
            ));
        }
    }
}

fn looks_like_config_key(token: &str) -> bool {
    token.len() >= 2
        && token.chars().any(|ch| ch.is_ascii_uppercase())
        && token
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn detect_config_without_reach(
    snapshot: &str,
    index: &SnapshotIndex<'_>,
    findings: &mut Vec<SnapshotPathology>,
) {
    let read: BTreeSet<&str> = index
        .functions
        .iter()
        .flat_map(|indexed| indexed.function.config_keys.iter().map(String::as_str))
        .collect();
    for indexed in &index.functions {
        for token in indexed
            .file
            .module_literals
            .keys()
            .filter(|token| looks_like_config_key(token))
        {
            if read.contains(token.as_str()) || index.dynamic_access {
                continue;
            }
            let step = ContractStep {
                edge: ContractEdgeKind::Configures,
                node: node(
                    indexed,
                    ContractRole::ConfigParameter,
                    CapabilityLevel::Partial,
                ),
                token: Some(token.clone()),
                detail: format!(
                    "accepted config `{token}` is present in the current module surface but no extracted environment read reaches it"
                ),
            };
            findings.push(pathology(
                snapshot,
                rule_names::ACCEPTED_CONFIG_NO_BEHAVIORAL_REACH,
                vec![node(
                    indexed,
                    ContractRole::ConfigParameter,
                    CapabilityLevel::Partial,
                )],
                vec![step],
                Vec::new(),
                vec![syntax_gap(
                    "module-level uppercase literals nominate accepted config; aliases and non-environment config containers are not resolved",
                )],
                format!("set `{token}` to two distinct values and assert a governing behavioral output changes"),
            ));
        }
    }
}

const LOSSY: &[&str] = &[
    "argmax",
    "argmin",
    "round",
    "floor",
    "ceil",
    "clip",
    "clamp",
    "quantize",
    "sign",
    "threshold",
    "onehot",
];

fn detect_lossy_confidence(
    snapshot: &str,
    index: &SnapshotIndex<'_>,
    findings: &mut Vec<SnapshotPathology>,
) {
    for indexed in &index.functions {
        let name = indexed.function.name.to_ascii_lowercase();
        if !["score", "confidence", "explanation", "trace"]
            .iter()
            .any(|part| name.contains(part))
        {
            continue;
        }
        let lossy: Vec<String> = indexed
            .function
            .references
            .iter()
            .filter(|token| LOSSY.contains(&token_leaf(token).to_ascii_lowercase().as_str()))
            .cloned()
            .collect();
        let reconstructed: Vec<String> = indexed
            .function
            .references
            .iter()
            .filter(|token| {
                let leaf = token_leaf(token).to_ascii_lowercase();
                leaf.contains("reconstruct") || leaf.contains("lookup") || leaf.contains("inverse")
            })
            .cloned()
            .collect();
        if lossy.is_empty() || reconstructed.is_empty() {
            continue;
        }
        let step = ContractStep {
            edge: ContractEdgeKind::Transforms,
            node: node(indexed, ContractRole::Consumer, CapabilityLevel::Partial),
            token: lossy.first().cloned(),
            detail: format!(
                "current public evidence path `{}` applies [{}] before deriving through [{}]",
                indexed.function.name,
                lossy.join(", "),
                reconstructed.join(", ")
            ),
        };
        findings.push(pathology(
            snapshot,
            rule_names::CONFIDENCE_DERIVED_AFTER_LOSSY_COMMIT,
            vec![node(indexed, ContractRole::Consumer, CapabilityLevel::Partial)],
            vec![step],
            Vec::new(),
            vec![syntax_gap(
                "lossy-operation classification does not prove domain information loss or behavioral impact",
            )],
            "retain the governing evidence and compare the public score/explanation before and after the lossy step",
        ));
    }
}

fn composite_call(text: &str) -> bool {
    if text.starts_with("blake3:") || text.len() < 24 {
        return false;
    }
    let Some(open) = text.find('(') else {
        return false;
    };
    let callee = text[..open]
        .rsplit(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .next()
        .unwrap_or_default();
    !callee
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_uppercase())
        && text[open + 1..].contains('(')
}

fn detect_repeated_work(
    snapshot: &str,
    index: &SnapshotIndex<'_>,
    findings: &mut Vec<SnapshotPathology>,
) {
    for indexed in &index.functions {
        for (call, count) in &indexed.function.call_texts {
            if *count < 2 || !composite_call(call) {
                continue;
            }
            let step = ContractStep {
                edge: ContractEdgeKind::Transforms,
                node: node(indexed, ContractRole::Consumer, CapabilityLevel::Partial),
                token: Some(call.clone()),
                detail: format!(
                    "current path `{}` contains {count} occurrences of composite call `{call}`",
                    indexed.function.name
                ),
            };
            findings.push(pathology(
                snapshot,
                rule_names::SAME_PATH_EXPENSIVE_WORK_REPEATED,
                vec![node(indexed, ContractRole::Consumer, CapabilityLevel::Partial)],
                vec![step],
                Vec::new(),
                vec![CoverageGap {
                    provider: FactProvider::Runtime,
                    capability: CapabilityLevel::Unknown,
                    reason: "cost, effects, and safe reuse require runtime/effect evidence".to_string(),
                }],
                "profile both occurrences and verify effect-free equivalence before considering reuse",
            ));
        }
    }
}

const FLATTEN: &[&str] = &[
    "flatten",
    "ravel",
    "flat",
    "concat",
    "concatenate",
    "chain",
    "vcat",
    "hcat",
    "vstack",
    "hstack",
    "stack",
];

fn detect_test_dimension_gap(
    snapshot: &str,
    index: &SnapshotIndex<'_>,
    findings: &mut Vec<SnapshotPathology>,
) {
    for production in &index.functions {
        if is_test(&production.file.path, production.function)
            || !production
                .function
                .references
                .iter()
                .any(|token| FLATTEN.contains(&token_leaf(token).to_ascii_lowercase().as_str()))
        {
            continue;
        }
        let prefix = format!("{}(", production.function.name);
        let oracle_calls = index
            .functions
            .iter()
            .filter(|candidate| is_test(&candidate.file.path, candidate.function))
            .flat_map(|candidate| candidate.function.call_texts.keys())
            .filter(|call| call.contains(&prefix))
            .count();
        if oracle_calls >= 2 {
            continue;
        }
        let step = ContractStep {
            edge: ContractEdgeKind::Exercises,
            node: node(production, ContractRole::Owner, CapabilityLevel::Partial),
            token: Some(production.function.name.clone()),
            detail: format!(
                "current flattening path `{}` has {oracle_calls} structurally distinct test invocation(s); a companion-sensitive partition oracle is not observed",
                production.function.name
            ),
        };
        findings.push(pathology(
            snapshot,
            rule_names::TEST_CONTRACT_DIMENSION_UNCOVERED,
            vec![node(production, ContractRole::Owner, CapabilityLevel::Partial)],
            vec![step],
            Vec::new(),
            vec![syntax_gap(
                "test discovery is textual and the semantic partition axis is not adapter-attested",
            )],
            "add a metamorphic oracle: hold one partition fixed, vary companion partitions, and compare the fixed result",
        ));
    }
}

fn build_summaries(snapshot: &str, findings: &[SnapshotPathology]) -> Vec<SnapshotPathology> {
    let mut by_path: BTreeMap<&std::path::Path, Vec<&SnapshotPathology>> = BTreeMap::new();
    for finding in findings {
        if let Some(anchor) = finding.anchors.first() {
            by_path
                .entry(anchor.path.as_path())
                .or_default()
                .push(finding);
        }
    }
    by_path
        .into_iter()
        .filter_map(|(path, grouped)| {
            if grouped.len() < 2 {
                return None;
            }
            let anchors: Vec<ContractNodeRef> = grouped
                .iter()
                .flat_map(|finding| finding.anchors.iter().cloned())
                .collect();
            let first = grouped.first()?.anchors.first()?.clone();
            let rules = grouped
                .iter()
                .map(|finding| finding.rule.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            Some(pathology(
                snapshot,
                rule_names::CONTRACT_CHAIN_INCOMPLETE,
                anchors,
                vec![ContractStep {
                    edge: ContractEdgeKind::Consumes,
                    node: first,
                    token: None,
                    detail: format!(
                        "current contract component {} carries multiple pathologies: {rules}",
                        path.display()
                    ),
                }],
                Vec::new(),
                Vec::new(),
                "verify the producer, consumer, gate, test, telemetry, and publication stages as one end-to-end contract",
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_parse::{ProjectSnapshotBuilder, RepositoryId};

    fn analysis(files: &[(&str, &str)]) -> Arc<ProjectAnalysis> {
        let temp = tempfile::tempdir().unwrap();
        let mut builder = ProjectSnapshotBuilder::new(
            temp.path(),
            RepositoryId::explicit("snapshot-refactor-test").unwrap(),
        )
        .unwrap();
        for (path, source) in files {
            builder = builder
                .with_overlay(path, source.as_bytes().to_vec())
                .unwrap();
        }
        ProjectAnalysis::build(builder.build().unwrap()).unwrap()
    }

    #[test]
    fn current_owner_consumer_split_uses_no_history_claim() {
        let report = analyze_snapshot_refactor(
            "current",
            analysis(&[(
                "scoring.py",
                "def decide(x):\n    return posterior.commit(x)\n\ndef public_score(x):\n    return model.raw_score(x)\n",
            )]),
        )
        .unwrap();
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::OWNER_CONSUMER_CONTRACT_SPLIT)
            .unwrap();
        let text = serde_json::to_string(finding).unwrap();
        for forbidden in ["owner_before", "owner_after", "persistence", "retired"] {
            assert!(
                !text.contains(forbidden),
                "snapshot finding contains {forbidden}: {text}"
            );
        }
    }

    #[test]
    fn current_sibling_admission_asymmetry_is_neutral_and_review_only() {
        let report = analyze_snapshot_refactor(
            "current",
            analysis(&[(
                "gates.jl",
                r#"
function require_save(activity)
    activity.controller_lambda_observations > 0 || throw(ArgumentError("empty"))
    isfinite(activity.controller_lambda_mean) || throw(ArgumentError("mean"))
end
function require_resume(activity)
    isnan(activity.controller_lambda_mean) &&
        iszero(activity.controller_lambda_observations) && return true
    isfinite(activity.controller_lambda_mean) || throw(ArgumentError("mean"))
end
"#,
            )]),
        )
        .unwrap();
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::SIBLING_ADMISSION_GUARDS_ASYMMETRIC)
            .expect("current sibling admission asymmetry");
        assert_eq!(finding.safety, SafetyClass::NeverAuto);
        assert!(finding.pathology_identity().starts_with("rsp1_"));
        let wire = serde_json::to_string(finding).unwrap();
        for forbidden in ["owner_before", "owner_after", "persistence", "retired"] {
            assert!(!wire.contains(forbidden), "{wire}");
        }
    }

    #[test]
    fn history_fixture_after_snapshot_exposes_current_split() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/refactor-history/py-owner-moved-stale/02-after");
        let report = snapshot_refactor_risk_paths(&[root]).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.rule == rule_names::OWNER_CONSUMER_CONTRACT_SPLIT),
            "{report:#?}"
        );
    }

    #[test]
    fn complete_current_adoption_does_not_split() {
        let report = analyze_snapshot_refactor(
            "current",
            analysis(&[(
                "scoring.py",
                "def decide(x):\n    return posterior.commit(x)\n\ndef public_score(x):\n    return posterior.committed_score(x)\n",
            )]),
        )
        .unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.rule != rule_names::OWNER_CONSUMER_CONTRACT_SPLIT)
        );
    }

    #[test]
    fn history_enrichment_keeps_the_snapshot_pathology_identity() {
        let before = analysis(&[(
            "scoring.py",
            "def decide(x):\n    return model.raw_score(x)\n\ndef public_score(x):\n    return model.raw_score(x)\n",
        )]);
        let after = analysis(&[(
            "scoring.py",
            "def decide(x):\n    return posterior.commit(x)\n\ndef public_score(x):\n    return model.raw_score(x)\n",
        )]);
        let snapshot = analyze_snapshot_refactor("current", Arc::clone(&after)).unwrap();
        let history = crate::refactor::analyze_refactor_risk(
            ("before".to_string(), before),
            ("after".to_string(), after),
        )
        .unwrap();
        let snapshot_id = snapshot
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::OWNER_CONSUMER_CONTRACT_SPLIT)
            .unwrap()
            .pathology_identity();
        let history_id = history
            .findings
            .iter()
            .find(|finding| {
                finding.rule == deslop_core::refactor_defect::rule_names::OWNER_MOVED_CONSUMER_STALE
            })
            .unwrap()
            .pathology_identity()
            .unwrap();
        assert_eq!(snapshot_id, history_id);
    }

    #[test]
    fn current_schema_config_lossy_and_duplicate_rules_fire() {
        let report = analyze_snapshot_refactor(
            "current",
            analysis(&[(
                "current.py",
                r#"
DEFAULTS = {"THRESHOLD": 0.5}
def build_manifest(run):
    return {"run_id": run.id, "metrics": {"loss": run.loss}}
def validate_manifest(value):
    return {"run_id", "metric"} <= value.keys()
def public_score(value):
    return round(reconstruct_score(value), 3)
def render(batch):
    a = expensive_transform(preprocess(batch))
    b = expensive_transform(preprocess(batch))
    return a, b
"#,
            )]),
        )
        .unwrap();
        let rules: BTreeSet<&str> = report
            .findings
            .iter()
            .map(|finding| finding.rule.as_str())
            .collect();
        assert!(rules.contains(rule_names::PRODUCER_VERIFIER_SCHEMA_MISMATCH));
        assert!(rules.contains(rule_names::ACCEPTED_CONFIG_NO_BEHAVIORAL_REACH));
        assert!(rules.contains(rule_names::CONFIDENCE_DERIVED_AFTER_LOSSY_COMMIT));
        assert!(rules.contains(rule_names::SAME_PATH_EXPENSIVE_WORK_REPEATED));
    }
}
