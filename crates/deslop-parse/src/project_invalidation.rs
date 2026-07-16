//! Changed-range and dependency-driven projection invalidation (M9.2).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{FileAnalysisChangeKind, ProjectAnalysisUpdate, SyntaxSpan};

pub const INVALIDATION_PLAN_SCHEMA: &str = "deslop.project-invalidation-plan/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionKind {
    OwnedSyntax,
    Scopes,
    ControlFlow,
    ProgramDependence,
    CloneBuckets,
    Metrics,
    Candidates,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "ranges")]
pub enum InvalidationScope {
    ChangedRanges(Vec<SyntaxSpan>),
    WholeFile,
    Dependency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvalidationReason {
    ContentChanged,
    FileAdded,
    FileRemoved,
    GrammarChanged,
    SyntaxUnavailable,
    DependencyChanged,
    MissingDependencyEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionInvalidation {
    path: PathBuf,
    projection: ProjectionKind,
    scope: InvalidationScope,
    reason: InvalidationReason,
}

impl ProjectionInvalidation {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn projection(&self) -> ProjectionKind {
        self.projection
    }

    pub fn scope(&self) -> &InvalidationScope {
        &self.scope
    }

    pub fn reason(&self) -> InvalidationReason {
        self.reason
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidationDependencyEvidence {
    Complete,
    MissingFor(BTreeSet<PathBuf>),
}

#[derive(Debug, Clone, Default)]
pub struct ProjectionDependencyIndex {
    project_paths: BTreeSet<PathBuf>,
    reverse_dependencies: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    covered_paths: BTreeSet<PathBuf>,
}

impl ProjectionDependencyIndex {
    pub fn new(project_paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            project_paths: project_paths.into_iter().collect(),
            reverse_dependencies: BTreeMap::new(),
            covered_paths: BTreeSet::new(),
        }
    }

    /// Mark one path's outgoing dependency evidence complete, including an empty set.
    pub fn record_dependencies(
        &mut self,
        dependent: impl Into<PathBuf>,
        dependencies: impl IntoIterator<Item = PathBuf>,
    ) {
        let dependent = dependent.into();
        self.project_paths.insert(dependent.clone());
        self.covered_paths.insert(dependent.clone());
        for dependency in dependencies {
            self.project_paths.insert(dependency.clone());
            self.reverse_dependencies
                .entry(dependency)
                .or_default()
                .insert(dependent.clone());
        }
    }

    pub fn evidence(&self) -> InvalidationDependencyEvidence {
        let missing = self
            .project_paths
            .difference(&self.covered_paths)
            .cloned()
            .collect::<BTreeSet<_>>();
        if missing.is_empty() {
            InvalidationDependencyEvidence::Complete
        } else {
            InvalidationDependencyEvidence::MissingFor(missing)
        }
    }

    fn transitive_dependents(&self, changed: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
        let mut found = BTreeSet::new();
        let mut queue = changed.iter().cloned().collect::<VecDeque<_>>();
        while let Some(path) = queue.pop_front() {
            if let Some(dependents) = self.reverse_dependencies.get(&path) {
                for dependent in dependents {
                    if !changed.contains(dependent) && found.insert(dependent.clone()) {
                        queue.push_back(dependent.clone());
                    }
                }
            }
        }
        found
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectInvalidationPlan {
    schema: String,
    previous_analysis: String,
    current_analysis: String,
    dependency_evidence_complete: bool,
    invalidations: Vec<ProjectionInvalidation>,
}

impl ProjectInvalidationPlan {
    pub fn derive(
        update: &ProjectAnalysisUpdate,
        dependencies: &ProjectionDependencyIndex,
    ) -> Self {
        let mut invalidations =
            BTreeMap::<(PathBuf, ProjectionKind), ProjectionInvalidation>::new();
        let mut changed = BTreeSet::new();

        for change in update.changes() {
            if change.kind() == FileAnalysisChangeKind::Reused {
                continue;
            }
            changed.insert(change.path().to_path_buf());
            let reason = match change.kind() {
                FileAnalysisChangeKind::Added => InvalidationReason::FileAdded,
                FileAnalysisChangeKind::Removed => InvalidationReason::FileRemoved,
                FileAnalysisChangeKind::Rebuilt => match change.rebuild_reason() {
                    Some(crate::FileRebuildReason::GrammarChanged) => {
                        InvalidationReason::GrammarChanged
                    }
                    Some(crate::FileRebuildReason::SyntaxUnavailable) | None => {
                        InvalidationReason::SyntaxUnavailable
                    }
                },
                FileAnalysisChangeKind::Incremental => InvalidationReason::ContentChanged,
                FileAnalysisChangeKind::Reused => unreachable!("reused changes are skipped"),
            };
            let syntax_scope = if change.kind() == FileAnalysisChangeKind::Incremental
                && !change.syntax_changed_ranges().is_empty()
            {
                InvalidationScope::ChangedRanges(change.syntax_changed_ranges().to_vec())
            } else {
                InvalidationScope::WholeFile
            };
            insert(
                &mut invalidations,
                change.path(),
                ProjectionKind::OwnedSyntax,
                syntax_scope,
                reason,
            );
            for projection in downstream_projections() {
                insert(
                    &mut invalidations,
                    change.path(),
                    projection,
                    InvalidationScope::WholeFile,
                    reason,
                );
            }
        }

        let dependency_evidence_complete = matches!(
            dependencies.evidence(),
            InvalidationDependencyEvidence::Complete
        );
        let (dependents, dependency_reason) = if dependency_evidence_complete {
            (
                dependencies.transitive_dependents(&changed),
                InvalidationReason::DependencyChanged,
            )
        } else {
            // Fail closed: without a complete dependency graph every unchanged path is a
            // possible dependent. Syntax ownership itself remains content-local.
            (
                dependencies
                    .project_paths
                    .difference(&changed)
                    .cloned()
                    .collect(),
                InvalidationReason::MissingDependencyEvidence,
            )
        };
        for path in dependents {
            for projection in dependency_projections() {
                insert(
                    &mut invalidations,
                    &path,
                    projection,
                    InvalidationScope::Dependency,
                    dependency_reason,
                );
            }
        }

        Self {
            schema: INVALIDATION_PLAN_SCHEMA.into(),
            previous_analysis: update.previous().id().as_str().into(),
            current_analysis: update.current().id().as_str().into(),
            dependency_evidence_complete,
            invalidations: invalidations.into_values().collect(),
        }
    }

    pub fn dependency_evidence_complete(&self) -> bool {
        self.dependency_evidence_complete
    }

    pub fn invalidations(&self) -> &[ProjectionInvalidation] {
        &self.invalidations
    }

    pub fn fan_out(&self) -> usize {
        self.invalidations
            .iter()
            .map(|item| item.path())
            .collect::<BTreeSet<_>>()
            .len()
    }

    pub fn invalidates(&self, path: &Path, projection: ProjectionKind) -> bool {
        self.invalidations
            .iter()
            .any(|item| item.path == path && item.projection == projection)
    }
}

fn insert(
    invalidations: &mut BTreeMap<(PathBuf, ProjectionKind), ProjectionInvalidation>,
    path: &Path,
    projection: ProjectionKind,
    scope: InvalidationScope,
    reason: InvalidationReason,
) {
    invalidations.insert(
        (path.to_path_buf(), projection),
        ProjectionInvalidation {
            path: path.to_path_buf(),
            projection,
            scope,
            reason,
        },
    );
}

fn downstream_projections() -> impl Iterator<Item = ProjectionKind> {
    [
        ProjectionKind::Scopes,
        ProjectionKind::ControlFlow,
        ProjectionKind::ProgramDependence,
        ProjectionKind::CloneBuckets,
        ProjectionKind::Metrics,
        ProjectionKind::Candidates,
    ]
    .into_iter()
}

fn dependency_projections() -> impl Iterator<Item = ProjectionKind> {
    [
        ProjectionKind::Scopes,
        ProjectionKind::ProgramDependence,
        ProjectionKind::CloneBuckets,
        ProjectionKind::Metrics,
        ProjectionKind::Candidates,
    ]
    .into_iter()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{ProjectAnalysis, ProjectSnapshotBuilder, RepositoryId};

    fn snapshot(files: &[(&str, &[u8])]) -> Arc<crate::ProjectSnapshot> {
        let temp = tempfile::tempdir().unwrap();
        let mut builder = ProjectSnapshotBuilder::new(
            temp.path(),
            RepositoryId::explicit("invalidation-test").unwrap(),
        )
        .unwrap();
        for (path, source) in files {
            builder = builder.with_overlay(path, source.to_vec()).unwrap();
        }
        builder.build().unwrap()
    }

    #[test]
    fn exact_change_invalidates_local_projections_and_transitive_dependents() {
        let previous = ProjectAnalysis::build(snapshot(&[
            ("src/a.rs", b"pub fn value() -> i32 { 1 }\n"),
            ("src/b.rs", b"fn use_value() -> i32 { value() }\n"),
            ("src/c.rs", b"fn top() -> i32 { use_value() }\n"),
            ("src/unrelated.rs", b"fn stable() {}\n"),
        ]))
        .unwrap();
        let update = previous
            .successor(snapshot(&[
                ("src/a.rs", b"pub fn value() -> i32 { 2 }\n"),
                ("src/b.rs", b"fn use_value() -> i32 { value() }\n"),
                ("src/c.rs", b"fn top() -> i32 { use_value() }\n"),
                ("src/unrelated.rs", b"fn stable() {}\n"),
            ]))
            .unwrap();
        let mut dependencies = ProjectionDependencyIndex::new([
            PathBuf::from("src/a.rs"),
            PathBuf::from("src/b.rs"),
            PathBuf::from("src/c.rs"),
            PathBuf::from("src/unrelated.rs"),
        ]);
        dependencies.record_dependencies("src/a.rs", []);
        dependencies.record_dependencies("src/b.rs", [PathBuf::from("src/a.rs")]);
        dependencies.record_dependencies("src/c.rs", [PathBuf::from("src/b.rs")]);
        dependencies.record_dependencies("src/unrelated.rs", []);

        let plan = ProjectInvalidationPlan::derive(&update, &dependencies);
        assert!(plan.dependency_evidence_complete());
        assert!(plan.invalidates(Path::new("src/a.rs"), ProjectionKind::OwnedSyntax));
        assert!(plan.invalidates(Path::new("src/b.rs"), ProjectionKind::ProgramDependence));
        assert!(plan.invalidates(Path::new("src/c.rs"), ProjectionKind::Candidates));
        assert!(!plan.invalidates(Path::new("src/unrelated.rs"), ProjectionKind::Metrics));
        assert_eq!(plan.fan_out(), 3);
    }

    #[test]
    fn incomplete_dependency_evidence_expands_invalidation_but_not_syntax() {
        let previous = ProjectAnalysis::build(snapshot(&[
            ("src/a.rs", b"fn a() -> i32 { 1 }\n"),
            ("src/b.rs", b"fn b() -> i32 { 2 }\n"),
        ]))
        .unwrap();
        let update = previous
            .successor(snapshot(&[
                ("src/a.rs", b"fn a() -> i32 { 3 }\n"),
                ("src/b.rs", b"fn b() -> i32 { 2 }\n"),
            ]))
            .unwrap();
        let dependencies =
            ProjectionDependencyIndex::new([PathBuf::from("src/a.rs"), PathBuf::from("src/b.rs")]);

        let plan = ProjectInvalidationPlan::derive(&update, &dependencies);
        assert!(!plan.dependency_evidence_complete());
        assert!(plan.invalidates(Path::new("src/b.rs"), ProjectionKind::Candidates));
        assert!(!plan.invalidates(Path::new("src/b.rs"), ProjectionKind::OwnedSyntax));
        assert!(plan.invalidations().iter().any(|item| {
            item.path() == Path::new("src/b.rs")
                && item.reason() == InvalidationReason::MissingDependencyEvidence
        }));
    }
}
