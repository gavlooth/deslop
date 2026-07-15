use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::Arc;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    ArchitectureComponentKey, ArchitectureGapKey, ArchitectureLevel, ArchitectureProjection,
    CycleSeamCandidate, CycleSeamCandidateKey, CycleSeamGapKey, CycleSeamProjection,
    DependencyDocument, DependencyEdge, DependencyEdgeKey, DependencyEdgeKind, DependencyNodeKey,
    DependencyNodeKind, FactCoverage, ProjectionId, VisibilityKind,
};

pub const MODULE_RESTRUCTURE_SCHEMA: &str = "deslop.module-restructure/1";
pub const MODULE_RESTRUCTURE_POLICY_SCHEMA: &str = "deslop.module-restructure-policy/1";
pub const MODULE_CHANGE_HISTORY_SCHEMA: &str = "deslop.module-change-history/1";

const POLICY_DOMAIN: &str = "deslop module restructure policy v1";
const PROFILE_DOMAIN: &str = "deslop module profile v1";
const CANDIDATE_DOMAIN: &str = "deslop module restructure candidate v1";
const GAP_DOMAIN: &str = "deslop module restructure gap v1";
const HISTORY_DOMAIN: &str = "deslop module change history v1";
const OBSERVATION_DOMAIN: &str = "deslop module change observation v1";

macro_rules! digest_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                validate_digest(&value, $prefix).map_err(D::Error::custom)?;
                Ok(Self(value))
            }
        }
    };
}

digest_id!(ModuleRestructurePolicyId, "mrp1_");
digest_id!(ModuleProfileKey, "mrf1_");
digest_id!(ModuleRestructureCandidateKey, "mrc1_");
digest_id!(ModuleRestructureGapKey, "mrg1_");
digest_id!(ModuleChangeHistoryId, "mch1_");
digest_id!(ModuleChangeObservationKey, "mco1_");

