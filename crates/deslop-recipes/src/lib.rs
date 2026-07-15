mod contract;
mod evaluation;
mod impact;
mod unreachable;

pub use contract::{
    CandidateDisposition, CandidateId, CandidateSource, CandidateTarget, ConditionEvidence,
    ConditionResult, ExpectedGraphChange, ExpectedGraphDelta, FixtureExpectation, GraphChangeKind,
    GraphEntityRef, ImpactCone, ImpactConeQuery, ImpactDirection, ProofState, RecipeCondition,
    RecipeContractError, RecipeFixture, RecipeFixtureRole, RecipeId, RollbackPlan,
    RollbackStrategy, TRANSFORMATION_CANDIDATE_SCHEMA, TRANSFORMATION_RECIPE_SCHEMA,
    TransformationCandidate, TransformationCandidateDraft, TransformationEdit,
    TransformationFamily, TransformationRecipe, TransformationRecipeDraft, ValidationPlan,
    ValidationStep, ValidationStepKind,
};
pub use evaluation::{
    B7Thresholds, CorpusLabel, EvaluationInterval, EvaluationObservation, EvaluationResourceBudget,
    FrozenRecipeCase, RECIPE_EVALUATION_CORPUS_SCHEMA, RECIPE_EVALUATION_REPORT_SCHEMA,
    RecipeEvaluationCorpusManifest, RecipeEvaluationError, RecipeEvaluationReport,
    RecipeEvaluationThresholdResults, RecipeEvaluationTotals, evaluate_recipe_observations,
    frozen_unreachable_rust_cases, frozen_unreachable_rust_manifest,
};
pub use impact::{ImpactQueryError, program_dependence_impact_cone};
pub use unreachable::{
    UnreachableRecipeError, detect_unreachable_literal_statements,
    unreachable_literal_statement_recipe,
};
