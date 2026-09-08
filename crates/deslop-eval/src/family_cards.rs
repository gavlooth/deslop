//! Reproducible evidence cards for the first four pilot detector families.
//!
//! Cards are a projection of a freshly validated [`pilot::evaluate_dir`]
//! report.  There is deliberately no report-file reader here: callers cannot
//! turn an arbitrary JSON document into evidence.  The pilot evaluator owns
//! license, case, leakage, schema, and pin validation; this module preserves
//! its disjoint strata rather than pooling them into a deployment claim.
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::pilot::{
    PILOT_EVAL_SCHEMA, PILOT_FAMILIES, PilotEvalReport, PilotLanguage, ProvenanceKind,
    StratumScore, evaluate_dir, family_rules, stratum_rules, stratum_supported,
};

/// Wire schema for the deterministic family-card artifact.
pub const FAMILY_CARDS_SCHEMA: &str = "deslop.family-cards/1";
/// Schema identifier for each card nested in a family-card artifact.
pub const FAMILY_CARD_SCHEMA: &str = "deslop.family-card/1";

const PILOT_LANGUAGES: [PilotLanguage; 2] = [PilotLanguage::Rust, PilotLanguage::Python];
const EXTERNAL_HOLDOUT_BLOCK: &str = "No independent held-out result exists: external roster/consent, frozen confirmatory protocol, and an independent artifact are unavailable. Promotion is blocked; this card remains pilot engineering evidence only.";
const SCOPE: &str = "Within-file analysis of the selected source range under the pinned analyzer configuration; cross-file/project context is unavailable and is never represented as complete coverage.";
const REVIEW_ONLY: &str = "review-only: a detector observation is not a cleanup authorization; each candidate requires a separate semantic and verification decision.";

/// Human-readable and machine-stable capability state.  Unsupported is not a
/// clean negative and therefore never receives a precision or recall value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CardCapability {
    Supported,
    Unsupported,
}

/// Whether a card has observed rows in the validated report.  `Missing` is a
/// first-class state for the required family/language matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CardDataStatus {
    Observed,
    Missing,
}

/// Promotion is intentionally represented as a gate, not as a threshold.
/// No numerical criterion is invented while the independent artifact is
/// absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionGate {
    pub status: String,
    pub permitted: bool,
    pub reason: String,
}

/// Coverage is repeated by exact provenance/unit-kind stratum.  It is kept
/// separate from confusion counts so unsupported and incomplete analysis rows
/// cannot disappear from a denominator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageRecord {
    pub provenance: ProvenanceKind,
    pub unit_kind: String,
    pub input_total: usize,
    pub analyzed: usize,
    pub unsupported: usize,
    pub abstained: usize,
    pub analysis_coverage: Option<f64>,
}

/// Recommendation count/rate is the evaluator's explicit reviewer-load proxy,
/// not measured human effort.  It is retained per disjoint stratum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewerLoadProxy {
    pub provenance: ProvenanceKind,
    pub unit_kind: String,
    pub input_total: usize,
    pub predicted_positive: usize,
    pub recommendation_rate: Option<f64>,
    pub definition: String,
}

/// One family/language card.  `metrics` contains the actual pilot strata
/// verbatim; no family-level pooling is performed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyEvidenceCard {
    pub schema: String,
    pub family: String,
    pub language: PilotLanguage,
    pub rules: Vec<String>,
    pub supported_rules: Vec<String>,
    pub capability: CardCapability,
    pub data_status: CardDataStatus,
    pub scope: String,
    /// Raw disjoint (provenance, unit-kind) score rows from the validated
    /// pilot report.  Optional rates remain null for zero denominators.
    pub metrics: Vec<StratumScore>,
    pub coverage: Vec<CoverageRecord>,
    pub reviewer_load_proxy: Vec<ReviewerLoadProxy>,
    pub semantic_preconditions: Vec<String>,
    pub counterexamples: Vec<String>,
    pub uncertainty: Vec<String>,
    pub disposition: String,
    pub promotion: PromotionGate,
}

/// The published family-card artifact.  The pin and report digest bind this
/// projection to the exact report produced by `evaluate_dir`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyCardsReport {
    pub schema: String,
    pub pilot_report_schema: String,
    pub protocol_pin: String,
    pub analyzer_config_pin: String,
    pub pilot_report_digest: String,
    pub evidence_status: String,
    pub promotion_status: String,
    pub report_notes: Vec<String>,
    /// Diagnostics retained from the validated report; they are not hidden
    /// by card projection.
    pub pilot_failures: Vec<String>,
    pub cards: Vec<FamilyEvidenceCard>,
}

