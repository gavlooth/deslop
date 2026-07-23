//! Shared sibling admission-gate pairing for snapshot and history analysis.
//!
//! The detector deliberately compares only bounded syntactic contexts:
//! fail-loud function groups, unique low-popularity local callees up to two
//! hops, and identifier-shaped domain fields. It never claims semantic
//! predicate equivalence.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use deslop_parse::{ContractFunction, RevisionContracts};

/// A zero-observation/NaN carve-out is specific enough to compare with two
/// shared fields (the count and the mean).
const MIN_ZERO_CARVE_OUT_OVERLAP: usize = 2;
/// General predicate-set divergence needs a substantially larger shared
/// contract surface.
const MIN_PREDICATE_OVERLAP: usize = 8;
/// Callees referenced by more groups are shared utilities, not part of one
/// gate's owned predicate context.
const MAX_CALLEE_GROUPS: usize = 3;
/// Fields appearing in too many gate contexts are generic infrastructure
/// dimensions and cannot establish sibling ownership by themselves.
const MAX_FIELD_CONTEXTS: usize = 12;
/// Bound transitive syntax expansion; no whole-project closure per pair.
const MAX_CLOSURE_DEPTH: usize = 2;

#[derive(Clone, Copy)]
pub(crate) struct GateAnchor<'a> {
    pub path: &'a Path,
    pub function: &'a ContractFunction,
}

pub(crate) struct SiblingGateAsymmetry<'a> {
    pub left: GateAnchor<'a>,
    pub right: GateAnchor<'a>,
    pub shared_identifiers: BTreeSet<String>,
    pub left_features: BTreeSet<String>,
    pub right_features: BTreeSet<String>,
    pub left_zero_nan_admission: bool,
    pub is_zero_nan_asymmetry: bool,
}

impl SiblingGateAsymmetry<'_> {
    pub fn pair_key(&self) -> (PathBuf, String, PathBuf, String) {
        (
            self.left.path.to_path_buf(),
            self.left.function.name.clone(),
            self.right.path.to_path_buf(),
            self.right.function.name.clone(),
        )
    }

    pub fn detail(&self) -> String {
        if self.is_zero_nan_asymmetry {
            let admitted = if self.left_zero_nan_admission {
                self.left.function.name.as_str()
            } else {
                self.right.function.name.as_str()
            };
            let strict = if self.left_zero_nan_admission {
                self.right.function.name.as_str()
            } else {
                self.left.function.name.as_str()
            };
            return format!(
                "`{admitted}` contains a zero-observation NaN admission while sibling \
                 `{strict}` does not"
            );
        }
        let left_only = self
            .left_features
            .difference(&self.right_features)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let right_only = self
            .right_features
            .difference(&self.left_features)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "sibling predicate features diverge ({} only: [{}]; {} only: [{}])",
            self.left.function.name, left_only, self.right.function.name, right_only
        )
    }
}

struct FunctionGroup<'a> {
    path: &'a Path,
    name: &'a str,
    functions: Vec<&'a ContractFunction>,
}

struct GateContext<'a> {
    anchor: GateAnchor<'a>,
    identifiers: BTreeSet<String>,
    direct_predicate_identifiers: BTreeSet<String>,
    direct_features: BTreeSet<String>,
    zero_nan_identifiers: BTreeSet<String>,
}

fn token_leaf(token: &str) -> &str {
    token.rsplit('.').next().unwrap_or(token)
}

fn is_type_like(token: &str) -> bool {
    token_leaf(token)
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_uppercase())
}

fn gate_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "admit",
        "check",
        "checkpoint",
        "guard",
        "launch",
        "preflight",
        "release",
        "require",
        "resume",
        "save",
        "validate",
        "verify",
    ]
    .iter()
    .any(|part| lower.contains(part))
}

fn zero_admission_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["checkpoint", "metadata", "resume", "undefined"]
        .iter()
        .any(|part| lower.contains(part))
}

fn function_groups(revision: &RevisionContracts) -> Vec<FunctionGroup<'_>> {
    let mut grouped: BTreeMap<(&Path, &str), Vec<&ContractFunction>> = BTreeMap::new();
    for file in &revision.files {
        for function in &file.functions {
            grouped
                .entry((file.path.as_path(), function.name.as_str()))
                .or_default()
                .push(function);
        }
    }
    grouped
        .into_iter()
        .map(|((path, name), functions)| FunctionGroup {
            path,
            name,
            functions,
        })
        .collect()
}

