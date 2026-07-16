use std::collections::BTreeSet;

use deslop_core::SafetyClass;

use crate::{
    CandidateDisposition, RecipeContractError, RollbackStrategy, TransformationCandidate,
    TransformationRecipe, adjacent_condition_merge_recipe, equivalent_branch_factoring_recipe,
    exhaustive_chain_to_match_recipe, extract_method_recipe, guard_clause_inversion_recipe,
    hoisted_private_function_order_recipe, independent_branch_split_recipe,
    inline_single_use_conversion_allocation_recipe, inline_single_use_helper_recipe,
    inline_single_use_temporary_recipe, literal_dead_arm_recipe,
    remove_independent_dead_local_recipe, remove_unused_pure_expression_recipe,
    responsibility_split_recipe, simple_import_order_recipe, unreachable_literal_statement_recipe,
};

const SAFE_AUTO_RECIPES: [&str; 3] = [
    "rust-remove-independent-unused-literal-local",
    "rust-remove-unreachable-literal-statement",
    "rust-remove-unused-pure-literal-expression",
];

/// Canonical catalog of every detector enabled by the production Rust projection.
pub fn enabled_rust_recipe_catalog() -> Result<Vec<TransformationRecipe>, RecipeContractError> {
    let mut recipes = vec![
        unreachable_literal_statement_recipe()?,
        equivalent_branch_factoring_recipe()?,
        adjacent_condition_merge_recipe()?,
        independent_branch_split_recipe()?,
        guard_clause_inversion_recipe()?,
        literal_dead_arm_recipe()?,
        exhaustive_chain_to_match_recipe()?,
        extract_method_recipe()?,
        responsibility_split_recipe()?,
        inline_single_use_temporary_recipe()?,
        inline_single_use_conversion_allocation_recipe()?,
        remove_unused_pure_expression_recipe()?,
        remove_independent_dead_local_recipe()?,
        simple_import_order_recipe()?,
        hoisted_private_function_order_recipe()?,
        inline_single_use_helper_recipe()?,
    ];
    recipes.sort_by(|left, right| left.name().cmp(right.name()));
    Ok(recipes)
}

/// Enforce the M5 terminal chain for one candidate emitted by an enabled detector.
///
/// Candidate construction already validates content addressing and graph obligations. This audit
/// joins that candidate to the exact enabled catalog and makes patch/delta/verification/rollback
/// coverage, plus the SafeAuto frontier, a production detection invariant.
pub fn audit_m5_candidate(candidate: &TransformationCandidate) -> Result<(), String> {
    let catalog = enabled_rust_recipe_catalog().map_err(|error| error.to_string())?;
    let Some(recipe) = catalog
        .iter()
        .find(|recipe| recipe.name() == candidate.recipe().name())
    else {
        return Err(format!(
            "candidate {} belongs to a detector outside the enabled M5 catalog",
            candidate.id()
        ));
    };
    if recipe.id() != candidate.recipe().id() {
        return Err("candidate recipe identity differs from the enabled catalog".into());
    }
    if candidate.edits().is_empty()
        || candidate
            .edits()
            .iter()
            .any(|edit| edit.revision_guard.as_str().is_empty())
    {
        return Err("enabled candidate lacks an exact revision-guarded patch".into());
    }
    if candidate.expected_delta().changes.is_empty() {
        return Err("enabled candidate lacks an expected graph delta".into());
    }

    let required_validation = candidate
        .validation_plan()
        .steps
        .iter()
        .filter(|step| step.required)
        .map(|step| step.key.as_str())
        .collect::<BTreeSet<_>>();
    if required_validation.is_empty() {
        return Err("enabled candidate lacks required verification".into());
    }
    let rollback = candidate.rollback_plan();
    if rollback.strategy != RollbackStrategy::ReverseExactEdits
        || !rollback.require_revision_guards
        || !required_validation.iter().all(|key| {
            rollback
                .validation_steps
                .iter()
                .any(|rollback_key| rollback_key == key)
        })
    {
        return Err("enabled candidate rollback does not cover every required verification".into());
    }

    if candidate.disposition() == CandidateDisposition::Automatic
        && (candidate.safety() != SafetyClass::SafeAuto
            || !SAFE_AUTO_RECIPES.contains(&candidate.recipe().name()))
    {
        return Err("automatic candidate is outside the audited SafeAuto frontier".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FixtureExpectation, RecipeFixtureRole};

    #[test]
    fn enabled_catalog_closes_recipe_fixture_validation_and_rollback_contracts() {
        let catalog = enabled_rust_recipe_catalog().unwrap();
        assert_eq!(catalog.len(), 16);
        assert_eq!(
            catalog
                .iter()
                .map(|recipe| recipe.name())
                .collect::<BTreeSet<_>>()
                .len(),
            catalog.len()
        );
        assert_eq!(
            catalog
                .iter()
                .map(|recipe| recipe.id())
                .collect::<BTreeSet<_>>()
                .len(),
            catalog.len()
        );

        for recipe in &catalog {
            assert!(!recipe.required_layers().is_empty(), "{}", recipe.name());
            assert!(
                !recipe.required_conditions().is_empty(),
                "{}",
                recipe.name()
            );
            assert_eq!(
                recipe
                    .fixtures()
                    .iter()
                    .map(|fixture| fixture.role)
                    .collect::<Vec<_>>(),
                vec![
                    RecipeFixtureRole::Positive,
                    RecipeFixtureRole::NoOp,
                    RecipeFixtureRole::MinimalCounterexample,
                    RecipeFixtureRole::AdversarialNearMiss,
                ],
                "{}",
                recipe.name()
            );
            assert!(matches!(
                recipe.fixtures()[0].expectation,
                FixtureExpectation::Candidate | FixtureExpectation::ReviewRequired
            ));
            let required = recipe
                .validation_plan()
                .steps
                .iter()
                .filter(|step| step.required)
                .map(|step| step.key.as_str())
                .collect::<BTreeSet<_>>();
            assert!(!required.is_empty(), "{}", recipe.name());
            assert_eq!(
                recipe.rollback_plan().strategy,
                RollbackStrategy::ReverseExactEdits
            );
            assert!(recipe.rollback_plan().require_revision_guards);
            assert!(required.iter().all(|key| {
                recipe
                    .rollback_plan()
                    .validation_steps
                    .iter()
                    .any(|rollback_key| rollback_key == key)
            }));
        }
    }

    #[test]
    fn safe_auto_frontier_is_exact_and_counterexample_fixtures_are_mandatory() {
        let catalog = enabled_rust_recipe_catalog().unwrap();
        let safe_auto = catalog
            .iter()
            .filter(|recipe| recipe.maximum_safety() == SafetyClass::SafeAuto)
            .map(|recipe| recipe.name())
            .collect::<Vec<_>>();
        assert_eq!(safe_auto, SAFE_AUTO_RECIPES);
        for recipe in catalog
            .iter()
            .filter(|recipe| SAFE_AUTO_RECIPES.contains(&recipe.name()))
        {
            assert_eq!(
                recipe.fixtures()[2].expectation,
                FixtureExpectation::NoCandidate
            );
            assert_eq!(
                recipe.fixtures()[3].expectation,
                FixtureExpectation::NoCandidate
            );
        }
    }
}
