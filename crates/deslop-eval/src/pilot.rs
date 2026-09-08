//! P1 read-only pilot evidence: deterministic local import and evaluation.
//!
//! Extends the existing evaluator model with case-level pilot evidence.
//! Read-only: no network, no command execution, no model calls. Imported
//! sources supply bytes only; license and checksum gates run BEFORE any
//! file is written.
//!
//! Independent human annotations are optional: unresolved/model-derived rows
//! are recorded truthfully and never scored as ground truth. The frozen M8
//! and legacy `Clean`/`Sloppy` corpus types are untouched.
//!
//! Digest convention: content digests use the `blake3:` prefix (blake3 is
//! already a workspace dependency and is the digest M8 uses for
//! content-addressed rows). Validation mirrors the strict M8 `sha256:`
//! shape: prefix plus exactly 64 hexadecimal digits.
//!
//! Authority limit: annotator declarations (`reviewer_declared_*`) are
//! self-attested input records, not externally verified identities. The
//! importer checks their shape (two distinct reviewers, blinding/provenance
//! declarations present, adjudication by a third party on disagreement) but
//! cannot prove the named humans exist or are independent. Treat the
//! resulting labels as declared-attestation evidence, never as verified
//! independent ground truth, until an external roster/consent record exists.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use deslop_analyzer::{AnalyzerConfig, scan_source_with_config};
use deslop_core::Lang;
use deslop_parse::SourceFile;
use serde::{Deserialize, Serialize};

use crate::m8_calibration::{LicenseDecision, LicenseRecord};

/// Wire schema for a pilot import bundle (engineering input document).
pub const PILOT_IMPORT_SCHEMA: &str = "deslop.pilot-import/1";
/// Wire schema for one imported pilot case.
pub const PILOT_CASE_SCHEMA: &str = "deslop.pilot-case/1";
/// Wire schema for the deterministic pilot evaluation report.
pub const PILOT_EVAL_SCHEMA: &str = "deslop.pilot-eval/1";

/// First deep-validation slice: exactly these four families, Rust/Python only.
pub const PILOT_FAMILIES: &[&str] = &[
    "duplication",
    "long-method-complexity",
    "wrappers-indirection",
    "branch-simplification",
];

/// Detector rules mapped to each pilot family (existing rule names only).
///
/// Declared capability matrix (BEFORE evaluation, never data-dependent).
///
/// Branch-simplification maps to `reimpl-boolean`, which is emitted ONLY for
/// Clojure sources: Rust/Python branch-simplification cases are stored and
/// reported as `capability: unsupported` with no detection confidence.
///
/// Wrappers-indirection is per-language (Rust offers `redundant-closure` and
/// `useless-format`; Python offers `py-list-comprehension-wrapper`); each
/// stratum records its actual rule subset.
///
/// Threshold-based misses stay misses inside the predeclared supported
/// domain: human truth is NEVER altered from detector output, and no
/// unsupported pattern is filtered post-hoc from findings.
pub fn family_rules(family: &str) -> Option<&'static [&'static str]> {
    Some(match family {
        "duplication" => &["duplicate-block", "near-duplicate"],
        "long-method-complexity" => &["long-method"],
        "wrappers-indirection" => &[
            "redundant-closure",
            "py-list-comprehension-wrapper",
            "useless-format",
        ],
        "branch-simplification" => &["reimpl-boolean"],
        _ => return None,
    })
}

/// Rules actually available for one stratum: the family mapping intersected
/// with per-language emission. Returns the supported subset, which may be
/// empty (branch-simplification on Rust/Python).
pub fn stratum_rules(family: &str, language: PilotLanguage) -> Option<Vec<&'static str>> {
    let rules = family_rules(family)?;
    let supported: Vec<&'static str> = rules
        .iter()
        .copied()
        .filter(|rule| match (*rule, language) {
            ("py-list-comprehension-wrapper", PilotLanguage::Python) => true,
            ("py-list-comprehension-wrapper", PilotLanguage::Rust) => false,
            ("redundant-closure" | "useless-format", PilotLanguage::Rust) => true,
            ("redundant-closure" | "useless-format", PilotLanguage::Python) => false,
            _ => true,
        })
        .collect();
    Some(supported)
}