fn gate_contexts(revision: &RevisionContracts) -> Vec<GateContext<'_>> {
    let groups = function_groups(revision);
    let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    let mut reference_popularity: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, group) in groups.iter().enumerate() {
        by_name.entry(group.name).or_default().push(index);
        let leaves: BTreeSet<&str> = group
            .functions
            .iter()
            .flat_map(|function| function.references.iter().map(|token| token_leaf(token)))
            .collect();
        for leaf in leaves {
            *reference_popularity.entry(leaf).or_default() += 1;
        }
    }

    let mut contexts = Vec::new();
    for (root_index, root) in groups.iter().enumerate() {
        if !gate_name(root.name)
            || !root
                .functions
                .iter()
                .any(|function| function.admission_guard.fail_loud)
        {
            continue;
        }
        let mut visited = BTreeSet::new();
        let mut frontier = vec![(root_index, 0_usize)];
        while let Some((index, depth)) = frontier.pop() {
            if !visited.insert(index) || depth >= MAX_CLOSURE_DEPTH {
                continue;
            }
            for token in groups[index]
                .functions
                .iter()
                .flat_map(|function| &function.references)
            {
                let leaf = token_leaf(token);
                if is_type_like(leaf)
                    || reference_popularity.get(leaf).copied().unwrap_or(0) > MAX_CALLEE_GROUPS
                {
                    continue;
                }
                let Some(candidates) = by_name.get(leaf) else {
                    continue;
                };
                if candidates.len() == 1 {
                    frontier.push((candidates[0], depth + 1));
                }
            }
        }

        let direct_features: BTreeSet<String> = root
            .functions
            .iter()
            .flat_map(|function| function.admission_guard.predicate_features.iter().cloned())
            .collect();
        let direct_predicate_identifiers: BTreeSet<String> = root
            .functions
            .iter()
            .flat_map(|function| {
                function
                    .admission_guard
                    .predicate_identifiers
                    .iter()
                    .cloned()
            })
            .collect();
        let mut identifiers = BTreeSet::new();
        let mut zero_nan_identifiers = BTreeSet::new();
        for index in visited {
            for function in &groups[index].functions {
                identifiers.extend(function.admission_guard.domain_identifiers.iter().cloned());
                zero_nan_identifiers.extend(
                    function
                        .admission_guard
                        .zero_nan_identifiers
                        .iter()
                        .cloned(),
                );
            }
        }
        if identifiers.len() < MIN_ZERO_CARVE_OUT_OVERLAP || direct_features.is_empty() {
            continue;
        }
        let anchor = root
            .functions
            .iter()
            .copied()
            .max_by_key(|function| {
                (
                    function.admission_guard.domain_identifiers.len(),
                    function.span.end_byte - function.span.start_byte,
                )
            })
            .expect("function group is nonempty");
        contexts.push(GateContext {
            anchor: GateAnchor {
                path: root.path,
                function: anchor,
            },
            identifiers,
            direct_predicate_identifiers,
            direct_features,
            zero_nan_identifiers,
        });
    }

    let mut field_popularity: BTreeMap<String, usize> = BTreeMap::new();
    for context in &contexts {
        for identifier in &context.identifiers {
            *field_popularity.entry(identifier.clone()).or_default() += 1;
        }
    }
    for context in &mut contexts {
        context.identifiers.retain(|identifier| {
            field_popularity
                .get(identifier.as_str())
                .copied()
                .unwrap_or(0)
                <= MAX_FIELD_CONTEXTS
        });
        context.direct_predicate_identifiers.retain(|identifier| {
            field_popularity
                .get(identifier.as_str())
                .copied()
                .unwrap_or(0)
                <= MAX_FIELD_CONTEXTS
        });
        context.zero_nan_identifiers.retain(|identifier| {
            field_popularity
                .get(identifier.as_str())
                .copied()
                .unwrap_or(0)
                <= MAX_FIELD_CONTEXTS
        });
    }
    contexts
}

