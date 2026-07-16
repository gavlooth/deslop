use deslop_parse::GraphEvidenceLayer;
use serde::{Deserialize, Serialize};

use crate::{CandidateId, TransformationCandidate, TransformationRecipe};

pub const GRAPH_GROUNDED_CLARITY_CANDIDATE_SCHEMA: &str =
    "deslop.graph-grounded-clarity-candidate/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClarityCandidateKind {
    Forwarding,
    ConversionAllocation,
    Wrapper,
    RepeatedError,
    DeadCode,
}

/// Typed family evidence for an already guarded graph-grounded transformation candidate.
///
/// This record adds no edit or write authority. The referenced transformation candidate retains
/// its exact recipe contract, revision guards, validation plan, and rollback plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphGroundedClarityCandidate {
    schema: String,
    candidate: CandidateId,
    kinds: Vec<ClarityCandidateKind>,
    graph_layers: Vec<GraphEvidenceLayer>,
}

impl GraphGroundedClarityCandidate {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn candidate(&self) -> &CandidateId {
        &self.candidate
    }

    pub fn kinds(&self) -> &[ClarityCandidateKind] {
        &self.kinds
    }

    pub fn graph_layers(&self) -> &[GraphEvidenceLayer] {
        &self.graph_layers
    }
}

pub fn graph_grounded_clarity_candidates(
    candidates: &[TransformationCandidate],
) -> Result<Vec<GraphGroundedClarityCandidate>, String> {
    let mut output = Vec::new();
    for candidate in candidates {
        let kinds = clarity_kinds(
            candidate.recipe(),
            candidate.edits().iter().map(|edit| edit.before.as_str()),
        );
        if kinds.is_empty() {
            continue;
        }
        let layers = candidate.recipe().required_layers();
        if !layers.contains(&GraphEvidenceLayer::ProgramDependence)
            || !layers.contains(&GraphEvidenceLayer::DataFlow)
        {
            return Err(format!(
                "clarity candidate {} lacks required graph grounding",
                candidate.id()
            ));
        }
        output.push(GraphGroundedClarityCandidate {
            schema: GRAPH_GROUNDED_CLARITY_CANDIDATE_SCHEMA.into(),
            candidate: candidate.id().clone(),
            kinds,
            graph_layers: layers.to_vec(),
        });
    }
    output.sort_by(|left, right| left.candidate.cmp(&right.candidate));
    Ok(output)
}

fn clarity_kinds<'a>(
    recipe: &TransformationRecipe,
    edit_before: impl Iterator<Item = &'a str>,
) -> Vec<ClarityCandidateKind> {
    let mut kinds = match recipe.name() {
        "rust-inline-exact-single-use-helper" => vec![
            ClarityCandidateKind::Forwarding,
            ClarityCandidateKind::Wrapper,
        ],
        "rust-inline-single-use-conversion-allocation" => {
            vec![ClarityCandidateKind::ConversionAllocation]
        }
        "rust-remove-unreachable-literal-statement"
        | "rust-remove-literal-dead-arm"
        | "rust-remove-unused-pure-literal-expression"
        | "rust-remove-independent-unused-literal-local" => {
            vec![ClarityCandidateKind::DeadCode]
        }
        "rust-factor-equivalent-branch-fragments"
            if edit_before.into_iter().any(is_error_fragment) =>
        {
            vec![ClarityCandidateKind::RepeatedError]
        }
        _ => Vec::new(),
    };
    kinds.sort();
    kinds
}

fn is_error_fragment(text: &str) -> bool {
    text.contains("Err(")
        || text.contains("return Err")
        || text.contains("panic!")
        || text.contains("unreachable!")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        equivalent_branch_factoring_recipe, inline_single_use_conversion_allocation_recipe,
        inline_single_use_helper_recipe, unreachable_literal_statement_recipe,
    };

    #[test]
    fn catalog_covers_all_m5_25_graph_grounded_families() {
        assert_eq!(
            clarity_kinds(
                &inline_single_use_helper_recipe().unwrap(),
                std::iter::empty()
            ),
            [
                ClarityCandidateKind::Forwarding,
                ClarityCandidateKind::Wrapper
            ]
        );
        assert_eq!(
            clarity_kinds(
                &inline_single_use_conversion_allocation_recipe().unwrap(),
                std::iter::empty(),
            ),
            [ClarityCandidateKind::ConversionAllocation]
        );
        assert_eq!(
            clarity_kinds(
                &equivalent_branch_factoring_recipe().unwrap(),
                ["if failed { return Err(problem); } else { return Err(problem); }"].into_iter(),
            ),
            [ClarityCandidateKind::RepeatedError]
        );
        assert_eq!(
            clarity_kinds(
                &unreachable_literal_statement_recipe().unwrap(),
                std::iter::empty(),
            ),
            [ClarityCandidateKind::DeadCode]
        );
    }

    #[test]
    fn branch_factoring_without_error_semantics_is_not_relabelled() {
        assert!(
            clarity_kinds(
                &equivalent_branch_factoring_recipe().unwrap(),
                ["if flag { log(); } else { log(); }"].into_iter(),
            )
            .is_empty()
        );
    }
}