/// Whether detection confidence exists for one stratum. Branch
/// simplification on Rust/Python has none (`reimpl-boolean` is
/// Clojure-only); those cases are stored, reported unsupported, and never
/// scored as negatives.
pub fn stratum_supported(family: &str, language: PilotLanguage) -> bool {
    stratum_rules(family, language).is_some_and(|rules| !rules.is_empty())
        && !(family == "branch-simplification"
            && matches!(language, PilotLanguage::Rust | PilotLanguage::Python))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObservationLabel {
    Present,
    Absent,
    Uncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CleanupJudgment {
    Desirable,
    Undesirable,
    Uncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeclaredCheckObservation {
    Preserved,
    NotPreserved,
    Unknown,
    Unbound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaseOutcome {
    Evaluated,
    Rejected,
    TimedOut,
    Unparsable,
    Unbuildable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProvenanceKind {
    Natural,
    Challenge,
    Imported,
    /// Clearly synthetic engineering smoke data. Never pilot evidence, never
    /// scored alongside natural/challenge/imported rows.
    EngineeringSynthetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnnotationProvenance {
    IndependentHuman,
    ModelDerived,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PilotSplit {
    Calibration,
    SealedConfirmatory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PilotLanguage {
    Rust,
    Python,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvenanceRecord {
    pub kind: ProvenanceKind,
    #[serde(default)]
    pub detail: String,
}

/// One reviewer's independent record: their own observation AND cleanup
/// judgment, plus their provenance/blinding declarations. Disagreement is
/// derived from these records, never from a caller bit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerRecord {
    pub reviewer_id: String,
    pub observation: ObservationLabel,
    pub cleanup_judgment: CleanupJudgment,
    pub reviewer_declared_provenance: AnnotationProvenance,
    pub reviewer_declared_blinded: bool,
}
/// Adjudication by a third party, required on disagreement. Originals are
/// retained; the resolved labels live beside them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdjudicationRecord {
    pub adjudicator_id: String,
    pub adjudicator_declared_provenance: AnnotationProvenance,
    pub adjudicator_declared_blinded: bool,
    pub resolved_observation: ObservationLabel,
    pub resolved_cleanup: CleanupJudgment,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnnotationRecord {
    pub reviewers: Vec<ReviewerRecord>,
    pub adjudication: Option<AdjudicationRecord>,
}

/// Derived annotation resolution. Originals are never dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedAnnotation {
    pub observation: ObservationLabel,
    pub cleanup: CleanupJudgment,
    pub agreement: bool,
    pub eligible_reviewer_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRange {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
}

/// Identity links for transitive leakage grouping. Every non-empty identity
/// must sit in a single split; the importer unions identities transitively
/// so distinct clone ids cannot bypass a shared family/fork/task/trajectory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeakageIdentities {
    #[serde(default)]
    pub fork_id: String,
    #[serde(default)]
    pub near_clone_id: String,
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub trajectory_id: String,
}

/// Engineering input for one pilot case (human-supplied observations only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PilotCaseInput {
    pub repo_family: String,
    pub clone_group: String,
    #[serde(default)]
    pub leakage: LeakageIdentities,
    pub revision: String,
    pub unit_kind: String,
    pub family: String,
    pub language: PilotLanguage,
    pub source_path: String,
    pub source_text: String,
    /// Caller-selected region under task (1-based, inclusive, validated
    /// against the supplied bytes — never auto-widened to the whole file).
    pub source_range: SourceRangeInput,
    pub task_contract: String,
    pub declared_check: DeclaredCheckObservation,
    pub check_evidence: Option<CheckEvidence>,
    /// Untrusted supplier observation about candidate/check disposition.
    /// Retained separately; never gates static detection, never trims R1
    /// denominators. Actual analyzer status derives from FileReport.
    pub supplier_outcome: CaseOutcome,
    #[serde(default)]
    pub preconditions: Vec<String>,
    #[serde(default)]
    pub counterexamples: Vec<String>,
    pub selection_probability: Option<f64>,
    pub workload_stratum: String,
    pub split: PilotSplit,
    pub annotation: AnnotationRecord,
    pub provenance: ProvenanceRecord,
    pub retrieval_checksum: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRangeInput {
    pub start_line: usize,
    pub end_line: usize,
}

/// Untrusted declared check observation bound to a candidate digest.
/// P1 runs no checks: `declared_check` is a retained supplier observation,
/// never a trusted receipt, never proof of equivalence, never a ProofState.
/// A `Preserved` claim without evidence binding is reported unbound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckEvidence {
    pub candidate_digest: String,
    pub evidence_ref: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PilotImportBundle {
    pub schema: String,
    pub protocol_pin: String,
    pub analyzer_config_pin: String,
    /// Grant covering ALL bundle source bytes.
    pub source_license: LicenseRecord,
    /// Independent grant covering the annotation data (reviewer records).
    /// A source-repo grant is never an annotation-data grant.
    pub annotation_license: LicenseRecord,
    pub cases: Vec<PilotCaseInput>,
}

/// One imported pilot case with derived identities. The source bytes are
/// stored verbatim so evaluation re-runs the analyzer on exactly the
/// imported state (local engineering data only; secrets must be redacted
/// before import per the protocol).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PilotCase {
    pub schema: String,
    pub case_id: String,
    pub repo_family: String,
    pub clone_group: String,
    pub leakage: LeakageIdentities,
    pub revision: String,
    pub unit_kind: String,
    pub family: String,
    pub language: PilotLanguage,
    pub source_range: SourceRange,
    pub source_checksum: String,
    pub source_text: String,
    pub task_contract: String,
    pub declared_check: DeclaredCheckObservation,
    pub check_evidence: Option<CheckEvidence>,
    /// Untrusted supplier observation (see input). Actual analyzer status
    /// derives from FileReport at eval; this never gates detection.
    pub supplier_outcome: CaseOutcome,
    pub preconditions: Vec<String>,
    pub counterexamples: Vec<String>,
    pub selection_probability: Option<f64>,
    pub workload_stratum: String,
    pub split: PilotSplit,
    pub eligibility: PilotEligibility,
    pub annotation: AnnotationRecord,
    pub resolved: ResolvedAnnotation,
    pub provenance: ProvenanceRecord,
    pub retrieval_checksum: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PilotEligibility {
    Eligible,
    PilotOnly,
    SealedConfirmatory,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeCounts {
    pub evaluated: usize,
    pub rejected: usize,
    pub timed_out: usize,
    pub unparsable: usize,
    pub unbuildable: usize,
}

/// Optional-rate detector score for one stratum. Rates are `None` on a zero
/// denominator — never an arbitrary float default. Counts are always present
/// so a zero-eligible stratum still reports its abstention truthfully.
/// Per-unit confusions count once; findings counts stay separate (see
/// `failures`). `recommendation_rate` is a case-positive proxy
/// (positive predictions / input rows), explicitly NOT measured human
/// effort. `analysis_coverage` is successful supported analyses / input
/// rows. Scoring confusions (TP/FP/FN/TN) use eligible known labels only;
/// analysis counters advance for every Complete supported row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StratumScore {
    pub family: String,
    pub language: PilotLanguage,
    pub provenance: ProvenanceKind,
    pub unit_kind: String,
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub true_negatives: usize,
    pub input_total: usize,
    pub eligible_total: usize,
    pub abstained: usize,
    pub unsupported: usize,
    pub analyzed: usize,
    pub predicted_positive: usize,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
    /// Recommendation-count proxy: scored rows / input rows. Explicitly NOT
    /// measured human effort.
    pub recommendation_rate: Option<f64>,
    pub abstention: Option<f64>,
    /// Declared-analyzed/supported-unit fraction: successful supported
    /// analyses / input rows, explicit denominator.
    pub analysis_coverage: Option<f64>,
}

/// Wire schema for the published pilot manifest (strict, versioned).
pub const PILOT_MANIFEST_SCHEMA: &str = "deslop.pilot-manifest/1";

/// Strict typed import manifest: version-checked BEFORE any field is read.
/// Reuses the actual manifest shape (no permissive Value getters).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PilotManifest {
    pub schema: String,
    pub protocol_pin: String,
    pub analyzer_config_pin: String,
    pub source_license: LicenseRecord,
    pub annotation_license: LicenseRecord,
    pub cases: BTreeMap<String, String>,
}

/// Per-case static-analysis prediction evidence. `predicted` is the
/// observable detector verdict on the SELECTED range (family rules
/// intersecting the selected lines); `rules` is the actual rule subset run.
/// Ground-truth scoring is separate and only uses eligible known labels —
/// a synthetic positive prediction stays observable here while truth rates
/// stay null. Sealed rows never appear here (excluded before analysis).
/// `analysis_status` is the actual derived FileReport status; supplier
/// outcomes never appear in this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CasePrediction {
    pub case_id: String,
    pub analysis_status: String,
    pub rules: Vec<String>,
    pub predicted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PilotEvalReport {
    pub schema: String,
    pub protocol_pin: String,
    pub analyzer_config_pin: String,
    pub cases_total: usize,
    pub sealed_excluded: usize,
    pub strata: Vec<StratumScore>,
    /// Supplier-outcome buckets: EVERY unsealed row counted in its own
    /// bucket independently of analysis/scoring (Evaluated included).
    pub supplier_outcomes: OutcomeCounts,
    /// Successful (Complete) supported analyses over unsealed rows.
    pub analyses_complete: usize,
    /// Unsealed rows that reached no analysis (unsupported stratum or
    /// non-Complete FileReport status).
    pub analyses_unavailable: usize,
    /// Per-case prediction evidence for every analyzed row.
    pub predictions: Vec<CasePrediction>,
    pub unresolved_case_ids: Vec<String>,
    /// Root attestation: annotations are declarations; external independence
    /// and confirmatory validation are unproven (not only a module comment).
    pub declaration_note: String,
    /// Clustered-CI unavailable typed note until real protocol data exists.
    pub confidence_interval_note: String,
    /// Sampling contract: unweighted sampled-case rates, never deployment
    /// precision/prevalence.
    pub sampling_note: String,
    pub failures: Vec<String>,
}

fn validate_digest(value: &str, what: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("blake3:") else {
        bail!("{what} must use the blake3: prefix");
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{what} must contain exactly 64 hexadecimal digits");
    }
    Ok(())
}

fn content_digest(text: &str) -> String {
    format!("blake3:{}", blake3::hash(text.as_bytes()).to_hex())
}

fn language_of(language: PilotLanguage) -> Lang {
    match language {
        PilotLanguage::Rust => Lang::Rust,
        PilotLanguage::Python => Lang::Python,
    }
}

fn extension_of(language: PilotLanguage) -> &'static str {
    match language {
        PilotLanguage::Rust => "rs",
        PilotLanguage::Python => "py",
    }
}

fn validate_source_path(path: &str, language: PilotLanguage) -> Result<()> {
    if path.is_empty() || path.starts_with('/') || path.contains("..") {
        bail!("source_path `{path}` must be a relative path without `..`");
    }
    let expected = format!(".{}", extension_of(language));
    if !path.ends_with(expected.as_str()) {
        bail!("source_path `{path}` must end with `{expected}`");
    }
    Ok(())
}

/// Validate the caller-selected byte range against the supplied source:
/// in bounds, non-empty, and valid UTF-8 (the text is already `String`, so
/// this checks line geometry, not encoding). Never widened to the file.
fn validate_source_range(input: &SourceRangeInput, text: &str, path: &str) -> Result<SourceRange> {
    let total = text.lines().count().max(1);
    if input.start_line == 0 || input.end_line == 0 {
        bail!("source_range for `{path}` is 1-based; zero is out of bounds");
    }
    if input.start_line > input.end_line {
        bail!("source_range for `{path}` starts after it ends");
    }
    if input.end_line > total {
        bail!(
            "source_range for `{path}` ends at line {} but the source has {total} lines",
            input.end_line
        );
    }
    // Language function scope: require the selected lines to be non-blank so
    // a degenerate whitespace range cannot claim a region task.
    let selected: Vec<&str> = text
        .lines()
        .skip(input.start_line - 1)
        .take(input.end_line - input.start_line + 1)
        .collect();
    if selected.iter().all(|line| line.trim().is_empty()) {
        bail!("source_range for `{path}` selects only blank lines");
    }
    Ok(SourceRange {
        path: path.to_string(),
        start_line: input.start_line,
        end_line: input.end_line,
    })
}

/// Full effective analyzer config pin: the canonical JSON of the existing
/// `AnalyzerConfigSnapshot` (resolved behavior, not sparse input), hashed
/// with blake3. Tests compute this pin from the live default rather than
/// pinning incidental values by hand.
pub fn analyzer_config_pin() -> String {
    let snapshot = AnalyzerConfig::default().snapshot();
    let bytes = serde_json::to_vec(&snapshot).expect("canonical config bytes");
    format!("blake3:{}", blake3::hash(&bytes).to_hex())
}

fn check_analyzer_config_pin(pin: &str) -> Result<()> {
    validate_digest(pin, "analyzer_config_pin")?;
    if analyzer_config_pin() != pin {
        bail!(
            "analyzer_config_pin drift: manifest `{pin}` != live `{}`",
            analyzer_config_pin()
        );
    }
    Ok(())
}

/// Deterministic case id over canonical semantic fields incl. source bytes
/// and the pinned protocol + analyzer context. Inputs are sorted by the
/// caller before hashing, so ids are reorder-independent; the output root
/// plays no part.
fn case_id_of(input: &PilotCaseInput, protocol_pin: &str, analyzer_config_pin: &str) -> String {
    let canonical = serde_json::json!([
        input.repo_family,
        input.clone_group,
        input.leakage,
        input.revision,
        input.unit_kind,
        input.family,
        format!("{:?}", input.language),
        input.source_path,
        input.source_text,
        input.source_range,
        input.task_contract,
        format!("{:?}", input.declared_check),
        input.check_evidence,
        format!("{:?}", input.supplier_outcome),
        input.preconditions,
        input.counterexamples,
        input.selection_probability.map(f64::to_bits),
        input.workload_stratum,
        format!("{:?}", input.split),
        input.annotation,
        input.provenance,
        input.retrieval_checksum,
        protocol_pin,
        analyzer_config_pin,
    ]);
    let bytes = serde_json::to_vec(&canonical).expect("canonical case bytes");
    format!("pilot1_{}", blake3::hash(&bytes).to_hex())
}

/// Resolve the annotation from per-reviewer records. Requires at least two
/// distinct reviewers with blinding + provenance declarations. Agreement on
/// both observation and cleanup resolves directly; disagreement requires a
/// third-party adjudicator (distinct id) with a rationale, and the resolved
/// labels come from the adjudicator while originals are retained.
/// Unresolved/model-derived rows resolve to Uncertain and never score.
fn resolve_annotation(
    annotation: &AnnotationRecord,
    provenance: ProvenanceKind,
) -> Result<ResolvedAnnotation> {
    // Distinct per-rater ids: [A, A, B] is rejected (3 records but only 2
    // distinct raters, and the duplicate double-counts one voice).
    if annotation.reviewers.len() < 2 {
        bail!("annotation requires at least two reviewer records");
    }
    let mut ids: Vec<&str> = annotation
        .reviewers
        .iter()
        .map(|reviewer| reviewer.reviewer_id.as_str())
        .collect();
    ids.sort();
    let distinct_before = ids.len();
    ids.dedup();
    if ids.len() != distinct_before {
        bail!("duplicate reviewer id in annotation records");
    }
    if ids.len() < 2 {
        bail!("annotation requires at least two distinct reviewers");
    }
    for reviewer in &annotation.reviewers {
        if reviewer.reviewer_id.trim().is_empty() {
            bail!("reviewer ids must be non-empty");
        }
        if !reviewer.reviewer_declared_blinded {
            bail!(
                "reviewer `{}` must declare blinding to model identity and tool verdict",
                reviewer.reviewer_id
            );
        }
    }
    // Synthetic engineering fixtures are never independent-human evidence,
    // even with two declared humans: raw labels preserved, derived ground
    // truth forced Uncertain with zero eligible reviewers (never scored).
    let synthetic = provenance == ProvenanceKind::EngineeringSynthetic;
    let all_human = !synthetic
        && annotation.reviewers.iter().all(|reviewer| {
            reviewer.reviewer_declared_provenance == AnnotationProvenance::IndependentHuman
        });
    let first = &annotation.reviewers[0];
    let agreed = annotation.reviewers.iter().all(|reviewer| {
        reviewer.observation == first.observation
            && reviewer.cleanup_judgment == first.cleanup_judgment
    });
    if agreed {
        // Non-human agreement keeps the asserted raw labels on the record
        // but derives Uncertain ground truth with zero eligible reviewers.
        let (observation, cleanup, eligible_reviewer_count) = if all_human {
            (first.observation, first.cleanup_judgment, ids.len())
        } else {
            (ObservationLabel::Uncertain, CleanupJudgment::Uncertain, 0)
        };
        return Ok(ResolvedAnnotation {
            observation,
            cleanup,
            agreement: true,
            eligible_reviewer_count,
        });
    }
    // Disagreement: require a distinct third-party adjudicator.
    let Some(adjudication) = &annotation.adjudication else {
        bail!("reviewer disagreement requires third-party adjudication");
    };
    if adjudication.adjudicator_id.trim().is_empty() {
        bail!("adjudicator id must be non-empty");
    }
    if ids.contains(&adjudication.adjudicator_id.as_str()) {
        bail!("adjudicator must be a third party distinct from reviewers");
    }
    if !adjudication.adjudicator_declared_blinded {
        bail!("adjudicator must declare blinding to model identity and tool verdict");
    }
    if adjudication.rationale.trim().is_empty() {
        bail!("adjudication requires a rationale");
    }
    // Adjudicated non-human rows likewise retain raw labels but derive
    // Uncertain ground truth with zero eligible reviewers.
    let human_resolution = all_human
        && adjudication.adjudicator_declared_provenance == AnnotationProvenance::IndependentHuman;
    Ok(ResolvedAnnotation {
        observation: if human_resolution {
            adjudication.resolved_observation
        } else {
            ObservationLabel::Uncertain
        },
        cleanup: if human_resolution {
            adjudication.resolved_cleanup
        } else {
            CleanupJudgment::Uncertain
        },
        agreement: false,
        eligible_reviewer_count: if human_resolution { ids.len() } else { 0 },
    })
}

fn derive_eligibility(
    split: PilotSplit,
    provenance: ProvenanceKind,
    resolved: &ResolvedAnnotation,
) -> PilotEligibility {
    if split == PilotSplit::SealedConfirmatory {
        return PilotEligibility::SealedConfirmatory;
    }
    // Imported preference benchmarks and synthetic engineering fixtures are
    // never eligible detection truth, whatever their reviewer records claim.
    if provenance == ProvenanceKind::Imported || provenance == ProvenanceKind::EngineeringSynthetic
    {
        return PilotEligibility::PilotOnly;
    }
    if resolved.eligible_reviewer_count >= 2 {
        PilotEligibility::Eligible
    } else {
        PilotEligibility::PilotOnly
    }
}

fn validate_input(input: &PilotCaseInput) -> Result<ResolvedAnnotation> {
    if family_rules(&input.family).is_none() {
        bail!(
            "unknown pilot family `{}` (expected one of {})",
            input.family,
            PILOT_FAMILIES.join(", ")
        );
    }
    if input.repo_family.trim().is_empty()
        || input.clone_group.trim().is_empty()
        || input.revision.trim().is_empty()
        || input.unit_kind.trim().is_empty()
        || input.task_contract.trim().is_empty()
        || input.workload_stratum.trim().is_empty()
    {
        bail!(
            "repo_family/clone_group/revision/unit_kind/task_contract/workload_stratum must be non-empty"
        );
    }
    if input.source_text.is_empty() {
        bail!("source_text must be non-empty");
    }
    validate_source_path(&input.source_path, input.language)?;
    validate_source_range(&input.source_range, &input.source_text, &input.source_path)?;
    if let Some(probability) = input.selection_probability {
        // An observed sampled unit cannot have inclusion p=0: require
        // 0 < p <= 1 when Some; None means unknown/non-sampling.
        if !(probability.is_finite() && 0.0 < probability && probability <= 1.0) {
            bail!("selection_probability must be finite with 0 < p <= 1");
        }
    } else if input.provenance.kind == ProvenanceKind::Natural {
        // Natural rows without a selection probability carry no
        // natural-precision evidence: reject at import, never silently pool.
        bail!("natural provenance requires selection_probability");
    }
    let rules = family_rules(&input.family).unwrap_or(&[]);
    for rule in rules {
        if !deslop_core::rules::is_known(rule) {
            bail!(
                "pilot family `{}` maps to unknown rule `{rule}` (stale mapping)",
                input.family
            );
        }
    }
    // Retrieval checksum verified for ALL sources before copy, whatever the
    // provenance: the declared bytes must match the declared digest.
    validate_digest(&input.retrieval_checksum, "retrieval_checksum")?;
    if content_digest(&input.source_text) != input.retrieval_checksum {
        bail!("retrieval_checksum mismatch for `{}`", input.source_path);
    }
    validate_declared_check(input)?;
    resolve_annotation(&input.annotation, input.provenance.kind)
}

/// Declared check observations are untrusted supplier metadata: P1 runs no
/// checks, manufactures no trusted receipts, no ProofState, no P2 authority.
/// A `Preserved` claim must bind to a candidate digest + evidence reference;
/// anything less is reported unbound (`Unbound`) or `Unknown` explicitly.
fn validate_declared_check(input: &PilotCaseInput) -> Result<()> {
    match (&input.declared_check, &input.check_evidence) {
        (DeclaredCheckObservation::Preserved, None) => {
            bail!("declared Preserved check requires check_evidence binding");
        }
        (DeclaredCheckObservation::Preserved, Some(evidence)) => {
            validate_digest(&evidence.candidate_digest, "candidate_digest")?;
            if evidence.evidence_ref.trim().is_empty() {
                bail!("check_evidence requires a non-empty evidence_ref");
            }
        }
        (DeclaredCheckObservation::Unbound, Some(_)) => {
            bail!("Unbound declared check must not carry check_evidence");
        }
        _ => {}
    }
    Ok(())
}

fn check_license(bundle: &PilotImportBundle) -> Result<()> {
    // License attestation gate BEFORE any write: TWO explicit approved
    // grants with evidence — one for source bytes, one for annotation data.
    // Never a bare SPDX string, never ambient repo metadata. `reason` is a
    // free human explanation, never parsed for policy.
    check_grant(&bundle.source_license, "source")?;
    check_grant(&bundle.annotation_license, "annotation")?;
    Ok(())
}

fn check_grant(record: &LicenseRecord, role: &str) -> Result<()> {
    if record.decision != LicenseDecision::Approved {
        bail!("pilot import blocked: {role} license decision is not approved");
    }
    if record.spdx.as_deref().is_none_or(str::is_empty) {
        bail!("pilot import blocked: approved {role} license requires an SPDX expression");
    }
    if record.evidence_uri.trim().is_empty() {
        bail!("pilot import blocked: approved {role} license requires evidence_uri");
    }
    if record.reason.trim().is_empty() {
        bail!("pilot import blocked: approved {role} license requires a reason");
    }
    if !is_valid_checked_on(&record.checked_on) {
        bail!(
            "pilot import blocked: {role} license checked_on `{}` is not a valid YYYY-MM-DD calendar date",
            record.checked_on
        );
    }
    Ok(())
}

/// Validate `checked_on` as an accurate YYYY-MM-DD calendar date. Boring
/// hand-rolled civil-date math — no new date dependency for one gate. No
/// staleness judgment: expiry policy belongs to a declared policy file, not
/// to this importer.
fn is_valid_checked_on(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    let [year, month, day]: [&str; 3] = match parts.as_slice() {
        [year, month, day] => [*year, *month, *day],
        _ => return false,
    };
    let (Ok(year), Ok(month), Ok(day)) = (
        year.parse::<i64>(),
        month.parse::<u32>(),
        day.parse::<u32>(),
    ) else {
        return false;
    };
    if !(1900..=2100).contains(&year) || !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => return false,
    };
    day <= max_day
}

fn to_case(
    input: &PilotCaseInput,
    resolved: ResolvedAnnotation,
    protocol_pin: &str,
    analyzer_config_pin: &str,
) -> PilotCase {
    let range = validate_source_range(&input.source_range, &input.source_text, &input.source_path)
        .expect("range validated before conversion");
    PilotCase {
        schema: PILOT_CASE_SCHEMA.to_string(),
        case_id: case_id_of(input, protocol_pin, analyzer_config_pin),
        repo_family: input.repo_family.clone(),
        clone_group: input.clone_group.clone(),
        leakage: input.leakage.clone(),
        revision: input.revision.clone(),
        unit_kind: input.unit_kind.clone(),
        family: input.family.clone(),
        language: input.language,
        source_range: range,
        source_checksum: content_digest(&input.source_text),
        source_text: input.source_text.clone(),
        task_contract: input.task_contract.clone(),
        declared_check: input.declared_check,
        check_evidence: input.check_evidence.clone(),
        supplier_outcome: input.supplier_outcome,
        preconditions: input.preconditions.clone(),
        counterexamples: input.counterexamples.clone(),
        selection_probability: input.selection_probability,
        workload_stratum: input.workload_stratum.clone(),
        split: input.split,
        eligibility: derive_eligibility(input.split, input.provenance.kind, &resolved),
        annotation: input.annotation.clone(),
        resolved,
        provenance: input.provenance.clone(),
        retrieval_checksum: input.retrieval_checksum.clone(),
    }
}

/// Transitive leakage groups: union repo_family, clone_group, and every
/// non-empty fork/near-clone/task/trajectory identity. Each group must sit
/// in exactly one split, and exact duplicate source content under one
/// revision must not straddle splits either.
fn check_leakage(inputs: &[PilotCaseInput]) -> Result<()> {
    // Union-find over input indices.
    let mut parent: Vec<usize> = (0..inputs.len()).collect();
    fn find(parent: &mut [usize], mut node: usize) -> usize {
        while parent[node] != node {
            parent[node] = parent[parent[node]];
            node = parent[node];
        }
        node
    }
    fn union(parent: &mut [usize], left: usize, right: usize) {
        let (mut a, mut b) = (find(parent, left), find(parent, right));
        if a == b {
            return;
        }
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        parent[b] = a;
    }
    let mut key_first: BTreeMap<String, usize> = BTreeMap::new();
    for (index, input) in inputs.iter().enumerate() {
        let mut keys = vec![
            format!("family:{}", input.repo_family),
            format!("clone:{}", input.clone_group),
        ];
        if !input.leakage.fork_id.trim().is_empty() {
            keys.push(format!("fork:{}", input.leakage.fork_id));
        }
        if !input.leakage.near_clone_id.trim().is_empty() {
            keys.push(format!("nearclone:{}", input.leakage.near_clone_id));
        }
        if !input.leakage.task_id.trim().is_empty() {
            keys.push(format!("task:{}", input.leakage.task_id));
        }
        if !input.leakage.trajectory_id.trim().is_empty() {
            keys.push(format!("trajectory:{}", input.leakage.trajectory_id));
        }
        // Exact duplicate source content under one revision is the same unit.
        keys.push(format!(
            "content:{}:{}",
            input.revision,
            content_digest(&input.source_text)
        ));
        for key in keys {
            if let Some(first) = key_first.get(&key) {
                union(&mut parent, *first, index);
            } else {
                key_first.insert(key, index);
            }
        }
    }
    let mut group_splits: BTreeMap<usize, BTreeSet<PilotSplit>> = BTreeMap::new();
    for (index, input) in inputs.iter().enumerate() {
        group_splits
            .entry(find(&mut parent, index))
            .or_default()
            .insert(input.split);
    }
    for splits in group_splits.values() {
        if splits.len() > 1 {
            bail!(
                "leakage group (shared family/fork/near-clone/task/trajectory/content) spans multiple splits"
            );
        }
    }
    Ok(())
}

/// Reject exact duplicate declared-unit evidence records. The declared
/// case-unit is (repo_family, revision, source digest, path, range, family,
/// unit_kind, provenance, task_contract): two rows declaring the SAME unit
/// are a data error, not two independent truths. Distinct units/contexts —
/// including unrelated repos selecting identical lines — must NOT collapse;
/// the identity carries repo/source identity precisely so they do not.
/// No blanket overlap filter, no post-hoc FN exclusion.
fn check_duplicate_units(inputs: &[PilotCaseInput]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for input in inputs {
        let key = (
            input.repo_family.clone(),
            input.revision.clone(),
            content_digest(&input.source_text),
            input.source_path.clone(),
            input.source_range.start_line,
            input.source_range.end_line,
            input.family.clone(),
            format!("{:?}", input.language),
            format!("{:?}", input.provenance.kind),
            input.task_contract.clone(),
        );
        if !seen.insert(key) {
            bail!(
                "duplicate declared case-unit for `{}` (same repo/revision/source/path/range/family/unit/provenance/contract)",
                input.source_path
            );
        }
    }
    Ok(())
}

/// Import a bundle into `dir`, writing deterministic `cases/<case_id>.json`.
/// The license gate runs before any file is created; on any validation error
/// nothing is written.
pub fn import_bundle(bundle_path: &Path, dir: &Path) -> Result<Vec<PilotCase>> {
    let text = fs::read_to_string(bundle_path)
        .with_context(|| format!("failed to read {}", bundle_path.display()))?;
    let bundle: PilotImportBundle = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", bundle_path.display()))?;
    if bundle.schema != PILOT_IMPORT_SCHEMA {
        bail!("unsupported pilot import schema `{}`", bundle.schema);
    }
    if bundle.protocol_pin.trim().is_empty() {
        bail!("protocol_pin must be non-empty");
    }
    check_analyzer_config_pin(&bundle.analyzer_config_pin)?;
    check_license(&bundle)?;
    if bundle.cases.is_empty() {
        bail!("pilot import bundle carries no cases");
    }

    // Validate everything before touching the filesystem.
    let mut sorted = bundle.cases.clone();
    sorted.sort_by(|a, b| {
        (
            &a.repo_family,
            &a.clone_group,
            &a.source_path,
            &a.source_text,
            &a.task_contract,
        )
            .cmp(&(
                &b.repo_family,
                &b.clone_group,
                &b.source_path,
                &b.source_text,
                &b.task_contract,
            ))
    });
    let mut resolved_list = Vec::with_capacity(sorted.len());
    for input in &sorted {
        resolved_list.push(validate_input(input)?);
    }
    check_leakage(&sorted)?;
    check_duplicate_units(&sorted)?;

    let mut cases: Vec<PilotCase> = sorted
        .iter()
        .zip(resolved_list)
        .map(|(input, resolved)| {
            to_case(
                input,
                resolved,
                &bundle.protocol_pin,
                &bundle.analyzer_config_pin,
            )
        })
        .collect();
    // Duplicate content collapses to one case id: report, don't double-count.
    let mut seen = BTreeSet::new();
    for case in &cases {
        if !seen.insert(case.case_id.clone()) {
            bail!(
                "duplicate pilot case content (case_id `{}` twice)",
                case.case_id
            );
        }
    }
    cases.sort_by(|a, b| a.case_id.cmp(&b.case_id));

    // All checks passed: stage into a temp sibling, then atomically publish.
    // Manifest relative paths are confined to `cases/<case_id>.json` (see
    // evaluate_dir): no caller-controlled traversal survives import.
    // The stage sits BESIDE dir (not inside it): publishing clears dir, so
    // an inside stage would be deleted with it.
    let stage = dir.with_extension("stage-pilot-import");
    if stage.exists() {
        fs::remove_dir_all(&stage)
            .with_context(|| format!("failed to clear {}", stage.display()))?;
    }
    let stage_cases = stage.join("cases");
    fs::create_dir_all(&stage_cases)
        .with_context(|| format!("failed to create {}", stage_cases.display()))?;
    let mut manifest = BTreeMap::<String, String>::new();
    for case in &cases {
        assert_case_id_shape(&case.case_id)?;
        let path = stage_cases.join(format!("{}.json", case.case_id));
        let rendered = serde_json::to_string_pretty(case)?;
        fs::write(&path, format!("{rendered}\n"))
            .with_context(|| format!("failed to write {}", path.display()))?;
        manifest.insert(case.case_id.clone(), format!("cases/{}.json", case.case_id));
    }
    let header = serde_json::json!({
        "schema": PILOT_MANIFEST_SCHEMA,
        "protocol_pin": bundle.protocol_pin,
        "analyzer_config_pin": bundle.analyzer_config_pin,
        "source_license": bundle.source_license,
        "annotation_license": bundle.annotation_license,
        "cases": manifest,
    });
    fs::write(
        stage.join("manifest.json"),
        format!("{}\n", serde_json::to_string_pretty(&header)?),
    )
    .with_context(|| format!("failed to write {}", stage.join("manifest.json").display()))?;
    if dir.exists() {
        fs::remove_dir_all(dir).with_context(|| format!("failed to clear {}", dir.display()))?;
    }
    fs::create_dir_all(dir.parent().unwrap_or(Path::new(".")))
        .with_context(|| format!("failed to create parent of {}", dir.display()))?;
    fs::rename(&stage, dir)
        .with_context(|| format!("failed to publish {} to {}", stage.display(), dir.display()))?;
    Ok(cases)
}

/// Case ids are importer-derived (`pilot1_` + 16 hex chars): anything else
/// in a manifest path is rejected before any filesystem join.
fn assert_case_id_shape(case_id: &str) -> Result<()> {
    let suffix = case_id.strip_prefix("pilot1_").context("bad case id")?;
    if suffix.len() != 64 || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("bad case id `{case_id}`");
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
struct Tally {
    true_positives: usize,
    false_positives: usize,
    false_negatives: usize,
    true_negatives: usize,
    abstained: usize,
    /// Supported-stratum rows with no detection confidence (currently
    /// branch-simplification on Rust/Python): stored and counted, never
    /// scored.
    unsupported: usize,
    /// Rows in this stratum reaching Complete supported analysis
    /// (independent of eligibility).
    analyzed: usize,
    /// Rows in this stratum with a positive range-constrained prediction
    /// (independent of eligibility).
    predicted_positive: usize,
}

/// Canonical identity for one declared case unit. Keeping this as a named
/// value avoids repeating a high-arity tuple and makes duplicate detection
/// reviewable without changing its equality/ordering semantics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct UnitIdentity {
    repo_family: String,
    revision: String,
    source_checksum: String,
    path: String,
    start_line: usize,
    end_line: usize,
    family: String,
    unit_kind: String,
    provenance: u8,
    task_contract: String,
}

fn rate(numerator: usize, denominator: usize) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        Some(numerator as f64 / denominator as f64)
    }
}

/// Sampling contract for reported rates. Rates are UNWEIGHTED sampled-case
/// descriptive estimates — explicitly NOT deployment precision, NOT
/// prevalence. Unequal selection probabilities and workload stratification
/// are disclosed, never silently pooled into population claims; no
/// weighted/clustered CI is invented without a declared protocol (the
/// confirmatory freeze is externally blocked). Natural rows missing a
/// selection probability carry no natural-precision evidence.
const SAMPLING_NOTE: &str = "sampling: reported rates are unweighted sampled-case descriptive estimates, not deployment precision or prevalence; unequal selection probabilities and workload strata are disclosed per row and never pooled into population claims; no weighted or clustered interval is computed without a declared protocol.";

/// Score validated cases (private). The ONLY supported entries are
/// [`import_bundle`] (license gate + validation before any write) and
/// [`evaluate_dir`] (manifest license re-check, pin checks, confined load,
/// per-row revalidation). There is intentionally no public in-memory entry:
/// callers cannot bypass the license gate with hand-built rows.
///
/// Detection scoring is independent of declared check/supplier outcomes:
/// `declared_check`/`supplier_outcome` never change a detection tally. Every
/// outcome is counted; supplier observations appear in outcome buckets rather
/// than vanishing from denominators. Conditional detector precision uses
/// predicted-positive cases only; recall uses all eligible ground-truth
/// cases including misses. Results are stratified by (family, language,
/// provenance) and never pooled.
fn evaluate_cases(
    cases: &[PilotCase],
    protocol_pin: &str,
    analyzer_config_pin: &str,
) -> Result<PilotEvalReport> {
    let mut sorted: Vec<&PilotCase> = cases.iter().collect();
    sorted.sort_by(|a, b| a.case_id.cmp(&b.case_id));

    // The evaluation must run under the pinned FULL effective analyzer
    // config: recompute the live pin and refuse drift instead of silently
    // scoring under new defaults.
    check_analyzer_config_pin(analyzer_config_pin)?;

    // Case-unit estimand: one declared case-unit (repo/revision/source/path/
    // range/family/unit/provenance/contract) is one unit of evidence.
    // Duplicate declared units are rejected at import AND at eval (defense
    // against hand-mutated stores); distinct units/contexts each count.
    // No independence or CI claim: rates are conditional sampled-case
    // descriptives with explicit disjoint denominators.
    let mut supplier_outcomes = OutcomeCounts::default();
    let mut failures: Vec<String> = Vec::new();
    let mut unresolved_case_ids: Vec<String> = Vec::new();
    let mut sealed_excluded = 0usize;
    let mut analyses_complete = 0usize;
    let mut analyses_unavailable = 0usize;
    let mut predictions: Vec<CasePrediction> = Vec::new();
    let mut tallies: BTreeMap<(String, PilotLanguage, ProvenanceKind, String), Tally> =
        BTreeMap::new();
    let mut cover_seen: BTreeMap<
        (String, PilotLanguage, ProvenanceKind, String),
        BTreeSet<String>,
    > = BTreeMap::new();
    let mut cover_total: BTreeMap<(String, PilotLanguage, ProvenanceKind, String), usize> =
        BTreeMap::new();
    let mut seen_units: BTreeSet<UnitIdentity> = BTreeSet::new();

    for case in sorted {
        // Identity/digest revalidation runs on EVERY row incl. sealed: a
        // mutated sealed JSON fails instead of counting toward the holdout.
        // Sealed rows are then excluded BEFORE analysis AND before
        // supplier-outcome/label counters: integrity hashing is not
        // statistical use, and sealed labels never leak into counts.
        revalidate_case(case, protocol_pin, analyzer_config_pin)?;
        if case.split == PilotSplit::SealedConfirmatory {
            sealed_excluded += 1;
            continue;
        }
        // Duplicate declared units rejected at eval too (mutated stores).
        let unit_key = UnitIdentity {
            repo_family: case.repo_family.clone(),
            revision: case.revision.clone(),
            source_checksum: case.source_checksum.clone(),
            path: case.source_range.path.clone(),
            start_line: case.source_range.start_line,
            end_line: case.source_range.end_line,
            family: case.family.clone(),
            unit_kind: case.unit_kind.clone(),
            provenance: case.provenance.kind as u8,
            task_contract: case.task_contract.clone(),
        };
        if !seen_units.insert(unit_key) {
            bail!("duplicate declared case-unit for case `{}`", case.case_id);
        }
        // Supplier outcomes: EVERY unsealed row counted in its OWN bucket
        // independently of analysis/scoring. Actual analysis and scored
        // counts are tracked separately below.
        match case.supplier_outcome {
            CaseOutcome::Evaluated => supplier_outcomes.evaluated += 1,
            CaseOutcome::Rejected => supplier_outcomes.rejected += 1,
            CaseOutcome::TimedOut => supplier_outcomes.timed_out += 1,
            CaseOutcome::Unparsable => supplier_outcomes.unparsable += 1,
            CaseOutcome::Unbuildable => supplier_outcomes.unbuildable += 1,
        }
        // Declared capability BEFORE analysis (never data-dependent).
        let Some(rules) = stratum_rules(&case.family, case.language) else {
            let key = (
                case.family.clone(),
                case.language,
                case.provenance.kind,
                case.unit_kind.clone(),
            );
            *cover_total.entry(key).or_default() += 1;
            failures.push(format!(
                "{}: unknown family `{}`",
                case.case_id, case.family
            ));
            continue;
        };
        // Live rule-name check at eval time too: a mapping that rotted since
        // import fails loudly instead of scoring recall 0. All-negative
        // corpora remain legitimate — this checks names, not hit rates.
        for rule in &rules {
            if !deslop_core::rules::is_known(rule) {
                bail!(
                    "pilot family `{}` maps to unknown rule `{rule}` (stale mapping)",
                    case.family
                );
            }
        }
        let key = (
            case.family.clone(),
            case.language,
            case.provenance.kind,
            case.unit_kind.clone(),
        );
        *cover_total.entry(key.clone()).or_default() += 1;
        if !stratum_supported(&case.family, case.language) {
            // Unsupported stratum/context: explicit NO prediction. Stored,
            // counted, reported — human truth is NOT altered.
            tallies.entry(key).or_default().unsupported += 1;
            analyses_unavailable += 1;
            continue;
        }
        // Static ANALYSIS runs for EVERY unsealed supported case with
        // available capability, irrespective of annotation eligibility.
        // Ground-truth SCORING below uses eligible known labels only.
        let source = SourceFile::new_with_lang(
            PathBuf::from(&case.source_range.path),
            case.source_text.clone(),
            language_of(case.language),
        );
        // Isolated-file scope: scan_source sees one file only. Cross-file
        // truth needs a pinned multi-file scope, which is explicitly
        // unavailable — never faked as complete-project evaluation.
        let report = scan_source_with_config(&source, AnalyzerConfig::default());
        // Actual analyzer status derives from FileReport: non-Complete goes
        // to analysis abstention + diagnostics, never to TN/FN.
        if report.analysis.status != deslop_core::AnalysisStatus::Complete {
            tallies.entry(key).or_default().abstained += 1;
            analyses_unavailable += 1;
            failures.push(format!(
                "{}: analysis {:?} ({} diagnostics)",
                case.case_id,
                report.analysis.status,
                report.analysis.diagnostics.len()
            ));
            continue;
        }
        analyses_complete += 1;
        // Range-constrained hit: only findings whose span overlaps the
        // SELECTED range count (existing evaluator overlap convention: a
        // finding matches when its span intersects the selected lines). A
        // positive elsewhere in the file never leaks into this unit.
        let start = case.source_range.start_line;
        let end = case.source_range.end_line;
        let hit = report.findings.iter().any(|finding| {
            rules.contains(&finding.rule.as_str())
                && finding.span.start_line <= end
                && finding.span.end_line >= start
        });
        predictions.push(CasePrediction {
            case_id: case.case_id.clone(),
            analysis_status: format!("{:?}", report.analysis.status),
            rules: rules.iter().map(|rule| rule.to_string()).collect(),
            predicted: hit,
        });
        // Analysis counters advance for EVERY Complete supported row,
        // independent of annotation eligibility: positive-prediction and
        // analysis coverage below derive from observable predictions, not
        // from scoring confusions.
        {
            let tally = tallies.entry(key.clone()).or_default();
            tally.analyzed += 1;
            if hit {
                tally.predicted_positive += 1;
            }
        }
        // Ground-truth scoring: eligible known labels ONLY. Synthetic,
        // model-derived, unresolved, uncertain, and abstained rows are
        // truthfully reported elsewhere and never score here — while their
        // predictions above stay observable.
        let eligible = case.eligibility == PilotEligibility::Eligible
            && case.resolved.eligible_reviewer_count >= 2
            && case.resolved.observation != ObservationLabel::Uncertain;
        if !eligible {
            if case.eligibility != PilotEligibility::Eligible
                || case.resolved.eligible_reviewer_count < 2
            {
                unresolved_case_ids.push(case.case_id.clone());
            } else {
                tallies.entry(key).or_default().abstained += 1;
            }
            continue;
        }
        let tally = tallies.entry(key.clone()).or_default();
        match case.resolved.observation {
            ObservationLabel::Present if hit => {
                tally.true_positives += 1;
                cover_seen
                    .entry(key)
                    .or_default()
                    .insert(case.case_id.clone());
            }
            ObservationLabel::Present => tally.false_negatives += 1,
            ObservationLabel::Absent if hit => tally.false_positives += 1,
            ObservationLabel::Absent => tally.true_negatives += 1,
            ObservationLabel::Uncertain => tally.abstained += 1,
        }
    }

    let mut strata: Vec<StratumScore> = Vec::new();
    let mut keys: Vec<(String, PilotLanguage, ProvenanceKind, String)> =
        cover_total.keys().cloned().collect();
    keys.sort_by(|a, b| {
        (&a.0, a.1 as u8, a.2 as u8, &a.3).cmp(&(&b.0, b.1 as u8, b.2 as u8, &b.3))
    });
    for key in keys {
        let tally = tallies.get(&key).cloned().unwrap_or_default();
        // Disjoint denominators, conditional on complete supported analysis
        // plus known eligible labels (never pooled, never double-counted):
        // - input_total: every unsealed imported row in this stratum.
        // - analyses_complete is report-global; per-stratum scored rows all
        //   reached Complete analysis by construction.
        // - eligible_total: scored rows + abstained rows (certain-eligible
        //   denominator incl. analysis abstention).
        // - scored: rows reaching a detector verdict (TP+FP+FN+TN).
        // Conditional detector precision = TP / predicted-positive (TP+FP);
        // recall = TP / ground-truth-positive (TP+FN).
        // analysis_coverage = successful supported analyses / input rows
        // (explicit basis). recommendation_rate is a recommendation-count
        // proxy (scored / input), explicitly NOT measured human effort.
        // Zero denominators yield null, never zero.
        let total = cover_total.get(&key).copied().unwrap_or(0);
        let scored = tally.true_positives
            + tally.false_positives
            + tally.false_negatives
            + tally.true_negatives;
        strata.push(StratumScore {
            family: key.0.clone(),
            language: key.1,
            provenance: key.2,
            unit_kind: key.3.clone(),
            true_positives: tally.true_positives,
            false_positives: tally.false_positives,
            false_negatives: tally.false_negatives,
            true_negatives: tally.true_negatives,
            input_total: total,
            eligible_total: scored + tally.abstained,
            abstained: tally.abstained,
            unsupported: tally.unsupported,
            analyzed: tally.analyzed,
            predicted_positive: tally.predicted_positive,
            precision: rate(
                tally.true_positives,
                tally.true_positives + tally.false_positives,
            ),
            recall: rate(
                tally.true_positives,
                tally.true_positives + tally.false_negatives,
            ),
            // Case-positive proxy: positive predictions / input rows
            // (observable predictions, independent of eligibility).
            recommendation_rate: rate(tally.predicted_positive, total),
            abstention: rate(tally.abstained, total),
            // Successful supported analyses / input rows (explicit basis,
            // independent of eligibility).
            analysis_coverage: rate(tally.analyzed, total),
        });
    }
    unresolved_case_ids.sort();
    failures.sort();
    predictions.sort_by(|a, b| a.case_id.cmp(&b.case_id));

    Ok(PilotEvalReport {
        schema: PILOT_EVAL_SCHEMA.to_string(),
        protocol_pin: protocol_pin.to_string(),
        analyzer_config_pin: analyzer_config_pin.to_string(),
        cases_total: cases.len(),
        sealed_excluded,
        strata,
        supplier_outcomes,
        analyses_complete,
        analyses_unavailable,
        predictions,
        unresolved_case_ids,
        declaration_note: DECLARATION_NOTE.to_string(),
        confidence_interval_note: CONFIDENCE_INTERVAL_NOTE.to_string(),
        sampling_note: SAMPLING_NOTE.to_string(),
        failures,
    })
}

/// Root-level attestation: annotations are self-declared records, not
/// externally verified independence; confirmatory validation is unproven.
const DECLARATION_NOTE: &str = "Annotations are self-declared reviewer records (declared provenance/blinding/identities); external independence is unverified and confirmatory validation is unproven until a roster, consent record, and frozen protocol exist. Scores use declared-attestation ground truth only where eligibility holds; all other rows are reported, never scored.";
/// Clustered-CI unavailable: typed note until real protocol data exists.
const CONFIDENCE_INTERVAL_NOTE: &str = "clustered-confidence-unavailable: no repository/task clustering model on local engineering data; rates are point estimates with explicit disjoint denominators; zero denominators are null. A confirmatory interval requires pilot variance plus owner sign-off under the frozen protocol.";

/// Revalidate a stored case at eval time: schema, digests, case identity,
/// source range, reviewer records, and derived resolution/eligibility. A
/// mutated JSON row fails instead of scoring under edited labels. The
/// bundle-level license is re-checked by the caller via the manifest.
fn revalidate_case(case: &PilotCase, protocol_pin: &str, analyzer_config_pin: &str) -> Result<()> {
    if case.schema != PILOT_CASE_SCHEMA {
        bail!(
            "case `{}` has unsupported schema `{}`",
            case.case_id,
            case.schema
        );
    }
    assert_case_id_shape(&case.case_id)?;
    validate_source_path(&case.source_range.path, case.language)?;
    let range_input = SourceRangeInput {
        start_line: case.source_range.start_line,
        end_line: case.source_range.end_line,
    };
    validate_source_range(&range_input, &case.source_text, &case.source_range.path)?;
    validate_digest(&case.retrieval_checksum, "retrieval_checksum")?;
    if content_digest(&case.source_text) != case.retrieval_checksum {
        bail!("retrieval_checksum mismatch for case `{}`", case.case_id);
    }
    if content_digest(&case.source_text) != case.source_checksum {
        bail!("source_checksum mismatch for case `{}`", case.case_id);
    }
    // Rebuild the input view and recompute identity + resolution.
    let rebuilt = PilotCaseInput {
        repo_family: case.repo_family.clone(),
        clone_group: case.clone_group.clone(),
        leakage: case.leakage.clone(),
        revision: case.revision.clone(),
        unit_kind: case.unit_kind.clone(),
        family: case.family.clone(),
        language: case.language,
        source_path: case.source_range.path.clone(),
        source_text: case.source_text.clone(),
        source_range: range_input,
        task_contract: case.task_contract.clone(),
        declared_check: case.declared_check,
        check_evidence: case.check_evidence.clone(),
        supplier_outcome: case.supplier_outcome,
        preconditions: case.preconditions.clone(),
        counterexamples: case.counterexamples.clone(),
        selection_probability: case.selection_probability,
        workload_stratum: case.workload_stratum.clone(),
        split: case.split,
        annotation: case.annotation.clone(),
        provenance: case.provenance.clone(),
        retrieval_checksum: case.retrieval_checksum.clone(),
    };
    let resolved = resolve_annotation(&case.annotation, case.provenance.kind)?;
    if resolved != case.resolved {
        bail!("annotation resolution mismatch for case `{}`", case.case_id);
    }
    let eligibility = derive_eligibility(case.split, case.provenance.kind, &resolved);
    if eligibility != case.eligibility {
        bail!("eligibility mismatch for case `{}`", case.case_id);
    }
    if case_id_of(&rebuilt, protocol_pin, analyzer_config_pin) != case.case_id {
        bail!("case identity mismatch for `{}`", case.case_id);
    }
    Ok(())
}
/// Evaluate an imported pilot directory: load cases in manifest order,
/// require the declared `protocol_pin` plus the pinned analyzer config, and
/// re-check the bundle license from the manifest before scoring. Manifest
/// relative paths are confined to `cases/<case_id>.json`; the report is
/// written atomically via a temp file + rename.
pub fn evaluate_dir(dir: &Path, protocol_pin: &str) -> Result<PilotEvalReport> {
    let manifest_text = fs::read_to_string(dir.join("manifest.json"))
        .with_context(|| format!("failed to read {}", dir.join("manifest.json").display()))?;
    let manifest: PilotManifest = serde_json::from_str(&manifest_text)
        .context("pilot manifest is not a strict pilot manifest")?;
    if manifest.schema != PILOT_MANIFEST_SCHEMA {
        bail!("unsupported pilot manifest schema `{}`", manifest.schema);
    }
    if manifest.protocol_pin != protocol_pin {
        bail!(
            "protocol_pin mismatch: manifest `{}` != declared `{protocol_pin}`",
            manifest.protocol_pin
        );
    }
    check_analyzer_config_pin(&manifest.analyzer_config_pin)?;
    // Dual license re-checked at eval: a swapped-in manifest cannot score
    // under grants that were never given.
    check_license(&PilotImportBundle {
        schema: PILOT_IMPORT_SCHEMA.to_string(),
        protocol_pin: manifest.protocol_pin.clone(),
        analyzer_config_pin: manifest.analyzer_config_pin.clone(),
        source_license: manifest.source_license.clone(),
        annotation_license: manifest.annotation_license.clone(),
        cases: Vec::new(),
    })?;
    let config_pin = manifest.analyzer_config_pin.clone();
    let entries = manifest.cases;
    let mut case_ids: Vec<&String> = entries.keys().collect();
    case_ids.sort();
    let mut cases = Vec::with_capacity(case_ids.len());
    for case_id in case_ids {
        assert_case_id_shape(case_id)?;
        let rel = entries
            .get(case_id)
            .with_context(|| format!("pilot manifest entry `{case_id}` is not a path"))?;
        // Confine to data-only `cases/<case_id>.json`: reject traversal,
        // absolute paths, and any entry that does not name its own case id.
        if *rel != format!("cases/{case_id}.json") {
            bail!("pilot manifest entry `{case_id}` escapes cases/ confinement");
        }
        let case: PilotCase = serde_json::from_str(
            &fs::read_to_string(dir.join(rel))
                .with_context(|| format!("failed to read pilot case `{case_id}`"))?,
        )
        .with_context(|| format!("failed to parse pilot case `{case_id}`"))?;
        if case.schema != PILOT_CASE_SCHEMA || case.case_id != *case_id {
            bail!("pilot case identity mismatch for `{case_id}`");
        }
        cases.push(case);
    }
    let report = evaluate_cases(&cases, protocol_pin, &config_pin)?;
    let rendered = serde_json::to_string_pretty(&report)?;
    let tmp = dir.join(".report.json.tmp");
    fs::write(&tmp, format!("{rendered}\n"))
        .with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, dir.join("report.json"))
        .with_context(|| format!("failed to publish {}", dir.join("report.json").display()))?;
    Ok(report)
}