pub(crate) fn sibling_gate_asymmetries(
    revision: &RevisionContracts,
) -> Vec<SiblingGateAsymmetry<'_>> {
    let contexts = gate_contexts(revision);
    let mut findings = Vec::new();
    for left_index in 0..contexts.len() {
        for right_index in left_index + 1..contexts.len() {
            let left = &contexts[left_index];
            let right = &contexts[right_index];
            let direct_shared: BTreeSet<String> = left
                .direct_predicate_identifiers
                .intersection(&right.direct_predicate_identifiers)
                .cloned()
                .collect();
            let left_zero = !left.zero_nan_identifiers.is_empty();
            let right_zero = !right.zero_nan_identifiers.is_empty();
            let zero_shared: BTreeSet<String> = if left_zero && !right_zero {
                left.zero_nan_identifiers
                    .intersection(&right.direct_predicate_identifiers)
                    .cloned()
                    .collect()
            } else if right_zero && !left_zero {
                right
                    .zero_nan_identifiers
                    .intersection(&left.direct_predicate_identifiers)
                    .cloned()
                    .collect()
            } else {
                BTreeSet::new()
            };
            let has_count = zero_shared.iter().any(|identifier| {
                let lower = identifier.to_ascii_lowercase();
                lower.contains("count") || lower.contains("observation")
            });
            let has_value = zero_shared.iter().any(|identifier| {
                let lower = identifier.to_ascii_lowercase();
                ["mean", "rate", "effect", "lambda", "entropy"]
                    .iter()
                    .any(|part| lower.contains(part))
            });
            let zero_side_name = if left_zero {
                left.anchor.function.name.as_str()
            } else {
                right.anchor.function.name.as_str()
            };
            let zero_asymmetry = left_zero != right_zero
                && zero_shared.len() >= MIN_ZERO_CARVE_OUT_OVERLAP
                && has_count
                && has_value
                && zero_admission_name(zero_side_name);
            let union_size = left
                .direct_predicate_identifiers
                .union(&right.direct_predicate_identifiers)
                .count();
            let high_overlap = direct_shared.len() >= MIN_PREDICATE_OVERLAP
                && direct_shared.len().saturating_mul(5) >= union_size.saturating_mul(3)
                && left.direct_features != right.direct_features;
            if !zero_asymmetry && !high_overlap {
                continue;
            }
            let shared = if zero_asymmetry {
                zero_shared
            } else {
                direct_shared
            };
            findings.push(SiblingGateAsymmetry {
                left: left.anchor,
                right: right.anchor,
                shared_identifiers: shared.into_iter().take(24).collect(),
                left_features: left.direct_features.clone(),
                right_features: right.direct_features.clone(),
                left_zero_nan_admission: left_zero,
                is_zero_nan_asymmetry: zero_asymmetry,
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use deslop_parse::{ContractSnapshot, ProjectAnalysis, ProjectSnapshotBuilder, RepositoryId};

    use super::*;

    fn revision(path: &str, source: &str) -> RevisionContracts {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = ProjectSnapshotBuilder::new(
            temp.path(),
            RepositoryId::explicit("sibling-gate-test").unwrap(),
        )
        .unwrap()
        .with_overlay(path, source.as_bytes().to_vec())
        .unwrap()
        .build()
        .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        ContractSnapshot::from_analysis("current", &analysis)
            .unwrap()
            .revision_contracts()
    }

    #[test]
    fn zero_nan_carve_out_pairs_two_fail_loud_siblings() {
        let revision = revision(
            "gates.jl",
            r#"
function require_save(activity)
    activity.controller_lambda_observations > 0 || throw(ArgumentError("empty"))
    isfinite(activity.controller_lambda_mean) || throw(ArgumentError("mean"))
end
function require_resume(activity)
    isnan(activity.controller_lambda_mean) &&
        iszero(activity.controller_lambda_observations) && return true
    isfinite(activity.controller_lambda_mean) || throw(ArgumentError("mean"))
end
"#,
        );
        let findings = sibling_gate_asymmetries(&revision);
        assert_eq!(findings.len(), 1, "{:#?}", findings.len());
        assert_eq!(findings[0].shared_identifiers.len(), 2);
    }

    #[test]
    fn aligned_and_low_overlap_gates_do_not_pair() {
        let aligned = revision(
            "gates.py",
            r#"
def require_save(activity):
    assert activity.controller_lambda_observations > 0
    assert activity.controller_lambda_mean > 0
def verify_release(activity):
    assert activity.controller_lambda_observations > 0
    assert activity.controller_lambda_mean > 0
"#,
        );
        assert!(sibling_gate_asymmetries(&aligned).is_empty());

        let low_overlap = revision(
            "gates.js",
            r#"
function requireSave(activity) {
  if (activity.controller_lambda_observations <= 0) throw new Error("empty");
}
function verifyRun(status) {
  if (status.controller_lambda_observations === 0 && status.release_run_id === null) {
    throw new Error("identity");
  }
}
"#,
        );
        assert!(sibling_gate_asymmetries(&low_overlap).is_empty());
    }
}
