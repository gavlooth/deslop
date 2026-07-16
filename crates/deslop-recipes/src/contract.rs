use std::collections::BTreeSet;
use std::fmt;
use std::path::{Component, Path};

use deslop_core::{RevisionGuard, SafetyClass, Span, revision_guard};
use deslop_parse::{
    AdapterCapability, CapabilityAuthority, CapabilitySupport, GraphEligibilityDecision,
    GraphEvidenceLayer, GraphRecipeRequirement, NodeKey, SubtreeFingerprint,
};
use serde::{Deserialize, Serialize};

pub const TRANSFORMATION_RECIPE_SCHEMA: &str = "deslop.transformation-recipe/1";
pub const TRANSFORMATION_CANDIDATE_SCHEMA: &str = "deslop.transformation-candidate/1";

const RECIPE_ID_DOMAIN: &str = "deslop.transformation-recipe-id/1";
const CANDIDATE_ID_DOMAIN: &str = "deslop.transformation-candidate-id/1";

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
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                validate_digest_id(&value, $prefix).map_err(serde::de::Error::custom)?;
                Ok(Self(value))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

digest_id!(RecipeId, "rcp1_");
digest_id!(CandidateId, "tcn1_");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransformationFamily {
    BranchControl,
    FunctionExpression,
    DependencyModule,
    CloneCeremonyDeadCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProofState {
    Proven,
    Disproven,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeFixtureRole {
    Positive,
    NoOp,
    MinimalCounterexample,
    AdversarialNearMiss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureExpectation {
    Candidate,
    NoCandidate,
    ReviewRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeCondition {
    pub key: String,
    pub description: String,
    pub layer: GraphEvidenceLayer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeFixture {
    pub role: RecipeFixtureRole,
    pub name: String,
    pub expectation: FixtureExpectation,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ValidationStepKind {
    Parse,
    Format,
    StaticAnalysis,
    Build,
    Test,
    GraphDelta,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationStep {
    pub key: String,
    pub kind: ValidationStepKind,
    pub description: String,
    pub command: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationPlan {
    pub steps: Vec<ValidationStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RollbackStrategy {
    ReverseExactEdits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollbackPlan {
    pub strategy: RollbackStrategy,
    pub require_revision_guards: bool,
    pub validation_steps: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TransformationRecipeDraft {
    pub name: String,
    pub version: String,
    pub family: TransformationFamily,
    pub required_layers: Vec<GraphEvidenceLayer>,
    pub required_conditions: Vec<RecipeCondition>,
    pub forbidden_conditions: Vec<RecipeCondition>,
    pub maximum_safety: SafetyClass,
    pub validation_plan: ValidationPlan,
    pub rollback_plan: RollbackPlan,
    pub fixtures: Vec<RecipeFixture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransformationRecipe {
    schema: String,
    id: RecipeId,
    name: String,
    version: String,
    family: TransformationFamily,
    required_layers: Vec<GraphEvidenceLayer>,
    required_conditions: Vec<RecipeCondition>,
    forbidden_conditions: Vec<RecipeCondition>,
    maximum_safety: SafetyClass,
    validation_plan: ValidationPlan,
    rollback_plan: RollbackPlan,
    fixtures: Vec<RecipeFixture>,
}

impl TransformationRecipe {
    pub fn new(mut draft: TransformationRecipeDraft) -> Result<Self, RecipeContractError> {
        draft.required_layers.sort();
        draft
            .required_conditions
            .sort_by(|left, right| left.key.cmp(&right.key));
        draft
            .forbidden_conditions
            .sort_by(|left, right| left.key.cmp(&right.key));
        draft.fixtures.sort_by_key(|fixture| fixture.role);
        draft
            .validation_plan
            .steps
            .sort_by(|left, right| left.key.cmp(&right.key));
        draft.rollback_plan.validation_steps.sort();

        let id = derive_recipe_id(&draft)?;
        let recipe = Self {
            schema: TRANSFORMATION_RECIPE_SCHEMA.into(),
            id,
            name: draft.name,
            version: draft.version,
            family: draft.family,
            required_layers: draft.required_layers,
            required_conditions: draft.required_conditions,
            forbidden_conditions: draft.forbidden_conditions,
            maximum_safety: draft.maximum_safety,
            validation_plan: draft.validation_plan,
            rollback_plan: draft.rollback_plan,
            fixtures: draft.fixtures,
        };
        recipe.validate()?;
        Ok(recipe)
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn id(&self) -> &RecipeId {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn family(&self) -> TransformationFamily {
        self.family
    }

    pub fn required_layers(&self) -> &[GraphEvidenceLayer] {
        &self.required_layers
    }

    pub fn required_conditions(&self) -> &[RecipeCondition] {
        &self.required_conditions
    }

    pub fn forbidden_conditions(&self) -> &[RecipeCondition] {
        &self.forbidden_conditions
    }

    pub fn maximum_safety(&self) -> SafetyClass {
        self.maximum_safety
    }

    pub fn validation_plan(&self) -> &ValidationPlan {
        &self.validation_plan
    }

    pub fn rollback_plan(&self) -> &RollbackPlan {
        &self.rollback_plan
    }

    pub fn fixtures(&self) -> &[RecipeFixture] {
        &self.fixtures
    }

    pub fn eligibility_requirement(&self) -> GraphRecipeRequirement {
        GraphRecipeRequirement::new(self.id.as_str(), self.required_layers.clone())
            .expect("a validated recipe retains dependency-closed graph layers")
    }

    fn validate(&self) -> Result<(), RecipeContractError> {
        if self.schema != TRANSFORMATION_RECIPE_SCHEMA {
            return Err(invalid("unsupported transformation-recipe schema"));
        }
        validate_text("recipe name", &self.name)?;
        validate_text("recipe version", &self.version)?;
        validate_canonical_layers(&self.required_layers)?;
        GraphRecipeRequirement::new(self.name.clone(), self.required_layers.clone())
            .map_err(|error| invalid(error.to_string()))?;
        validate_conditions("required conditions", &self.required_conditions)?;
        validate_conditions("forbidden conditions", &self.forbidden_conditions)?;
        if self
            .required_conditions
            .iter()
            .chain(&self.forbidden_conditions)
            .any(|condition| {
                self.required_layers
                    .binary_search(&condition.layer)
                    .is_err()
            })
        {
            return Err(invalid(
                "recipe condition names a graph layer outside the requirements",
            ));
        }
        if self.required_conditions.is_empty() {
            return Err(invalid(
                "a recipe requires at least one positive obligation",
            ));
        }
        let required = self
            .required_conditions
            .iter()
            .map(|condition| condition.key.as_str())
            .collect::<BTreeSet<_>>();
        if self
            .forbidden_conditions
            .iter()
            .any(|condition| required.contains(condition.key.as_str()))
        {
            return Err(invalid("required and forbidden condition keys overlap"));
        }
        validate_validation_plan(&self.validation_plan)?;
        validate_rollback_plan(&self.rollback_plan, &self.validation_plan)?;
        validate_fixtures(&self.fixtures)?;

        let draft = TransformationRecipeDraft {
            name: self.name.clone(),
            version: self.version.clone(),
            family: self.family,
            required_layers: self.required_layers.clone(),
            required_conditions: self.required_conditions.clone(),
            forbidden_conditions: self.forbidden_conditions.clone(),
            maximum_safety: self.maximum_safety,
            validation_plan: self.validation_plan.clone(),
            rollback_plan: self.rollback_plan.clone(),
            fixtures: self.fixtures.clone(),
        };
        if self.id != derive_recipe_id(&draft)? {
            return Err(invalid("transformation-recipe identity is stale"));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransformationRecipeWire {
    schema: String,
    id: RecipeId,
    name: String,
    version: String,
    family: TransformationFamily,
    required_layers: Vec<GraphEvidenceLayer>,
    required_conditions: Vec<RecipeCondition>,
    forbidden_conditions: Vec<RecipeCondition>,
    maximum_safety: SafetyClass,
    validation_plan: ValidationPlan,
    rollback_plan: RollbackPlan,
    fixtures: Vec<RecipeFixture>,
}

impl<'de> Deserialize<'de> for TransformationRecipe {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = TransformationRecipeWire::deserialize(deserializer)?;
        let recipe = Self {
            schema: wire.schema,
            id: wire.id,
            name: wire.name,
            version: wire.version,
            family: wire.family,
            required_layers: wire.required_layers,
            required_conditions: wire.required_conditions,
            forbidden_conditions: wire.forbidden_conditions,
            maximum_safety: wire.maximum_safety,
            validation_plan: wire.validation_plan,
            rollback_plan: wire.rollback_plan,
            fixtures: wire.fixtures,
        };
        recipe.validate().map_err(serde::de::Error::custom)?;
        Ok(recipe)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphEntityRef {
    pub layer: GraphEvidenceLayer,
    pub graph: String,
    pub entity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionEvidence {
    pub entity: GraphEntityRef,
    pub detail: String,
    pub capability: Option<AdapterCapability>,
    pub support: Option<CapabilitySupport>,
    pub authority: Option<CapabilityAuthority>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionResult {
    pub condition: String,
    pub state: ProofState,
    pub evidence: Vec<ConditionEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImpactDirection {
    Incoming,
    Outgoing,
    Bidirectional,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactConeQuery {
    pub roots: Vec<GraphEntityRef>,
    pub direction: ImpactDirection,
    pub layers: Vec<GraphEvidenceLayer>,
    pub maximum_depth: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactCone {
    pub query: ImpactConeQuery,
    pub entities: Vec<GraphEntityRef>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GraphChangeKind {
    Add,
    Remove,
    Modify,
    Preserve,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedGraphChange {
    pub kind: GraphChangeKind,
    pub entity: GraphEntityRef,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedGraphDelta {
    pub changes: Vec<ExpectedGraphChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateSource {
    pub project_snapshot: String,
    pub analysis: String,
    pub program_dependence_projection: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateTarget {
    pub entity: GraphEntityRef,
    pub node: NodeKey,
    pub span: Span,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtree_fingerprint: Option<SubtreeFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformationEdit {
    pub target: NodeKey,
    pub span: Span,
    pub before: String,
    pub after: String,
    pub revision_guard: RevisionGuard,
}

impl TransformationEdit {
    pub fn exact_node_replacement(
        target: NodeKey,
        span: Span,
        before: String,
        after: String,
    ) -> Self {
        let guard = revision_guard(target.file().path.as_path(), span, &before);
        Self {
            target,
            span,
            before,
            after,
            revision_guard: guard,
        }
    }

    pub fn exact_node_deletion(target: NodeKey, span: Span, before: String) -> Self {
        Self::exact_node_replacement(target, span, before, String::new())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateDisposition {
    Automatic,
    ReviewRequired,
}

#[derive(Debug, Clone)]
pub struct TransformationCandidateDraft {
    pub recipe: TransformationRecipe,
    pub source: CandidateSource,
    pub target: CandidateTarget,
    pub eligibility: GraphEligibilityDecision,
    pub required_results: Vec<ConditionResult>,
    pub forbidden_results: Vec<ConditionResult>,
    pub impact: ImpactCone,
    pub expected_delta: ExpectedGraphDelta,
    pub edits: Vec<TransformationEdit>,
    pub safety: SafetyClass,
    pub disposition: CandidateDisposition,
    pub validation_plan: ValidationPlan,
    pub rollback_plan: RollbackPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransformationCandidate {
    schema: String,
    id: CandidateId,
    recipe: TransformationRecipe,
    source: CandidateSource,
    target: CandidateTarget,
    eligibility: GraphEligibilityDecision,
    required_results: Vec<ConditionResult>,
    forbidden_results: Vec<ConditionResult>,
    impact: ImpactCone,
    expected_delta: ExpectedGraphDelta,
    edits: Vec<TransformationEdit>,
    safety: SafetyClass,
    disposition: CandidateDisposition,
    validation_plan: ValidationPlan,
    rollback_plan: RollbackPlan,
}

impl TransformationCandidate {
    pub fn new(mut draft: TransformationCandidateDraft) -> Result<Self, RecipeContractError> {
        draft
            .required_results
            .sort_by(|left, right| left.condition.cmp(&right.condition));
        draft
            .forbidden_results
            .sort_by(|left, right| left.condition.cmp(&right.condition));
        for result in draft
            .required_results
            .iter_mut()
            .chain(&mut draft.forbidden_results)
        {
            result.evidence.sort_by(|left, right| {
                (&left.entity, &left.detail).cmp(&(&right.entity, &right.detail))
            });
        }
        draft.impact.query.roots.sort();
        draft.impact.query.layers.sort();
        draft.impact.entities.sort();
        draft.expected_delta.changes.sort();
        draft.edits.sort_by(|left, right| {
            (
                left.target.file().path.as_path(),
                left.span.start_byte,
                left.span.end_byte,
            )
                .cmp(&(
                    right.target.file().path.as_path(),
                    right.span.start_byte,
                    right.span.end_byte,
                ))
        });
        draft
            .validation_plan
            .steps
            .sort_by(|left, right| left.key.cmp(&right.key));
        draft.rollback_plan.validation_steps.sort();

        let id = derive_candidate_id(&draft)?;
        let candidate = Self {
            schema: TRANSFORMATION_CANDIDATE_SCHEMA.into(),
            id,
            recipe: draft.recipe,
            source: draft.source,
            target: draft.target,
            eligibility: draft.eligibility,
            required_results: draft.required_results,
            forbidden_results: draft.forbidden_results,
            impact: draft.impact,
            expected_delta: draft.expected_delta,
            edits: draft.edits,
            safety: draft.safety,
            disposition: draft.disposition,
            validation_plan: draft.validation_plan,
            rollback_plan: draft.rollback_plan,
        };
        candidate.validate()?;
        Ok(candidate)
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn id(&self) -> &CandidateId {
        &self.id
    }

    pub fn recipe(&self) -> &TransformationRecipe {
        &self.recipe
    }

    pub fn source(&self) -> &CandidateSource {
        &self.source
    }

    pub fn target(&self) -> &CandidateTarget {
        &self.target
    }

    pub fn eligibility(&self) -> &GraphEligibilityDecision {
        &self.eligibility
    }

    pub fn required_results(&self) -> &[ConditionResult] {
        &self.required_results
    }

    pub fn forbidden_results(&self) -> &[ConditionResult] {
        &self.forbidden_results
    }

    pub fn impact(&self) -> &ImpactCone {
        &self.impact
    }

    pub fn expected_delta(&self) -> &ExpectedGraphDelta {
        &self.expected_delta
    }

    pub fn edits(&self) -> &[TransformationEdit] {
        &self.edits
    }

    pub fn safety(&self) -> SafetyClass {
        self.safety
    }

    pub fn disposition(&self) -> CandidateDisposition {
        self.disposition
    }

    pub fn validation_plan(&self) -> &ValidationPlan {
        &self.validation_plan
    }

    pub fn rollback_plan(&self) -> &RollbackPlan {
        &self.rollback_plan
    }

    fn validate(&self) -> Result<(), RecipeContractError> {
        if self.schema != TRANSFORMATION_CANDIDATE_SCHEMA {
            return Err(invalid("unsupported transformation-candidate schema"));
        }
        self.recipe.validate()?;
        validate_digest_id(&self.source.project_snapshot, "ps1_")?;
        validate_digest_id(&self.source.analysis, "pa1_")?;
        validate_digest_id(&self.source.program_dependence_projection, "pj1_")?;
        validate_target(&self.target)?;
        if self.eligibility.consumer() != self.recipe.id().as_str()
            || self.eligibility.required_layers() != self.recipe.required_layers()
        {
            return Err(invalid(
                "candidate eligibility does not bind its exact recipe requirements",
            ));
        }
        validate_results(
            "required results",
            &self.required_results,
            self.recipe.required_conditions(),
        )?;
        validate_results(
            "forbidden results",
            &self.forbidden_results,
            self.recipe.forbidden_conditions(),
        )?;
        validate_impact(&self.impact)?;
        if !self.impact.entities.contains(&self.target.entity) {
            return Err(invalid(
                "candidate target is outside its computed impact cone",
            ));
        }
        validate_delta(&self.expected_delta)?;
        validate_edits(&self.edits)?;
        if self.edits.is_empty() {
            return Err(invalid(
                "a transformation candidate requires at least one exact edit",
            ));
        }
        if !self
            .edits
            .iter()
            .any(|edit| edit.target == self.target.node)
        {
            return Err(invalid("candidate edits do not include the target node"));
        }
        if safety_rank(self.safety) < safety_rank(self.recipe.maximum_safety()) {
            return Err(invalid("candidate safety exceeds the recipe maximum"));
        }
        validate_validation_plan(&self.validation_plan)?;
        validate_rollback_plan(&self.rollback_plan, &self.validation_plan)?;
        if self.validation_plan != *self.recipe.validation_plan()
            || self.rollback_plan != *self.recipe.rollback_plan()
        {
            return Err(invalid(
                "candidate validation or rollback plan diverges from its recipe",
            ));
        }
        if self.disposition == CandidateDisposition::Automatic {
            if self.safety != SafetyClass::SafeAuto || !self.eligibility.eligible() {
                return Err(invalid(
                    "automatic candidate lacks SafeAuto graph eligibility",
                ));
            }
            if self
                .required_results
                .iter()
                .any(|result| result.state != ProofState::Proven)
                || self
                    .forbidden_results
                    .iter()
                    .any(|result| result.state != ProofState::Disproven)
            {
                return Err(invalid(
                    "automatic candidate has an unproven obligation or possible forbidden condition",
                ));
            }
        }

        let draft = self.as_draft();
        if self.id != derive_candidate_id(&draft)? {
            return Err(invalid("transformation-candidate identity is stale"));
        }
        Ok(())
    }

    fn as_draft(&self) -> TransformationCandidateDraft {
        TransformationCandidateDraft {
            recipe: self.recipe.clone(),
            source: self.source.clone(),
            target: self.target.clone(),
            eligibility: self.eligibility.clone(),
            required_results: self.required_results.clone(),
            forbidden_results: self.forbidden_results.clone(),
            impact: self.impact.clone(),
            expected_delta: self.expected_delta.clone(),
            edits: self.edits.clone(),
            safety: self.safety,
            disposition: self.disposition,
            validation_plan: self.validation_plan.clone(),
            rollback_plan: self.rollback_plan.clone(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransformationCandidateWire {
    schema: String,
    id: CandidateId,
    recipe: TransformationRecipe,
    source: CandidateSource,
    target: CandidateTarget,
    eligibility: GraphEligibilityDecision,
    required_results: Vec<ConditionResult>,
    forbidden_results: Vec<ConditionResult>,
    impact: ImpactCone,
    expected_delta: ExpectedGraphDelta,
    edits: Vec<TransformationEdit>,
    safety: SafetyClass,
    disposition: CandidateDisposition,
    validation_plan: ValidationPlan,
    rollback_plan: RollbackPlan,
}

impl<'de> Deserialize<'de> for TransformationCandidate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = TransformationCandidateWire::deserialize(deserializer)?;
        let candidate = Self {
            schema: wire.schema,
            id: wire.id,
            recipe: wire.recipe,
            source: wire.source,
            target: wire.target,
            eligibility: wire.eligibility,
            required_results: wire.required_results,
            forbidden_results: wire.forbidden_results,
            impact: wire.impact,
            expected_delta: wire.expected_delta,
            edits: wire.edits,
            safety: wire.safety,
            disposition: wire.disposition,
            validation_plan: wire.validation_plan,
            rollback_plan: wire.rollback_plan,
        };
        candidate.validate().map_err(serde::de::Error::custom)?;
        Ok(candidate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecipeContractError {
    #[error("invalid transformation contract: {0}")]
    Invalid(String),
    #[error("transformation identity failed: {0}")]
    Identity(String),
}

fn invalid(detail: impl Into<String>) -> RecipeContractError {
    RecipeContractError::Invalid(detail.into())
}

fn derive_recipe_id(draft: &TransformationRecipeDraft) -> Result<RecipeId, RecipeContractError> {
    let payload = serde_json::to_vec(&(
        TRANSFORMATION_RECIPE_SCHEMA,
        &draft.name,
        &draft.version,
        draft.family,
        &draft.required_layers,
        &draft.required_conditions,
        &draft.forbidden_conditions,
        draft.maximum_safety,
        &draft.validation_plan,
        &draft.rollback_plan,
        &draft.fixtures,
    ))
    .map_err(|error| RecipeContractError::Identity(error.to_string()))?;
    Ok(RecipeId(derive_id(RECIPE_ID_DOMAIN, "rcp1_", &payload)))
}

fn derive_candidate_id(
    draft: &TransformationCandidateDraft,
) -> Result<CandidateId, RecipeContractError> {
    let payload = serde_json::to_vec(&(
        TRANSFORMATION_CANDIDATE_SCHEMA,
        &draft.recipe,
        &draft.source,
        &draft.target,
        &draft.eligibility,
        &draft.required_results,
        &draft.forbidden_results,
        &draft.impact,
        &draft.expected_delta,
        &draft.edits,
        draft.safety,
        draft.disposition,
        &draft.validation_plan,
        &draft.rollback_plan,
    ))
    .map_err(|error| RecipeContractError::Identity(error.to_string()))?;
    Ok(CandidateId(derive_id(
        CANDIDATE_ID_DOMAIN,
        "tcn1_",
        &payload,
    )))
}

fn derive_id(domain: &str, prefix: &str, payload: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(domain.len() as u64).to_le_bytes());
    hasher.update(domain.as_bytes());
    hasher.update(&(payload.len() as u64).to_le_bytes());
    hasher.update(payload);
    format!("{prefix}{}", hasher.finalize().to_hex())
}

fn validate_digest_id(value: &str, prefix: &str) -> Result<(), RecipeContractError> {
    if !value.strip_prefix(prefix).is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(invalid(format!(
            "identity must be canonical lowercase {prefix}<64-hex>"
        )));
    }
    Ok(())
}

fn validate_text(label: &str, value: &str) -> Result<(), RecipeContractError> {
    if value.trim().is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{label} must be canonical nonempty control-free text"
        )));
    }
    Ok(())
}

fn validate_canonical_layers(layers: &[GraphEvidenceLayer]) -> Result<(), RecipeContractError> {
    if layers.is_empty() || layers.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid(
            "recipe graph layers must be canonical, nonempty, and distinct",
        ));
    }
    Ok(())
}

fn validate_conditions(
    label: &str,
    conditions: &[RecipeCondition],
) -> Result<(), RecipeContractError> {
    if conditions.windows(2).any(|pair| pair[0].key >= pair[1].key) {
        return Err(invalid(format!("{label} are not canonical and distinct")));
    }
    for condition in conditions {
        validate_text("condition key", &condition.key)?;
        validate_text("condition description", &condition.description)?;
    }
    Ok(())
}

fn validate_fixtures(fixtures: &[RecipeFixture]) -> Result<(), RecipeContractError> {
    let expected = [
        RecipeFixtureRole::Positive,
        RecipeFixtureRole::NoOp,
        RecipeFixtureRole::MinimalCounterexample,
        RecipeFixtureRole::AdversarialNearMiss,
    ];
    if fixtures.len() != expected.len() || fixtures.iter().map(|fixture| fixture.role).ne(expected)
    {
        return Err(invalid(
            "recipe fixtures must contain exactly the four canonical fixture roles",
        ));
    }
    for fixture in fixtures {
        validate_text("fixture name", &fixture.name)?;
        validate_text("fixture description", &fixture.description)?;
        match (fixture.role, fixture.expectation) {
            (RecipeFixtureRole::Positive, FixtureExpectation::Candidate)
            | (RecipeFixtureRole::NoOp, FixtureExpectation::NoCandidate)
            | (
                RecipeFixtureRole::MinimalCounterexample | RecipeFixtureRole::AdversarialNearMiss,
                FixtureExpectation::NoCandidate | FixtureExpectation::ReviewRequired,
            ) => {}
            _ => {
                return Err(invalid(
                    "fixture role and expected outcome contradict the convention",
                ));
            }
        }
    }
    Ok(())
}

fn validate_validation_plan(plan: &ValidationPlan) -> Result<(), RecipeContractError> {
    if plan.steps.is_empty() || plan.steps.windows(2).any(|pair| pair[0].key >= pair[1].key) {
        return Err(invalid(
            "validation steps must be canonical, nonempty, and distinct",
        ));
    }
    for step in &plan.steps {
        validate_text("validation-step key", &step.key)?;
        validate_text("validation-step description", &step.description)?;
        if let Some(command) = &step.command {
            validate_text("validation-step command", command)?;
        }
        if matches!(step.kind, ValidationStepKind::Custom) && step.command.is_none() {
            return Err(invalid("custom validation step requires an exact command"));
        }
    }
    if !plan
        .steps
        .iter()
        .any(|step| step.required && step.kind == ValidationStepKind::Parse)
        || !plan
            .steps
            .iter()
            .any(|step| step.required && step.kind == ValidationStepKind::GraphDelta)
    {
        return Err(invalid(
            "validation plan requires parse and graph-delta checks",
        ));
    }
    Ok(())
}

fn validate_rollback_plan(
    rollback: &RollbackPlan,
    validation: &ValidationPlan,
) -> Result<(), RecipeContractError> {
    if !rollback.require_revision_guards || rollback.validation_steps.is_empty() {
        return Err(invalid(
            "rollback requires exact revision guards and validation steps",
        ));
    }
    if rollback
        .validation_steps
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
    {
        return Err(invalid(
            "rollback validation-step keys must be canonical and distinct",
        ));
    }
    let keys = validation
        .steps
        .iter()
        .map(|step| step.key.as_str())
        .collect::<BTreeSet<_>>();
    if rollback.validation_steps.iter().any(|key| {
        validate_text("rollback validation-step key", key).is_err() || !keys.contains(key.as_str())
    }) {
        return Err(invalid(
            "rollback references a missing or invalid validation step",
        ));
    }
    Ok(())
}

fn validate_entity(entity: &GraphEntityRef) -> Result<(), RecipeContractError> {
    validate_text("graph entity graph", &entity.graph)?;
    validate_text("graph entity identity", &entity.entity)
}

fn validate_results(
    label: &str,
    results: &[ConditionResult],
    conditions: &[RecipeCondition],
) -> Result<(), RecipeContractError> {
    if results.len() != conditions.len()
        || results
            .iter()
            .zip(conditions)
            .any(|(result, condition)| result.condition != condition.key)
    {
        return Err(invalid(format!(
            "{label} do not close over the recipe conditions"
        )));
    }
    for (result, condition) in results.iter().zip(conditions) {
        if result
            .evidence
            .windows(2)
            .any(|pair| (&pair[0].entity, &pair[0].detail) >= (&pair[1].entity, &pair[1].detail))
        {
            return Err(invalid(format!(
                "{label} evidence is not canonical and distinct"
            )));
        }
        if result.state != ProofState::Unknown && result.evidence.is_empty() {
            return Err(invalid(format!(
                "{label} Proven/Disproven state requires exact evidence"
            )));
        }
        for evidence in &result.evidence {
            validate_entity(&evidence.entity)?;
            validate_text("condition evidence detail", &evidence.detail)?;
            if evidence.entity.layer != condition.layer {
                return Err(invalid(format!(
                    "{label} evidence names the wrong graph layer"
                )));
            }
            match (evidence.capability, evidence.support, evidence.authority) {
                (None, None, None) => {}
                (Some(_), Some(CapabilitySupport::Provided), Some(_)) => {}
                (
                    Some(_),
                    Some(CapabilitySupport::Unknown | CapabilitySupport::Unsupported),
                    None,
                ) => {}
                _ => {
                    return Err(invalid(
                        "condition evidence capability/support/authority disagree",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_target(target: &CandidateTarget) -> Result<(), RecipeContractError> {
    validate_entity(&target.entity)?;
    validate_span(target.span)?;
    let anchor = target.node.anchor();
    if target.span.start_byte != anchor.start_byte() as usize
        || target.span.end_byte != anchor.end_byte() as usize
        || target.span.start_line != anchor.start_row() as usize + 1
        || target.span.end_line != anchor.end_row() as usize + 1
    {
        return Err(invalid(
            "candidate target span does not match its exact retained node",
        ));
    }
    validate_repo_path(target.node.file().path.as_path())?;
    if target
        .subtree_fingerprint
        .as_ref()
        .is_some_and(|fingerprint| fingerprint.root() != &target.node)
    {
        return Err(invalid(
            "candidate subtree fingerprint does not belong to its exact target node",
        ));
    }
    Ok(())
}

fn validate_span(span: Span) -> Result<(), RecipeContractError> {
    if span.start_line == 0 || span.end_line < span.start_line || span.end_byte <= span.start_byte {
        return Err(invalid("candidate span is empty or reversed"));
    }
    Ok(())
}

fn validate_repo_path(path: &Path) -> Result<(), RecipeContractError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid(
            "candidate paths must be normalized, nonempty, and root-relative",
        ));
    }
    Ok(())
}

fn validate_impact(impact: &ImpactCone) -> Result<(), RecipeContractError> {
    if impact.query.maximum_depth == 0
        || impact.query.roots.is_empty()
        || impact.query.roots.windows(2).any(|pair| pair[0] >= pair[1])
        || impact.query.layers.is_empty()
        || impact
            .query
            .layers
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        || impact.entities.is_empty()
        || impact.entities.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(invalid(
            "impact cone query/result must be canonical, bounded, and nonempty",
        ));
    }
    for entity in impact.query.roots.iter().chain(&impact.entities) {
        validate_entity(entity)?;
    }
    if impact
        .query
        .roots
        .iter()
        .any(|root| !impact.entities.contains(root))
        || impact
            .entities
            .iter()
            .any(|entity| impact.query.layers.binary_search(&entity.layer).is_err())
    {
        return Err(invalid(
            "impact cone omits a root or contains an unrequested layer",
        ));
    }
    Ok(())
}

fn validate_delta(delta: &ExpectedGraphDelta) -> Result<(), RecipeContractError> {
    if delta.changes.is_empty() || delta.changes.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid(
            "expected graph changes must be canonical, nonempty, and distinct",
        ));
    }
    for change in &delta.changes {
        validate_entity(&change.entity)?;
        validate_text("expected graph-change rationale", &change.rationale)?;
    }
    if delta
        .changes
        .windows(2)
        .any(|pair| pair[0].kind == pair[1].kind && pair[0].entity == pair[1].entity)
    {
        return Err(invalid(
            "expected graph changes duplicate a kind/entity with different rationale",
        ));
    }
    Ok(())
}

fn validate_edits(edits: &[TransformationEdit]) -> Result<(), RecipeContractError> {
    for edit in edits {
        validate_span(edit.span)?;
        validate_repo_path(edit.target.file().path.as_path())?;
        let anchor = edit.target.anchor();
        if edit.span.start_byte < anchor.start_byte() as usize
            || edit.span.end_byte > anchor.end_byte() as usize
        {
            return Err(invalid("candidate edit escapes its exact target node"));
        }
        let expected = revision_guard(edit.target.file().path.as_path(), edit.span, &edit.before);
        if edit.revision_guard != expected {
            return Err(invalid("candidate edit revision guard is stale"));
        }
        if edit.before == edit.after {
            return Err(invalid("candidate edit does not change source bytes"));
        }
    }
    for pair in edits.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        let left_key = (
            left.target.file().path.as_path(),
            left.span.start_byte,
            left.span.end_byte,
        );
        let right_key = (
            right.target.file().path.as_path(),
            right.span.start_byte,
            right.span.end_byte,
        );
        if left_key >= right_key
            || (left.target.file().path == right.target.file().path
                && left.span.end_byte > right.span.start_byte)
        {
            return Err(invalid(
                "candidate edits are noncanonical, duplicate, or overlapping",
            ));
        }
    }
    Ok(())
}

fn safety_rank(safety: SafetyClass) -> u8 {
    match safety {
        SafetyClass::SafeAuto => 0,
        SafetyClass::AnalyzerConfirmed => 1,
        SafetyClass::SafeWithPrecondition => 2,
        SafetyClass::RiskySuggest => 3,
        SafetyClass::LlmOnly => 4,
        SafetyClass::NeverAuto => 5,
    }
}

#[cfg(test)]
mod tests {
    use deslop_core::SafetyClass;
    use deslop_parse::GraphEvidenceLayer;
    use serde_json::{Value, json};

    use super::{
        FixtureExpectation, RecipeCondition, RecipeFixture, RecipeFixtureRole, RollbackPlan,
        RollbackStrategy, TransformationFamily, TransformationRecipe, TransformationRecipeDraft,
        ValidationPlan, ValidationStep, ValidationStepKind,
    };

    fn recipe_draft() -> TransformationRecipeDraft {
        TransformationRecipeDraft {
            name: "remove-unreachable-statement".into(),
            version: "1.0.0".into(),
            family: TransformationFamily::CloneCeremonyDeadCode,
            required_layers: vec![GraphEvidenceLayer::ControlFlow],
            required_conditions: vec![RecipeCondition {
                key: "entry-unreachable".into(),
                description: "The retained control point is unreachable from callable entry."
                    .into(),
                layer: GraphEvidenceLayer::ControlFlow,
            }],
            forbidden_conditions: vec![RecipeCondition {
                key: "conservative-control".into(),
                description: "The retained path contains conservative control evidence.".into(),
                layer: GraphEvidenceLayer::ControlFlow,
            }],
            maximum_safety: SafetyClass::SafeWithPrecondition,
            validation_plan: ValidationPlan {
                steps: vec![
                    ValidationStep {
                        key: "parse".into(),
                        kind: ValidationStepKind::Parse,
                        description: "Parse the exact edited source.".into(),
                        command: None,
                        required: true,
                    },
                    ValidationStep {
                        key: "graph-delta".into(),
                        kind: ValidationStepKind::GraphDelta,
                        description: "Compare the actual and expected graph delta.".into(),
                        command: None,
                        required: true,
                    },
                ],
            },
            rollback_plan: RollbackPlan {
                strategy: RollbackStrategy::ReverseExactEdits,
                require_revision_guards: true,
                validation_steps: vec!["parse".into(), "graph-delta".into()],
            },
            fixtures: vec![
                RecipeFixture {
                    role: RecipeFixtureRole::AdversarialNearMiss,
                    name: "reachable-after-conditional".into(),
                    expectation: FixtureExpectation::NoCandidate,
                    description: "A syntactically later statement remains reachable.".into(),
                },
                RecipeFixture {
                    role: RecipeFixtureRole::Positive,
                    name: "literal-after-return".into(),
                    expectation: FixtureExpectation::Candidate,
                    description: "An exact unreachable expression statement is removable.".into(),
                },
                RecipeFixture {
                    role: RecipeFixtureRole::MinimalCounterexample,
                    name: "referenced-declaration".into(),
                    expectation: FixtureExpectation::ReviewRequired,
                    description:
                        "An unreachable declaration still participates in name resolution.".into(),
                },
                RecipeFixture {
                    role: RecipeFixtureRole::NoOp,
                    name: "all-reachable".into(),
                    expectation: FixtureExpectation::NoCandidate,
                    description: "Every retained statement is entry-reachable.".into(),
                },
            ],
        }
    }

    fn recipe() -> TransformationRecipe {
        TransformationRecipe::new(recipe_draft()).unwrap()
    }

    #[test]
    fn recipe_wire_is_canonical_content_bound_and_round_trips() {
        let recipe = recipe();
        let bytes = serde_json::to_vec(&recipe).unwrap();
        let decoded: TransformationRecipe = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(decoded, recipe);
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
        assert_eq!(recipe.fixtures()[0].role, RecipeFixtureRole::Positive);
        assert_eq!(
            recipe.fixtures()[3].role,
            RecipeFixtureRole::AdversarialNearMiss
        );

        let mut changed = recipe_draft();
        changed.version = "1.0.1".into();
        assert_ne!(
            TransformationRecipe::new(changed).unwrap().id(),
            recipe.id()
        );
    }

    #[test]
    fn recipe_wire_rejects_stale_identity_unknown_fields_and_noncanonical_order() {
        let mut stale = serde_json::to_value(recipe()).unwrap();
        stale["version"] = json!("1.0.1");
        assert!(serde_json::from_value::<TransformationRecipe>(stale).is_err());

        let mut unknown = serde_json::to_value(recipe()).unwrap();
        unknown["unexpected"] = Value::Bool(true);
        assert!(serde_json::from_value::<TransformationRecipe>(unknown).is_err());

        let mut reordered = serde_json::to_value(recipe()).unwrap();
        reordered["fixtures"].as_array_mut().unwrap().swap(0, 1);
        assert!(serde_json::from_value::<TransformationRecipe>(reordered).is_err());
    }

    #[test]
    fn recipe_requires_dependency_closed_layers_and_exact_fixture_roles() {
        let mut unclosed = recipe_draft();
        unclosed.required_layers = vec![GraphEvidenceLayer::ProgramDependence];
        unclosed.required_conditions[0].layer = GraphEvidenceLayer::ProgramDependence;
        unclosed.forbidden_conditions[0].layer = GraphEvidenceLayer::ProgramDependence;
        assert!(TransformationRecipe::new(unclosed).is_err());

        let mut missing_role = recipe_draft();
        missing_role.fixtures.pop();
        assert!(TransformationRecipe::new(missing_role).is_err());

        let mut contradictory = recipe_draft();
        contradictory.fixtures[1].expectation = FixtureExpectation::NoCandidate;
        assert!(TransformationRecipe::new(contradictory).is_err());
    }
}
