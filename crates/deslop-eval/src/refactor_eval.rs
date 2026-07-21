//! Evaluation of the refactor-defect detector families over the frozen
//! history corpus (`tests/refactor-history/`).
//!
//! Design: `docs/REFACTOR_DEFECT_ACCUMULATION.md` (Phase 4). Each manifest
//! case is an ordered sequence of complete source snapshots with per-rule
//! expectations. The report scores precision and recall per detector family
//! (strictly: any fired rule without a `should_fire: true` expectation is a
//! false positive), plus the abstention rate, entity-match evidence rate,
//! and causal-path completeness the design requires. Recall is reported for
//! the syntax-only mode; the optional semantic-provider mode has no corpus
//! rows yet and is reported as an explicit `null`, never silently folded in.
//!
//! The report separates confidence, priority, and fix safety (acceptance
//! gate 9): every finding is a syntactic candidate, priority inputs are
//! triage only, and fix safety is structurally `never-auto`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use deslop_core::refactor_defect::rule_names;
use serde::{Deserialize, Serialize};

use crate::{EvalBaseline, read_json_file};

/// Wire schema identifier for the refactor evaluation report.
pub const REFACTOR_EVAL_SCHEMA: &str = "deslop.refactor-eval/1";

/// Wire schema identifier for the frozen per-family promotion gates.
pub const REFACTOR_PROMOTION_SCHEMA: &str = "deslop.refactor-promotion/1";

#[derive(Debug, Deserialize)]
struct HistoryManifest {
    schema: String,
    #[allow(dead_code)]
    note: Option<String>,
    cases: Vec<HistoryCase>,
}

#[derive(Debug, Deserialize)]
struct HistoryCase {
    name: String,
    #[allow(dead_code)]
    language: Option<String>,
    revisions: Vec<String>,
    expectations: Vec<HistoryExpectation>,
    #[allow(dead_code)]
    coverage: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct HistoryExpectation {
    rule: String,
    should_fire: bool,
    #[serde(default)]
    summary: bool,
    #[allow(dead_code)]
    note: Option<String>,
}

/// Per-family evaluation row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyScore {
    pub rule: String,
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub precision: f64,
    /// Recall in the syntax-only mode (the corpus carries no semantic
    /// provider artifacts).
    pub recall: f64,
    /// Fraction of cases without a positive expectation for this family in
    /// which the family correctly emitted nothing.
    pub abstention_rate: f64,
    /// Fraction of findings whose owner migration carries entity-match
    /// evidence (findings without an owner have no entity to match and do
    /// not count against the rate).
    pub entity_match_evidence_rate: f64,
    /// Fraction of findings with a non-empty causal path and a non-empty
    /// suggested verification (acceptance gate 7).
    pub causal_path_complete_rate: f64,
    /// Recall with optional semantic-provider evidence joined. `None` until
    /// the corpus carries provider-artifact cases; never silently reused
    /// from the syntax-only number.
    pub semantic_provider_recall: Option<f64>,
}

/// The refactor evaluation report (`deslop.refactor-eval/1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorEvalReport {
    pub schema: String,
    pub cases: usize,
    /// Confidence statement: what a passing family does and does not prove.
    pub confidence: String,
    /// Priority statement: what priority inputs may be used for.
    pub priority: String,
    /// Fix safety: structurally review-only.
    pub fix_safety: String,
    pub families: Vec<FamilyScore>,
}

/// Frozen per-family promotion gates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorPromotion {
    pub schema: String,
    pub families: Vec<FamilyPromotion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyPromotion {
    pub rule: String,
    /// Whether the family is promoted: it may report findings under its
    /// evidence contract. Promotion never changes fix safety.
    pub promoted: bool,
    /// Minimum corpus precision the family must hold to stay promoted.
    pub min_precision: f64,
    /// Why the family is promoted or blocked, including standing caveats
    /// (for example lexical surface classification gaps).
    pub note: String,
}