/// Build cards from a freshly validated pilot directory.
///
/// This is the only public construction path.  In particular, it does not
/// accept a caller-provided `PilotEvalReport` or a report JSON path: the
/// directory is revalidated by [`evaluate_dir`] on every invocation.
pub fn generate_family_cards(dir: &Path, protocol_pin: &str) -> Result<FamilyCardsReport> {
    let report = evaluate_dir(dir, protocol_pin)?;
    build_from_validated_report(&report)
}

/// Write a card artifact atomically.  The report has already been derived by
/// [`generate_family_cards`]; this function only serializes that typed result.
pub fn write_family_cards(path: &Path, cards: &FamilyCardsReport) -> Result<()> {
    cards.validate()?;
    let rendered = serde_json::to_string_pretty(cards)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create card output directory {}",
            parent.display()
        )
    })?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(tmp, "{rendered}")?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)
        .with_context(|| format!("failed to publish {}", path.display()))?;
    Ok(())
}

impl FamilyCardsReport {
    /// Validate the matrix and canonical mappings before serialization.  This
    /// keeps the file writer from becoming a way to publish a hand-assembled
    /// or schema-drifted card artifact.
    pub fn validate(&self) -> Result<()> {
        if self.schema != FAMILY_CARDS_SCHEMA {
            bail!("unsupported family-card schema `{}`", self.schema);
        }
        if self.pilot_report_schema != PILOT_EVAL_SCHEMA {
            bail!(
                "family cards require `{PILOT_EVAL_SCHEMA}`, got `{}`",
                self.pilot_report_schema
            );
        }
        if self.cards.len() != PILOT_FAMILIES.len() * PILOT_LANGUAGES.len() {
            bail!(
                "family-card matrix has {} cards; expected {}",
                self.cards.len(),
                PILOT_FAMILIES.len() * PILOT_LANGUAGES.len()
            );
        }
        let mut identities = BTreeSet::new();
        for &family in PILOT_FAMILIES {
            let all_rules = family_rules(family).ok_or_else(|| {
                anyhow::anyhow!("pilot family mapping disappeared for `{family}`")
            })?;
            for language in PILOT_LANGUAGES {
                let Some(card) = self
                    .cards
                    .iter()
                    .find(|card| card.family == family && card.language == language)
                else {
                    bail!("family-card matrix omits `{family}`/{language:?}");
                };
                if !identities.insert((card.family.clone(), card.language)) {
                    bail!("family-card matrix duplicates `{family}`/{language:?}");
                }
                if card.schema != FAMILY_CARD_SCHEMA {
                    bail!(
                        "unsupported nested family-card schema `{}` for `{family}`/{language:?}",
                        card.schema
                    );
                }
                let mapped: Vec<String> =
                    all_rules.iter().map(|rule| (*rule).to_string()).collect();
                if card.rules != mapped {
                    bail!("family-card rules drifted for `{family}`/{language:?}");
                }
                let mapped_supported = if stratum_supported(family, language) {
                    stratum_rules(family, language)
                        .ok_or_else(|| anyhow::anyhow!("pilot language mapping disappeared"))?
                        .into_iter()
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                if card.supported_rules != mapped_supported {
                    bail!("family-card supported rules drifted for `{family}`/{language:?}");
                }
                let expected_capability = if stratum_supported(family, language) {
                    CardCapability::Supported
                } else {
                    CardCapability::Unsupported
                };
                if card.capability != expected_capability {
                    bail!("family-card capability drifted for `{family}`/{language:?}");
                }
                if card
                    .metrics
                    .iter()
                    .any(|metric| metric.family != family || metric.language != language)
                {
                    bail!("family-card metrics cross a family/language boundary");
                }
                if card.coverage.len() != card.metrics.len()
                    || card.reviewer_load_proxy.len() != card.metrics.len()
                {
                    bail!("family-card derived metric rows are not one-to-one");
                }
                if card.promotion.permitted || card.promotion.status != "blocked" {
                    bail!("family-card promotion gate must remain blocked");
                }
            }
        }
        Ok(())
    }
}

/// Deterministically project the report without writing it.  Kept private so
/// callers cannot bypass `evaluate_dir`'s validation gates.
fn build_from_validated_report(report: &PilotEvalReport) -> Result<FamilyCardsReport> {
    if report.schema != PILOT_EVAL_SCHEMA {
        bail!("unsupported pilot report schema `{}`", report.schema);
    }

    let mut cards = Vec::with_capacity(PILOT_FAMILIES.len() * PILOT_LANGUAGES.len());
    for &family in PILOT_FAMILIES {
        let all_rules = family_rules(family)
            .ok_or_else(|| anyhow::anyhow!("pilot family mapping disappeared for `{family}`"))?;
        for rule in all_rules {
            if !deslop_core::rules::is_known(rule) {
                bail!("family `{family}` maps to unknown rule `{rule}`");
            }
        }
        for language in PILOT_LANGUAGES {
            let supported = stratum_rules(family, language).ok_or_else(|| {
                anyhow::anyhow!("pilot language mapping disappeared for `{family}`")
            })?;
            let metrics: Vec<StratumScore> = report
                .strata
                .iter()
                .filter(|score| score.family == family && score.language == language)
                .cloned()
                .collect();
            let capability = if stratum_supported(family, language) {
                CardCapability::Supported
            } else {
                CardCapability::Unsupported
            };
            let supported_rules: Vec<String> = if capability == CardCapability::Supported {
                supported.into_iter().map(str::to_string).collect()
            } else {
                Vec::new()
            };
            let data_status = if metrics.is_empty() {
                CardDataStatus::Missing
            } else {
                CardDataStatus::Observed
            };
            let mut uncertainty = vec![EXTERNAL_HOLDOUT_BLOCK.to_string()];
            uncertainty.push(report.declaration_note.clone());
            uncertainty.push(report.confidence_interval_note.clone());
            uncertainty.push(report.sampling_note.clone());
            uncertainty.push("Metrics are descriptive sampled-case rates, not deployment precision, prevalence, or a confidence interval; zero denominators remain null.".to_string());
            if data_status == CardDataStatus::Missing {
                uncertainty.push("No unsealed pilot stratum was observed for this family/language; no detector rate is asserted.".to_string());
            }
            if capability == CardCapability::Unsupported {
                uncertainty.push("The canonical mapping has no detector capability for this language; unsupported rows are not scored as negatives.".to_string());
            }
            if !report.failures.is_empty() {
                uncertainty.push(format!(
                    "Validated pilot report retained {} diagnostic failure(s); inspect report.failures before interpretation.",
                    report.failures.len()
                ));
            }
            let (semantic_preconditions, counterexamples) = family_contract(family);
            let coverage = metrics.iter().map(coverage_of).collect();
            let reviewer_load_proxy = metrics.iter().map(reviewer_load_of).collect();
            cards.push(FamilyEvidenceCard {
                schema: FAMILY_CARD_SCHEMA.to_string(),
                family: family.to_string(),
                language,
                rules: all_rules.iter().map(|rule| (*rule).to_string()).collect(),
                supported_rules,
                capability,
                data_status,
                scope: SCOPE.to_string(),
                metrics,
                coverage,
                reviewer_load_proxy,
                semantic_preconditions,
                counterexamples,
                uncertainty,
                disposition: if capability == CardCapability::Unsupported {
                    "unsupported".to_string()
                } else {
                    REVIEW_ONLY.to_string()
                },
                promotion: PromotionGate {
                    status: "blocked".to_string(),
                    permitted: false,
                    reason: EXTERNAL_HOLDOUT_BLOCK.to_string(),
                },
            });
        }
    }

    let report_json = serde_json::to_vec(report)?;
    Ok(FamilyCardsReport {
        schema: FAMILY_CARDS_SCHEMA.to_string(),
        pilot_report_schema: report.schema.clone(),
        protocol_pin: report.protocol_pin.clone(),
        analyzer_config_pin: report.analyzer_config_pin.clone(),
        pilot_report_digest: format!("blake3:{}", blake3::hash(&report_json).to_hex()),
        evidence_status: "pilot-engineering-evidence-only".to_string(),
        promotion_status: "blocked-pending-independent-held-out-artifact".to_string(),
        report_notes: vec![
            "Cards are generated from the actual validated pilot report returned by evaluate_dir; no arbitrary report JSON is accepted.".to_string(),
            "Canonical family/rule/language mappings are checked before card construction.".to_string(),
            EXTERNAL_HOLDOUT_BLOCK.to_string(),
        ],
        pilot_failures: report.failures.clone(),
        cards,
    })
}

fn coverage_of(score: &StratumScore) -> CoverageRecord {
    CoverageRecord {
        provenance: score.provenance,
        unit_kind: score.unit_kind.clone(),
        input_total: score.input_total,
        analyzed: score.analyzed,
        unsupported: score.unsupported,
        abstained: score.abstained,
        analysis_coverage: score.analysis_coverage,
    }
}

fn reviewer_load_of(score: &StratumScore) -> ReviewerLoadProxy {
    ReviewerLoadProxy {
        provenance: score.provenance,
        unit_kind: score.unit_kind.clone(),
        input_total: score.input_total,
        predicted_positive: score.predicted_positive,
        recommendation_rate: score.recommendation_rate,
        definition: "predicted-positive cases / input rows; proxy only, not measured reviewer time or effort".to_string(),
    }
}

fn family_contract(family: &str) -> (Vec<String>, Vec<String>) {
    match family {
        "duplication" => (
            vec![
                "At least two regions share the same or near-same token sequence in context.".to_string(),
                "The selected source range and within-file scope are sufficient to observe both regions; cross-file reuse is unavailable.".to_string(),
                "Any cleanup candidate must preserve task requirements, compatibility, side effects, and public behavior under exact verification.".to_string(),
            ],
            vec![
                "Version-conditional branches, test fixtures, and compatibility shims may be intentionally redundant.".to_string(),
                "Generated code or required defensive checks may repeat tokens without being removable.".to_string(),
            ],
        ),
        "long-method-complexity" => (
            vec![
                "The selected callable concentrates multiple responsibilities or branch clusters.".to_string(),
                "A complexity/length observation does not establish a safe split boundary or a maintenance benefit.".to_string(),
                "Any extraction must preserve evaluation order, error timing, ownership/types, and the declared task contract.".to_string(),
            ],
            vec![
                "An inherent single algorithm may be long without a clean split line; extraction can harm the contract.".to_string(),
                "Generated code may be retained by a regeneration policy despite real concentration.".to_string(),
            ],
        ),
        "wrappers-indirection" => (
            vec![
                "The wrapper forwards unchanged arguments or adds only needless formatting with no required behavior.".to_string(),
                "Reflection, dynamic dispatch, error mapping, type/lifetime adaptation, and side effects must be ruled out.".to_string(),
                "Any cleanup must preserve API identity, dispatch, evaluation order, and error behavior.".to_string(),
            ],
            vec![
                "A thin public-API stability or compatibility wrapper can be structurally redundant yet required.".to_string(),
                "Reflection or dynamic dispatch can make apparently unused indirection observable.".to_string(),
            ],
        ),
        "branch-simplification" => (
            vec![
                "A boolean expression has observable literal selection or redundant-looking boolean structure.".to_string(),
                "Truth-table equivalence and language-specific overload/macro behavior must be established before any rewrite.".to_string(),
                "Automatic application additionally requires exact candidate verification; this card grants none.".to_string(),
            ],
            vec![
                "Defensive checks and NaN-sensitive floating-point comparisons can require explicit-looking branches.".to_string(),
                "Macro- or overload-dependent semantics can invalidate a seemingly redundant form.".to_string(),
            ],
        ),
        _ => (Vec::new(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_complete_and_ordered_without_fabricated_metrics() {
        let report = PilotEvalReport {
            schema: PILOT_EVAL_SCHEMA.to_string(),
            protocol_pin: "test-pin".to_string(),
            analyzer_config_pin: "test-config".to_string(),
            cases_total: 0,
            sealed_excluded: 0,
            strata: Vec::new(),
            supplier_outcomes: Default::default(),
            analyses_complete: 0,
            analyses_unavailable: 0,
            predictions: Vec::new(),
            unresolved_case_ids: Vec::new(),
            declaration_note: "declarations".to_string(),
            confidence_interval_note: "no CI".to_string(),
            sampling_note: "sample".to_string(),
            failures: Vec::new(),
        };
        let cards = build_from_validated_report(&report).expect("valid empty pilot report");
        assert_eq!(cards.cards.len(), 8);
        assert!(cards.cards.iter().all(|card| {
            card.data_status == CardDataStatus::Missing && card.metrics.is_empty()
        }));
        assert!(cards.cards.iter().all(|card| !card.promotion.permitted));
    }
}
