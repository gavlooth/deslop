use std::collections::{BTreeMap, BTreeSet};

use deslop_core::{Lang, SafetyClass};
use serde::{Deserialize, Serialize};

pub const RECIPE_EVALUATION_CORPUS_SCHEMA: &str = "deslop.recipe-evaluation-corpus/1";
pub const RECIPE_EVALUATION_REPORT_SCHEMA: &str = "deslop.recipe-evaluation-report/1";
const CORPUS_ID_DOMAIN: &str = "deslop recipe evaluation corpus id v1";
const CASE_ID_DOMAIN: &str = "deslop recipe evaluation case id v1";
const EXPANDED_DIGEST_DOMAIN: &str = "deslop recipe evaluation expanded corpus v1";
const REPORT_ID_DOMAIN: &str = "deslop recipe evaluation report id v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CorpusLabel {
    Opportunity,
    HardNegative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenRecipeCase {
    pub id: String,
    pub cluster: String,
    pub label: CorpusLabel,
    pub source: String,
    pub target_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationResourceBudget {
    pub maximum_source_bytes: usize,
    pub maximum_candidates: usize,
    pub maximum_wall_time_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeEvaluationCorpusManifest {
    pub schema: String,
    pub corpus_id: String,
    pub recipe_name: String,
    pub language: Lang,
    pub generator: String,
    pub seed: u64,
    pub positive_clusters: usize,
    pub hard_negative_clusters: usize,
    pub variants_per_cluster: usize,
    pub positive_families: Vec<String>,
    pub hard_negative_families: Vec<String>,
    pub layout_variants: usize,
    pub labelled_opportunities: usize,
    pub hard_negatives: usize,
    pub expected_safety: SafetyClass,
    pub protected_span_policy: String,
    pub protected_api_policy: String,
    pub behavior_oracle: String,
    pub independence_policy: String,
    pub resource_budget: EvaluationResourceBudget,
    pub expanded_digest: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationObservation {
    pub case_id: String,
    pub emitted: bool,
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeEvaluationTotals {
    pub true_positive: usize,
    pub false_positive: usize,
    pub true_negative: usize,
    pub false_negative: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationInterval {
    pub estimate: f64,
    pub lower_95: f64,
    pub upper_95: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct B7Thresholds {
    pub precision_lower_95: f64,
    pub recall_lower_95: f64,
    pub hard_negative_fpr_upper_95: f64,
    pub maximum_ece: f64,
}

impl Default for B7Thresholds {
    fn default() -> Self {
        Self {
            precision_lower_95: 0.90,
            recall_lower_95: 0.70,
            hard_negative_fpr_upper_95: 0.02,
            maximum_ece: 0.05,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeEvaluationThresholdResults {
    pub precision: bool,
    pub recall: bool,
    pub hard_negative_fpr: bool,
    pub calibration: bool,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeEvaluationReport {
    pub schema: String,
    pub report_id: String,
    pub corpus_id: String,
    pub corpus_digest: String,
    pub recipe_name: String,
    pub language: Lang,
    pub raw_totals: RecipeEvaluationTotals,
    pub cluster_totals: RecipeEvaluationTotals,
    pub precision: EvaluationInterval,
    pub recall: EvaluationInterval,
    pub hard_negative_fpr: EvaluationInterval,
    pub expected_calibration_error: f64,
    pub opportunity_coverage: f64,
    pub overall_action_rate: f64,
    pub hard_negative_abstention: f64,
    pub overall_abstention: f64,
    pub thresholds: B7Thresholds,
    pub threshold_results: RecipeEvaluationThresholdResults,
}

impl RecipeEvaluationReport {
    pub fn passed(&self) -> bool {
        self.threshold_results.passed
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RecipeEvaluationError {
    #[error("invalid recipe evaluation corpus: {0}")]
    InvalidCorpus(String),
    #[error("invalid recipe evaluation observations: {0}")]
    InvalidObservations(String),
    #[error("recipe evaluation identity failed: {0}")]
    Identity(String),
}

pub fn frozen_unreachable_rust_manifest()
-> Result<RecipeEvaluationCorpusManifest, RecipeEvaluationError> {
    let manifest: RecipeEvaluationCorpusManifest =
        serde_json::from_str(include_str!("../corpus/unreachable_literal_rust_v1.json"))
            .map_err(|error| RecipeEvaluationError::InvalidCorpus(error.to_string()))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub fn frozen_unreachable_rust_cases() -> Result<Vec<FrozenRecipeCase>, RecipeEvaluationError> {
    let manifest = frozen_unreachable_rust_manifest()?;
    Ok(expand_cases(&manifest))
}

pub fn evaluate_recipe_observations(
    manifest: &RecipeEvaluationCorpusManifest,
    observations: &[EvaluationObservation],
    thresholds: B7Thresholds,
) -> Result<RecipeEvaluationReport, RecipeEvaluationError> {
    validate_manifest(manifest)?;
    let cases = expand_cases(manifest);
    if observations.len() != cases.len() {
        return Err(RecipeEvaluationError::InvalidObservations(format!(
            "expected {} observations, received {}",
            cases.len(),
            observations.len()
        )));
    }
    let observation_map = observations
        .iter()
        .map(|observation| (observation.case_id.as_str(), observation))
        .collect::<BTreeMap<_, _>>();
    if observation_map.len() != observations.len()
        || observations.iter().any(|observation| {
            !observation.confidence.is_finite() || !(0.0..=1.0).contains(&observation.confidence)
        })
    {
        return Err(RecipeEvaluationError::InvalidObservations(
            "case identities must be unique and confidence must be finite within [0,1]".into(),
        ));
    }
    let case_ids = cases
        .iter()
        .map(|case| case.id.as_str())
        .collect::<BTreeSet<_>>();
    if observation_map.keys().any(|id| !case_ids.contains(id))
        || case_ids.iter().any(|id| !observation_map.contains_key(id))
    {
        return Err(RecipeEvaluationError::InvalidObservations(
            "observations do not close exactly over the frozen corpus".into(),
        ));
    }

    let mut raw = RecipeEvaluationTotals {
        true_positive: 0,
        false_positive: 0,
        true_negative: 0,
        false_negative: 0,
    };
    let mut clusters = BTreeMap::<(&str, CorpusLabel), Vec<bool>>::new();
    let mut calibration = Vec::with_capacity(cases.len());
    for case in &cases {
        let observation = observation_map[case.id.as_str()];
        match (case.label, observation.emitted) {
            (CorpusLabel::Opportunity, true) => raw.true_positive += 1,
            (CorpusLabel::Opportunity, false) => raw.false_negative += 1,
            (CorpusLabel::HardNegative, true) => raw.false_positive += 1,
            (CorpusLabel::HardNegative, false) => raw.true_negative += 1,
        }
        clusters
            .entry((&case.cluster, case.label))
            .or_default()
            .push(observation.emitted);
        calibration.push((
            observation.confidence,
            case.label == CorpusLabel::Opportunity,
        ));
    }
    let mut clustered = RecipeEvaluationTotals {
        true_positive: 0,
        false_positive: 0,
        true_negative: 0,
        false_negative: 0,
    };
    for ((_, label), emitted) in clusters {
        match label {
            CorpusLabel::Opportunity if emitted.iter().all(|value| *value) => {
                clustered.true_positive += 1;
            }
            CorpusLabel::Opportunity => clustered.false_negative += 1,
            CorpusLabel::HardNegative if emitted.iter().any(|value| *value) => {
                clustered.false_positive += 1;
            }
            CorpusLabel::HardNegative => clustered.true_negative += 1,
        }
    }

    let precision = wilson(
        clustered.true_positive,
        clustered.true_positive + clustered.false_positive,
    );
    let recall = wilson(
        clustered.true_positive,
        clustered.true_positive + clustered.false_negative,
    );
    let hard_negative_fpr = wilson(
        clustered.false_positive,
        clustered.false_positive + clustered.true_negative,
    );
    let ece = expected_calibration_error(&calibration, 10);
    let opportunity_total = raw.true_positive + raw.false_negative;
    let hard_negative_total = raw.false_positive + raw.true_negative;
    let all = opportunity_total + hard_negative_total;
    let action = raw.true_positive + raw.false_positive;
    let threshold_results = RecipeEvaluationThresholdResults {
        precision: precision.lower_95 >= thresholds.precision_lower_95,
        recall: recall.lower_95 >= thresholds.recall_lower_95,
        hard_negative_fpr: hard_negative_fpr.upper_95 <= thresholds.hard_negative_fpr_upper_95,
        calibration: ece <= thresholds.maximum_ece,
        passed: false,
    };
    let threshold_results = RecipeEvaluationThresholdResults {
        passed: threshold_results.precision
            && threshold_results.recall
            && threshold_results.hard_negative_fpr
            && threshold_results.calibration,
        ..threshold_results
    };
    let mut report = RecipeEvaluationReport {
        schema: RECIPE_EVALUATION_REPORT_SCHEMA.into(),
        report_id: String::new(),
        corpus_id: manifest.corpus_id.clone(),
        corpus_digest: manifest.expanded_digest.clone(),
        recipe_name: manifest.recipe_name.clone(),
        language: manifest.language,
        raw_totals: raw,
        cluster_totals: clustered,
        precision,
        recall,
        hard_negative_fpr,
        expected_calibration_error: ece,
        opportunity_coverage: ratio(raw.true_positive, opportunity_total),
        overall_action_rate: ratio(action, all),
        hard_negative_abstention: ratio(raw.true_negative, hard_negative_total),
        overall_abstention: ratio(all - action, all),
        thresholds,
        threshold_results,
    };
    let payload = canonical_without_field(&report, "report_id")?;
    report.report_id = derive_id(REPORT_ID_DOMAIN, "b7r1_", &payload);
    Ok(report)
}

fn validate_manifest(
    manifest: &RecipeEvaluationCorpusManifest,
) -> Result<(), RecipeEvaluationError> {
    if manifest.schema != RECIPE_EVALUATION_CORPUS_SCHEMA
        || manifest.language != Lang::Rust
        || manifest.recipe_name != "rust-remove-unreachable-literal-statement"
        || manifest.generator != "unreachable-literal-rust-grid/1"
        || manifest.positive_clusters != 200
        || manifest.hard_negative_clusters != 200
        || manifest.variants_per_cluster != 5
        || manifest.layout_variants != 40
        || manifest.positive_families.len() != 5
        || manifest.hard_negative_families.len() != 5
        || manifest.labelled_opportunities
            != manifest.positive_clusters * manifest.variants_per_cluster
        || manifest.hard_negatives
            != manifest.hard_negative_clusters * manifest.variants_per_cluster
        || manifest.labelled_opportunities < 1_000
        || manifest.hard_negatives < 1_000
        || manifest.expected_safety != SafetyClass::SafeAuto
        || manifest.seed != 5_251_007
    {
        return Err(RecipeEvaluationError::InvalidCorpus(
            "frozen recipe corpus shape or authority changed".into(),
        ));
    }
    for text in manifest
        .positive_families
        .iter()
        .chain(&manifest.hard_negative_families)
        .chain([
            &manifest.protected_span_policy,
            &manifest.protected_api_policy,
            &manifest.behavior_oracle,
            &manifest.independence_policy,
        ])
    {
        if text.trim().is_empty() || text.trim() != text {
            return Err(RecipeEvaluationError::InvalidCorpus(
                "frozen corpus text must be canonical and nonempty".into(),
            ));
        }
    }
    if manifest.resource_budget.maximum_source_bytes == 0
        || manifest.resource_budget.maximum_candidates < manifest.labelled_opportunities
        || manifest.resource_budget.maximum_wall_time_ms == 0
    {
        return Err(RecipeEvaluationError::InvalidCorpus(
            "frozen corpus resource budget is invalid".into(),
        ));
    }
    let cases = expand_cases(manifest);
    let expanded = expanded_digest(&cases)?;
    if expanded != manifest.expanded_digest {
        return Err(RecipeEvaluationError::InvalidCorpus(format!(
            "expanded corpus digest is stale: expected {expanded}"
        )));
    }
    let payload = canonical_without_field(manifest, "corpus_id")?;
    let expected = derive_id(CORPUS_ID_DOMAIN, "b2r1_", &payload);
    if manifest.corpus_id != expected {
        return Err(RecipeEvaluationError::InvalidCorpus(format!(
            "corpus identity is stale: expected {expected}"
        )));
    }
    Ok(())
}

fn expand_cases(manifest: &RecipeEvaluationCorpusManifest) -> Vec<FrozenRecipeCase> {
    let mut cases = Vec::with_capacity(manifest.labelled_opportunities + manifest.hard_negatives);
    for label in [CorpusLabel::Opportunity, CorpusLabel::HardNegative] {
        let clusters = match label {
            CorpusLabel::Opportunity => manifest.positive_clusters,
            CorpusLabel::HardNegative => manifest.hard_negative_clusters,
        };
        for cluster in 0..clusters {
            for variant in 0..manifest.variants_per_cluster {
                cases.push(case(label, cluster, variant));
            }
        }
    }
    cases
}

fn case(label: CorpusLabel, cluster: usize, variant: usize) -> FrozenRecipeCase {
    let family = cluster % 5;
    let layout = cluster / 5;
    let literal = match family {
        0 => format!("{}", layout * 10 + variant),
        1 => format!("{layout}.{variant}"),
        2 => if variant.is_multiple_of(2) {
            "true"
        } else {
            "false"
        }
        .into(),
        3 => format!("'{}'", (b'a' + variant as u8) as char),
        4 => format!("\"layout-{layout}-variant-{variant}\""),
        _ => unreachable!(),
    };
    let cluster_id = match label {
        CorpusLabel::Opportunity => format!("positive-{cluster:03}"),
        CorpusLabel::HardNegative => format!("hard-negative-{cluster:03}"),
    };
    let source = match label {
        CorpusLabel::Opportunity => format!(
            "fn positive_{cluster}_{variant}() {{ /* layout-{layout:02} */ return; {literal}; }}\n"
        ),
        CorpusLabel::HardNegative => match family {
            0 => format!(
                "fn negative_{cluster}_{variant}() {{ /* layout-{layout:02} */ {literal}; }}\n"
            ),
            1 => format!(
                "fn negative_{cluster}_{variant}() {{ /* layout-{layout:02} */ return; sink_{layout}_{variant}(); }}\n"
            ),
            2 => format!(
                "fn negative_{cluster}_{variant}() {{ /* layout-{layout:02} */ return; {variant} + {}; }}\n",
                variant + 1
            ),
            3 => format!(
                "fn negative_{cluster}_{variant}() {{ /* layout-{layout:02} */ return; VALUE_{layout}_{variant}; }}\n"
            ),
            4 => format!(
                "fn negative_{cluster}_{variant}(flag: bool) {{ /* layout-{layout:02} */ if flag {{ return; }} {literal}; }}\n"
            ),
            _ => unreachable!(),
        },
    };
    let target_text = (label == CorpusLabel::Opportunity).then(|| format!("{literal};"));
    let payload = serde_json::to_vec(&(label, &cluster_id, variant, &source, &target_text))
        .expect("frozen case payload is serializable");
    FrozenRecipeCase {
        id: derive_id(CASE_ID_DOMAIN, "b2c1_", &payload),
        cluster: cluster_id,
        label,
        source,
        target_text,
    }
}

fn expanded_digest(cases: &[FrozenRecipeCase]) -> Result<String, RecipeEvaluationError> {
    let payload = serde_json::to_vec(
        &cases
            .iter()
            .map(|case| {
                (
                    &case.id,
                    &case.cluster,
                    case.label,
                    &case.source,
                    &case.target_text,
                )
            })
            .collect::<Vec<_>>(),
    )
    .map_err(|error| RecipeEvaluationError::Identity(error.to_string()))?;
    Ok(derive_id(EXPANDED_DIGEST_DOMAIN, "b2x1_", &payload))
}

fn canonical_without_field(
    value: &impl Serialize,
    field: &str,
) -> Result<Vec<u8>, RecipeEvaluationError> {
    let mut value = serde_json::to_value(value)
        .map_err(|error| RecipeEvaluationError::Identity(error.to_string()))?;
    let object = value.as_object_mut().ok_or_else(|| {
        RecipeEvaluationError::Identity("identity payload is not a JSON object".into())
    })?;
    if object.remove(field).is_none() {
        return Err(RecipeEvaluationError::Identity(format!(
            "identity payload omits {field}"
        )));
    }
    serde_json::to_vec(&value).map_err(|error| RecipeEvaluationError::Identity(error.to_string()))
}

fn derive_id(domain: &str, prefix: &str, payload: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(domain.len() as u64).to_le_bytes());
    hasher.update(domain.as_bytes());
    hasher.update(&(payload.len() as u64).to_le_bytes());
    hasher.update(payload);
    format!("{prefix}{}", hasher.finalize().to_hex())
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn wilson(successes: usize, total: usize) -> EvaluationInterval {
    if total == 0 {
        return EvaluationInterval {
            estimate: 0.0,
            lower_95: 0.0,
            upper_95: 1.0,
        };
    }
    let z = 1.959_963_984_540_054_f64;
    let n = total as f64;
    let estimate = successes as f64 / n;
    let denominator = 1.0 + z * z / n;
    let center = (estimate + z * z / (2.0 * n)) / denominator;
    let radius =
        z * ((estimate * (1.0 - estimate) / n + z * z / (4.0 * n * n)).sqrt()) / denominator;
    EvaluationInterval {
        estimate: round_metric(estimate),
        lower_95: round_metric((center - radius).max(0.0)),
        upper_95: round_metric((center + radius).min(1.0)),
    }
}

fn round_metric(value: f64) -> f64 {
    const SCALE: f64 = 1_000_000_000_000.0;
    (value * SCALE).round() / SCALE
}

fn expected_calibration_error(observations: &[(f64, bool)], bin_count: usize) -> f64 {
    let mut bins = vec![(0_usize, 0.0_f64, 0_usize); bin_count];
    for (confidence, positive) in observations {
        let index = ((*confidence * bin_count as f64).floor() as usize).min(bin_count - 1);
        bins[index].0 += 1;
        bins[index].1 += confidence;
        bins[index].2 += usize::from(*positive);
    }
    bins.into_iter()
        .filter(|(count, _, _)| *count != 0)
        .map(|(count, confidence, positives)| {
            let weight = count as f64 / observations.len() as f64;
            let mean_confidence = confidence / count as f64;
            let accuracy = positives as f64 / count as f64;
            weight * (mean_confidence - accuracy).abs()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_manifest() -> RecipeEvaluationCorpusManifest {
        serde_json::from_str(include_str!("../corpus/unreachable_literal_rust_v1.json")).unwrap()
    }

    fn perfect_observations(cases: &[FrozenRecipeCase]) -> Vec<EvaluationObservation> {
        cases
            .iter()
            .map(|case| EvaluationObservation {
                case_id: case.id.clone(),
                emitted: case.label == CorpusLabel::Opportunity,
                confidence: if case.label == CorpusLabel::Opportunity {
                    1.0
                } else {
                    0.0
                },
            })
            .collect()
    }

    #[test]
    fn frozen_corpus_expands_to_exact_hashed_b2_slice() {
        let manifest = raw_manifest();
        let cases = expand_cases(&manifest);
        assert_eq!(cases.len(), 2_000);
        assert_eq!(
            cases
                .iter()
                .filter(|case| case.label == CorpusLabel::Opportunity)
                .count(),
            1_000
        );
        assert_eq!(
            cases
                .iter()
                .filter(|case| case.label == CorpusLabel::HardNegative)
                .count(),
            1_000
        );
        assert_eq!(manifest.expanded_digest, expanded_digest(&cases).unwrap());
        validate_manifest(&manifest).unwrap();
    }

    #[test]
    fn perfect_cluster_evidence_passes_recipe_specific_b7_bounds() {
        let manifest = frozen_unreachable_rust_manifest().unwrap();
        let cases = expand_cases(&manifest);
        let report = evaluate_recipe_observations(
            &manifest,
            &perfect_observations(&cases),
            B7Thresholds::default(),
        )
        .unwrap();

        assert!(report.passed());
        assert_eq!(
            report.raw_totals,
            RecipeEvaluationTotals {
                true_positive: 1_000,
                false_positive: 0,
                true_negative: 1_000,
                false_negative: 0,
            }
        );
        assert_eq!(
            report.cluster_totals,
            RecipeEvaluationTotals {
                true_positive: 200,
                false_positive: 0,
                true_negative: 200,
                false_negative: 0,
            }
        );
        assert!(report.precision.lower_95 > 0.98);
        assert!(report.recall.lower_95 > 0.98);
        assert!(report.hard_negative_fpr.upper_95 < 0.02);
        assert_eq!(report.expected_calibration_error, 0.0);
        assert_eq!(report.opportunity_coverage, 1.0);
        assert_eq!(report.hard_negative_abstention, 1.0);
        assert_eq!(report.overall_abstention, 0.5);
    }

    #[test]
    fn one_hard_negative_cluster_false_positive_fails_the_strict_fpr_gate() {
        let manifest = frozen_unreachable_rust_manifest().unwrap();
        let cases = expand_cases(&manifest);
        let mut observations = perfect_observations(&cases);
        let index = cases
            .iter()
            .position(|case| case.label == CorpusLabel::HardNegative)
            .unwrap();
        observations[index].emitted = true;
        observations[index].confidence = 1.0;
        let report =
            evaluate_recipe_observations(&manifest, &observations, B7Thresholds::default())
                .unwrap();

        assert!(!report.passed());
        assert!(!report.threshold_results.hard_negative_fpr);
        assert!(report.hard_negative_fpr.upper_95 > 0.02);
    }

    #[test]
    fn manifest_and_observation_mutations_fail_closed() {
        let mut manifest = raw_manifest();
        manifest.seed += 1;
        assert!(validate_manifest(&manifest).is_err());

        let manifest = frozen_unreachable_rust_manifest().unwrap();
        let cases = expand_cases(&manifest);
        let mut observations = perfect_observations(&cases);
        observations.pop();
        assert!(
            evaluate_recipe_observations(&manifest, &observations, B7Thresholds::default())
                .is_err()
        );
    }
}