/// Run the refactor evaluation over one history corpus.
pub fn run_refactor_eval(corpus_root: &Path) -> Result<RefactorEvalReport> {
    let manifest: HistoryManifest = read_json_file(&corpus_root.join("manifest.json"))?;
    if manifest.schema != "deslop.refactor-history-manifest/1" {
        bail!(
            "unsupported refactor-history manifest schema `{}`",
            manifest.schema
        );
    }

    #[derive(Default)]
    struct Tally {
        true_positives: usize,
        false_positives: usize,
        false_negatives: usize,
        correct_abstentions: usize,
        abstention_opportunities: usize,
        findings: usize,
        findings_with_owner: usize,
        findings_with_match_evidence: usize,
        findings_with_complete_path: usize,
    }
    let mut tallies: BTreeMap<&'static str, Tally> = rule_names::ALL
        .iter()
        .map(|rule| (*rule, Tally::default()))
        .collect();

    for case in &manifest.cases {
        let roots: Vec<std::path::PathBuf> = case
            .revisions
            .iter()
            .map(|revision| corpus_root.join(&case.name).join(revision))
            .collect();
        let report = deslop_analyzer::refactor::refactor_risk_window_paths(&roots)
            .with_context(|| format!("refactor eval case {}", case.name))?;

        let fired: BTreeSet<&str> = report
            .findings
            .iter()
            .map(|finding| finding.rule.as_str())
            .collect();
        let fired_summaries: BTreeSet<&str> = report
            .summaries
            .iter()
            .map(|summary| summary.rule.as_str())
            .collect();
        let expected: BTreeSet<&str> = case
            .expectations
            .iter()
            .filter(|expectation| expectation.should_fire && !expectation.summary)
            .map(|expectation| expectation.rule.as_str())
            .collect();
        let expected_summaries: BTreeSet<&str> = case
            .expectations
            .iter()
            .filter(|expectation| expectation.should_fire && expectation.summary)
            .map(|expectation| expectation.rule.as_str())
            .collect();

        for rule in rule_names::ALL {
            let summary_rule = *rule == rule_names::ADOPTION_CHAIN_INCOMPLETE;
            let (actual, wanted) = if summary_rule {
                (
                    fired_summaries.contains(rule),
                    expected_summaries.contains(rule),
                )
            } else {
                (fired.contains(rule), expected.contains(rule))
            };
            let tally = tallies.get_mut(rule).expect("all rules tallied");
            match (actual, wanted) {
                (true, true) => tally.true_positives += 1,
                (true, false) => tally.false_positives += 1,
                (false, true) => tally.false_negatives += 1,
                (false, false) => tally.correct_abstentions += 1,
            }
            if !wanted {
                tally.abstention_opportunities += 1;
            }
        }
        for finding in report.findings.iter().chain(&report.summaries) {
            let Some(tally) = tallies.get_mut(finding.rule.as_str()) else {
                continue;
            };
            tally.findings += 1;
            if let Some(owner) = &finding.owner {
                tally.findings_with_owner += 1;
                if !owner.match_evidence.is_empty() {
                    tally.findings_with_match_evidence += 1;
                }
            }
            if !finding.causal_path.is_empty() && !finding.suggested_verification.trim().is_empty()
            {
                tally.findings_with_complete_path += 1;
            }
        }
    }

    let ratio = |numerator: usize, denominator: usize| {
        if denominator == 0 {
            1.0
        } else {
            numerator as f64 / denominator as f64
        }
    };
    let families = tallies
        .into_iter()
        .map(|(rule, tally)| FamilyScore {
            rule: rule.to_string(),
            true_positives: tally.true_positives,
            false_positives: tally.false_positives,
            false_negatives: tally.false_negatives,
            precision: ratio(
                tally.true_positives,
                tally.true_positives + tally.false_positives,
            ),
            recall: ratio(
                tally.true_positives,
                tally.true_positives + tally.false_negatives,
            ),
            abstention_rate: ratio(tally.correct_abstentions, tally.abstention_opportunities),
            entity_match_evidence_rate: ratio(
                tally.findings_with_match_evidence,
                tally.findings_with_owner,
            ),
            causal_path_complete_rate: ratio(tally.findings_with_complete_path, tally.findings),
            semantic_provider_recall: None,
        })
        .collect();

    Ok(RefactorEvalReport {
        schema: REFACTOR_EVAL_SCHEMA.to_string(),
        cases: manifest.cases.len(),
        confidence: "every finding is a syntactic candidate with explicit evidence, \
                     counter-evidence, and coverage gaps; a passing family proves corpus \
                     precision, not semantic correctness of any individual finding"
            .to_string(),
        priority: "priority inputs (persistence, independent churn, boundary distance, stale \
                   edges) are triage only, never confidence and never fix safety"
            .to_string(),
        fix_safety: "never-auto: no detector in this family creates or applies an automatic \
                     edit"
            .to_string(),
        families,
    })
}

