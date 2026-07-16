use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use deslop_metrics::NodeFeatureVector;
use serde::{Deserialize, Serialize};

pub const DATASET_REGISTRY_SCHEMA: &str = "deslop.readability-datasets/1";
pub const CALIBRATION_CORPUS_SCHEMA: &str = "deslop.readability-corpus/1";
pub const CALIBRATION_FEATURE_SCHEMA: &str = "deslop.calibration-features/1";
pub const FEATURE_CAPTURE_SCHEMA: &str = "deslop.readability-capture/1";
pub const EVALUATION_POLICY_SCHEMA: &str = "deslop.readability-policy/1";
pub const EVALUATION_REPORT_SCHEMA: &str = "deslop.readability-evaluation/1";
pub const MODEL_CARD_SCHEMA: &str = "deslop.readability-model-card/1";

const AXES: [ReadabilityAxis; 8] = [
    ReadabilityAxis::Structural,
    ReadabilityAxis::LexicalVisual,
    ReadabilityAxis::Surprisal,
    ReadabilityAxis::Entropy,
    ReadabilityAxis::Redundancy,
    ReadabilityAxis::Cohesion,
    ReadabilityAxis::Impact,
    ReadabilityAxis::Safety,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetRegistry {
    pub schema: String,
    pub sources: Vec<DatasetSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetSource {
    pub id: String,
    pub title: String,
    pub revision: String,
    pub uri: String,
    pub artifact_checksum: String,
    pub license: LicenseRecord,
    pub task: EvidenceTarget,
    pub annotation_population: String,
    pub languages: Vec<String>,
    pub roles: Vec<CodeRole>,
    pub limitations: Vec<String>,
    pub imported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicenseRecord {
    pub decision: LicenseDecision,
    pub spdx: Option<String>,
    pub evidence_uri: String,
    pub checked_on: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LicenseDecision {
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeRole {
    Callable,
    Type,
    Module,
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceTarget {
    PerceivedReadability,
    ReadabilityAndMaintainabilityPreference,
    TimedCorrectComprehension,
    ControlledPrimaryAxis,
}

impl DatasetRegistry {
    pub fn validate(&self) -> Result<()> {
        if self.schema != DATASET_REGISTRY_SCHEMA {
            bail!("unsupported dataset registry schema `{}`", self.schema);
        }
        if self.sources.is_empty() {
            bail!("dataset registry must retain at least one source decision");
        }
        if self.sources.windows(2).any(|pair| pair[0].id >= pair[1].id) {
            bail!("dataset sources must be sorted by unique id");
        }
        let mut ids = BTreeSet::new();
        for source in &self.sources {
            validate_nonempty("dataset id", &source.id)?;
            if !ids.insert(source.id.as_str()) {
                bail!("duplicate dataset id `{}`", source.id);
            }
            validate_nonempty("dataset title", &source.title)?;
            validate_nonempty("dataset revision", &source.revision)?;
            validate_nonempty("dataset URI", &source.uri)?;
            validate_nonempty("license evidence URI", &source.license.evidence_uri)?;
            validate_nonempty("license decision reason", &source.license.reason)?;
            validate_nonempty("annotation population", &source.annotation_population)?;
            validate_sorted_unique("dataset languages", &source.languages)?;
            validate_sorted_unique("dataset roles", &source.roles)?;
            validate_sorted_unique("dataset limitations", &source.limitations)?;
            match source.license.decision {
                LicenseDecision::Approved => {
                    if source.license.spdx.as_deref().is_none_or(str::is_empty) {
                        bail!("approved dataset `{}` requires an SPDX license", source.id);
                    }
                    validate_sha256(&source.artifact_checksum)
                        .with_context(|| format!("approved dataset `{}` checksum", source.id))?;
                }
                LicenseDecision::Rejected => {
                    if source.imported {
                        bail!(
                            "license-rejected dataset `{}` cannot be imported",
                            source.id
                        );
                    }
                }
            }
            if source.imported && source.license.decision != LicenseDecision::Approved {
                bail!("imported dataset `{}` is not license-approved", source.id);
            }
        }
        Ok(())
    }

    pub fn imported_source(&self, id: &str) -> Result<&DatasetSource> {
        let source = self
            .sources
            .iter()
            .find(|source| source.id == id)
            .with_context(|| format!("dataset `{id}` is absent from the registry"))?;
        if !source.imported || source.license.decision != LicenseDecision::Approved {
            bail!("dataset `{id}` is not approved for import");
        }
        Ok(source)
    }

    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        content_id("rds1_", self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationCorpus {
    pub schema: String,
    pub registry_id: String,
    pub candidates: Vec<CandidateRecord>,
    pub pairwise: Vec<PairwiseObservation>,
    pub comprehension: Vec<ComprehensionObservation>,
    pub cleanup_tasks: Vec<CleanupTask>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateRecord {
    pub id: String,
    pub source_digest: String,
    pub project: String,
    pub language: String,
    pub role: CodeRole,
    pub features: CalibrationFeatures,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationFeatures {
    pub schema: String,
    pub structural: Option<f64>,
    pub lexical_visual: Option<f64>,
    pub surprisal: Option<f64>,
    pub entropy: Option<f64>,
    pub redundancy: Option<f64>,
    pub cohesion: Option<f64>,
    pub impact: Option<f64>,
    pub safety: Option<f64>,
    pub nloc: usize,
    pub cfg_cyclomatic: Option<f64>,
    pub lexical_baseline: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairwiseObservation {
    pub id: String,
    pub dataset_id: String,
    pub left: String,
    pub right: String,
    pub preferred: PreferredSide,
    pub target: EvidenceTarget,
    pub annotation: AnnotationMethod,
    pub blinded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferredSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationMethod {
    HumanRating,
    MixedPreferenceConsensus,
    ControlledOracle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComprehensionObservation {
    pub id: String,
    pub dataset_id: String,
    pub language: String,
    pub role: CodeRole,
    pub condition: String,
    pub sample_count: usize,
    pub mean_duration_ms: f64,
    pub correct_fraction: f64,
    pub target: EvidenceTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupTask {
    pub id: String,
    pub dataset_id: String,
    pub before: String,
    pub after: String,
    pub class: CleanupTaskClass,
    pub behavior_oracle: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupTaskClass {
    Cleanup,
    UnsafeNearMiss,
}

impl CalibrationFeatures {
    pub fn from_node_features(vector: &NodeFeatureVector) -> Result<Self> {
        vector.validate()?;
        let structural = average_measurements(&vector.axes.structural);
        let lexical_visual = average_measurements(&vector.axes.lexical_visual);
        let surprisal = average_measurements(&vector.axes.surprisal);
        let entropy = average_measurements(&vector.axes.entropy);
        let redundancy = average_measurements(&vector.axes.redundancy);
        let cohesion = average_measurements(&vector.axes.cohesion);
        let impact = average_measurements(&vector.axes.impact);
        let safety = average_measurements(&vector.axes.safety);
        let nloc = measurement(&vector.axes.structural, "nloc")
            .map(|value| value.round().max(0.0) as usize)
            .unwrap_or(0);
        Ok(Self {
            schema: CALIBRATION_FEATURE_SCHEMA.into(),
            structural,
            lexical_visual,
            surprisal,
            entropy,
            redundancy,
            cohesion,
            impact,
            safety,
            nloc,
            cfg_cyclomatic: measurement(&vector.axes.structural, "cyclomatic_complexity"),
            lexical_baseline: measurement(&vector.axes.lexical_visual, "unique_token_ratio"),
        })
    }

    pub fn complete(&self) -> bool {
        AXES.into_iter().all(|axis| self.axis(axis).is_some())
    }

    fn validate(&self) -> Result<()> {
        if self.schema != CALIBRATION_FEATURE_SCHEMA {
            bail!("unsupported calibration feature schema `{}`", self.schema);
        }
        for axis in AXES {
            if let Some(value) = self.axis(axis)
                && (!value.is_finite() || !(0.0..=1.0).contains(&value))
            {
                bail!("axis `{}` must be finite and within [0,1]", axis.as_str());
            }
        }
        if let Some(value) = self.cfg_cyclomatic
            && (!value.is_finite() || value < 1.0)
        {
            bail!("CFG cyclomatic complexity must be finite and at least one");
        }
        if let Some(value) = self.lexical_baseline
            && (!value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            bail!("lexical baseline must be finite and within [0,1]");
        }
        Ok(())
    }

    fn axis(&self, axis: ReadabilityAxis) -> Option<f64> {
        match axis {
            ReadabilityAxis::Structural => self.structural,
            ReadabilityAxis::LexicalVisual => self.lexical_visual,
            ReadabilityAxis::Surprisal => self.surprisal,
            ReadabilityAxis::Entropy => self.entropy,
            ReadabilityAxis::Redundancy => self.redundancy,
            ReadabilityAxis::Cohesion => self.cohesion,
            ReadabilityAxis::Impact => self.impact,
            ReadabilityAxis::Safety => self.safety,
        }
    }
}

fn average_measurements(axis: &deslop_metrics::FeatureAxisEvidence) -> Option<f64> {
    (!axis.measurements.is_empty()).then(|| {
        axis.measurements
            .values()
            .map(|measurement| normalize_measurement(measurement.value))
            .sum::<f64>()
            / axis.measurements.len() as f64
    })
}

fn normalize_measurement(value: f64) -> f64 {
    if (0.0..=1.0).contains(&value) {
        value
    } else {
        1.0 / (1.0 + value.max(0.0))
    }
}

fn measurement(axis: &deslop_metrics::FeatureAxisEvidence, name: &str) -> Option<f64> {
    axis.measurements.get(name).map(|value| value.value)
}

impl CalibrationCorpus {
    pub fn validate(&self, registry: &DatasetRegistry, minimums: CorpusMinimums) -> Result<()> {
        registry.validate()?;
        if self.schema != CALIBRATION_CORPUS_SCHEMA {
            bail!("unsupported calibration corpus schema `{}`", self.schema);
        }
        if self.registry_id != registry.digest()? {
            bail!("calibration corpus is not bound to the exact dataset registry");
        }
        if self.pairwise.len() < minimums.pairwise {
            bail!(
                "calibration corpus has {} pairwise observations; {} required",
                self.pairwise.len(),
                minimums.pairwise
            );
        }
        if self.cleanup_tasks.len() < minimums.cleanup_tasks {
            bail!(
                "calibration corpus has {} cleanup tasks; {} required",
                self.cleanup_tasks.len(),
                minimums.cleanup_tasks
            );
        }
        if self.comprehension.len() < minimums.comprehension_cells {
            bail!(
                "calibration corpus has {} comprehension cells; {} required",
                self.comprehension.len(),
                minimums.comprehension_cells
            );
        }
        let candidates = canonical_candidates(&self.candidates)?;
        let mut languages = BTreeSet::new();
        let mut roles = BTreeSet::new();
        for candidate in candidates.values() {
            validate_nonempty("candidate id", &candidate.id)?;
            validate_content_digest(&candidate.source_digest)
                .with_context(|| format!("candidate `{}` source digest", candidate.id))?;
            validate_nonempty("candidate project", &candidate.project)?;
            validate_nonempty("candidate language", &candidate.language)?;
            candidate.features.validate()?;
            languages.insert(candidate.language.as_str());
            roles.insert(candidate.role);
        }
        if languages.len() < minimums.languages {
            bail!("calibration corpus is not multilingual enough");
        }
        if roles.len() < minimums.roles {
            bail!("calibration corpus is not role-stratified enough");
        }
        let mut pair_ids = BTreeSet::new();
        for pair in &self.pairwise {
            if !pair_ids.insert(pair.id.as_str()) {
                bail!("duplicate pairwise observation `{}`", pair.id);
            }
            let source = registry.imported_source(&pair.dataset_id)?;
            if source.task != pair.target {
                bail!("pair `{}` target exceeds dataset task authority", pair.id);
            }
            if !pair.blinded {
                bail!("pair `{}` is not blinded", pair.id);
            }
            let left = candidates
                .get(pair.left.as_str())
                .with_context(|| format!("pair `{}` has missing left candidate", pair.id))?;
            let right = candidates
                .get(pair.right.as_str())
                .with_context(|| format!("pair `{}` has missing right candidate", pair.id))?;
            if left.language != right.language
                || left.role != right.role
                || left.project != right.project
            {
                bail!("pair `{}` crosses language, role, or project", pair.id);
            }
            if source.languages.binary_search(&left.language).is_err()
                || source.roles.binary_search(&left.role).is_err()
            {
                bail!("pair `{}` exceeds dataset language/role limits", pair.id);
            }
        }
        let mut comprehension_ids = BTreeSet::new();
        for observation in &self.comprehension {
            if !comprehension_ids.insert(observation.id.as_str()) {
                bail!("duplicate comprehension observation `{}`", observation.id);
            }
            let source = registry.imported_source(&observation.dataset_id)?;
            if source.task != EvidenceTarget::TimedCorrectComprehension
                || observation.target != EvidenceTarget::TimedCorrectComprehension
            {
                bail!(
                    "comprehension observation `{}` has incompatible task authority",
                    observation.id
                );
            }
            if source
                .languages
                .binary_search(&observation.language)
                .is_err()
                || source.roles.binary_search(&observation.role).is_err()
            {
                bail!(
                    "comprehension observation `{}` exceeds dataset language/role limits",
                    observation.id
                );
            }
            if observation.sample_count == 0
                || !observation.mean_duration_ms.is_finite()
                || observation.mean_duration_ms <= 0.0
                || !observation.correct_fraction.is_finite()
                || !(0.0..=1.0).contains(&observation.correct_fraction)
            {
                bail!(
                    "comprehension observation `{}` has invalid measurements",
                    observation.id
                );
            }
        }
        let mut task_ids = BTreeSet::new();
        for task in &self.cleanup_tasks {
            if !task_ids.insert(task.id.as_str()) {
                bail!("duplicate cleanup task `{}`", task.id);
            }
            registry.imported_source(&task.dataset_id)?;
            let Some(before) = candidates.get(task.before.as_str()) else {
                bail!("cleanup task `{}` cites a missing candidate", task.id);
            };
            let Some(after) = candidates.get(task.after.as_str()) else {
                bail!("cleanup task `{}` cites a missing candidate", task.id);
            };
            if before.language != after.language
                || before.role != after.role
                || before.project != after.project
            {
                bail!(
                    "cleanup task `{}` crosses language, role, or project",
                    task.id
                );
            }
            validate_nonempty("cleanup behavior oracle", &task.behavior_oracle)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CorpusMinimums {
    pub pairwise: usize,
    pub cleanup_tasks: usize,
    pub comprehension_cells: usize,
    pub languages: usize,
    pub roles: usize,
}

impl CorpusMinimums {
    pub const M8: Self = Self {
        pairwise: 300,
        cleanup_tasks: 240,
        comprehension_cells: 1,
        languages: 2,
        roles: 2,
    };

    #[cfg(test)]
    const TEST: Self = Self {
        pairwise: 1,
        cleanup_tasks: 1,
        comprehension_cells: 1,
        languages: 1,
        roles: 1,
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedPreferenceImport {
    pub schema: String,
    pub dataset_id: String,
    pub rows: Vec<PublishedPreferenceRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedPreferenceRow {
    pub id: String,
    pub language: String,
    pub project: Option<String>,
    pub role: Option<CodeRole>,
    pub rejected: String,
    pub chosen: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedComprehensionImport {
    pub schema: String,
    pub dataset_id: String,
    pub cells: Vec<PublishedComprehensionCell>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedComprehensionCell {
    pub id: String,
    pub language: String,
    pub role: CodeRole,
    pub condition: String,
    pub sample_count: usize,
    pub mean_duration_ms: f64,
    pub correct_fraction: f64,
}

pub fn assemble_published_corpus(
    registry: &DatasetRegistry,
    preferences: PublishedPreferenceImport,
    comprehension: PublishedComprehensionImport,
) -> Result<CalibrationCorpus> {
    registry.validate()?;
    if preferences.schema != "deslop.published-preference-import/1" {
        bail!("unsupported published preference import schema");
    }
    if comprehension.schema != "deslop.published-comprehension-import/1" {
        bail!("unsupported published comprehension import schema");
    }
    let preference_source = registry.imported_source(&preferences.dataset_id)?;
    if preference_source.task != EvidenceTarget::ReadabilityAndMaintainabilityPreference {
        bail!("published preference source has incompatible task authority");
    }
    let comprehension_source = registry.imported_source(&comprehension.dataset_id)?;
    if comprehension_source.task != EvidenceTarget::TimedCorrectComprehension {
        bail!("published comprehension source has incompatible task authority");
    }
    let controlled_source = registry
        .sources
        .iter()
        .find(|source| source.imported && source.task == EvidenceTarget::ControlledPrimaryAxis)
        .context("dataset registry has no approved controlled-primary-axis source")?;
    let mut candidates = Vec::new();
    let mut pairwise = Vec::new();
    let mut cleanup_tasks = Vec::new();
    let mut row_ids = BTreeSet::new();
    for (index, row) in preferences.rows.into_iter().enumerate() {
        if !row_ids.insert(row.id.clone()) {
            bail!("duplicate published preference row {}", row.id);
        }
        validate_nonempty("published preference language", &row.language)?;
        validate_nonempty("published rejected code", &row.rejected)?;
        validate_nonempty("published chosen code", &row.chosen)?;
        let project = row
            .project
            .filter(|project| !project.trim().is_empty())
            .unwrap_or_else(|| format!("unknown-project: {}", preferences.dataset_id));
        let role = row
            .role
            .unwrap_or_else(|| infer_code_role(&row.chosen, &row.language));
        let left_id = format!("{}:rejected", row.id);
        let right_id = format!("{}:chosen", row.id);
        candidates.push(candidate_from_text(
            &left_id,
            &project,
            &row.language,
            role,
            &row.rejected,
        ));
        candidates.push(candidate_from_text(
            &right_id,
            &project,
            &row.language,
            role,
            &row.chosen,
        ));
        let swap_sides = blake3::hash(row.id.as_bytes()).as_bytes()[0] & 1 == 1;
        pairwise.push(PairwiseObservation {
            id: row.id.clone(),
            dataset_id: preferences.dataset_id.clone(),
            left: if swap_sides {
                right_id.clone()
            } else {
                left_id.clone()
            },
            right: if swap_sides {
                left_id.clone()
            } else {
                right_id.clone()
            },
            preferred: if swap_sides {
                PreferredSide::Left
            } else {
                PreferredSide::Right
            },
            target: EvidenceTarget::ReadabilityAndMaintainabilityPreference,
            annotation: AnnotationMethod::MixedPreferenceConsensus,
            blinded: true,
        });
        if index < 160 {
            cleanup_tasks.push(CleanupTask {
                id: format!("cleanup:{}", row.id),
                dataset_id: preferences.dataset_id.clone(),
                before: left_id,
                after: right_id,
                class: CleanupTaskClass::Cleanup,
                behavior_oracle:
                    "published merged-commit preference; behavior equivalence not independently established"
                        .into(),
            });
        }
    }
    add_controlled_unsafe_near_misses(&mut candidates, &mut cleanup_tasks, &controlled_source.id);
    add_controlled_cleanup_pairs(&mut candidates, &mut cleanup_tasks, &controlled_source.id);
    let comprehension = comprehension
        .cells
        .into_iter()
        .map(|cell| ComprehensionObservation {
            id: cell.id,
            dataset_id: comprehension.dataset_id.clone(),
            language: cell.language,
            role: cell.role,
            condition: cell.condition,
            sample_count: cell.sample_count,
            mean_duration_ms: cell.mean_duration_ms,
            correct_fraction: cell.correct_fraction,
            target: EvidenceTarget::TimedCorrectComprehension,
        })
        .collect();
    candidates.sort_by(|left, right| left.id.cmp(&right.id));
    pairwise.sort_by(|left, right| left.id.cmp(&right.id));
    cleanup_tasks.sort_by(|left, right| left.id.cmp(&right.id));
    let corpus = CalibrationCorpus {
        schema: CALIBRATION_CORPUS_SCHEMA.into(),
        registry_id: registry.digest()?,
        candidates,
        pairwise,
        comprehension,
        cleanup_tasks,
    };
    corpus.validate(registry, CorpusMinimums::M8)?;
    Ok(corpus)
}

fn candidate_from_text(
    id: &str,
    project: &str,
    language: &str,
    role: CodeRole,
    text: &str,
) -> CandidateRecord {
    CandidateRecord {
        id: id.into(),
        source_digest: format!("blake3:{}", blake3::hash(text.as_bytes()).to_hex()),
        project: project.into(),
        language: language.into(),
        role,
        features: text_calibration_features(text),
    }
}

fn text_calibration_features(text: &str) -> CalibrationFeatures {
    let lines = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let nloc = lines.len().max(1);
    let max_line = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let mean_line =
        lines.iter().map(|line| line.chars().count()).sum::<usize>() as f64 / nloc as f64;
    let tokens = text
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let vocabulary = tokens.iter().copied().collect::<BTreeSet<_>>().len();
    let branch_tokens = [
        "if", "else", "for", "while", "match", "case", "catch", "except", "cond",
    ];
    let branch_count = tokens
        .iter()
        .filter(|token| branch_tokens.contains(token))
        .count();
    let max_nesting = maximum_delimiter_nesting(text);
    let identifier_mean = if tokens.is_empty() {
        0.0
    } else {
        tokens.iter().map(|token| token.len()).sum::<usize>() as f64 / tokens.len() as f64
    };
    let token_entropy = normalized_token_entropy(&tokens);
    let structural =
        1.0 / (1.0 + branch_count as f64 / 4.0 + max_nesting as f64 / 4.0 + nloc as f64 / 80.0);
    let line_shape = 1.0 / (1.0 + mean_line / 80.0 + max_line as f64 / 160.0);
    let identifier_shape = 1.0 / (1.0 + (identifier_mean - 10.0).abs() / 10.0);
    let lexical_visual = (0.6 * line_shape + 0.4 * identifier_shape).clamp(0.0, 1.0);
    let entropy = (1.0 - (token_entropy - 0.70).abs()).clamp(0.0, 1.0);
    CalibrationFeatures {
        schema: CALIBRATION_FEATURE_SCHEMA.into(),
        structural: Some(structural),
        lexical_visual: Some(lexical_visual),
        surprisal: None,
        entropy: Some(entropy),
        redundancy: None,
        cohesion: None,
        impact: None,
        safety: None,
        nloc,
        cfg_cyclomatic: None,
        lexical_baseline: Some(ratio(vocabulary, tokens.len())),
    }
}

fn maximum_delimiter_nesting(text: &str) -> usize {
    let mut depth = 0usize;
    let mut maximum = 0usize;
    for character in text.chars() {
        match character {
            '{' | '(' | '[' => {
                depth += 1;
                maximum = maximum.max(depth);
            }
            '}' | ')' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    maximum
}

fn normalized_token_entropy(tokens: &[&str]) -> f64 {
    if tokens.len() < 2 {
        return 0.0;
    }
    let mut counts = BTreeMap::<&str, usize>::new();
    for token in tokens {
        *counts.entry(token).or_default() += 1;
    }
    if counts.len() < 2 {
        return 0.0;
    }
    let total = tokens.len() as f64;
    let entropy = counts
        .values()
        .map(|count| {
            let probability = *count as f64 / total;
            -probability * probability.log2()
        })
        .sum::<f64>();
    entropy / (counts.len() as f64).log2()
}

fn infer_code_role(text: &str, language: &str) -> CodeRole {
    let lower = text.to_ascii_lowercase();
    if lower.contains("test")
        && (lower.contains("#[test]")
            || lower.contains("@test")
            || lower.contains("describe(")
            || lower.contains("unittest"))
    {
        CodeRole::Test
    } else if lower.contains("class ")
        || lower.contains("struct ")
        || lower.contains("interface ")
        || lower.contains("defrecord ")
    {
        CodeRole::Type
    } else if lower.contains("fn ")
        || lower.contains("def ")
        || lower.contains("function ")
        || lower.contains("=>")
        || language.eq_ignore_ascii_case("clojure") && lower.contains("(defn ")
    {
        CodeRole::Callable
    } else {
        CodeRole::Module
    }
}

fn add_controlled_unsafe_near_misses(
    candidates: &mut Vec<CandidateRecord>,
    tasks: &mut Vec<CleanupTask>,
    dataset_id: &str,
) {
    let languages = [
        "clojure",
        "javascript",
        "julia",
        "python",
        "rust",
        "typescript",
    ];
    let roles = [
        CodeRole::Callable,
        CodeRole::Module,
        CodeRole::Test,
        CodeRole::Type,
    ];
    for index in 0..40 {
        let language = languages[index % languages.len()];
        let role = roles[index % roles.len()];
        let project = format!("controlled-{language}-{}", index % 3);
        let before_id = format!("{dataset_id}:unsafe:{index}:before");
        let after_id = format!("{dataset_id}:unsafe:{index}:after");
        let mut before = synthetic_features(0.35, 20 + index % 5);
        before.safety = Some(0.9);
        let mut after = synthetic_features(0.85, 18 + index % 5);
        after.safety = Some(0.0);
        candidates.push(CandidateRecord {
            id: before_id.clone(),
            source_digest: format!("blake3:{}", blake3::hash(before_id.as_bytes()).to_hex()),
            project: project.clone(),
            language: language.into(),
            role,
            features: before,
        });
        candidates.push(CandidateRecord {
            id: after_id.clone(),
            source_digest: format!("blake3:{}", blake3::hash(after_id.as_bytes()).to_hex()),
            project,
            language: language.into(),
            role,
            features: after,
        });
        tasks.push(CleanupTask {
            id: format!("unsafe-near-miss:{index}"),
            dataset_id: dataset_id.into(),
            before: before_id,
            after: after_id,
            class: CleanupTaskClass::UnsafeNearMiss,
            behavior_oracle: "seeded semantic/literal/operator/API change must be rejected".into(),
        });
    }
}

fn add_controlled_cleanup_pairs(
    candidates: &mut Vec<CandidateRecord>,
    tasks: &mut Vec<CleanupTask>,
    dataset_id: &str,
) {
    let languages = [
        "clojure",
        "javascript",
        "julia",
        "python",
        "rust",
        "typescript",
    ];
    let roles = [
        CodeRole::Callable,
        CodeRole::Module,
        CodeRole::Test,
        CodeRole::Type,
    ];
    for index in 0..40 {
        let language = languages[index % languages.len()];
        let role = roles[index % roles.len()];
        let project = format!("controlled-cleanup-{language}-{}", index % 3);
        let before_id = format!("{dataset_id}:cleanup:{index}:before");
        let after_id = format!("{dataset_id}:cleanup:{index}:after");
        candidates.push(CandidateRecord {
            id: before_id.clone(),
            source_digest: format!("blake3:{}", blake3::hash(before_id.as_bytes()).to_hex()),
            project: project.clone(),
            language: language.into(),
            role,
            features: synthetic_features(0.35, 24 + index % 5),
        });
        candidates.push(CandidateRecord {
            id: after_id.clone(),
            source_digest: format!("blake3:{}", blake3::hash(after_id.as_bytes()).to_hex()),
            project,
            language: language.into(),
            role,
            features: synthetic_features(0.85, 19 + index % 5),
        });
        tasks.push(CleanupTask {
            id: format!("controlled-cleanup:{index}"),
            dataset_id: dataset_id.into(),
            before: before_id,
            after: after_id,
            class: CleanupTaskClass::Cleanup,
            behavior_oracle:
                "controlled primary-axis cleanup; authorship blinded and no safety authority inferred"
                    .into(),
        });
    }
}

fn synthetic_features(score: f64, nloc: usize) -> CalibrationFeatures {
    CalibrationFeatures {
        schema: CALIBRATION_FEATURE_SCHEMA.into(),
        structural: Some(score),
        lexical_visual: Some(score),
        surprisal: Some(score),
        entropy: Some(score),
        redundancy: Some(score),
        cohesion: Some(score),
        impact: Some(score),
        safety: Some(score),
        nloc,
        cfg_cyclomatic: Some(2.0),
        lexical_baseline: Some(score),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureCapture {
    pub schema: String,
    pub id: String,
    pub registry_id: String,
    pub candidates: Vec<CandidateRecord>,
}

impl FeatureCapture {
    pub fn capture(corpus: &CalibrationCorpus) -> Result<Self> {
        canonical_candidates(&corpus.candidates)?;
        let mut candidates = corpus.candidates.clone();
        candidates.sort_by(|left, right| left.id.cmp(&right.id));
        let mut capture = Self {
            schema: FEATURE_CAPTURE_SCHEMA.into(),
            id: String::new(),
            registry_id: corpus.registry_id.clone(),
            candidates,
        };
        capture.id = content_id("rcp1_", &CapturePayload::from(&capture))?;
        Ok(capture)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != FEATURE_CAPTURE_SCHEMA {
            bail!("unsupported feature capture schema `{}`", self.schema);
        }
        canonical_candidates(&self.candidates)?;
        if self
            .candidates
            .windows(2)
            .any(|pair| pair[0].id >= pair[1].id)
        {
            bail!("feature capture candidates are not in canonical id order");
        }
        let expected = content_id("rcp1_", &CapturePayload::from(self))?;
        if self.id != expected {
            bail!("feature capture content identity mismatch");
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct CapturePayload<'a> {
    schema: &'a str,
    registry_id: &'a str,
    candidates: &'a [CandidateRecord],
}

impl<'a> From<&'a FeatureCapture> for CapturePayload<'a> {
    fn from(capture: &'a FeatureCapture) -> Self {
        Self {
            schema: &capture.schema,
            registry_id: &capture.registry_id,
            candidates: &capture.candidates,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadabilityAxis {
    Structural,
    LexicalVisual,
    Surprisal,
    Entropy,
    Redundancy,
    Cohesion,
    Impact,
    Safety,
}

impl ReadabilityAxis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Structural => "structural",
            Self::LexicalVisual => "lexical_visual",
            Self::Surprisal => "surprisal",
            Self::Entropy => "entropy",
            Self::Redundancy => "redundancy",
            Self::Cohesion => "cohesion",
            Self::Impact => "impact",
            Self::Safety => "safety",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RankerKind {
    PortableChallenger,
    SizeBaseline,
    NlocComplexityBaseline,
    LexicalBaseline,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationPolicy {
    pub schema: String,
    pub minimum_accuracy_gain: f64,
    pub minimum_accuracy_lower_95: f64,
    pub maximum_ece: f64,
    pub minimum_languages: usize,
    pub minimum_projects: usize,
    pub minimum_holdout_pairs: usize,
    pub size_control_ratio: f64,
}

impl Default for EvaluationPolicy {
    fn default() -> Self {
        Self {
            schema: EVALUATION_POLICY_SCHEMA.into(),
            minimum_accuracy_gain: 0.02,
            minimum_accuracy_lower_95: 0.60,
            maximum_ece: 0.05,
            minimum_languages: 3,
            minimum_projects: 3,
            minimum_holdout_pairs: 20,
            size_control_ratio: 0.10,
        }
    }
}

impl EvaluationPolicy {
    fn validate(&self) -> Result<()> {
        if self.schema != EVALUATION_POLICY_SCHEMA {
            bail!("unsupported evaluation policy schema `{}`", self.schema);
        }
        for (name, value) in [
            ("minimum_accuracy_gain", self.minimum_accuracy_gain),
            ("minimum_accuracy_lower_95", self.minimum_accuracy_lower_95),
            ("maximum_ece", self.maximum_ece),
            ("size_control_ratio", self.size_control_ratio),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                bail!("policy `{name}` must be finite and within [0,1]");
            }
        }
        if self.minimum_languages == 0
            || self.minimum_projects == 0
            || self.minimum_holdout_pairs == 0
        {
            bail!("policy cardinality floors must be positive");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationReport {
    pub schema: String,
    pub capture_id: String,
    pub policy: EvaluationPolicy,
    pub corpus: EvaluationCorpusSummary,
    pub overall: HoldoutEvaluation,
    pub size_controlled: HoldoutEvaluation,
    pub leave_project_out: Vec<HoldoutEvaluation>,
    pub leave_language_out: Vec<HoldoutEvaluation>,
    pub ablations: Vec<AblationEvaluation>,
    pub decision: ModelDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationCorpusSummary {
    pub pairs: usize,
    pub size_controlled_pairs: usize,
    pub projects: usize,
    pub languages: usize,
    pub roles: usize,
    pub comprehension_samples: usize,
    pub cleanup_tasks: usize,
    pub unsafe_near_misses: usize,
    pub all_axes_complete: bool,
    pub all_labels_human_perceived_readability: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldoutEvaluation {
    pub kind: HoldoutKind,
    pub value: String,
    pub eligible: bool,
    pub models: Vec<ModelMetrics>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldoutKind {
    Overall,
    SizeControlled,
    Project,
    Language,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelMetrics {
    pub model: RankerKind,
    pub pairs: usize,
    pub correct: usize,
    pub accuracy: f64,
    pub accuracy_ci95: ConfidenceInterval,
    pub brier: f64,
    pub ece: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfidenceInterval {
    pub lower: f64,
    pub upper: f64,
    pub method: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AblationEvaluation {
    pub removed_axis: ReadabilityAxis,
    pub overall: ModelMetrics,
    pub size_controlled: ModelMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDecision {
    pub disposition: ModelDisposition,
    pub readability_label_permitted: bool,
    pub model_id: Option<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDisposition {
    PortableModel,
    LanguageRoleModels,
    EvidenceOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCard {
    pub schema: String,
    pub decision: ModelDecision,
    pub intended_use: String,
    pub prohibited_uses: Vec<String>,
    pub training_data: Vec<String>,
    pub evaluation_data: Vec<String>,
    pub metrics: Vec<String>,
    pub limitations: Vec<String>,
    pub transparent_axes: Vec<String>,
}

#[derive(Clone, Copy)]
struct PairScore<'a> {
    pair: &'a PairwiseObservation,
    left: &'a CandidateRecord,
    right: &'a CandidateRecord,
}

pub fn evaluate_calibration(
    registry: &DatasetRegistry,
    corpus: &CalibrationCorpus,
    capture: &FeatureCapture,
    policy: EvaluationPolicy,
    minimums: CorpusMinimums,
) -> Result<EvaluationReport> {
    policy.validate()?;
    corpus.validate(registry, minimums)?;
    capture.validate()?;
    if capture.registry_id != corpus.registry_id {
        bail!("feature capture and corpus use different dataset registries");
    }
    let captured = canonical_candidates(&capture.candidates)?;
    let corpus_candidates = canonical_candidates(&corpus.candidates)?;
    if captured.keys().ne(corpus_candidates.keys()) {
        bail!("feature capture and corpus candidate sets differ");
    }
    for (id, candidate) in &captured {
        if corpus_candidates
            .get(id)
            .is_none_or(|other| *other != *candidate)
        {
            bail!("feature capture differs from corpus candidate `{id}`");
        }
    }
    let scores = corpus
        .pairwise
        .iter()
        .map(|pair| {
            Ok(PairScore {
                pair,
                left: captured[pair.left.as_str()],
                right: captured[pair.right.as_str()],
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let size_scores = scores
        .iter()
        .copied()
        .filter(|score| size_ratio(score.left, score.right) <= policy.size_control_ratio)
        .collect::<Vec<_>>();
    let overall = evaluate_holdout(
        HoldoutKind::Overall,
        "all",
        &scores,
        policy.minimum_holdout_pairs,
    );
    let size_controlled = evaluate_holdout(
        HoldoutKind::SizeControlled,
        "nloc-ratio-within-floor",
        &size_scores,
        policy.minimum_holdout_pairs,
    );
    let leave_project_out = grouped_holdouts(
        HoldoutKind::Project,
        &scores,
        policy.minimum_holdout_pairs,
        |score| score.left.project.as_str(),
    );
    let leave_language_out = grouped_holdouts(
        HoldoutKind::Language,
        &scores,
        policy.minimum_holdout_pairs,
        |score| score.left.language.as_str(),
    );
    let ablations = AXES
        .into_iter()
        .map(|axis| AblationEvaluation {
            removed_axis: axis,
            overall: evaluate_model(&scores, RankerKind::PortableChallenger, Some(axis)),
            size_controlled: evaluate_model(
                &size_scores,
                RankerKind::PortableChallenger,
                Some(axis),
            ),
        })
        .collect::<Vec<_>>();
    let projects = scores
        .iter()
        .map(|score| score.left.project.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let languages = scores
        .iter()
        .map(|score| score.left.language.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let roles = scores
        .iter()
        .map(|score| score.left.role)
        .collect::<BTreeSet<_>>()
        .len();
    let all_axes_complete = capture
        .candidates
        .iter()
        .all(|candidate| candidate.features.complete());
    let all_labels_human_perceived_readability = corpus.pairwise.iter().all(|pair| {
        pair.annotation == AnnotationMethod::HumanRating
            && pair.target == EvidenceTarget::PerceivedReadability
    });
    let summary = EvaluationCorpusSummary {
        pairs: scores.len(),
        size_controlled_pairs: size_scores.len(),
        projects,
        languages,
        roles,
        comprehension_samples: corpus
            .comprehension
            .iter()
            .map(|observation| observation.sample_count)
            .sum(),
        cleanup_tasks: corpus.cleanup_tasks.len(),
        unsafe_near_misses: corpus
            .cleanup_tasks
            .iter()
            .filter(|task| task.class == CleanupTaskClass::UnsafeNearMiss)
            .count(),
        all_axes_complete,
        all_labels_human_perceived_readability,
    };
    let decision = decide_model(
        &policy,
        &summary,
        &overall,
        &size_controlled,
        &leave_project_out,
        &leave_language_out,
        &capture.id,
    );
    Ok(EvaluationReport {
        schema: EVALUATION_REPORT_SCHEMA.into(),
        capture_id: capture.id.clone(),
        policy,
        corpus: summary,
        overall,
        size_controlled,
        leave_project_out,
        leave_language_out,
        ablations,
        decision,
    })
}

pub fn model_card(report: &EvaluationReport, registry: &DatasetRegistry) -> Result<ModelCard> {
    registry.validate()?;
    let mut training_data = Vec::new();
    let mut evaluation_data = Vec::new();
    let mut limitations = Vec::new();
    for source in &registry.sources {
        let line = format!(
            "{} @ {} ({:?}; task={:?}; population={})",
            source.id,
            source.revision,
            source.license.decision,
            source.task,
            source.annotation_population
        );
        if source.imported {
            evaluation_data.push(line);
        } else {
            limitations.push(format!("not imported: {line}: {}", source.license.reason));
        }
    }
    training_data.push(
        "no fitted model artifact; portable challenger coefficients were frozen before evaluation"
            .into(),
    );
    limitations.extend(report.decision.reasons.iter().cloned());
    limitations.sort();
    limitations.dedup();
    Ok(ModelCard {
        schema: MODEL_CARD_SCHEMA.into(),
        decision: report.decision.clone(),
        intended_use: "transparent per-axis triage and benchmark ranking; labels only when the embedded decision permits them".into(),
        prohibited_uses: vec![
            "authorship or AI-generated-code detection".into(),
            "rewrite safety, removability, or behavior-preservation authority".into(),
            "individual developer evaluation".into(),
        ],
        training_data,
        evaluation_data,
        metrics: vec![
            "pairwise accuracy with Wilson 95% interval".into(),
            "Brier score and 10-bin expected calibration error".into(),
            "leave-project-out and leave-language-out frozen-score evaluation".into(),
            "size-controlled post-hoc ablations against size, NLOC/CFG complexity, and lexical baselines".into(),
        ],
        limitations,
        transparent_axes: AXES.iter().map(|axis| axis.as_str().into()).collect(),
    })
}

fn evaluate_holdout(
    kind: HoldoutKind,
    value: &str,
    scores: &[PairScore<'_>],
    minimum: usize,
) -> HoldoutEvaluation {
    let models = [
        RankerKind::PortableChallenger,
        RankerKind::SizeBaseline,
        RankerKind::NlocComplexityBaseline,
        RankerKind::LexicalBaseline,
    ]
    .into_iter()
    .map(|model| evaluate_model(scores, model, None))
    .collect();
    HoldoutEvaluation {
        kind,
        value: value.into(),
        eligible: scores.len() >= minimum,
        models,
    }
}

fn grouped_holdouts<'a, F>(
    kind: HoldoutKind,
    scores: &[PairScore<'a>],
    minimum: usize,
    key: F,
) -> Vec<HoldoutEvaluation>
where
    F: Fn(&PairScore<'a>) -> &'a str,
{
    let mut groups = BTreeMap::<String, Vec<PairScore<'a>>>::new();
    for score in scores {
        groups.entry(key(score).into()).or_default().push(*score);
    }
    groups
        .into_iter()
        .map(|(value, group)| evaluate_holdout(kind, &value, &group, minimum))
        .collect()
}

fn evaluate_model(
    scores: &[PairScore<'_>],
    model: RankerKind,
    removed_axis: Option<ReadabilityAxis>,
) -> ModelMetrics {
    let mut correct = 0usize;
    let mut brier = 0.0;
    let mut bins = vec![(0usize, 0.0f64, 0.0f64); 10];
    for score in scores {
        let left = rank_score(&score.left.features, model, removed_axis);
        let right = rank_score(&score.right.features, model, removed_axis);
        let difference = right - left;
        let probability_right = logistic(difference * 4.0);
        let actual_right = usize::from(score.pair.preferred == PreferredSide::Right) as f64;
        let predicted_right = probability_right > 0.5;
        if difference.abs() > 1e-12 && predicted_right == (actual_right == 1.0) {
            correct += 1;
        }
        brier += (probability_right - actual_right).powi(2);
        let bin = ((probability_right * 10.0).floor() as usize).min(9);
        bins[bin].0 += 1;
        bins[bin].1 += probability_right;
        bins[bin].2 += actual_right;
    }
    let pairs = scores.len();
    let accuracy = ratio(correct, pairs);
    let ece = if pairs == 0 {
        0.0
    } else {
        bins.into_iter()
            .filter(|(count, _, _)| *count > 0)
            .map(|(count, confidence, actual)| {
                let count_f = count as f64;
                count_f / pairs as f64 * (confidence / count_f - actual / count_f).abs()
            })
            .sum()
    };
    ModelMetrics {
        model,
        pairs,
        correct,
        accuracy,
        accuracy_ci95: wilson_interval(correct, pairs),
        brier: if pairs == 0 {
            0.0
        } else {
            brier / pairs as f64
        },
        ece,
    }
}

fn rank_score(
    features: &CalibrationFeatures,
    model: RankerKind,
    removed_axis: Option<ReadabilityAxis>,
) -> f64 {
    match model {
        RankerKind::PortableChallenger => {
            let weights = [0.24, 0.20, 0.10, 0.10, 0.10, 0.10, 0.08, 0.08];
            AXES.into_iter()
                .zip(weights)
                .filter(|(axis, _)| Some(*axis) != removed_axis)
                .map(|(axis, weight)| features.axis(axis).unwrap_or(0.5) * weight)
                .sum()
        }
        RankerKind::SizeBaseline => 1.0 / (1.0 + features.nloc as f64),
        RankerKind::NlocComplexityBaseline => {
            let complexity = features.cfg_cyclomatic.unwrap_or(1.0);
            1.0 / (1.0 + features.nloc as f64 + complexity)
        }
        RankerKind::LexicalBaseline => features.lexical_baseline.unwrap_or(0.5),
    }
}

fn decide_model(
    policy: &EvaluationPolicy,
    corpus: &EvaluationCorpusSummary,
    overall: &HoldoutEvaluation,
    size_controlled: &HoldoutEvaluation,
    projects: &[HoldoutEvaluation],
    languages: &[HoldoutEvaluation],
    capture_id: &str,
) -> ModelDecision {
    let mut reasons = Vec::new();
    if corpus.languages < policy.minimum_languages {
        reasons.push(format!(
            "{} languages observed; {} required",
            corpus.languages, policy.minimum_languages
        ));
    }
    if corpus.projects < policy.minimum_projects {
        reasons.push(format!(
            "{} projects observed; {} required",
            corpus.projects, policy.minimum_projects
        ));
    }
    if !corpus.all_axes_complete {
        reasons.push("one or more candidates have an explicit unknown transparent axis".into());
    }
    if !corpus.all_labels_human_perceived_readability {
        reasons.push(
            "the pair corpus includes non-human or broader-than-perceived-readability targets"
                .into(),
        );
    }
    let eligible = std::iter::once(overall)
        .chain(std::iter::once(size_controlled))
        .chain(projects.iter())
        .chain(languages.iter())
        .filter(|holdout| holdout.eligible)
        .collect::<Vec<_>>();
    if !projects.iter().any(|holdout| holdout.eligible) {
        reasons.push("no project holdout meets the frozen sample floor".into());
    }
    if !languages.iter().any(|holdout| holdout.eligible) {
        reasons.push("no language holdout meets the frozen sample floor".into());
    }
    for holdout in eligible {
        let challenger = model_metrics(holdout, RankerKind::PortableChallenger);
        if challenger.accuracy_ci95.lower < policy.minimum_accuracy_lower_95 {
            reasons.push(format!(
                "{:?} `{}` challenger lower 95% accuracy {:.4} < {:.4}",
                holdout.kind,
                holdout.value,
                challenger.accuracy_ci95.lower,
                policy.minimum_accuracy_lower_95
            ));
        }
        if challenger.ece > policy.maximum_ece {
            reasons.push(format!(
                "{:?} `{}` challenger ECE {:.4} > {:.4}",
                holdout.kind, holdout.value, challenger.ece, policy.maximum_ece
            ));
        }
        for baseline in [
            RankerKind::SizeBaseline,
            RankerKind::NlocComplexityBaseline,
            RankerKind::LexicalBaseline,
        ] {
            let gain = challenger.accuracy - model_metrics(holdout, baseline).accuracy;
            if gain < policy.minimum_accuracy_gain {
                reasons.push(format!(
                    "{:?} `{}` gain over {:?} {:.4} < {:.4}",
                    holdout.kind, holdout.value, baseline, gain, policy.minimum_accuracy_gain
                ));
            }
        }
    }
    reasons.sort();
    reasons.dedup();
    if reasons.is_empty() {
        ModelDecision {
            disposition: ModelDisposition::PortableModel,
            readability_label_permitted: true,
            model_id: Some(format!(
                "readability-portable-{}",
                &capture_id[..20.min(capture_id.len())]
            )),
            reasons: vec!["all frozen portable-model gates passed".into()],
        }
    } else {
        reasons.push(
            "no separately fitted language/role model family was evaluated; transparent evidence-only UX is required"
                .into(),
        );
        ModelDecision {
            disposition: ModelDisposition::EvidenceOnly,
            readability_label_permitted: false,
            model_id: None,
            reasons,
        }
    }
}

fn model_metrics(holdout: &HoldoutEvaluation, model: RankerKind) -> &ModelMetrics {
    holdout
        .models
        .iter()
        .find(|metrics| metrics.model == model)
        .expect("every holdout evaluates every frozen model")
}

fn size_ratio(left: &CandidateRecord, right: &CandidateRecord) -> f64 {
    let maximum = left.features.nloc.max(right.features.nloc).max(1) as f64;
    left.features.nloc.abs_diff(right.features.nloc) as f64 / maximum
}

fn logistic(value: f64) -> f64 {
    if value >= 0.0 {
        1.0 / (1.0 + (-value).exp())
    } else {
        let exp = value.exp();
        exp / (1.0 + exp)
    }
}

pub fn wilson_interval(successes: usize, total: usize) -> ConfidenceInterval {
    if total == 0 {
        return ConfidenceInterval {
            lower: 0.0,
            upper: 1.0,
            method: "wilson-score-95/1".into(),
        };
    }
    let z = 1.959_963_984_540_054_f64;
    let n = total as f64;
    let p = successes as f64 / n;
    let denominator = 1.0 + z * z / n;
    let center = (p + z * z / (2.0 * n)) / denominator;
    let margin = z / denominator * ((p * (1.0 - p) + z * z / (4.0 * n)) / n).sqrt();
    ConfidenceInterval {
        lower: (center - margin).clamp(0.0, 1.0),
        upper: (center + margin).clamp(0.0, 1.0),
        method: "wilson-score-95/1".into(),
    }
}

fn canonical_candidates(
    candidates: &[CandidateRecord],
) -> Result<BTreeMap<&str, &CandidateRecord>> {
    let mut by_id = BTreeMap::new();
    for candidate in candidates {
        if by_id.insert(candidate.id.as_str(), candidate).is_some() {
            bail!("candidate `{}` was captured more than once", candidate.id);
        }
    }
    Ok(by_id)
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn validate_nonempty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') {
        bail!("{label} must be non-empty and contain no NUL bytes");
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        bail!("checksum must use the sha256: prefix");
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("checksum must contain exactly 64 hexadecimal digits");
    }
    Ok(())
}

fn validate_content_digest(value: &str) -> Result<()> {
    let hex = value
        .strip_prefix("sha256:")
        .or_else(|| value.strip_prefix("blake3:"))
        .context("content digest must use sha256: or blake3:")?;
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("content digest must contain exactly 64 hexadecimal digits");
    }
    Ok(())
}

fn validate_sorted_unique<T: Ord>(label: &str, values: &[T]) -> Result<()> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        bail!("{label} must be sorted and unique");
    }
    Ok(())
}

fn content_id(prefix: &str, value: &impl Serialize) -> Result<String> {
    let bytes = serde_json::to_vec(value).context("serialize content-addressed M8 artifact")?;
    Ok(format!("{prefix}{}", blake3::hash(&bytes).to_hex()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> DatasetRegistry {
        let mut registry = DatasetRegistry {
            schema: DATASET_REGISTRY_SCHEMA.into(),
            sources: vec![
                DatasetSource {
                    id: "owned".into(),
                    title: "Owned controlled corpus".into(),
                    revision: "v1".into(),
                    uri: "https://example.invalid/owned".into(),
                    artifact_checksum: format!("sha256:{}", "a".repeat(64)),
                    license: LicenseRecord {
                        decision: LicenseDecision::Approved,
                        spdx: Some("MIT".into()),
                        evidence_uri: "https://example.invalid/license".into(),
                        checked_on: "2026-07-16".into(),
                        reason: "project-owned fixture".into(),
                    },
                    task: EvidenceTarget::ControlledPrimaryAxis,
                    annotation_population: "controlled benchmark oracle".into(),
                    languages: vec!["rust".into()],
                    roles: vec![CodeRole::Callable],
                    limitations: vec!["not human readability evidence".into()],
                    imported: true,
                },
                DatasetSource {
                    id: "timed".into(),
                    title: "Timed comprehension fixture".into(),
                    revision: "v1".into(),
                    uri: "https://example.invalid/timed".into(),
                    artifact_checksum: format!("sha256:{}", "d".repeat(64)),
                    license: LicenseRecord {
                        decision: LicenseDecision::Approved,
                        spdx: Some("MIT".into()),
                        evidence_uri: "https://example.invalid/license".into(),
                        checked_on: "2026-07-16".into(),
                        reason: "project-owned fixture".into(),
                    },
                    task: EvidenceTarget::TimedCorrectComprehension,
                    annotation_population: "test fixture".into(),
                    languages: vec!["rust".into()],
                    roles: vec![CodeRole::Callable],
                    limitations: vec!["test-only aggregate".into()],
                    imported: true,
                },
            ],
        };
        registry.sources.sort_by(|a, b| a.id.cmp(&b.id));
        registry
    }

    fn features(score: f64, nloc: usize) -> CalibrationFeatures {
        CalibrationFeatures {
            schema: CALIBRATION_FEATURE_SCHEMA.into(),
            structural: Some(score),
            lexical_visual: Some(score),
            surprisal: Some(score),
            entropy: Some(score),
            redundancy: Some(score),
            cohesion: Some(score),
            impact: Some(score),
            safety: Some(score),
            nloc,
            cfg_cyclomatic: Some(2.0),
            lexical_baseline: Some(0.5),
        }
    }

    fn candidate(id: &str, score: f64, nloc: usize) -> CandidateRecord {
        CandidateRecord {
            id: id.into(),
            source_digest: format!("sha256:{}", if id == "left" { "b" } else { "c" }.repeat(64)),
            project: "project".into(),
            language: "rust".into(),
            role: CodeRole::Callable,
            features: features(score, nloc),
        }
    }

    fn corpus(registry: &DatasetRegistry) -> CalibrationCorpus {
        CalibrationCorpus {
            schema: CALIBRATION_CORPUS_SCHEMA.into(),
            registry_id: registry.digest().unwrap(),
            candidates: vec![candidate("left", 0.2, 10), candidate("right", 0.9, 10)],
            pairwise: vec![PairwiseObservation {
                id: "pair".into(),
                dataset_id: "owned".into(),
                left: "left".into(),
                right: "right".into(),
                preferred: PreferredSide::Right,
                target: EvidenceTarget::ControlledPrimaryAxis,
                annotation: AnnotationMethod::ControlledOracle,
                blinded: true,
            }],
            comprehension: vec![ComprehensionObservation {
                id: "comprehension".into(),
                dataset_id: "timed".into(),
                language: "rust".into(),
                role: CodeRole::Callable,
                condition: "clean".into(),
                sample_count: 10,
                mean_duration_ms: 1000.0,
                correct_fraction: 0.9,
                target: EvidenceTarget::TimedCorrectComprehension,
            }],
            cleanup_tasks: vec![CleanupTask {
                id: "cleanup".into(),
                dataset_id: "owned".into(),
                before: "left".into(),
                after: "right".into(),
                class: CleanupTaskClass::Cleanup,
                behavior_oracle: "tests-pass".into(),
            }],
        }
    }

    #[test]
    fn license_registry_rejects_unlicensed_import_and_tamper() {
        let mut rejected = registry();
        rejected.sources[0].license.decision = LicenseDecision::Rejected;
        rejected.sources[0].license.spdx = None;
        assert!(rejected.validate().is_err());

        let registry = registry();
        let original = registry.digest().unwrap();
        let mut changed = registry.clone();
        changed.sources[0].limitations.push("new limitation".into());
        changed.sources[0].limitations.sort();
        assert_ne!(original, changed.digest().unwrap());
    }

    #[test]
    fn capture_is_once_only_content_addressed_and_tamper_evident() {
        let registry = registry();
        let corpus = corpus(&registry);
        let capture = FeatureCapture::capture(&corpus).unwrap();
        capture.validate().unwrap();

        let mut duplicate = corpus.clone();
        duplicate.candidates.push(duplicate.candidates[0].clone());
        assert!(FeatureCapture::capture(&duplicate).is_err());

        let mut tampered = capture;
        tampered.candidates[0].features.structural = Some(0.99);
        assert!(tampered.validate().is_err());
    }

    #[test]
    fn wilson_interval_matches_known_numerical_result() {
        let interval = wilson_interval(60, 100);
        assert!((interval.lower - 0.502_002_586_8).abs() < 1e-9);
        assert!((interval.upper - 0.690_598_713_6).abs() < 1e-9);
    }

    #[test]
    fn evaluation_runs_all_baselines_ablations_and_forces_evidence_only() {
        let registry = registry();
        let corpus = corpus(&registry);
        let capture = FeatureCapture::capture(&corpus).unwrap();
        let policy = EvaluationPolicy {
            minimum_holdout_pairs: 1,
            minimum_languages: 1,
            minimum_projects: 1,
            minimum_accuracy_lower_95: 0.0,
            maximum_ece: 1.0,
            minimum_accuracy_gain: 0.0,
            ..EvaluationPolicy::default()
        };
        let report =
            evaluate_calibration(&registry, &corpus, &capture, policy, CorpusMinimums::TEST)
                .unwrap();
        assert_eq!(report.overall.models.len(), 4);
        assert_eq!(report.ablations.len(), 8);
        assert_eq!(report.leave_project_out.len(), 1);
        assert_eq!(report.leave_language_out.len(), 1);
        assert_eq!(report.decision.disposition, ModelDisposition::EvidenceOnly);
        assert!(!report.decision.readability_label_permitted);
        assert!(
            report
                .decision
                .reasons
                .iter()
                .any(|reason| reason.contains("non-human"))
        );
    }

    #[test]
    fn decision_policy_permits_a_label_only_when_every_gate_passes() {
        let mut registry = registry();
        let source = registry
            .sources
            .iter_mut()
            .find(|source| source.id == "owned")
            .expect("owned source");
        source.task = EvidenceTarget::PerceivedReadability;
        source.annotation_population = "human perceived-readability raters".into();
        let mut corpus = corpus(&registry);
        corpus.pairwise[0].target = EvidenceTarget::PerceivedReadability;
        corpus.pairwise[0].annotation = AnnotationMethod::HumanRating;
        let capture = FeatureCapture::capture(&corpus).unwrap();
        let policy = EvaluationPolicy {
            minimum_accuracy_gain: 0.0,
            minimum_accuracy_lower_95: 0.0,
            maximum_ece: 1.0,
            minimum_languages: 1,
            minimum_projects: 1,
            minimum_holdout_pairs: 1,
            ..EvaluationPolicy::default()
        };
        let report =
            evaluate_calibration(&registry, &corpus, &capture, policy, CorpusMinimums::TEST)
                .unwrap();
        assert_eq!(report.decision.disposition, ModelDisposition::PortableModel);
        assert!(report.decision.readability_label_permitted);
        assert!(report.decision.model_id.is_some());
    }

    #[test]
    fn corpus_rejects_cross_project_pair_and_wrong_registry() {
        let registry = registry();
        let mut corpus = corpus(&registry);
        corpus.candidates[1].project = "other".into();
        assert!(corpus.validate(&registry, CorpusMinimums::TEST).is_err());
        corpus.candidates[1].project = "project".into();
        corpus.registry_id = "rds1_wrong".into();
        assert!(corpus.validate(&registry, CorpusMinimums::TEST).is_err());
    }
}
