mod branch;
mod branch_split;
mod branch_terminal;
mod clarity;
mod condition_merge;
mod contract;
mod definition_of_done;
mod evaluation;
mod extract_method;
mod guard_clause;
mod impact;
mod inline_helper;
mod local_cleanup;
mod ordering;
mod project;
mod semantic_evidence;
mod unreachable;

pub use branch::{
    BranchGraphEvidence, BranchRecipeError, branch_graph_evidence,
    detect_equivalent_branch_fragments, equivalent_branch_factoring_recipe,
};
pub use branch_split::{
    ActionDependenceSlice, BranchSplitDependenceEvidence, BranchSplitRecipeError,
    detect_independent_branch_splits, independent_branch_split_recipe,
};
pub use branch_terminal::{
    DeadArmGraphEvidence, ExhaustiveChainGraphEvidence, TerminalBranchRecipeError,
    detect_exhaustive_chain_matches, detect_literal_dead_arms, exhaustive_chain_to_match_recipe,
    literal_dead_arm_recipe,
};
pub use clarity::{
    ClarityCandidateKind, GRAPH_GROUNDED_CLARITY_CANDIDATE_SCHEMA, GraphGroundedClarityCandidate,
    graph_grounded_clarity_candidates,
};
pub use condition_merge::{
    ConditionMergeRecipeError, adjacent_condition_merge_recipe, detect_adjacent_condition_merges,
};
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
pub use definition_of_done::{audit_m5_candidate, enabled_rust_recipe_catalog};
pub use evaluation::{
    B7Thresholds, CorpusLabel, EvaluationInterval, EvaluationObservation, EvaluationResourceBudget,
    FrozenRecipeCase, RECIPE_EVALUATION_CORPUS_SCHEMA, RECIPE_EVALUATION_REPORT_SCHEMA,
    RecipeEvaluationCorpusManifest, RecipeEvaluationError, RecipeEvaluationReport,
    RecipeEvaluationThresholdResults, RecipeEvaluationTotals, evaluate_recipe_observations,
    frozen_unreachable_rust_cases, frozen_unreachable_rust_manifest,
};
pub use extract_method::{
    ExtractMethodRecipeError, ExtractionInputEvidence, ExtractionInputOrigin,
    ExtractionOutputEvidence, ExtractionOwnershipMode, ExtractionSignatureEvidence,
    ExtractionSliceEvidence, ResponsibilityClusterEvidence, ResponsibilitySplitEvidence,
    detect_extract_method_candidates, detect_responsibility_split_candidates,
    extract_method_recipe, responsibility_split_recipe,
};
pub use guard_clause::{
    GuardClauseExitEvidence, GuardClauseRecipeError, detect_guard_clause_inversions,
    guard_clause_inversion_recipe,
};
pub use impact::{ImpactQueryError, program_dependence_impact_cone};
pub use inline_helper::{
    InlineHelperEvidence, InlineHelperRecipeError, detect_inline_single_use_helpers,
    inline_single_use_helper_recipe,
};
pub use local_cleanup::{
    LocalCleanupRecipeError, detect_local_cleanup_candidates,
    inline_single_use_conversion_allocation_recipe, inline_single_use_temporary_recipe,
    remove_independent_dead_local_recipe, remove_unused_pure_expression_recipe,
};
pub use ordering::{
    OrderingRecipeError, detect_ordering_candidates, hoisted_private_function_order_recipe,
    simple_import_order_recipe,
};
pub use project::{
    RECIPE_DETECTION_REPORT_SCHEMA, RecipeAbstention, RecipeDetectionReport,
    build_rust_recipe_projection, detect_rust_recipe_report, detect_rust_recipes,
};
pub use semantic_evidence::{
    CommentIntent, CommentSemanticEvidence, IdentifierSemanticEvidence, IdentifierSemanticRole,
    ROLE_SCOPE_COMMENT_EVIDENCE_SCHEMA, RoleScopeCommentEvidence, RoleScopeCommentEvidenceId,
    RoleScopeCommentEvidenceInput, SemanticEvidenceDisposition, role_scope_comment_evidence,
};
pub use unreachable::{
    UnreachableRecipeError, detect_unreachable_literal_statements,
    unreachable_literal_statement_recipe,
};