impl ModuleRestructurePolicyId {
    pub fn from_parts(parts: &[&[u8]]) -> Result<Self, ModuleRestructureBuildError> {
        if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
            return Err(invalid(
                "module-restructure policy identity requires nonempty parts",
            ));
        }
        Ok(Self(derive_id(POLICY_DOMAIN, "mrp1_", parts)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleRatio {
    numerator: usize,
    denominator: usize,
}
impl ModuleRatio {
    pub fn numerator(&self) -> usize {
        self.numerator
    }
    pub fn denominator(&self) -> usize {
        self.denominator
    }
    fn new(numerator: usize, denominator: usize) -> Option<Self> {
        (denominator != 0).then_some(Self {
            numerator,
            denominator,
        })
    }
    fn valid(&self) -> bool {
        self.denominator != 0 && self.numerator <= self.denominator
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleChangeObservationDraft {
    pub left: DependencyNodeKey,
    pub right: DependencyNodeKey,
    pub joint_changes: usize,
    pub left_changes: usize,
    pub right_changes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleChangeObservation {
    key: ModuleChangeObservationKey,
    left: DependencyNodeKey,
    right: DependencyNodeKey,
    joint_changes: usize,
    left_changes: usize,
    right_changes: usize,
    jaccard: ModuleRatio,
}
impl ModuleChangeObservation {
    pub fn key(&self) -> &ModuleChangeObservationKey {
        &self.key
    }
    pub fn left(&self) -> &DependencyNodeKey {
        &self.left
    }
    pub fn right(&self) -> &DependencyNodeKey {
        &self.right
    }
    pub fn joint_changes(&self) -> usize {
        self.joint_changes
    }
    pub fn left_changes(&self) -> usize {
        self.left_changes
    }
    pub fn right_changes(&self) -> usize {
        self.right_changes
    }
    pub fn jaccard(&self) -> &ModuleRatio {
        &self.jaccard
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleChangeHistory {
    schema: String,
    id: ModuleChangeHistoryId,
    dependency_projection_id: ProjectionId,
    coverage: FactCoverage,
    reasons: Vec<String>,
    observations: Vec<ModuleChangeObservation>,
}
impl ModuleChangeHistory {
    pub fn new(
        dependency_projection_id: ProjectionId,
        coverage: FactCoverage,
        mut reasons: Vec<String>,
        drafts: Vec<ModuleChangeObservationDraft>,
    ) -> Result<Self, ModuleRestructureBuildError> {
        if (coverage == FactCoverage::Complete) != reasons.is_empty() {
            return Err(invalid("module history coverage and reasons disagree"));
        }
        canonical_texts(&mut reasons)?;
        let mut observations = Vec::new();
        for draft in drafts {
            if draft.left == draft.right
                || draft.joint_changes == 0
                || draft.joint_changes > draft.left_changes
                || draft.joint_changes > draft.right_changes
            {
                return Err(invalid("module history observation counts are invalid"));
            }
            let (left, right, left_changes, right_changes) = if draft.left < draft.right {
                (
                    draft.left,
                    draft.right,
                    draft.left_changes,
                    draft.right_changes,
                )
            } else {
                (
                    draft.right,
                    draft.left,
                    draft.right_changes,
                    draft.left_changes,
                )
            };
            let mut observation = ModuleChangeObservation {
                key: ModuleChangeObservationKey(String::new()),
                left,
                right,
                joint_changes: draft.joint_changes,
                left_changes,
                right_changes,
                jaccard: ModuleRatio::new(
                    draft.joint_changes,
                    left_changes + right_changes - draft.joint_changes,
                )
                .expect("positive denominator"),
            };
            observation.key = observation_key(&dependency_projection_id, &observation)?;
            observations.push(observation);
        }
        observations.sort_by(|a, b| a.key.cmp(&b.key));
        let mut history = Self {
            schema: MODULE_CHANGE_HISTORY_SCHEMA.into(),
            id: ModuleChangeHistoryId(String::new()),
            dependency_projection_id,
            coverage,
            reasons,
            observations,
        };
        history.id = history_id(&history)?;
        history.validate()?;
        Ok(history)
    }
    pub fn id(&self) -> &ModuleChangeHistoryId {
        &self.id
    }
    pub fn dependency_projection_id(&self) -> &ProjectionId {
        &self.dependency_projection_id
    }
    pub fn coverage(&self) -> FactCoverage {
        self.coverage
    }
    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }
    pub fn observations(&self) -> &[ModuleChangeObservation] {
        &self.observations
    }
    fn validate(&self) -> Result<(), ModuleRestructureBuildError> {
        if self.schema != MODULE_CHANGE_HISTORY_SCHEMA
            || (self.coverage == FactCoverage::Complete) != self.reasons.is_empty()
            || self.id != history_id(self)?
        {
            return Err(invalid("module history contract is invalid"));
        }
        validate_texts(&self.reasons)?;
        validate_sorted("history observations", &self.observations, |v| {
            v.key.as_str()
        })?;
        let mut pairs = BTreeSet::new();
        for value in &self.observations {
            let expected = ModuleRatio::new(
                value.joint_changes,
                value.left_changes + value.right_changes - value.joint_changes,
            );
            if value.left >= value.right
                || !value.jaccard.valid()
                || expected.as_ref() != Some(&value.jaccard)
                || !pairs.insert((&value.left, &value.right))
                || value.key != observation_key(&self.dependency_projection_id, value)?
            {
                return Err(invalid("module history observation is invalid"));
            }
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleChangeHistoryWire {
    schema: String,
    id: ModuleChangeHistoryId,
    dependency_projection_id: ProjectionId,
    coverage: FactCoverage,
    reasons: Vec<String>,
    observations: Vec<ModuleChangeObservation>,
}
impl<'de> Deserialize<'de> for ModuleChangeHistory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let w = ModuleChangeHistoryWire::deserialize(deserializer)?;
        let value = Self {
            schema: w.schema,
            id: w.id,
            dependency_projection_id: w.dependency_projection_id,
            coverage: w.coverage,
            reasons: w.reasons,
            observations: w.observations,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleProfile {
    key: ModuleProfileKey,
    module: DependencyNodeKey,
    files: Vec<DependencyNodeKey>,
    internal_file_dependencies: Vec<DependencyEdgeKey>,
    incoming_module_dependencies: Vec<DependencyEdgeKey>,
    outgoing_module_dependencies: Vec<DependencyEdgeKey>,
    public_api_nodes: Vec<DependencyNodeKey>,
    external_api_users: Vec<DependencyEdgeKey>,
    cohesion: Option<ModuleRatio>,
    coverage: FactCoverage,
}
impl ModuleProfile {
    pub fn key(&self) -> &ModuleProfileKey {
        &self.key
    }
    pub fn module(&self) -> &DependencyNodeKey {
        &self.module
    }
    pub fn files(&self) -> &[DependencyNodeKey] {
        &self.files
    }
    pub fn internal_file_dependencies(&self) -> &[DependencyEdgeKey] {
        &self.internal_file_dependencies
    }
    pub fn incoming_module_dependencies(&self) -> &[DependencyEdgeKey] {
        &self.incoming_module_dependencies
    }
    pub fn outgoing_module_dependencies(&self) -> &[DependencyEdgeKey] {
        &self.outgoing_module_dependencies
    }
    pub fn public_api_nodes(&self) -> &[DependencyNodeKey] {
        &self.public_api_nodes
    }
    pub fn external_api_users(&self) -> &[DependencyEdgeKey] {
        &self.external_api_users
    }
    pub fn cohesion(&self) -> Option<&ModuleRatio> {
        self.cohesion.as_ref()
    }
    pub fn coverage(&self) -> FactCoverage {
        self.coverage
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModuleRestructureDisposition {
    ReviewRequired,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ModuleRestructureKind {
    MoveFile {
        file: DependencyNodeKey,
        from_module: DependencyNodeKey,
        to_module: DependencyNodeKey,
    },
    SplitModule {
        module: DependencyNodeKey,
        groups: Vec<Vec<DependencyNodeKey>>,
    },
    MergeModules {
        left: DependencyNodeKey,
        right: DependencyNodeKey,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleRestructureScore {
    authority_penalty: usize,
    coupling_reduction: usize,
    cohesion_separation: usize,
    public_api_impact: usize,
    external_api_users: usize,
    affected_files: usize,
    seam_support: usize,
    history_support: usize,
    history_conflicts: usize,
}
impl ModuleRestructureScore {
    pub fn authority_penalty(&self) -> usize {
        self.authority_penalty
    }
    pub fn coupling_reduction(&self) -> usize {
        self.coupling_reduction
    }
    pub fn cohesion_separation(&self) -> usize {
        self.cohesion_separation
    }
    pub fn public_api_impact(&self) -> usize {
        self.public_api_impact
    }
    pub fn external_api_users(&self) -> usize {
        self.external_api_users
    }
    pub fn affected_files(&self) -> usize {
        self.affected_files
    }
    pub fn seam_support(&self) -> usize {
        self.seam_support
    }
    pub fn history_support(&self) -> usize {
        self.history_support
    }
    pub fn history_conflicts(&self) -> usize {
        self.history_conflicts
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleRestructureCandidate {
    key: ModuleRestructureCandidateKey,
    kind: ModuleRestructureKind,
    disposition: ModuleRestructureDisposition,
    affected_files: Vec<DependencyNodeKey>,
    dependency_edges: Vec<DependencyEdgeKey>,
    public_api_nodes: Vec<DependencyNodeKey>,
    external_api_users: Vec<DependencyEdgeKey>,
    seam_candidates: Vec<CycleSeamCandidateKey>,
    history_observations: Vec<ModuleChangeObservationKey>,
    evidence_coverage: FactCoverage,
    score: ModuleRestructureScore,
    rank: u32,
    review_obligations: Vec<String>,
}
impl ModuleRestructureCandidate {
    pub fn key(&self) -> &ModuleRestructureCandidateKey {
        &self.key
    }
    pub fn kind(&self) -> &ModuleRestructureKind {
        &self.kind
    }
    pub fn disposition(&self) -> ModuleRestructureDisposition {
        self.disposition
    }
    pub fn affected_files(&self) -> &[DependencyNodeKey] {
        &self.affected_files
    }
    pub fn dependency_edges(&self) -> &[DependencyEdgeKey] {
        &self.dependency_edges
    }
    pub fn public_api_nodes(&self) -> &[DependencyNodeKey] {
        &self.public_api_nodes
    }
    pub fn external_api_users(&self) -> &[DependencyEdgeKey] {
        &self.external_api_users
    }
    pub fn seam_candidates(&self) -> &[CycleSeamCandidateKey] {
        &self.seam_candidates
    }
    pub fn history_observations(&self) -> &[ModuleChangeObservationKey] {
        &self.history_observations
    }
    pub fn evidence_coverage(&self) -> FactCoverage {
        self.evidence_coverage
    }
    pub fn score(&self) -> &ModuleRestructureScore {
        &self.score
    }
    pub fn rank(&self) -> u32 {
        self.rank
    }
    pub fn review_obligations(&self) -> &[String] {
        &self.review_obligations
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ModuleRestructureGapKind {
    SourceArchitecture { gap: ArchitectureGapKey },
    SourceCycleSeam { gap: CycleSeamGapKey },
    MissingCycleSeams { component: ArchitectureComponentKey },
    IncompleteHistory { actual: FactCoverage },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleRestructureGap {
    key: ModuleRestructureGapKey,
    kind: ModuleRestructureGapKind,
}
impl ModuleRestructureGap {
    pub fn key(&self) -> &ModuleRestructureGapKey {
        &self.key
    }
    pub fn kind(&self) -> &ModuleRestructureGapKind {
        &self.kind
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ModuleHistoryStatus {
    NotProvided,
    Provided {
        history: ModuleChangeHistoryId,
        coverage: FactCoverage,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleRestructureCoverageEvidence {
    status: FactCoverage,
    reasons: Vec<String>,
}
impl ModuleRestructureCoverageEvidence {
    pub fn status(&self) -> FactCoverage {
        self.status
    }
    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleRestructureDocument {
    schema: String,
    projection_id: ProjectionId,
    architecture_projection_id: ProjectionId,
    dependency_projection_id: ProjectionId,
    cycle_seam_projection_id: Option<ProjectionId>,
    history_status: ModuleHistoryStatus,
    policy: ModuleRestructurePolicyId,
    coverage: ModuleRestructureCoverageEvidence,
    profiles: Vec<ModuleProfile>,
    candidates: Vec<ModuleRestructureCandidate>,
    gaps: Vec<ModuleRestructureGap>,
}
impl ModuleRestructureDocument {
    pub fn projection_id(&self) -> &ProjectionId {
        &self.projection_id
    }
    pub fn architecture_projection_id(&self) -> &ProjectionId {
        &self.architecture_projection_id
    }
    pub fn dependency_projection_id(&self) -> &ProjectionId {
        &self.dependency_projection_id
    }
    pub fn cycle_seam_projection_id(&self) -> Option<&ProjectionId> {
        self.cycle_seam_projection_id.as_ref()
    }
    pub fn history_status(&self) -> &ModuleHistoryStatus {
        &self.history_status
    }
    pub fn policy(&self) -> &ModuleRestructurePolicyId {
        &self.policy
    }
    pub fn coverage(&self) -> &ModuleRestructureCoverageEvidence {
        &self.coverage
    }
    pub fn profiles(&self) -> &[ModuleProfile] {
        &self.profiles
    }
    pub fn candidates(&self) -> &[ModuleRestructureCandidate] {
        &self.candidates
    }
    pub fn gaps(&self) -> &[ModuleRestructureGap] {
        &self.gaps
    }
    fn validate(&self) -> Result<(), ModuleRestructureBuildError> {
        if self.schema != MODULE_RESTRUCTURE_SCHEMA {
            return Err(invalid("unsupported module-restructure schema"));
        }
        validate_sorted("module profiles", &self.profiles, |v| v.key.as_str())?;
        validate_sorted("module candidates", &self.candidates, |v| v.key.as_str())?;
        validate_sorted("module gaps", &self.gaps, |v| v.key.as_str())?;
        for profile in &self.profiles {
            for values in [&profile.files, &profile.public_api_nodes] {
                validate_distinct(values)?;
            }
            for values in [
                &profile.internal_file_dependencies,
                &profile.incoming_module_dependencies,
                &profile.outgoing_module_dependencies,
                &profile.external_api_users,
            ] {
                validate_distinct(values)?;
            }
            let denominator = profile
                .files
                .len()
                .saturating_mul(profile.files.len().saturating_sub(1));
            if profile.files.is_empty()
                || profile.cohesion
                    != ModuleRatio::new(profile.internal_file_dependencies.len(), denominator)
                || profile.key != profile_key(&self.dependency_projection_id, profile)?
            {
                return Err(invalid("module profile is invalid"));
            }
        }
        let mut ranks = BTreeSet::new();
        for candidate in &self.candidates {
            validate_kind(&candidate.kind)?;
            validate_distinct(&candidate.affected_files)?;
            validate_distinct(&candidate.dependency_edges)?;
            validate_distinct(&candidate.public_api_nodes)?;
            validate_distinct(&candidate.external_api_users)?;
            validate_distinct(&candidate.seam_candidates)?;
            validate_distinct(&candidate.history_observations)?;
            if candidate.disposition != ModuleRestructureDisposition::ReviewRequired
                || candidate.rank == 0
                || candidate.affected_files.is_empty()
                || candidate.review_obligations != review_obligations()
                || candidate.score.authority_penalty > 1
                || (candidate.score.authority_penalty == 0)
                    != (candidate.evidence_coverage == FactCoverage::Complete)
                || candidate.score.public_api_impact != candidate.public_api_nodes.len()
                || candidate.score.external_api_users != candidate.external_api_users.len()
                || candidate.score.affected_files != candidate.affected_files.len()
                || candidate.score.seam_support != candidate.seam_candidates.len()
                || candidate.score.history_support + candidate.score.history_conflicts
                    != candidate.history_observations.len()
                || !ranks.insert(candidate.rank)
                || candidate.key
                    != candidate_key(&self.architecture_projection_id, &self.policy, candidate)?
            {
                return Err(invalid("module candidate is invalid"));
            }
        }
        if ranks != (1..=self.candidates.len() as u32).collect() {
            return Err(invalid("module ranks are not contiguous"));
        }
        let mut ordered = self.candidates.iter().collect::<Vec<_>>();
        ordered.sort_by(|left, right| wire_order(left).cmp(&wire_order(right)));
        if ordered
            .iter()
            .enumerate()
            .any(|(i, v)| v.rank != i as u32 + 1)
        {
            return Err(invalid("module rank does not match score order"));
        }
        for gap in &self.gaps {
            if gap.key != gap_key(&self.architecture_projection_id, &self.policy, &gap.kind)? {
                return Err(invalid("module gap is invalid"));
            }
        }
        if self.coverage != coverage(&self.candidates, &self.gaps) {
            return Err(invalid("module coverage is invalid"));
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleRestructureDocumentWire {
    schema: String,
    projection_id: ProjectionId,
    architecture_projection_id: ProjectionId,
    dependency_projection_id: ProjectionId,
    cycle_seam_projection_id: Option<ProjectionId>,
    history_status: ModuleHistoryStatus,
    policy: ModuleRestructurePolicyId,
    coverage: ModuleRestructureCoverageEvidence,
    profiles: Vec<ModuleProfile>,
    candidates: Vec<ModuleRestructureCandidate>,
    gaps: Vec<ModuleRestructureGap>,
}
impl<'de> Deserialize<'de> for ModuleRestructureDocument {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let w = ModuleRestructureDocumentWire::deserialize(d)?;
        let v = Self {
            schema: w.schema,
            projection_id: w.projection_id,
            architecture_projection_id: w.architecture_projection_id,
            dependency_projection_id: w.dependency_projection_id,
            cycle_seam_projection_id: w.cycle_seam_projection_id,
            history_status: w.history_status,
            policy: w.policy,
            coverage: w.coverage,
            profiles: w.profiles,
            candidates: w.candidates,
            gaps: w.gaps,
        };
        v.validate().map_err(D::Error::custom)?;
        Ok(v)
    }
}

#[derive(Debug, Clone)]
pub struct ModuleRestructureProjection {
    id: ProjectionId,
    architecture: Arc<ArchitectureProjection>,
    cycle_seams: Option<Arc<CycleSeamProjection>>,
    history: Option<Arc<ModuleChangeHistory>>,
    policy: ModuleRestructurePolicyId,
    document: ModuleRestructureDocument,
}
impl ModuleRestructureProjection {
    pub fn id(&self) -> &ProjectionId {
        &self.id
    }
    pub fn architecture(&self) -> &Arc<ArchitectureProjection> {
        &self.architecture
    }
    pub fn cycle_seams(&self) -> Option<&Arc<CycleSeamProjection>> {
        self.cycle_seams.as_ref()
    }
    pub fn history(&self) -> Option<&Arc<ModuleChangeHistory>> {
        self.history.as_ref()
    }
    pub fn policy(&self) -> &ModuleRestructurePolicyId {
        &self.policy
    }
    pub fn document(&self) -> &ModuleRestructureDocument {
        &self.document
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleRestructureBuildError {
    Invalid(String),
    Identity(String),
}
impl fmt::Display for ModuleRestructureBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(v) => write!(f, "invalid module-restructure evidence: {v}"),
            Self::Identity(v) => write!(f, "module-restructure identity error: {v}"),
        }
    }
}
impl std::error::Error for ModuleRestructureBuildError {}

struct Model<'a> {
    module_files: BTreeMap<DependencyNodeKey, BTreeSet<DependencyNodeKey>>,
    file_module: BTreeMap<DependencyNodeKey, DependencyNodeKey>,
    file_edges: Vec<&'a DependencyEdge>,
    module_edges: Vec<&'a DependencyEdge>,
    public_apis: BTreeMap<DependencyNodeKey, BTreeSet<DependencyNodeKey>>,
    api_users: BTreeMap<DependencyNodeKey, BTreeSet<DependencyEdgeKey>>,
}
#[derive(Default)]
struct Evidence {
    files: BTreeSet<DependencyNodeKey>,
    edges: BTreeSet<DependencyEdgeKey>,
    apis: BTreeSet<DependencyNodeKey>,
    users: BTreeSet<DependencyEdgeKey>,
    seams: BTreeSet<CycleSeamCandidateKey>,
    history: BTreeSet<ModuleChangeObservationKey>,
    history_support: usize,
    history_conflicts: usize,
}
struct Draft {
    kind: ModuleRestructureKind,
    evidence: Evidence,
    coupling: usize,
    separation: usize,
    complete: bool,
}

pub fn derive_module_restructure(
    architecture: Arc<ArchitectureProjection>,
    cycle_seams: Option<Arc<CycleSeamProjection>>,
    history: Option<Arc<ModuleChangeHistory>>,
    policy: ModuleRestructurePolicyId,
) -> Result<ModuleRestructureProjection, ModuleRestructureBuildError> {
    let dependency = architecture.dependency();
    if cycle_seams
        .as_ref()
        .is_some_and(|v| v.architecture().id() != architecture.id())
    {
        return Err(invalid("foreign cycle-seam projection"));
    }
    if history
        .as_ref()
        .is_some_and(|v| v.dependency_projection_id() != dependency.id())
    {
        return Err(invalid("foreign module history"));
    }
    let model = model(dependency.document())?;
    if let Some(h) = &history {
        let nodes = dependency
            .document()
            .nodes()
            .iter()
            .map(|v| v.key())
            .collect::<BTreeSet<_>>();
        if h.observations
            .iter()
            .any(|v| !nodes.contains(&v.left) || !nodes.contains(&v.right))
        {
            return Err(invalid("history endpoint is absent"));
        }
    }
    let source_coverage = architecture.document().coverage().status();
    let mut profiles = profiles(dependency.id(), &model, source_coverage)?;
    profiles.sort_by(|a, b| a.key.cmp(&b.key));
    let mut gap_kinds = architecture
        .document()
        .gaps()
        .iter()
        .map(|v| ModuleRestructureGapKind::SourceArchitecture {
            gap: v.key().clone(),
        })
        .collect::<BTreeSet<_>>();
    if let Some(s) = &cycle_seams {
        gap_kinds.extend(s.document().gaps().iter().map(|v| {
            ModuleRestructureGapKind::SourceCycleSeam {
                gap: v.key().clone(),
            }
        }));
    } else {
        gap_kinds.extend(
            architecture
                .document()
                .components()
                .iter()
                .filter(|v| v.level() == ArchitectureLevel::Module && v.cyclic())
                .map(|v| ModuleRestructureGapKind::MissingCycleSeams {
                    component: v.key().clone(),
                }),
        );
    }
    if let Some(h) = &history
        && h.coverage() != FactCoverage::Complete
    {
        gap_kinds.insert(ModuleRestructureGapKind::IncompleteHistory {
            actual: h.coverage(),
        });
    }
    let seam_values = cycle_seams
        .as_ref()
        .map(|v| v.document().candidates())
        .unwrap_or(&[]);
    let history_values = history.as_ref().map(|v| v.observations()).unwrap_or(&[]);
    let module_cycle_present = architecture
        .document()
        .components()
        .iter()
        .any(|component| component.level() == ArchitectureLevel::Module && component.cyclic());
    let seam_complete = cycle_seams.as_ref().map_or(!module_cycle_present, |value| {
        value.document().coverage().status() == FactCoverage::Complete
    });
    let complete = source_coverage == FactCoverage::Complete
        && seam_complete
        && history
            .as_ref()
            .is_none_or(|v| v.coverage() == FactCoverage::Complete);
    let mut drafts = enumerate(&model, seam_values, history_values, complete);
    drafts.sort_by(|left, right| draft_order(left).cmp(&draft_order(right)));
    let mut candidates = Vec::new();
    for (index, d) in drafts.into_iter().enumerate() {
        let score = score(&d);
        let mut v = ModuleRestructureCandidate {
            key: ModuleRestructureCandidateKey(String::new()),
            kind: d.kind,
            disposition: ModuleRestructureDisposition::ReviewRequired,
            affected_files: d.evidence.files.into_iter().collect(),
            dependency_edges: d.evidence.edges.into_iter().collect(),
            public_api_nodes: d.evidence.apis.into_iter().collect(),
            external_api_users: d.evidence.users.into_iter().collect(),
            seam_candidates: d.evidence.seams.into_iter().collect(),
            history_observations: d.evidence.history.into_iter().collect(),
            evidence_coverage: if d.complete {
                FactCoverage::Complete
            } else {
                FactCoverage::Partial
            },
            score,
            rank: u32::try_from(index + 1).map_err(|_| invalid("too many candidates"))?,
            review_obligations: review_obligations(),
        };
        v.key = candidate_key(architecture.id(), &policy, &v)?;
        candidates.push(v);
    }
    candidates.sort_by(|a, b| a.key.cmp(&b.key));
    let mut gaps = gap_kinds
        .into_iter()
        .map(|v| make_gap(architecture.id(), &policy, v))
        .collect::<Result<Vec<_>, _>>()?;
    gaps.sort_by(|a, b| a.key.cmp(&b.key));
    let cov = coverage(&candidates, &gaps);
    let seam_id = cycle_seams.as_ref().map(|v| v.id().clone());
    let history_status = history
        .as_ref()
        .map_or(ModuleHistoryStatus::NotProvided, |v| {
            ModuleHistoryStatus::Provided {
                history: v.id().clone(),
                coverage: v.coverage(),
            }
        });
    let payload = serde_json::to_vec(&(
        architecture.id(),
        dependency.id(),
        &seam_id,
        &history_status,
        &policy,
        &cov,
        &profiles,
        &candidates,
        &gaps,
    ))
    .map_err(|e| ModuleRestructureBuildError::Identity(e.to_string()))?;
    let id = dependency
        .resolution()
        .scope_graph()
        .analysis()
        .derive_projection_id(
            MODULE_RESTRUCTURE_SCHEMA,
            &payload,
            policy.as_str().as_bytes(),
        )
        .map_err(|e| ModuleRestructureBuildError::Identity(e.to_string()))?;
    let document = ModuleRestructureDocument {
        schema: MODULE_RESTRUCTURE_SCHEMA.into(),
        projection_id: id.clone(),
        architecture_projection_id: architecture.id().clone(),
        dependency_projection_id: dependency.id().clone(),
        cycle_seam_projection_id: seam_id,
        history_status,
        policy: policy.clone(),
        coverage: cov,
        profiles,
        candidates,
        gaps,
    };
    document.validate()?;
    Ok(ModuleRestructureProjection {
        id,
        architecture,
        cycle_seams,
        history,
        policy,
        document,
    })
}

fn model(source: &DependencyDocument) -> Result<Model<'_>, ModuleRestructureBuildError> {
    let modules = source
        .nodes()
        .iter()
        .filter(|v| matches!(v.kind(), DependencyNodeKind::Module { .. }))
        .map(|v| v.key().clone())
        .collect::<BTreeSet<_>>();
    let files = source
        .nodes()
        .iter()
        .filter_map(|v| match v.kind() {
            DependencyNodeKind::File { path } => Some((path.clone(), v.key().clone())),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let mut module_files = modules
        .iter()
        .cloned()
        .map(|v| (v, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut file_module = BTreeMap::new();
    for edge in source
        .edges()
        .iter()
        .filter(|v| v.kind() == DependencyEdgeKind::ModuleContainsFile)
    {
        if !module_files
            .get_mut(edge.from())
            .is_some_and(|v| v.insert(edge.to().clone()))
            || file_module
                .insert(edge.to().clone(), edge.from().clone())
                .is_some()
        {
            return Err(invalid("module file ownership is invalid"));
        }
    }
    if module_files.values().any(|v| v.is_empty()) {
        return Err(invalid("module has no files"));
    }
    let mut public_apis: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for node in source.nodes() {
        if let DependencyNodeKind::LocalApi {
            file, visibility, ..
        } = node.kind()
            && visibility.kind == VisibilityKind::Public
            && let Some(f) = files.get(file)
        {
            public_apis
                .entry(f.clone())
                .or_default()
                .insert(node.key().clone());
        }
    }
    let api_owner = public_apis
        .iter()
        .flat_map(|(f, apis)| apis.iter().map(move |a| (a.clone(), f.clone())))
        .collect::<BTreeMap<_, _>>();
    let mut api_users: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for edge in source
        .edges()
        .iter()
        .filter(|v| v.kind() == DependencyEdgeKind::ApiUse)
    {
        if let Some(owner) = api_owner.get(edge.to())
            && file_module.get(edge.from()) != file_module.get(owner)
        {
            api_users
                .entry(edge.to().clone())
                .or_default()
                .insert(edge.key().clone());
        }
    }
    Ok(Model {
        module_files,
        file_module,
        file_edges: source
            .edges()
            .iter()
            .filter(|v| v.kind() == DependencyEdgeKind::FileDependency)
            .collect(),
        module_edges: source
            .edges()
            .iter()
            .filter(|v| v.kind() == DependencyEdgeKind::ModuleDependency)
            .collect(),
        public_apis,
        api_users,
    })
}

fn profiles(
    dep: &ProjectionId,
    m: &Model<'_>,
    coverage: FactCoverage,
) -> Result<Vec<ModuleProfile>, ModuleRestructureBuildError> {
    m.module_files
        .iter()
        .map(|(module, files)| {
            let internal = canon(
                m.file_edges
                    .iter()
                    .filter(|v| files.contains(v.from()) && files.contains(v.to()))
                    .map(|v| v.key().clone())
                    .collect(),
            );
            let incoming = canon(
                m.module_edges
                    .iter()
                    .filter(|v| v.to() == module)
                    .map(|v| v.key().clone())
                    .collect(),
            );
            let outgoing = canon(
                m.module_edges
                    .iter()
                    .filter(|v| v.from() == module)
                    .map(|v| v.key().clone())
                    .collect(),
            );
            let apis = canon(
                files
                    .iter()
                    .flat_map(|f| m.public_apis.get(f).into_iter().flatten())
                    .cloned()
                    .collect(),
            );
            let users = canon(
                apis.iter()
                    .flat_map(|a| m.api_users.get(a).into_iter().flatten())
                    .cloned()
                    .collect(),
            );
            let mut v = ModuleProfile {
                key: ModuleProfileKey(String::new()),
                module: module.clone(),
                files: files.iter().cloned().collect(),
                internal_file_dependencies: internal,
                incoming_module_dependencies: incoming,
                outgoing_module_dependencies: outgoing,
                public_api_nodes: apis,
                external_api_users: users,
                cohesion: None,
                coverage,
            };
            let denominator = v
                .files
                .len()
                .saturating_mul(v.files.len().saturating_sub(1));
            v.cohesion = ModuleRatio::new(v.internal_file_dependencies.len(), denominator);
            v.key = profile_key(dep, &v)?;
            Ok(v)
        })
        .collect()
}

fn enumerate(
    m: &Model<'_>,
    seams: &[CycleSeamCandidate],
    history: &[ModuleChangeObservation],
    complete: bool,
) -> Vec<Draft> {
    let mut out = Vec::new();
    for (source, files) in &m.module_files {
        if files.len() > 1 {
            for file in files {
                let source_edges = incident(m, file, source);
                for target in m.module_files.keys().filter(|v| *v != source) {
                    let target_edges = incident(m, file, target);
                    if target_edges.len() > source_edges.len() {
                        let mut e = evidence(m, std::iter::once(file));
                        e.edges.extend(
                            source_edges
                                .iter()
                                .chain(&target_edges)
                                .map(|v| v.key().clone()),
                        );
                        pair_support(&mut e, file, target, source, seams, history);
                        out.push(Draft {
                            kind: ModuleRestructureKind::MoveFile {
                                file: file.clone(),
                                from_module: source.clone(),
                                to_module: target.clone(),
                            },
                            evidence: e,
                            coupling: target_edges.len() - source_edges.len(),
                            separation: 0,
                            complete,
                        });
                    }
                }
            }
            let groups = components(files, &m.file_edges);
            if groups.len() > 1 {
                let mut e = evidence(m, files.iter());
                e.edges.extend(
                    m.file_edges
                        .iter()
                        .filter(|v| files.contains(v.from()) && files.contains(v.to()))
                        .map(|v| v.key().clone()),
                );
                split_history(&mut e, &groups, history);
                out.push(Draft {
                    kind: ModuleRestructureKind::SplitModule {
                        module: source.clone(),
                        groups: groups.clone(),
                    },
                    evidence: e,
                    coupling: 0,
                    separation: groups.len() - 1,
                    complete,
                });
            }
        }
    }
    let modules = m.module_files.keys().collect::<Vec<_>>();
    for (i, left) in modules.iter().enumerate() {
        for right in modules.iter().skip(i + 1) {
            let forward = m
                .module_edges
                .iter()
                .filter(|v| v.from() == *left && v.to() == *right)
                .collect::<Vec<_>>();
            let reverse = m
                .module_edges
                .iter()
                .filter(|v| v.from() == *right && v.to() == *left)
                .collect::<Vec<_>>();
            if forward.is_empty() || reverse.is_empty() {
                continue;
            }
            let mut e = evidence(
                m,
                m.module_files[*left].iter().chain(&m.module_files[*right]),
            );
            e.edges
                .extend(forward.iter().chain(&reverse).map(|v| v.key().clone()));
            pair_support(&mut e, left, right, left, seams, history);
            out.push(Draft {
                kind: ModuleRestructureKind::MergeModules {
                    left: (*left).clone(),
                    right: (*right).clone(),
                },
                evidence: e,
                coupling: forward.len() + reverse.len(),
                separation: 0,
                complete,
            });
        }
    }
    out
}
fn incident<'a>(
    m: &'a Model<'a>,
    file: &DependencyNodeKey,
    module: &DependencyNodeKey,
) -> Vec<&'a DependencyEdge> {
    m.file_edges
        .iter()
        .copied()
        .filter(|v| {
            (v.from() == file && m.file_module.get(v.to()) == Some(module))
                || (v.to() == file && m.file_module.get(v.from()) == Some(module))
        })
        .collect()
}
fn evidence<'a>(m: &Model<'_>, files: impl IntoIterator<Item = &'a DependencyNodeKey>) -> Evidence {
    let files = files.into_iter().cloned().collect::<BTreeSet<_>>();
    let apis = files
        .iter()
        .flat_map(|f| m.public_apis.get(f).into_iter().flatten())
        .cloned()
        .collect::<BTreeSet<_>>();
    let users = apis
        .iter()
        .flat_map(|a| m.api_users.get(a).into_iter().flatten())
        .cloned()
        .collect();
    Evidence {
        files,
        apis,
        users,
        ..Evidence::default()
    }
}
fn pair_support(
    e: &mut Evidence,
    left: &DependencyNodeKey,
    right: &DependencyNodeKey,
    context: &DependencyNodeKey,
    seams: &[CycleSeamCandidate],
    history: &[ModuleChangeObservation],
) {
    for s in seams.iter().filter(|s| {
        s.level() == ArchitectureLevel::Module
            && ((s.from() == context && s.to() == right)
                || (s.from() == right && s.to() == context)
                || (s.from() == left && s.to() == right)
                || (s.from() == right && s.to() == left))
    }) {
        e.seams.insert(s.key().clone());
    }
    for h in history.iter().filter(|h| {
        (h.left() == left && h.right() == right) || (h.left() == right && h.right() == left)
    }) {
        e.history.insert(h.key().clone());
        e.history_support += 1;
    }
}
fn split_history(
    e: &mut Evidence,
    groups: &[Vec<DependencyNodeKey>],
    history: &[ModuleChangeObservation],
) {
    let map = groups
        .iter()
        .enumerate()
        .flat_map(|(i, g)| g.iter().map(move |f| (f, i)))
        .collect::<BTreeMap<_, _>>();
    for h in history.iter().filter(|h| {
        map.get(h.left())
            .zip(map.get(h.right()))
            .is_some_and(|(a, b)| a != b)
    }) {
        e.history.insert(h.key().clone());
        e.history_conflicts += 1;
    }
}
fn components(
    files: &BTreeSet<DependencyNodeKey>,
    edges: &[&DependencyEdge],
) -> Vec<Vec<DependencyNodeKey>> {
    let mut adj = files
        .iter()
        .cloned()
        .map(|v| (v, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for e in edges
        .iter()
        .filter(|e| files.contains(e.from()) && files.contains(e.to()))
    {
        adj.get_mut(e.from()).unwrap().insert(e.to().clone());
        adj.get_mut(e.to()).unwrap().insert(e.from().clone());
    }
    let mut remaining = files.clone();
    let mut groups = Vec::new();
    while let Some(root) = remaining.pop_first() {
        let mut group = BTreeSet::from([root.clone()]);
        let mut q = VecDeque::from([root]);
        while let Some(v) = q.pop_front() {
            for n in &adj[&v] {
                if remaining.remove(n) {
                    group.insert(n.clone());
                    q.push_back(n.clone());
                }
            }
        }
        groups.push(group.into_iter().collect());
    }
    groups.sort();
    groups
}

fn score(d: &Draft) -> ModuleRestructureScore {
    ModuleRestructureScore {
        authority_penalty: usize::from(!d.complete),
        coupling_reduction: d.coupling,
        cohesion_separation: d.separation,
        public_api_impact: d.evidence.apis.len(),
        external_api_users: d.evidence.users.len(),
        affected_files: d.evidence.files.len(),
        seam_support: d.evidence.seams.len(),
        history_support: d.evidence.history_support,
        history_conflicts: d.evidence.history_conflicts,
    }
}
type CandidateOrder<'a> = (
    usize,
    usize,
    usize,
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    Reverse<usize>,
    usize,
    &'a ModuleRestructureKind,
);

fn draft_order(d: &Draft) -> CandidateOrder<'_> {
    let s = score(d);
    (
        s.authority_penalty,
        s.public_api_impact,
        s.external_api_users,
        Reverse(s.coupling_reduction),
        Reverse(s.cohesion_separation),
        Reverse(s.seam_support),
        Reverse(s.history_support),
        s.history_conflicts,
        &d.kind,
    )
}
fn wire_order(v: &ModuleRestructureCandidate) -> CandidateOrder<'_> {
    let s = &v.score;
    (
        s.authority_penalty,
        s.public_api_impact,
        s.external_api_users,
        Reverse(s.coupling_reduction),
        Reverse(s.cohesion_separation),
        Reverse(s.seam_support),
        Reverse(s.history_support),
        s.history_conflicts,
        &v.kind,
    )
}
fn validate_kind(v: &ModuleRestructureKind) -> Result<(), ModuleRestructureBuildError> {
    match v {
        ModuleRestructureKind::MoveFile {
            from_module,
            to_module,
            ..
        } if from_module == to_module => Err(invalid("move source equals target")),
        ModuleRestructureKind::SplitModule { groups, .. }
            if groups.len() < 2
                || groups.iter().any(Vec::is_empty)
                || groups.windows(2).any(|v| v[0] >= v[1])
                || groups.iter().any(|g| g.windows(2).any(|v| v[0] >= v[1])) =>
        {
            Err(invalid("split groups are invalid"))
        }
        ModuleRestructureKind::MergeModules { left, right } if left >= right => {
            Err(invalid("merge endpoints are invalid"))
        }
        _ => Ok(()),
    }
}
fn review_obligations() -> Vec<String> {
    vec![
        "confirm destination and responsibility ownership".into(),
        "preserve public API and initialization semantics".into(),
        "run impacted build and behavior verification".into(),
        "update imports exports and build declarations".into(),
    ]
}

fn observation_key(
    dep: &ProjectionId,
    v: &ModuleChangeObservation,
) -> Result<ModuleChangeObservationKey, ModuleRestructureBuildError> {
    let p = serde_json::to_vec(&(
        &v.left,
        &v.right,
        v.joint_changes,
        v.left_changes,
        v.right_changes,
        &v.jaccard,
    ))
    .map_err(|e| ModuleRestructureBuildError::Identity(e.to_string()))?;
    Ok(ModuleChangeObservationKey(derive_id(
        OBSERVATION_DOMAIN,
        "mco1_",
        &[dep.as_str().as_bytes(), &p],
    )))
}
fn history_id(
    v: &ModuleChangeHistory,
) -> Result<ModuleChangeHistoryId, ModuleRestructureBuildError> {
    let p = serde_json::to_vec(&(
        &v.dependency_projection_id,
        v.coverage,
        &v.reasons,
        &v.observations,
    ))
    .map_err(|e| ModuleRestructureBuildError::Identity(e.to_string()))?;
    Ok(ModuleChangeHistoryId(derive_id(
        HISTORY_DOMAIN,
        "mch1_",
        &[v.dependency_projection_id.as_str().as_bytes(), &p],
    )))
}
fn profile_key(
    dep: &ProjectionId,
    v: &ModuleProfile,
) -> Result<ModuleProfileKey, ModuleRestructureBuildError> {
    let p = serde_json::to_vec(&(
        &v.module,
        &v.files,
        &v.internal_file_dependencies,
        &v.incoming_module_dependencies,
        &v.outgoing_module_dependencies,
        &v.public_api_nodes,
        &v.external_api_users,
        &v.cohesion,
        v.coverage,
    ))
    .map_err(|e| ModuleRestructureBuildError::Identity(e.to_string()))?;
    Ok(ModuleProfileKey(derive_id(
        PROFILE_DOMAIN,
        "mrf1_",
        &[dep.as_str().as_bytes(), &p],
    )))
}
fn candidate_key(
    a: &ProjectionId,
    p: &ModuleRestructurePolicyId,
    v: &ModuleRestructureCandidate,
) -> Result<ModuleRestructureCandidateKey, ModuleRestructureBuildError> {
    let bytes = serde_json::to_vec(&(
        &v.kind,
        v.disposition,
        &v.affected_files,
        &v.dependency_edges,
        &v.public_api_nodes,
        &v.external_api_users,
        &v.seam_candidates,
        &v.history_observations,
        v.evidence_coverage,
        &v.score,
        v.rank,
        &v.review_obligations,
    ))
    .map_err(|e| ModuleRestructureBuildError::Identity(e.to_string()))?;
    Ok(ModuleRestructureCandidateKey(derive_id(
        CANDIDATE_DOMAIN,
        "mrc1_",
        &[a.as_str().as_bytes(), p.as_str().as_bytes(), &bytes],
    )))
}
fn make_gap(
    a: &ProjectionId,
    p: &ModuleRestructurePolicyId,
    kind: ModuleRestructureGapKind,
) -> Result<ModuleRestructureGap, ModuleRestructureBuildError> {
    let bytes = serde_json::to_vec(&kind)
        .map_err(|e| ModuleRestructureBuildError::Identity(e.to_string()))?;
    Ok(ModuleRestructureGap {
        key: ModuleRestructureGapKey(derive_id(
            GAP_DOMAIN,
            "mrg1_",
            &[a.as_str().as_bytes(), p.as_str().as_bytes(), &bytes],
        )),
        kind,
    })
}
fn gap_key(
    a: &ProjectionId,
    p: &ModuleRestructurePolicyId,
    k: &ModuleRestructureGapKind,
) -> Result<ModuleRestructureGapKey, ModuleRestructureBuildError> {
    Ok(make_gap(a, p, k.clone())?.key)
}
fn coverage(
    c: &[ModuleRestructureCandidate],
    g: &[ModuleRestructureGap],
) -> ModuleRestructureCoverageEvidence {
    let status = if g.is_empty()
        && c.iter()
            .all(|v| v.evidence_coverage == FactCoverage::Complete)
    {
        FactCoverage::Complete
    } else {
        FactCoverage::Partial
    };
    ModuleRestructureCoverageEvidence {
        status,
        reasons: g
            .iter()
            .map(|v| format!("module-restructure gap {}", v.key.as_str()))
            .collect(),
    }
}
fn canon<T: Ord>(mut v: Vec<T>) -> Vec<T> {
    v.sort();
    v.dedup();
    v
}
fn validate_distinct<T: Ord>(v: &[T]) -> Result<(), ModuleRestructureBuildError> {
    if v.windows(2).any(|p| p[0] >= p[1]) {
        Err(invalid("evidence is not canonical"))
    } else {
        Ok(())
    }
}
fn canonical_texts(v: &mut Vec<String>) -> Result<(), ModuleRestructureBuildError> {
    for x in v.iter() {
        validate_text(x)?;
    }
    v.sort();
    v.dedup();
    Ok(())
}
fn validate_texts(v: &[String]) -> Result<(), ModuleRestructureBuildError> {
    validate_distinct(v)?;
    for x in v {
        validate_text(x)?;
    }
    Ok(())
}
fn validate_text(v: &str) -> Result<(), ModuleRestructureBuildError> {
    if v.trim().is_empty() || v.trim() != v {
        Err(invalid("text is not canonical"))
    } else {
        Ok(())
    }
}
fn validate_sorted<T>(
    label: &str,
    v: &[T],
    key: impl Fn(&T) -> &str,
) -> Result<(), ModuleRestructureBuildError> {
    let keys = v.iter().map(key).collect::<Vec<_>>();
    if keys.windows(2).any(|p| p[0] >= p[1]) {
        Err(invalid(format!("{label} are not canonical")))
    } else {
        Ok(())
    }
}
fn validate_digest(v: &str, p: &str) -> Result<(), ModuleRestructureBuildError> {
    let Some(d) = v.strip_prefix(p) else {
        return Err(invalid(format!("identity must start with {p}")));
    };
    if d.len() != 64
        || !d
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Err(invalid("identity digest is invalid"))
    } else {
        Ok(())
    }
}
fn invalid(v: impl Into<String>) -> ModuleRestructureBuildError {
    ModuleRestructureBuildError::Invalid(v.into())
}
fn derive_id(domain: &str, prefix: &str, parts: &[&[u8]]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(&(domain.len() as u64).to_le_bytes());
    h.update(domain.as_bytes());
    for p in parts {
        h.update(&(p.len() as u64).to_le_bytes());
        h.update(p);
    }
    format!("{prefix}{}", h.finalize().to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle_seam::tests::{complete_cycle_data_flow, cycle_architecture};
    use crate::dependency::tests::{FixtureEndpoint, dependency_fixture};
    use crate::{CycleSeamPolicyId, FactCoverageEvidence, derive_cycle_seams};

    fn policy() -> ModuleRestructurePolicyId {
        ModuleRestructurePolicyId::from_parts(&[b"module-restructure-test/1"]).unwrap()
    }

    fn exact_sources() -> (Arc<ArchitectureProjection>, Arc<CycleSeamProjection>) {
        let architecture = cycle_architecture();
        let data_flow = complete_cycle_data_flow(&architecture);
        let seams = Arc::new(
            derive_cycle_seams(
                Arc::clone(&architecture),
                Some(data_flow),
                CycleSeamPolicyId::from_parts(&[b"module-restructure-seams/1"]).unwrap(),
            )
            .unwrap(),
        );
        (architecture, seams)
    }

    fn module_keys(architecture: &ArchitectureProjection) -> Vec<DependencyNodeKey> {
        let mut keys = architecture
            .dependency()
            .document()
            .nodes()
            .iter()
            .filter(|node| matches!(node.kind(), DependencyNodeKind::Module { .. }))
            .map(|node| node.key().clone())
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }

    #[test]
    fn exact_cycle_emits_one_merge_with_seam_api_and_impact_evidence() {
        let (architecture, seams) = exact_sources();
        let projection =
            derive_module_restructure(architecture, Some(seams), None, policy()).unwrap();
        let document = projection.document();
        assert_eq!(document.coverage().status(), FactCoverage::Complete);
        assert_eq!(document.profiles().len(), 2);
        assert_eq!(document.candidates().len(), 1);
        assert!(document.gaps().is_empty());
        let candidate = &document.candidates()[0];
        assert!(matches!(
            candidate.kind(),
            ModuleRestructureKind::MergeModules { .. }
        ));
        assert_eq!(
            candidate.disposition(),
            ModuleRestructureDisposition::ReviewRequired
        );
        assert_eq!(candidate.rank(), 1);
        assert_eq!(candidate.affected_files().len(), 2);
        assert_eq!(candidate.dependency_edges().len(), 2);
        assert_eq!(candidate.public_api_nodes().len(), 2);
        assert_eq!(candidate.external_api_users().len(), 2);
        assert_eq!(candidate.seam_candidates().len(), 2);
        assert_eq!(candidate.score().coupling_reduction(), 2);
        assert_eq!(candidate.score().authority_penalty(), 0);
        assert!(matches!(
            document.history_status(),
            ModuleHistoryStatus::NotProvided
        ));
    }

    #[test]
    fn missing_seams_retains_merge_as_partial_review_evidence() {
        let architecture = cycle_architecture();
        let projection = derive_module_restructure(architecture, None, None, policy()).unwrap();
        assert_eq!(projection.document().candidates().len(), 1);
        assert_eq!(
            projection.document().coverage().status(),
            FactCoverage::Partial
        );
        assert!(projection.document().gaps().iter().any(|gap| matches!(
            gap.kind(),
            ModuleRestructureGapKind::MissingCycleSeams { .. }
        )));
        assert_eq!(
            projection.document().candidates()[0].score().seam_support(),
            0
        );
        assert_eq!(
            projection.document().candidates()[0].evidence_coverage(),
            FactCoverage::Partial
        );
    }

    #[test]
    fn optional_history_changes_support_and_identity_without_becoming_legality() {
        let (architecture, seams) = exact_sources();
        let modules = module_keys(&architecture);
        let history = Arc::new(
            ModuleChangeHistory::new(
                architecture.dependency().id().clone(),
                FactCoverage::Complete,
                vec![],
                vec![ModuleChangeObservationDraft {
                    left: modules[0].clone(),
                    right: modules[1].clone(),
                    joint_changes: 3,
                    left_changes: 5,
                    right_changes: 4,
                }],
            )
            .unwrap(),
        );
        assert_eq!(history.observations()[0].jaccard().numerator(), 3);
        assert_eq!(history.observations()[0].jaccard().denominator(), 6);
        let with_history = derive_module_restructure(
            Arc::clone(&architecture),
            Some(Arc::clone(&seams)),
            Some(history),
            policy(),
        )
        .unwrap();
        let without_history =
            derive_module_restructure(architecture, Some(seams), None, policy()).unwrap();
        assert_ne!(with_history.id(), without_history.id());
        let candidate = &with_history.document().candidates()[0];
        assert_eq!(candidate.history_observations().len(), 1);
        assert_eq!(candidate.score().history_support(), 1);
        assert_eq!(
            candidate.disposition(),
            ModuleRestructureDisposition::ReviewRequired
        );
        assert_eq!(
            with_history.document().coverage().status(),
            FactCoverage::Complete
        );
    }

    #[test]
    fn incomplete_history_is_explicit_and_downgrades_candidates() {
        let (architecture, seams) = exact_sources();
        let modules = module_keys(&architecture);
        let history = Arc::new(
            ModuleChangeHistory::new(
                architecture.dependency().id().clone(),
                FactCoverage::Partial,
                vec!["history covers only the retained release window".into()],
                vec![ModuleChangeObservationDraft {
                    left: modules[0].clone(),
                    right: modules[1].clone(),
                    joint_changes: 1,
                    left_changes: 2,
                    right_changes: 2,
                }],
            )
            .unwrap(),
        );
        let projection =
            derive_module_restructure(architecture, Some(seams), Some(history), policy()).unwrap();
        assert_eq!(
            projection.document().coverage().status(),
            FactCoverage::Partial
        );
        assert!(projection.document().gaps().iter().any(|gap| matches!(
            gap.kind(),
            ModuleRestructureGapKind::IncompleteHistory {
                actual: FactCoverage::Partial
            }
        )));
        assert_eq!(
            projection.document().candidates()[0]
                .score()
                .authority_penalty(),
            1
        );
    }

    #[test]
    fn pinned_internal_model_enumerates_move_split_and_merge_without_emptying_source() {
        let architecture = cycle_architecture();
        let document = architecture.dependency().document();
        let mut files = document
            .nodes()
            .iter()
            .filter(|node| matches!(node.kind(), DependencyNodeKind::File { .. }))
            .map(|node| node.key().clone())
            .collect::<Vec<_>>();
        files.sort();
        let extra = document
            .nodes()
            .iter()
            .find(|node| {
                !files.contains(&node.key().clone())
                    && !matches!(node.kind(), DependencyNodeKind::Module { .. })
            })
            .unwrap()
            .key()
            .clone();
        let modules = module_keys(&architecture);
        let file_edges = document
            .edges()
            .iter()
            .filter(|edge| edge.kind() == DependencyEdgeKind::FileDependency)
            .collect::<Vec<_>>();
        let module_edges = document
            .edges()
            .iter()
            .filter(|edge| edge.kind() == DependencyEdgeKind::ModuleDependency)
            .collect::<Vec<_>>();
        let source = architecture
            .dependency()
            .document()
            .edges()
            .iter()
            .find(|edge| edge.kind() == DependencyEdgeKind::ModuleDependency)
            .unwrap()
            .from()
            .clone();
        let target = modules
            .iter()
            .find(|module| **module != source)
            .unwrap()
            .clone();
        let source_file = file_edges[0].from().clone();
        let target_file = file_edges[0].to().clone();
        let model = Model {
            module_files: BTreeMap::from([
                (
                    source.clone(),
                    BTreeSet::from([source_file.clone(), extra.clone()]),
                ),
                (target.clone(), BTreeSet::from([target_file.clone()])),
            ]),
            file_module: BTreeMap::from([
                (source_file.clone(), source.clone()),
                (extra, source.clone()),
                (target_file, target.clone()),
            ]),
            file_edges,
            module_edges,
            public_apis: BTreeMap::new(),
            api_users: BTreeMap::new(),
        };
        let drafts = enumerate(&model, &[], &[], true);
        assert_eq!(drafts.len(), 3);
        assert!(drafts.iter().any(|draft| matches!(draft.kind, ModuleRestructureKind::MoveFile { ref from_module, ref to_module, .. } if from_module == &source && to_module == &target)));
        assert!(drafts.iter().any(|draft| matches!(draft.kind, ModuleRestructureKind::SplitModule { ref groups, .. } if groups.len() == 2)));
        assert!(
            drafts
                .iter()
                .any(|draft| matches!(draft.kind, ModuleRestructureKind::MergeModules { .. }))
        );
    }

    #[test]
    fn foreign_history_is_rejected_before_candidate_join() {
        let (architecture, seams) = exact_sources();
        let foreign = dependency_fixture(
            FactCoverageEvidence::complete(),
            FixtureEndpoint::ProviderDeclaration,
            false,
        );
        let nodes = foreign.document().nodes();
        let history = Arc::new(
            ModuleChangeHistory::new(
                foreign.id().clone(),
                FactCoverage::Complete,
                vec![],
                vec![ModuleChangeObservationDraft {
                    left: nodes[0].key().clone(),
                    right: nodes[1].key().clone(),
                    joint_changes: 1,
                    left_changes: 1,
                    right_changes: 1,
                }],
            )
            .unwrap(),
        );
        assert!(
            derive_module_restructure(architecture, Some(seams), Some(history), policy()).is_err()
        );
    }

    #[test]
    fn one_way_coupling_does_not_create_a_merge_candidate() {
        let dependency = Arc::new(dependency_fixture(
            FactCoverageEvidence::complete(),
            FixtureEndpoint::ProviderDeclaration,
            false,
        ));
        let architecture = Arc::new(
            crate::derive_architecture(
                dependency,
                crate::ArchitecturePolicy::new(vec![], vec![]).unwrap(),
            )
            .unwrap(),
        );
        let projection = derive_module_restructure(architecture, None, None, policy()).unwrap();
        assert_eq!(
            projection.document().coverage().status(),
            FactCoverage::Complete
        );
        assert!(projection.document().candidates().is_empty());
        assert!(projection.document().gaps().is_empty());
    }

    #[test]
    fn change_history_round_trip_is_strict_and_count_bound() {
        let architecture = cycle_architecture();
        let modules = module_keys(&architecture);
        let history = ModuleChangeHistory::new(
            architecture.dependency().id().clone(),
            FactCoverage::Complete,
            vec![],
            vec![ModuleChangeObservationDraft {
                left: modules[0].clone(),
                right: modules[1].clone(),
                joint_changes: 2,
                left_changes: 3,
                right_changes: 4,
            }],
        )
        .unwrap();
        let bytes = serde_json::to_vec(&history).unwrap();
        let decoded: ModuleChangeHistory = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(bytes, serde_json::to_vec(&decoded).unwrap());
        let mut tampered = serde_json::to_value(&history).unwrap();
        tampered["observations"][0]["joint_changes"] = serde_json::json!(3);
        assert!(serde_json::from_value::<ModuleChangeHistory>(tampered).is_err());
    }

    #[test]
    fn deterministic_strict_round_trip_and_rank_tamper_rejection() {
        let (architecture, seams) = exact_sources();
        let first = derive_module_restructure(
            Arc::clone(&architecture),
            Some(Arc::clone(&seams)),
            None,
            policy(),
        )
        .unwrap();
        let second = derive_module_restructure(architecture, Some(seams), None, policy()).unwrap();
        assert_eq!(first.id(), second.id());
        let bytes = serde_json::to_vec(first.document()).unwrap();
        assert_eq!(bytes, serde_json::to_vec(second.document()).unwrap());
        let decoded: ModuleRestructureDocument = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(bytes, serde_json::to_vec(&decoded).unwrap());
        let mut tampered = serde_json::to_value(first.document()).unwrap();
        tampered["candidates"][0]["rank"] = serde_json::json!(9);
        assert!(serde_json::from_value::<ModuleRestructureDocument>(tampered).is_err());
    }
}