/// Compare a refactor evaluation report with the frozen `deslop.eval-baseline/1`
/// rows for the detector families.
pub fn assert_refactor_baseline(
    report: &RefactorEvalReport,
    baseline: &EvalBaseline,
) -> Result<()> {
    let scores: BTreeMap<&str, &FamilyScore> = report
        .families
        .iter()
        .map(|family| (family.rule.as_str(), family))
        .collect();
    for expected in &baseline.rules {
        let Some(score) = scores.get(expected.rule.as_str()) else {
            bail!(
                "baseline family `{}` missing from refactor eval report",
                expected.rule
            );
        };
        if score.precision < expected.precision - baseline.epsilon {
            bail!(
                "precision for `{}` regressed: {:.4} < baseline {:.4}",
                expected.rule,
                score.precision,
                expected.precision
            );
        }
        if score.recall < expected.recall - baseline.epsilon {
            bail!(
                "recall for `{}` regressed: {:.4} < baseline {:.4}",
                expected.rule,
                score.recall,
                expected.recall
            );
        }
    }
    Ok(())
}

/// Read and validate the frozen promotion gates beside a corpus.
pub fn read_promotion(corpus_root: &Path) -> Result<RefactorPromotion> {
    let promotion: RefactorPromotion = read_json_file(&corpus_root.join("promotion.json"))?;
    if promotion.schema != REFACTOR_PROMOTION_SCHEMA {
        bail!(
            "unsupported refactor promotion schema `{}`",
            promotion.schema
        );
    }
    Ok(promotion)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_baseline;

    fn corpus_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/refactor-history")
    }

    /// Deliberate-regeneration helper: prints the measured report so the
    /// frozen baseline can be updated intentionally, never silently.
    #[test]
    #[ignore = "manual baseline regeneration helper"]
    fn print_refactor_eval_report() {
        let report = run_refactor_eval(&corpus_root()).expect("run refactor eval");
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    }

    #[test]
    fn refactor_corpus_matches_frozen_baseline_and_promotion_gates() {
        let root = corpus_root();
        let report = run_refactor_eval(&root).expect("run refactor eval");
        assert_eq!(report.schema, REFACTOR_EVAL_SCHEMA);
        assert_eq!(report.families.len(), rule_names::ALL.len());
        // Gate 9: the report separates confidence, priority, and fix safety.
        assert!(report.confidence.contains("syntactic candidate"));
        assert!(report.priority.contains("triage only"));
        assert!(report.fix_safety.contains("never-auto"));
        let baseline = read_baseline(&root).expect(
            "tests/refactor-history/baseline.json is the frozen refactor eval ratchet; run \
             run_refactor_eval and print the report to regenerate deliberately",
        );
        if let Err(error) = assert_refactor_baseline(&report, &baseline) {
            panic!(
                "refactor eval regressed: {error}\nreport: {}",
                serde_json::to_string_pretty(&report).unwrap()
            );
        }
        let promotion = read_promotion(&root).expect("frozen promotion gates");
        if let Err(error) = assert_promotion(&report, &promotion) {
            panic!(
                "promotion gates failed: {error}\nreport: {}",
                serde_json::to_string_pretty(&report).unwrap()
            );
        }
        // Recall modes stay separate: the syntax-only corpus must not claim
        // semantic-provider recall.
        assert!(
            report
                .families
                .iter()
                .all(|family| family.semantic_provider_recall.is_none())
        );
    }
}

/// Enforce the frozen promotion gates: every family is listed exactly once,
/// promoted families hold their frozen precision threshold, and blocked
/// families carry the reason blocking them.
pub fn assert_promotion(report: &RefactorEvalReport, promotion: &RefactorPromotion) -> Result<()> {
    let listed: BTreeSet<&str> = promotion
        .families
        .iter()
        .map(|family| family.rule.as_str())
        .collect();
    for rule in rule_names::ALL {
        if !listed.contains(rule) {
            bail!("promotion gates omit family `{rule}`");
        }
    }
    let scores: BTreeMap<&str, &FamilyScore> = report
        .families
        .iter()
        .map(|family| (family.rule.as_str(), family))
        .collect();
    for family in &promotion.families {
        if family.note.trim().is_empty() {
            bail!("promotion entry `{}` carries no note", family.rule);
        }
        if !family.promoted {
            continue;
        }
        let Some(score) = scores.get(family.rule.as_str()) else {
            bail!("promoted family `{}` missing from report", family.rule);
        };
        if score.precision < family.min_precision {
            bail!(
                "promoted family `{}` fell below its frozen precision gate: {:.4} < {:.4}",
                family.rule,
                score.precision,
                family.min_precision
            );
        }
    }
    Ok(())
}
