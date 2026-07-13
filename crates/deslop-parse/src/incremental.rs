use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use tree_sitter::{InputEdit, Parser, Point};

use crate::analysis_provenance_for_tree;
use crate::arena::{SourcePoint, SyntaxArena, SyntaxSpan, tree_nodes_preorder};
use crate::identity::{NodeId, NodeKey, NodeKeyLookupError};
use crate::instrumentation::ProjectAnalysisUpdateInstrumentation;
use crate::snapshot::{
    FileRevisionKey, ParseLedger, ParsedFile, ProjectAnalysis, ProjectSnapshot, RepositoryId,
    SnapshotEntry, SnapshotEntryKind, byte_line_starts, parse_owned_file,
};

/// One validated sequential replacement record in the source coordinates before that edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEdit {
    old_range: Range<usize>,
    new_range: Range<usize>,
    start_point: SourcePoint,
    old_end_point: SourcePoint,
    new_end_point: SourcePoint,
}

/// One caller-observed UTF-8 replacement, addressed in the source state immediately before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReplacement {
    old_range: Range<usize>,
    replacement: String,
}

impl SourceReplacement {
    pub fn new(old_range: Range<usize>, replacement: impl Into<String>) -> Self {
        Self {
            old_range,
            replacement: replacement.into(),
        }
    }

    pub fn old_range(&self) -> Range<usize> {
        self.old_range.clone()
    }

    pub fn replacement(&self) -> &str {
        &self.replacement
    }
}

/// Ordered edit history for one logical source path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSourceEdits {
    path: PathBuf,
    replacements: Box<[SourceReplacement]>,
}

impl FileSourceEdits {
    pub fn new(path: impl Into<PathBuf>, replacements: Vec<SourceReplacement>) -> Self {
        Self {
            path: path.into(),
            replacements: replacements.into_boxed_slice(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn replacements(&self) -> &[SourceReplacement] {
        &self.replacements
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceEditEvidence {
    /// Caller-supplied ordered replacements reconstructed the exact successor bytes.
    ExactScript,
    /// A coarse old/new diff is sufficient for parser reuse, but not node identity.
    DerivedDiff,
}

impl SourceEdit {
    pub fn old_range(&self) -> Range<usize> {
        self.old_range.clone()
    }

    pub fn new_range(&self) -> Range<usize> {
        self.new_range.clone()
    }

    pub fn start_point(&self) -> SourcePoint {
        self.start_point
    }

    pub fn old_end_point(&self) -> SourcePoint {
        self.old_end_point
    }

    pub fn new_end_point(&self) -> SourcePoint {
        self.new_end_point
    }

    fn input_edit(&self) -> InputEdit {
        InputEdit {
            start_byte: self.old_range.start,
            old_end_byte: self.old_range.end,
            new_end_byte: self.new_range.end,
            start_position: tree_point(self.start_point),
            old_end_position: tree_point(self.old_end_point),
            new_end_position: tree_point(self.new_end_point),
        }
    }

    fn map_unchanged_range(&self, old: Range<usize>) -> Option<Range<usize>> {
        if old.start == old.end
            && self.old_range.start == self.old_range.end
            && old.start == self.old_range.start
        {
            return None;
        }
        if old.end <= self.old_range.start {
            return Some(old);
        }
        if old.start >= self.old_range.end {
            let new_start = shift_offset(old.start, self.old_range.end, self.new_range.end)?;
            let new_end = shift_offset(old.end, self.old_range.end, self.new_range.end)?;
            return Some(new_start..new_end);
        }
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FileAnalysisChangeKind {
    Reused,
    Incremental,
    Rebuilt,
    Added,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FileRebuildReason {
    GrammarChanged,
    SyntaxUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAnalysisChange {
    path: PathBuf,
    previous: Option<FileRevisionKey>,
    current: Option<FileRevisionKey>,
    kind: FileAnalysisChangeKind,
    rebuild_reason: Option<FileRebuildReason>,
    source_edit_evidence: Option<SourceEditEvidence>,
    source_invalidation_edit: Option<SourceEdit>,
    source_edits: Box<[SourceEdit]>,
    syntax_changed_ranges: Box<[SyntaxSpan]>,
}

impl FileAnalysisChange {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn previous(&self) -> Option<&FileRevisionKey> {
        self.previous.as_ref()
    }

    pub fn current(&self) -> Option<&FileRevisionKey> {
        self.current.as_ref()
    }

    pub fn kind(&self) -> FileAnalysisChangeKind {
        self.kind
    }

    pub fn rebuild_reason(&self) -> Option<FileRebuildReason> {
        self.rebuild_reason
    }

    pub fn source_edit_evidence(&self) -> Option<SourceEditEvidence> {
        self.source_edit_evidence
    }

    /// Canonical old-revision to final-new-revision byte invalidation.
    pub fn source_invalidation_edit(&self) -> Option<&SourceEdit> {
        self.source_invalidation_edit.as_ref()
    }

    /// Validated sequential edits, each addressed in the state produced by its predecessor.
    ///
    /// These ranges do not all share the old/final coordinate spaces. Use
    /// [`Self::source_invalidation_edit`] for canonical old-to-final invalidation.
    pub fn source_edits(&self) -> &[SourceEdit] {
        &self.source_edits
    }

    /// Tree-sitter structural changes in new-revision coordinates.
    ///
    /// These may be empty for token-text or trivia edits;
    /// [`Self::source_invalidation_edit`] remains the canonical byte invalidation and
    /// [`Self::source_edit_evidence`] states whether exact history authorizes correlation.
    pub fn syntax_changed_ranges(&self) -> &[SyntaxSpan] {
        &self.syntax_changed_ranges
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeExpiryReason {
    FileRemoved,
    GrammarChanged,
    SyntaxUnavailable,
    NodeChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Transition-local correlation evidence between two immutable analyses.
///
/// A re-anchor never refreshes proposal/work-order guards, preserves an editor document version,
/// or bypasses projection recomputation under the successor analysis identity.
pub enum NodeReanchor {
    Retained {
        node: NodeId,
        key: NodeKey,
    },
    Reanchored {
        node: NodeId,
        key: NodeKey,
        evidence: NodeReanchorEvidence,
    },
    Expired {
        reason: NodeExpiryReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeReanchorEvidence {
    TreeSitterReusedSubtree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeTransition {
    Retained(NodeId),
    Reanchored(NodeId),
    Expired(NodeExpiryReason),
}

#[derive(Debug)]
pub struct ProjectAnalysisUpdate {
    previous: Arc<ProjectAnalysis>,
    current: Arc<ProjectAnalysis>,
    changes: Box<[FileAnalysisChange]>,
    transitions: Box<[NodeTransition]>,
}

impl ProjectAnalysisUpdate {
    pub fn previous(&self) -> &Arc<ProjectAnalysis> {
        &self.previous
    }

    pub fn current(&self) -> &Arc<ProjectAnalysis> {
        &self.current
    }

    pub fn into_current(self) -> Arc<ProjectAnalysis> {
        self.current
    }

    pub fn changes(&self) -> &[FileAnalysisChange] {
        &self.changes
    }

    /// Measure deterministic successor work and retained transition storage.
    pub fn instrumentation(&self) -> ProjectAnalysisUpdateInstrumentation {
        let mut measured = ProjectAnalysisUpdateInstrumentation {
            files: self.changes.len(),
            previous_nodes: self.previous.node_count(),
            current_nodes: self.current.node_count(),
            successor_assembly_nodes: self.current.node_count(),
            transition_entries: self.transitions.len(),
            transition_bytes_lower_bound: self.transitions.len()
                * std::mem::size_of::<NodeTransition>(),
            ..ProjectAnalysisUpdateInstrumentation::default()
        };
        for change in &self.changes {
            measured.source_edits += change.source_edits.len();
            measured.syntax_changed_ranges += change.syntax_changed_ranges.len();
            let current_nodes = self
                .current
                .file_node_ids(&change.path)
                .map_or(0, |nodes| nodes.len());
            match change.kind {
                FileAnalysisChangeKind::Reused => measured.reused_files += 1,
                FileAnalysisChangeKind::Incremental => {
                    measured.incremental_files += 1;
                    measured.incrementally_rebuilt_nodes += current_nodes;
                }
                FileAnalysisChangeKind::Rebuilt => {
                    measured.rebuilt_files += 1;
                    measured.fully_rebuilt_nodes += current_nodes;
                }
                FileAnalysisChangeKind::Added => {
                    measured.added_files += 1;
                    measured.fully_rebuilt_nodes += current_nodes;
                }
                FileAnalysisChangeKind::Removed => measured.removed_files += 1,
            }
            match change.source_edit_evidence {
                Some(SourceEditEvidence::ExactScript) => {
                    let mut working_bytes = self
                        .previous
                        .file(&change.path)
                        .map_or(0, |file| file.source().len());
                    for edit in &change.source_edits {
                        measured.sequential_edit_validation_bytes_upper_bound = measured
                            .sequential_edit_validation_bytes_upper_bound
                            .saturating_add(working_bytes);
                        working_bytes = working_bytes
                            .saturating_sub(edit.old_range.len())
                            .saturating_add(edit.new_range.len());
                    }
                    measured.sequential_edit_validation_bytes_upper_bound = measured
                        .sequential_edit_validation_bytes_upper_bound
                        .saturating_add(
                            self.current
                                .file(&change.path)
                                .map_or(0, |file| file.source().len()),
                        );
                }
                Some(SourceEditEvidence::DerivedDiff) => {
                    measured.derived_diff_bytes_upper_bound = measured
                        .derived_diff_bytes_upper_bound
                        .saturating_add(
                            self.previous
                                .file(&change.path)
                                .map_or(0, |file| file.source().len()),
                        )
                        .saturating_add(
                            self.current
                                .file(&change.path)
                                .map_or(0, |file| file.source().len()),
                        );
                }
                None => {}
            }
        }
        for transition in &self.transitions {
            match transition {
                NodeTransition::Retained(_) => measured.retained_transitions += 1,
                NodeTransition::Reanchored(_) => measured.reanchored_transitions += 1,
                NodeTransition::Expired(_) => measured.expired_transitions += 1,
            }
        }
        measured
    }

    /// Re-anchor one key from this update's exact previous analysis, or expire it explicitly.
    pub fn reanchor(&self, previous: &NodeKey) -> Result<NodeReanchor, NodeKeyLookupError> {
        let previous_node = self.previous.node_by_key(previous)?;
        match &self.transitions[previous_node.id().index as usize] {
            NodeTransition::Retained(node) => Ok(NodeReanchor::Retained {
                node: *node,
                key: self
                    .current
                    .node_key(*node)
                    .expect("transition owns a current node")
                    .clone(),
            }),
            NodeTransition::Reanchored(node) => Ok(NodeReanchor::Reanchored {
                node: *node,
                key: self
                    .current
                    .node_key(*node)
                    .expect("transition owns a current node")
                    .clone(),
                evidence: NodeReanchorEvidence::TreeSitterReusedSubtree,
            }),
            NodeTransition::Expired(reason) => Ok(NodeReanchor::Expired { reason: *reason }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectAnalysisUpdateError {
    RepositoryMismatch {
        previous: RepositoryId,
        current: RepositoryId,
    },
    Build {
        path: Option<PathBuf>,
        message: String,
    },
    DuplicateEditPath {
        path: PathBuf,
    },
    UnexpectedEditPath {
        path: PathBuf,
    },
    InvalidEdit {
        path: PathBuf,
        edit_index: usize,
        message: String,
    },
    EditScriptMismatch {
        path: PathBuf,
    },
    RuntimeLanguageMismatch {
        path: PathBuf,
    },
}

impl fmt::Display for ProjectAnalysisUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RepositoryMismatch { previous, current } => write!(
                formatter,
                "successor repository {} does not match previous repository {}",
                current.as_str(),
                previous.as_str()
            ),
            Self::Build { path, message } => match path {
                Some(path) => write!(
                    formatter,
                    "failed to build successor file {}: {message}",
                    path.display()
                ),
                None => write!(
                    formatter,
                    "failed to assemble successor analysis: {message}"
                ),
            },
            Self::DuplicateEditPath { path } => {
                write!(
                    formatter,
                    "successor edit history repeats path {}",
                    path.display()
                )
            }
            Self::UnexpectedEditPath { path } => write!(
                formatter,
                "successor edit history does not describe a changed same-grammar file {}",
                path.display()
            ),
            Self::InvalidEdit {
                path,
                edit_index,
                message,
            } => write!(
                formatter,
                "successor edit {edit_index} for {} is invalid: {message}",
                path.display()
            ),
            Self::EditScriptMismatch { path } => write!(
                formatter,
                "successor edit history for {} does not reconstruct the snapshot bytes",
                path.display()
            ),
            Self::RuntimeLanguageMismatch { path } => write!(
                formatter,
                "successor retained a different runtime parser language for unchanged grammar selection {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ProjectAnalysisUpdateError {}

#[derive(Debug, Clone)]
enum BuildMode {
    Reused,
    Incremental {
        edits: Box<[SourceEdit]>,
        reanchor: bool,
    },
    Rebuilt(FileRebuildReason),
    Added,
    Removed,
}

impl ProjectAnalysis {
    /// Build a new immutable analysis from `snapshot`, reusing only explicitly compatible state.
    pub fn successor(
        self: &Arc<Self>,
        snapshot: Arc<ProjectSnapshot>,
    ) -> Result<ProjectAnalysisUpdate, ProjectAnalysisUpdateError> {
        self.successor_impl(snapshot, &BTreeMap::new())
    }

    /// Build a successor with exact ordered edit history that may authorize node re-anchoring.
    pub fn successor_with_edits(
        self: &Arc<Self>,
        snapshot: Arc<ProjectSnapshot>,
        edits: &[FileSourceEdits],
    ) -> Result<ProjectAnalysisUpdate, ProjectAnalysisUpdateError> {
        let mut by_path = BTreeMap::new();
        for file in edits {
            if by_path
                .insert(file.path.clone(), file.replacements.as_ref())
                .is_some()
            {
                return Err(ProjectAnalysisUpdateError::DuplicateEditPath {
                    path: file.path.clone(),
                });
            }
        }
        self.successor_impl(snapshot, &by_path)
    }

    fn successor_impl(
        self: &Arc<Self>,
        snapshot: Arc<ProjectSnapshot>,
        edits: &BTreeMap<PathBuf, &[SourceReplacement]>,
    ) -> Result<ProjectAnalysisUpdate, ProjectAnalysisUpdateError> {
        if self.snapshot().repository() != snapshot.repository() {
            return Err(ProjectAnalysisUpdateError::RepositoryMismatch {
                previous: self.snapshot().repository().clone(),
                current: snapshot.repository().clone(),
            });
        }

        let entries = snapshot
            .entries()
            .filter(|entry| entry.kind() == SnapshotEntryKind::Source)
            .map(|entry| (entry.path().to_path_buf(), entry))
            .collect::<BTreeMap<_, _>>();
        for path in edits.keys() {
            if self.file(path).is_none() || !entries.contains_key(path) {
                return Err(ProjectAnalysisUpdateError::UnexpectedEditPath { path: path.clone() });
            }
        }
        let mut paths = self
            .files()
            .map(|file| file.key().path.clone())
            .chain(entries.keys().cloned())
            .collect::<BTreeSet<_>>();
        let ledger = Arc::new(ParseLedger::default());
        let mut files = BTreeMap::new();
        let mut changes = Vec::with_capacity(paths.len());
        let mut modes = BTreeMap::new();

        for path in std::mem::take(&mut paths) {
            let previous = self.file_arc(&path);
            let current_entry = entries.get(&path).copied();
            let (file, change, mode) = build_file_transition(
                snapshot.repository(),
                &path,
                previous,
                current_entry,
                edits.get(&path).copied(),
                &ledger,
            )?;
            if let Some(file) = file {
                files.insert(path.clone(), file);
            }
            changes.push(change);
            modes.insert(path, mode);
        }

        let current = ProjectAnalysis::assemble(snapshot, files, ledger).map_err(|error| {
            ProjectAnalysisUpdateError::Build {
                path: None,
                message: error.to_string(),
            }
        })?;
        let transitions = build_node_transitions(self, &current, &modes);
        Ok(ProjectAnalysisUpdate {
            previous: Arc::clone(self),
            current,
            changes: changes.into_boxed_slice(),
            transitions: transitions.into_boxed_slice(),
        })
    }
}

fn build_file_transition(
    repository: &RepositoryId,
    path: &Path,
    previous: Option<Arc<ParsedFile>>,
    current_entry: Option<&SnapshotEntry>,
    edit_script: Option<&[SourceReplacement]>,
    ledger: &ParseLedger,
) -> Result<(Option<Arc<ParsedFile>>, FileAnalysisChange, BuildMode), ProjectAnalysisUpdateError> {
    let Some(entry) = current_entry else {
        if edit_script.is_some() {
            return Err(ProjectAnalysisUpdateError::UnexpectedEditPath {
                path: path.to_path_buf(),
            });
        }
        let previous_key = previous.expect("union path without entry belongs to previous analysis");
        return Ok((
            None,
            FileAnalysisChange {
                path: path.to_path_buf(),
                previous: Some(previous_key.key().clone()),
                current: None,
                kind: FileAnalysisChangeKind::Removed,
                rebuild_reason: None,
                source_edit_evidence: None,
                source_invalidation_edit: None,
                source_edits: Box::default(),
                syntax_changed_ranges: Box::default(),
            },
            BuildMode::Removed,
        ));
    };
    let key = file_key(repository, entry).map_err(|message| ProjectAnalysisUpdateError::Build {
        path: Some(path.to_path_buf()),
        message,
    })?;

    if let Some(previous) = previous.as_ref()
        && previous.grammar() == &key.grammar
    {
        let language_matches = entry.grammar_language().is_some_and(|language| {
            previous.language == *language
                && previous
                    .query_tree()
                    .is_none_or(|tree| &*tree.language() == language)
        });
        if !language_matches {
            return Err(ProjectAnalysisUpdateError::RuntimeLanguageMismatch {
                path: path.to_path_buf(),
            });
        }
    }

    if let Some(previous) = previous.as_ref()
        && previous.key() == &key
    {
        if edit_script.is_some() {
            return Err(ProjectAnalysisUpdateError::UnexpectedEditPath {
                path: path.to_path_buf(),
            });
        }
        ledger.record_requested(&key);
        ledger.record_owner(&key);
        ledger.record_reuse(&key);
        return Ok((
            Some(Arc::clone(previous)),
            FileAnalysisChange {
                path: path.to_path_buf(),
                previous: Some(previous.key().clone()),
                current: Some(key),
                kind: FileAnalysisChangeKind::Reused,
                rebuild_reason: None,
                source_edit_evidence: None,
                source_invalidation_edit: None,
                source_edits: Box::default(),
                syntax_changed_ranges: Box::default(),
            },
            BuildMode::Reused,
        ));
    }

    let mut source_change = match (
        previous.as_ref().and_then(|file| file.text()),
        std::str::from_utf8(entry.bytes()).ok(),
    ) {
        (Some(old_text), Some(new_text)) => {
            let canonical = derive_source_edit(old_text, new_text).map_err(|error| {
                ProjectAnalysisUpdateError::Build {
                    path: Some(path.to_path_buf()),
                    message: error.to_string(),
                }
            })?;
            let (evidence, edits) = if let Some(script) = edit_script {
                (
                    SourceEditEvidence::ExactScript,
                    apply_edit_script(path, old_text, new_text, script)?,
                )
            } else {
                (
                    SourceEditEvidence::DerivedDiff,
                    vec![canonical.clone()].into_boxed_slice(),
                )
            };
            Some((evidence, canonical, edits))
        }
        _ => None,
    };
    if edit_script.is_some() && source_change.is_none() {
        return Err(ProjectAnalysisUpdateError::UnexpectedEditPath {
            path: path.to_path_buf(),
        });
    }

    if let Some(previous) = previous.as_ref()
        && previous.grammar() == &key.grammar
        && previous.text().is_some()
        && previous.query_tree().is_some()
        && std::str::from_utf8(entry.bytes()).is_ok()
    {
        let (source_edit_evidence, source_invalidation_edit, edits) = source_change
            .take()
            .expect("changed comparable source owns source-change evidence");
        let reanchor = source_edit_evidence == SourceEditEvidence::ExactScript;
        let (parsed, changed_ranges) =
            parse_incremental_file(entry, key.clone(), previous, &edits, ledger).map_err(
                |error| ProjectAnalysisUpdateError::Build {
                    path: Some(path.to_path_buf()),
                    message: error.to_string(),
                },
            )?;
        return Ok((
            Some(Arc::new(parsed)),
            FileAnalysisChange {
                path: path.to_path_buf(),
                previous: Some(previous.key().clone()),
                current: Some(key),
                kind: FileAnalysisChangeKind::Incremental,
                rebuild_reason: None,
                source_edit_evidence: Some(source_edit_evidence),
                source_invalidation_edit: Some(source_invalidation_edit),
                source_edits: edits.clone(),
                syntax_changed_ranges: changed_ranges,
            },
            BuildMode::Incremental { edits, reanchor },
        ));
    }

    let (kind, rebuild_reason, mode) = match previous.as_deref() {
        None => (FileAnalysisChangeKind::Added, None, BuildMode::Added),
        Some(previous) if previous.grammar() != &key.grammar => (
            FileAnalysisChangeKind::Rebuilt,
            Some(FileRebuildReason::GrammarChanged),
            BuildMode::Rebuilt(FileRebuildReason::GrammarChanged),
        ),
        Some(_) => (
            FileAnalysisChangeKind::Rebuilt,
            Some(FileRebuildReason::SyntaxUnavailable),
            BuildMode::Rebuilt(FileRebuildReason::SyntaxUnavailable),
        ),
    };
    let parsed = parse_owned_file(entry, key.clone(), ledger).map_err(|error| {
        ProjectAnalysisUpdateError::Build {
            path: Some(path.to_path_buf()),
            message: error.to_string(),
        }
    })?;
    let (source_edit_evidence, source_invalidation_edit, source_edits) = source_change.map_or(
        (None, None, Box::default()),
        |(evidence, canonical, edits)| (Some(evidence), Some(canonical), edits),
    );
    Ok((
        Some(Arc::new(parsed)),
        FileAnalysisChange {
            path: path.to_path_buf(),
            previous: previous.as_ref().map(|file| file.key().clone()),
            current: Some(key),
            kind,
            rebuild_reason,
            source_edit_evidence,
            source_invalidation_edit,
            source_edits,
            syntax_changed_ranges: Box::default(),
        },
        mode,
    ))
}

fn parse_incremental_file(
    entry: &SnapshotEntry,
    key: FileRevisionKey,
    previous: &ParsedFile,
    edits: &[SourceEdit],
    ledger: &ParseLedger,
) -> anyhow::Result<(ParsedFile, Box<[SyntaxSpan]>)> {
    let old_text = previous
        .text()
        .expect("incremental precondition validates old UTF-8");
    let new_text =
        std::str::from_utf8(entry.bytes()).expect("incremental precondition validates UTF-8");
    validate_tree_sitter_size(old_text.len())?;
    validate_tree_sitter_size(new_text.len())?;
    let mut old_tree = previous
        .query_tree()
        .expect("incremental precondition validates old tree")
        .clone();
    for edit in edits {
        old_tree.edit(&edit.input_edit());
    }
    let language = entry
        .grammar_language()
        .cloned()
        .context("source has no stored parser language")?;
    let mut parser = Parser::new();
    parser.set_language(&language)?;
    ledger.record_requested(&key);
    ledger.record_owner(&key);
    ledger.record_invocation(&key);
    let text = Arc::<str>::from(new_text);
    let tree = parser
        .parse(text.as_ref(), Some(&old_tree))
        .context("Tree-sitter returned no syntax tree for incremental parse")?;
    let changed_ranges = old_tree
        .changed_ranges(&tree)
        .map(tree_range)
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let provenance = analysis_provenance_for_tree(&tree);
    let arena = Some(SyntaxArena::from_tree(
        &tree,
        entry.bytes(),
        key.grammar.clone(),
    )?);
    let query_node_index = arena
        .as_ref()
        .map(|arena| crate::query::build_query_node_index(&tree, arena))
        .transpose()?;
    Ok((
        ParsedFile {
            key,
            source: Arc::clone(entry.stored_source()),
            language,
            text: Some(text),
            tree: Some(tree),
            arena,
            query_node_index,
            provenance,
            line_starts: byte_line_starts(entry.bytes()),
        },
        changed_ranges,
    ))
}

fn build_node_transitions(
    previous: &ProjectAnalysis,
    current: &ProjectAnalysis,
    modes: &BTreeMap<PathBuf, BuildMode>,
) -> Vec<NodeTransition> {
    let mut transitions =
        vec![NodeTransition::Expired(NodeExpiryReason::NodeChanged); previous.node_count()];
    for file in previous.files() {
        let path = &file.key().path;
        let previous_ids = previous
            .file_node_ids(path)
            .expect("previous file owns a node range")
            .collect::<Vec<_>>();
        match modes
            .get(path)
            .expect("every previous file has a transition")
        {
            BuildMode::Reused => {
                let current_ids = current
                    .file_node_ids(path)
                    .expect("reused file remains in current analysis")
                    .collect::<Vec<_>>();
                debug_assert_eq!(previous_ids.len(), current_ids.len());
                for (previous_id, current_id) in previous_ids.into_iter().zip(current_ids) {
                    transitions[previous_id.index as usize] = NodeTransition::Retained(current_id);
                }
            }
            BuildMode::Incremental { edits, reanchor } => {
                if *reanchor {
                    map_incremental_nodes(
                        previous,
                        current,
                        file,
                        current
                            .file(path)
                            .expect("incremental file remains current"),
                        &previous_ids,
                        edits,
                        &mut transitions,
                    );
                } else {
                    expire_nodes(
                        &previous_ids,
                        NodeExpiryReason::NodeChanged,
                        &mut transitions,
                    );
                }
            }
            BuildMode::Rebuilt(FileRebuildReason::GrammarChanged) => {
                expire_nodes(
                    &previous_ids,
                    NodeExpiryReason::GrammarChanged,
                    &mut transitions,
                );
            }
            BuildMode::Rebuilt(FileRebuildReason::SyntaxUnavailable) => {
                expire_nodes(
                    &previous_ids,
                    NodeExpiryReason::SyntaxUnavailable,
                    &mut transitions,
                );
            }
            BuildMode::Removed => {
                expire_nodes(
                    &previous_ids,
                    NodeExpiryReason::FileRemoved,
                    &mut transitions,
                );
            }
            BuildMode::Added => unreachable!("an added path has no previous nodes"),
        }
    }
    transitions
}

fn map_incremental_nodes(
    previous: &ProjectAnalysis,
    current: &ProjectAnalysis,
    previous_file: &ParsedFile,
    current_file: &ParsedFile,
    previous_ids: &[NodeId],
    edits: &[SourceEdit],
    transitions: &mut [NodeTransition],
) {
    let (Some(previous_tree), Some(current_tree)) =
        (previous_file.query_tree(), current_file.query_tree())
    else {
        expire_nodes(
            previous_ids,
            NodeExpiryReason::SyntaxUnavailable,
            transitions,
        );
        return;
    };
    let previous_nodes = tree_nodes_preorder(previous_tree);
    let current_nodes = tree_nodes_preorder(current_tree);
    let current_ids = current
        .file_node_ids(&current_file.key().path)
        .expect("current file owns a node range")
        .collect::<Vec<_>>();
    if previous_nodes.len() != previous_ids.len() || current_nodes.len() != current_ids.len() {
        return;
    }
    let current_by_tree_id = current_nodes
        .iter()
        .enumerate()
        .map(|(local, node)| (node.id(), local))
        .collect::<HashMap<_, _>>();
    for (local, previous_node) in previous_nodes.iter().enumerate() {
        let previous_id = previous_ids[local];
        let Some(&current_local) = current_by_tree_id.get(&previous_node.id()) else {
            continue;
        };
        let current_id = current_ids[current_local];
        let previous_view = previous
            .node(previous_id)
            .expect("previous id remains valid");
        let current_view = current.node(current_id).expect("current id remains valid");
        let Some(mapped_range) =
            map_unchanged_range_through(edits, previous_view.span().byte_range())
        else {
            continue;
        };
        if mapped_range != current_view.span().byte_range()
            || previous_view.bytes() != current_view.bytes()
            || previous_view.raw_kind_id() != current_view.raw_kind_id()
            || previous_view.raw_kind() != current_view.raw_kind()
            || previous_view.raw_grammar_kind_id() != current_view.raw_grammar_kind_id()
            || previous_view.raw_grammar_kind() != current_view.raw_grammar_kind()
            || previous_view.is_named() != current_view.is_named()
            || previous_view.is_extra() != current_view.is_extra()
            || previous_view.is_error() != current_view.is_error()
            || previous_view.is_missing() != current_view.is_missing()
            || previous_view.has_error() != current_view.has_error()
            || previous_view.key().field_path() != current_view.key().field_path()
            || previous_view.key().anchor().structural_digest()
                != current_view.key().anchor().structural_digest()
        {
            continue;
        }
        transitions[previous_id.index as usize] = NodeTransition::Reanchored(current_id);
    }
}

fn expire_nodes(ids: &[NodeId], reason: NodeExpiryReason, transitions: &mut [NodeTransition]) {
    for id in ids {
        transitions[id.index as usize] = NodeTransition::Expired(reason);
    }
}

fn file_key(repository: &RepositoryId, entry: &SnapshotEntry) -> Result<FileRevisionKey, String> {
    let grammar = entry
        .grammar()
        .cloned()
        .ok_or_else(|| "source has no stored grammar selection".to_string())?;
    Ok(FileRevisionKey {
        repository: repository.clone(),
        path: entry.path().to_path_buf(),
        source: entry.revision().clone(),
        grammar,
    })
}

fn apply_edit_script(
    path: &Path,
    old: &str,
    expected: &str,
    replacements: &[SourceReplacement],
) -> Result<Box<[SourceEdit]>, ProjectAnalysisUpdateError> {
    validate_tree_sitter_size(old.len()).map_err(|error| ProjectAnalysisUpdateError::Build {
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    validate_tree_sitter_size(expected.len()).map_err(|error| {
        ProjectAnalysisUpdateError::Build {
            path: Some(path.to_path_buf()),
            message: error.to_string(),
        }
    })?;

    let mut current = old.to_owned();
    let mut edits = Vec::with_capacity(replacements.len());
    for (edit_index, replacement) in replacements.iter().enumerate() {
        let range = replacement.old_range();
        let invalid = |message: String| ProjectAnalysisUpdateError::InvalidEdit {
            path: path.to_path_buf(),
            edit_index,
            message,
        };
        if range.start > range.end {
            return Err(invalid(format!(
                "range start {} exceeds end {}",
                range.start, range.end
            )));
        }
        if range.end > current.len() {
            return Err(invalid(format!(
                "range end {} exceeds current source length {}",
                range.end,
                current.len()
            )));
        }
        if !current.is_char_boundary(range.start) || !current.is_char_boundary(range.end) {
            return Err(invalid(
                "range is not on UTF-8 character boundaries".to_string(),
            ));
        }
        let new_len = current
            .len()
            .checked_sub(range.len())
            .and_then(|length| length.checked_add(replacement.replacement().len()))
            .ok_or_else(|| invalid("resulting source length overflows usize".to_string()))?;
        validate_tree_sitter_size(new_len).map_err(|error| invalid(error.to_string()))?;
        let start_point = point_at(current.as_bytes(), range.start)
            .map_err(|error| invalid(error.to_string()))?;
        let old_end_point =
            point_at(current.as_bytes(), range.end).map_err(|error| invalid(error.to_string()))?;
        let new_end = range
            .start
            .checked_add(replacement.replacement().len())
            .ok_or_else(|| invalid("replacement end overflows usize".to_string()))?;
        current.replace_range(range.clone(), replacement.replacement());
        let new_end_point =
            point_at(current.as_bytes(), new_end).map_err(|error| invalid(error.to_string()))?;
        edits.push(SourceEdit {
            old_range: range.clone(),
            new_range: range.start..new_end,
            start_point,
            old_end_point,
            new_end_point,
        });
    }
    if current.as_bytes() != expected.as_bytes() {
        return Err(ProjectAnalysisUpdateError::EditScriptMismatch {
            path: path.to_path_buf(),
        });
    }
    Ok(edits.into_boxed_slice())
}

fn map_unchanged_range_through(edits: &[SourceEdit], old: Range<usize>) -> Option<Range<usize>> {
    edits
        .iter()
        .try_fold(old, |range, edit| edit.map_unchanged_range(range))
}

fn derive_source_edit(old: &str, new: &str) -> anyhow::Result<SourceEdit> {
    let old_bytes = old.as_bytes();
    let new_bytes = new.as_bytes();
    let mut start = old_bytes
        .iter()
        .zip(new_bytes)
        .take_while(|(old, new)| old == new)
        .count();
    while !old.is_char_boundary(start) || !new.is_char_boundary(start) {
        start -= 1;
    }
    let mut suffix = old_bytes[start..]
        .iter()
        .rev()
        .zip(new_bytes[start..].iter().rev())
        .take_while(|(old, new)| old == new)
        .count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    let start_point = point_at(old_bytes, start)?;
    debug_assert_eq!(start_point, point_at(new_bytes, start)?);
    Ok(SourceEdit {
        old_range: start..old_end,
        new_range: start..new_end,
        start_point,
        old_end_point: point_at(old_bytes, old_end)?,
        new_end_point: point_at(new_bytes, new_end)?,
    })
}

fn point_at(source: &[u8], offset: usize) -> anyhow::Result<SourcePoint> {
    if offset > source.len() {
        anyhow::bail!("source point {offset} exceeds {} bytes", source.len());
    }
    let row = source[..offset]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count();
    let column = source[..offset]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(offset, |line_break| offset - line_break - 1);
    validate_tree_sitter_coordinate(row, "row")?;
    validate_tree_sitter_coordinate(column, "column")?;
    Ok(SourcePoint::new(row, column))
}

fn validate_tree_sitter_size(bytes: usize) -> anyhow::Result<()> {
    validate_tree_sitter_coordinate(bytes, "source byte length")
}

fn validate_tree_sitter_coordinate(value: usize, name: &str) -> anyhow::Result<()> {
    if value > u32::MAX as usize {
        anyhow::bail!("{name} {value} exceeds Tree-sitter's {} limit", u32::MAX);
    }
    Ok(())
}

fn shift_offset(value: usize, old_end: usize, new_end: usize) -> Option<usize> {
    if new_end >= old_end {
        value.checked_add(new_end - old_end)
    } else {
        value.checked_sub(old_end - new_end)
    }
}

fn tree_point(point: SourcePoint) -> Point {
    Point::new(point.row(), point.column())
}

fn tree_range(range: tree_sitter::Range) -> SyntaxSpan {
    SyntaxSpan::new(
        range.start_byte,
        range.end_byte,
        SourcePoint::new(range.start_point.row, range.start_point.column),
        SourcePoint::new(range.end_point.row, range.end_point.column),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{FileParseCount, ProjectSnapshotBuilder, SourceStore};

    fn repository(value: &str) -> RepositoryId {
        RepositoryId::explicit(value).unwrap()
    }

    fn snapshot(
        repository: RepositoryId,
        files: &[(&str, &[u8])],
        store: Option<Arc<SourceStore>>,
    ) -> Arc<ProjectSnapshot> {
        let temp = tempfile::tempdir().unwrap();
        let mut builder = ProjectSnapshotBuilder::new(temp.path(), repository).unwrap();
        if let Some(store) = store {
            builder = builder.with_store(store);
        }
        for (path, source) in files {
            builder = builder.with_overlay(path, source.to_vec()).unwrap();
        }
        builder.build().unwrap()
    }

    fn node_key_by_text(analysis: &ProjectAnalysis, path: &str, text: &str) -> NodeKey {
        analysis
            .file_node_ids(Path::new(path))
            .unwrap()
            .map(|id| analysis.node(id).unwrap())
            .find(|node| node.text() == text)
            .unwrap_or_else(|| panic!("missing node text {text:?} in {path}"))
            .key()
            .clone()
    }

    fn all_node_keys(analysis: &ProjectAnalysis) -> Vec<NodeKey> {
        analysis
            .node_ids()
            .map(|id| analysis.node_key(id).unwrap().clone())
            .collect()
    }

    fn transition_counts(
        update: &ProjectAnalysisUpdate,
        keys: &[NodeKey],
    ) -> (usize, usize, usize) {
        keys.iter().fold(
            (0, 0, 0),
            |(retained, reanchored, expired), key| match update.reanchor(key).unwrap() {
                NodeReanchor::Retained { .. } => (retained + 1, reanchored, expired),
                NodeReanchor::Reanchored { .. } => (retained, reanchored + 1, expired),
                NodeReanchor::Expired { .. } => (retained, reanchored, expired + 1),
            },
        )
    }

    #[test]
    fn successor_reuses_unchanged_files_and_incrementally_rebuilds_edited_files() {
        let stable = b"fn stable() { same(); }\n";
        let old_edit = b"fn alpha() { one(); }\nfn beta() { two(); }\nfn gamma() { three(); }\n";
        let new_edit = b"fn alpha() { one(); }\nfn beta() { six(); }\nfn gamma() { three(); }\n";
        let removed = b"fn removed() {}\n";
        let added = b"fn added() {}\n";
        let store = Arc::new(SourceStore::default());
        let previous_snapshot = snapshot(
            repository("successor-repository"),
            &[
                ("stable.rs", stable),
                ("edit.rs", old_edit),
                ("remove.rs", removed),
            ],
            Some(Arc::clone(&store)),
        );
        let previous = ProjectAnalysis::build(previous_snapshot).unwrap();
        let previous_id = previous.id().clone();
        let previous_keys = all_node_keys(&previous);
        let stable_arc = previous.file_arc(Path::new("stable.rs")).unwrap();
        let old_alpha = node_key_by_text(&previous, "edit.rs", "alpha");
        let old_beta = node_key_by_text(&previous, "edit.rs", "beta");
        let old_gamma = node_key_by_text(&previous, "edit.rs", "gamma");
        let old_removed = node_key_by_text(&previous, "remove.rs", "removed");
        let old_stable = node_key_by_text(&previous, "stable.rs", "stable");
        let old_beta_id = previous.node_by_key(&old_beta).unwrap().id();

        let current_snapshot = snapshot(
            repository("successor-repository"),
            &[
                ("stable.rs", stable),
                ("edit.rs", new_edit),
                ("add.rs", added),
            ],
            Some(store),
        );
        let clean = ProjectAnalysis::build(Arc::clone(&current_snapshot)).unwrap();
        let edit_start = old_edit
            .windows(3)
            .position(|window| window == b"two")
            .unwrap();
        let edit_history = [FileSourceEdits::new(
            "edit.rs",
            vec![SourceReplacement::new(edit_start..edit_start + 3, "six")],
        )];
        let update = previous
            .successor_with_edits(current_snapshot, &edit_history)
            .unwrap();
        let current = update.current();
        assert_eq!(
            update.instrumentation(),
            ProjectAnalysisUpdateInstrumentation {
                files: 4,
                reused_files: 1,
                incremental_files: 1,
                rebuilt_files: 0,
                added_files: 1,
                removed_files: 1,
                source_edits: 1,
                syntax_changed_ranges: 0,
                sequential_edit_validation_bytes_upper_bound: 134,
                derived_diff_bytes_upper_bound: 0,
                previous_nodes: 76,
                current_nodes: 76,
                incrementally_rebuilt_nodes: 49,
                fully_rebuilt_nodes: 10,
                successor_assembly_nodes: 76,
                transition_entries: 76,
                retained_transitions: 17,
                reanchored_transitions: 36,
                expired_transitions: 23,
                transition_bytes_lower_bound: 1_824,
            }
        );

        assert_eq!(previous.id(), &previous_id);
        assert_eq!(all_node_keys(&previous), previous_keys);
        assert_eq!(
            previous.file(Path::new("edit.rs")).unwrap().source(),
            old_edit
        );
        assert!(Arc::ptr_eq(
            &stable_arc,
            &current.file_arc(Path::new("stable.rs")).unwrap()
        ));
        assert_eq!(
            update
                .changes()
                .iter()
                .map(|change| (change.path().to_path_buf(), change.kind()))
                .collect::<Vec<_>>(),
            vec![
                (PathBuf::from("add.rs"), FileAnalysisChangeKind::Added),
                (
                    PathBuf::from("edit.rs"),
                    FileAnalysisChangeKind::Incremental
                ),
                (PathBuf::from("remove.rs"), FileAnalysisChangeKind::Removed),
                (PathBuf::from("stable.rs"), FileAnalysisChangeKind::Reused),
            ]
        );
        let edit_change = update
            .changes()
            .iter()
            .find(|change| change.path() == Path::new("edit.rs"))
            .unwrap();
        assert_eq!(
            edit_change.source_edit_evidence(),
            Some(SourceEditEvidence::ExactScript)
        );
        let [edit] = edit_change.source_edits() else {
            panic!("one exact replacement must yield one source edit");
        };
        assert_eq!(&old_edit[edit.old_range()], b"two");
        assert_eq!(&new_edit[edit.new_range()], b"six");
        assert!(edit_change.syntax_changed_ranges().is_empty());

        let counts = current.parse_counts();
        let stable_count = counts
            .iter()
            .find(|(key, _)| key.path == Path::new("stable.rs"))
            .map(|(_, count)| *count)
            .unwrap();
        let edit_count = counts
            .iter()
            .find(|(key, _)| key.path == Path::new("edit.rs"))
            .map(|(_, count)| *count)
            .unwrap();
        let added_count = counts
            .iter()
            .find(|(key, _)| key.path == Path::new("add.rs"))
            .map(|(_, count)| *count)
            .unwrap();
        assert_eq!(
            stable_count,
            FileParseCount {
                requested: 1,
                owners: 1,
                parser_invocations: 0,
                reused: 1,
            }
        );
        assert_eq!(
            edit_count,
            FileParseCount {
                requested: 1,
                owners: 1,
                parser_invocations: 1,
                reused: 0,
            }
        );
        assert_eq!(
            added_count,
            FileParseCount {
                requested: 1,
                owners: 1,
                parser_invocations: 1,
                reused: 0,
            }
        );

        let assert_reanchored = |key: &NodeKey, expected_text: &str| {
            let NodeReanchor::Reanchored {
                node,
                key: new_key,
                evidence: NodeReanchorEvidence::TreeSitterReusedSubtree,
            } = update.reanchor(key).unwrap()
            else {
                panic!("expected {expected_text:?} to re-anchor");
            };
            assert_eq!(current.node(node).unwrap().text(), expected_text);
            assert_eq!(current.node_key(node).unwrap(), &new_key);
        };
        assert_reanchored(&old_alpha, "alpha");
        assert_reanchored(&old_gamma, "gamma");
        let NodeReanchor::Retained {
            node: stable_node,
            key: stable_key,
        } = update.reanchor(&old_stable).unwrap()
        else {
            panic!("the exact unchanged file must retain its node");
        };
        assert_eq!(current.node(stable_node).unwrap().text(), "stable");
        assert_eq!(current.node_key(stable_node).unwrap(), &stable_key);
        assert_eq!(
            update.reanchor(&old_beta).unwrap(),
            NodeReanchor::Expired {
                reason: NodeExpiryReason::NodeChanged
            }
        );
        assert_eq!(
            update.reanchor(&old_removed).unwrap(),
            NodeReanchor::Expired {
                reason: NodeExpiryReason::FileRemoved
            }
        );
        assert_eq!(
            current.node(old_beta_id).unwrap_err(),
            crate::NodeLookupError::WrongAnalysis
        );

        assert_eq!(current.id(), clean.id());
        assert_eq!(all_node_keys(current), all_node_keys(&clean));
        for path in ["add.rs", "edit.rs", "stable.rs"] {
            let incremental_file = current.file(Path::new(path)).unwrap();
            let clean_file = clean.file(Path::new(path)).unwrap();
            assert_eq!(incremental_file.source(), clean_file.source());
            assert_eq!(incremental_file.provenance(), clean_file.provenance());
            assert_eq!(incremental_file.arena, clean_file.arena);
        }
        let query = current
            .compile_syntax_query(Path::new("edit.rs"), "_ @node")
            .unwrap();
        let clean_query = clean
            .compile_syntax_query(Path::new("edit.rs"), "_ @node")
            .unwrap();
        let capture_keys = |analysis: &ProjectAnalysis, query: &crate::SyntaxQuery| {
            analysis
                .syntax_query_captures(
                    query,
                    analysis
                        .file_node_ids(Path::new("edit.rs"))
                        .unwrap()
                        .next()
                        .unwrap(),
                )
                .unwrap()
                .into_iter()
                .map(|capture| analysis.node_key(capture.node()).unwrap().clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            capture_keys(current, &query),
            capture_keys(&clean, &clean_query)
        );
    }

    #[test]
    fn derived_diff_never_authorizes_identity_when_duplicate_history_is_ambiguous() {
        let old = "fn same() {}\n";
        let appended = format!("{old}{old}");
        let previous = ProjectAnalysis::build(snapshot(
            repository("duplicate-history-repository"),
            &[("same.rs", old.as_bytes())],
            None,
        ))
        .unwrap();
        let previous_keys = all_node_keys(&previous);

        let derived = previous
            .successor(snapshot(
                repository("duplicate-history-repository"),
                &[("same.rs", appended.as_bytes())],
                None,
            ))
            .unwrap();
        assert_eq!(
            derived.changes()[0].source_edit_evidence(),
            Some(SourceEditEvidence::DerivedDiff)
        );
        assert_eq!(
            derived.instrumentation().derived_diff_bytes_upper_bound,
            old.len() + appended.len()
        );
        assert!(previous_keys.iter().all(|key| {
            derived.reanchor(key).unwrap()
                == NodeReanchor::Expired {
                    reason: NodeExpiryReason::NodeChanged,
                }
        }));

        let append_script = [FileSourceEdits::new(
            "same.rs",
            vec![SourceReplacement::new(old.len()..old.len(), old)],
        )];
        let exact_append = previous
            .successor_with_edits(
                snapshot(
                    repository("duplicate-history-repository"),
                    &[("same.rs", appended.as_bytes())],
                    None,
                ),
                &append_script,
            )
            .unwrap();
        assert_eq!(
            exact_append
                .instrumentation()
                .sequential_edit_validation_bytes_upper_bound,
            old.len() + appended.len()
        );
        let append_ranges = previous_keys
            .iter()
            .filter_map(|key| match exact_append.reanchor(key).unwrap() {
                NodeReanchor::Reanchored {
                    node,
                    evidence: NodeReanchorEvidence::TreeSitterReusedSubtree,
                    ..
                } => Some(
                    exact_append
                        .current()
                        .node(node)
                        .unwrap()
                        .span()
                        .byte_range(),
                ),
                NodeReanchor::Expired { .. } => None,
                NodeReanchor::Retained { .. } => unreachable!("the revision changed"),
            })
            .collect::<Vec<_>>();
        assert!(!append_ranges.is_empty());
        assert!(append_ranges.iter().all(|range| range.end <= old.len()));

        let prepend_script = [FileSourceEdits::new(
            "same.rs",
            vec![SourceReplacement::new(0..0, old)],
        )];
        let exact_prepend = previous
            .successor_with_edits(
                snapshot(
                    repository("duplicate-history-repository"),
                    &[("same.rs", appended.as_bytes())],
                    None,
                ),
                &prepend_script,
            )
            .unwrap();
        let prepend_ranges = previous_keys
            .iter()
            .filter_map(|key| match exact_prepend.reanchor(key).unwrap() {
                NodeReanchor::Reanchored {
                    node,
                    evidence: NodeReanchorEvidence::TreeSitterReusedSubtree,
                    ..
                } => Some(
                    exact_prepend
                        .current()
                        .node(node)
                        .unwrap()
                        .span()
                        .byte_range(),
                ),
                NodeReanchor::Expired { .. } => None,
                NodeReanchor::Retained { .. } => unreachable!("the revision changed"),
            })
            .collect::<Vec<_>>();
        assert!(!prepend_ranges.is_empty());
        assert!(prepend_ranges.iter().all(|range| range.start >= old.len()));
    }

    #[test]
    fn exact_multi_edit_history_uses_sequential_coordinates_and_preserves_middle_subtrees() {
        let old = "fn alpha() { one(); }\nfn beta() { two(); }\nfn gamma() { three(); }\n";
        let mut intermediate = old.to_string();
        let first_start = intermediate.find("one").unwrap();
        intermediate.replace_range(first_start..first_start + 3, "first");
        let second_start = intermediate.find("three").unwrap();
        let mut final_source = intermediate.clone();
        final_source.replace_range(second_start..second_start + 5, "third");
        let previous = ProjectAnalysis::build(snapshot(
            repository("multi-edit-repository"),
            &[("multi.rs", old.as_bytes())],
            None,
        ))
        .unwrap();
        let beta = node_key_by_text(&previous, "multi.rs", "beta");
        let history = [FileSourceEdits::new(
            "multi.rs",
            vec![
                SourceReplacement::new(first_start..first_start + 3, "first"),
                SourceReplacement::new(second_start..second_start + 5, "third"),
            ],
        )];
        let update = previous
            .successor_with_edits(
                snapshot(
                    repository("multi-edit-repository"),
                    &[("multi.rs", final_source.as_bytes())],
                    None,
                ),
                &history,
            )
            .unwrap();
        let change = &update.changes()[0];
        assert_eq!(
            change.source_edit_evidence(),
            Some(SourceEditEvidence::ExactScript)
        );
        assert_eq!(change.source_edits().len(), 2);
        assert_eq!(
            change.source_edits()[0].old_range(),
            first_start..first_start + 3
        );
        assert_eq!(
            change.source_edits()[1].old_range(),
            second_start..second_start + 5
        );
        let NodeReanchor::Reanchored {
            node,
            evidence: NodeReanchorEvidence::TreeSitterReusedSubtree,
            ..
        } = update.reanchor(&beta).unwrap()
        else {
            panic!("the exact untouched middle declaration must re-anchor");
        };
        assert_eq!(update.current().node(node).unwrap().text(), "beta");
    }

    #[test]
    fn exact_edit_history_rejects_malformed_or_non_reconstructing_scripts_before_parse() {
        let old = "fn old() {}\n";
        let new = "fn new() {}\n";
        let previous = ProjectAnalysis::build(snapshot(
            repository("edit-validation-repository"),
            &[("same.rs", old.as_bytes())],
            None,
        ))
        .unwrap();
        let make_current = || {
            snapshot(
                repository("edit-validation-repository"),
                &[("same.rs", new.as_bytes())],
                None,
            )
        };
        let valid = FileSourceEdits::new("same.rs", vec![SourceReplacement::new(3..6, "new")]);
        assert!(matches!(
            previous.successor_with_edits(make_current(), &[valid.clone(), valid]),
            Err(ProjectAnalysisUpdateError::DuplicateEditPath { .. })
        ));
        assert!(matches!(
            previous.successor_with_edits(
                make_current(),
                &[FileSourceEdits::new(
                    "absent.rs",
                    vec![SourceReplacement::new(0..0, "x")]
                )]
            ),
            Err(ProjectAnalysisUpdateError::UnexpectedEditPath { .. })
        ));
        let reversed_start = 8;
        let reversed_end = 3;
        for replacement in [
            SourceReplacement::new(reversed_start..reversed_end, "new"),
            SourceReplacement::new(3..usize::MAX, "new"),
        ] {
            assert!(matches!(
                previous.successor_with_edits(
                    make_current(),
                    &[FileSourceEdits::new("same.rs", vec![replacement])]
                ),
                Err(ProjectAnalysisUpdateError::InvalidEdit { edit_index: 0, .. })
            ));
        }
        assert!(matches!(
            previous.successor_with_edits(
                make_current(),
                &[FileSourceEdits::new("same.rs", Vec::new())]
            ),
            Err(ProjectAnalysisUpdateError::EditScriptMismatch { .. })
        ));

        let unicode_old = "fn é() {}\n";
        let unicode_previous = ProjectAnalysis::build(snapshot(
            repository("utf8-edit-validation-repository"),
            &[("same.rs", unicode_old.as_bytes())],
            None,
        ))
        .unwrap();
        assert!(matches!(
            unicode_previous.successor_with_edits(
                snapshot(
                    repository("utf8-edit-validation-repository"),
                    &[("same.rs", b"fn e() {}\n")],
                    None,
                ),
                &[FileSourceEdits::new(
                    "same.rs",
                    vec![SourceReplacement::new(4..5, "e")]
                )]
            ),
            Err(ProjectAnalysisUpdateError::InvalidEdit { edit_index: 0, .. })
        ));
    }

    #[test]
    fn source_and_structural_change_evidence_have_pinned_numeric_semantics() {
        let old = "fn alpha() { one(); }\nfn beta() { two(); }\nfn gamma() { three(); }\n";
        let new = "fn alpha() { one(); }\nfn beta() { second(); }\nfn gamma() { seven(); }\n";
        let peer = "fn peer() {}\n";
        assert_eq!((old.len(), new.len(), peer.len()), (67, 70, 13));
        let previous = ProjectAnalysis::build(snapshot(
            repository("numeric-change-repository"),
            &[("edit.rs", old.as_bytes()), ("peer.rs", peer.as_bytes())],
            None,
        ))
        .unwrap();
        let edit_keys = previous
            .file_node_ids(Path::new("edit.rs"))
            .unwrap()
            .map(|id| previous.node_key(id).unwrap().clone())
            .collect::<Vec<_>>();
        let peer_keys = previous
            .file_node_ids(Path::new("peer.rs"))
            .unwrap()
            .map(|id| previous.node_key(id).unwrap().clone())
            .collect::<Vec<_>>();
        assert_eq!((edit_keys.len(), peer_keys.len()), (49, 10));

        let make_current = || {
            snapshot(
                repository("numeric-change-repository"),
                &[("edit.rs", new.as_bytes()), ("peer.rs", peer.as_bytes())],
                None,
            )
        };
        let derived = previous.successor(make_current()).unwrap();
        let derived_change = derived
            .changes()
            .iter()
            .find(|change| change.path() == Path::new("edit.rs"))
            .unwrap();
        assert_eq!(
            derived_change.source_edit_evidence(),
            Some(SourceEditEvidence::DerivedDiff)
        );
        let canonical = derived_change.source_invalidation_edit().unwrap();
        assert_eq!(
            (canonical.old_range(), canonical.new_range()),
            (34..61, 34..64)
        );
        assert_eq!(
            (
                canonical.start_point(),
                canonical.old_end_point(),
                canonical.new_end_point()
            ),
            (
                SourcePoint::new(1, 12),
                SourcePoint::new(2, 18),
                SourcePoint::new(2, 18)
            )
        );
        assert_eq!(derived_change.syntax_changed_ranges().len(), 1);
        let structural = derived_change.syntax_changed_ranges()[0];
        assert_eq!(structural.byte_range(), 40..64);
        assert_eq!(
            (structural.start_point(), structural.end_point()),
            (SourcePoint::new(1, 18), SourcePoint::new(2, 18))
        );
        assert_eq!(transition_counts(&derived, &edit_keys), (0, 0, 49));
        assert_eq!(transition_counts(&derived, &peer_keys), (10, 0, 0));

        let exact = previous
            .successor_with_edits(
                make_current(),
                &[FileSourceEdits::new(
                    "edit.rs",
                    vec![
                        SourceReplacement::new(34..37, "second"),
                        SourceReplacement::new(59..64, "seven"),
                    ],
                )],
            )
            .unwrap();
        let exact_change = exact
            .changes()
            .iter()
            .find(|change| change.path() == Path::new("edit.rs"))
            .unwrap();
        assert_eq!(
            exact_change.source_edit_evidence(),
            Some(SourceEditEvidence::ExactScript)
        );
        assert_eq!(
            exact_change
                .source_edits()
                .iter()
                .map(|edit| (edit.old_range(), edit.new_range()))
                .collect::<Vec<_>>(),
            vec![(34..37, 34..40), (59..64, 59..64)]
        );
        assert_eq!(
            (
                exact_change.source_invalidation_edit().unwrap().old_range(),
                exact_change.source_invalidation_edit().unwrap().new_range()
            ),
            (34..61, 34..64)
        );
        assert!(exact_change.syntax_changed_ranges().is_empty());
        assert_eq!(transition_counts(&exact, &edit_keys), (0, 24, 25));
        assert_eq!(transition_counts(&exact, &peer_keys), (10, 0, 0));
        for update in [&derived, &exact] {
            let counts = update.current().parse_counts();
            let edit = counts
                .iter()
                .find(|(key, _)| key.path == Path::new("edit.rs"))
                .unwrap()
                .1;
            let peer = counts
                .iter()
                .find(|(key, _)| key.path == Path::new("peer.rs"))
                .unwrap()
                .1;
            assert_eq!(
                *edit,
                FileParseCount {
                    requested: 1,
                    owners: 1,
                    parser_invocations: 1,
                    reused: 0,
                }
            );
            assert_eq!(
                *peer,
                FileParseCount {
                    requested: 1,
                    owners: 1,
                    parser_invocations: 0,
                    reused: 1,
                }
            );
        }
    }

    #[test]
    fn partial_repair_and_empty_file_transitions_expire_changed_zero_width_nodes() {
        let partial = "function f(a: string { return a; }\n";
        let complete = "function f(a: string) { return a; }\n";
        assert_eq!((partial.len(), complete.len()), (35, 36));
        let previous = ProjectAnalysis::build(snapshot(
            repository("partial-transition-repository"),
            &[("partial.ts", partial.as_bytes())],
            None,
        ))
        .unwrap();
        let previous_keys = all_node_keys(&previous);
        assert_eq!(previous_keys.len(), 20);
        assert!(
            previous
                .node_ids()
                .map(|id| previous.node(id).unwrap())
                .any(|node| node.is_missing() || node.is_error())
        );
        let update = previous
            .successor_with_edits(
                snapshot(
                    repository("partial-transition-repository"),
                    &[("partial.ts", complete.as_bytes())],
                    None,
                ),
                &[FileSourceEdits::new(
                    "partial.ts",
                    vec![SourceReplacement::new(20..20, ")")],
                )],
            )
            .unwrap();
        assert_eq!(
            update.changes()[0].syntax_changed_ranges()[0].byte_range(),
            20..21
        );
        assert_eq!(transition_counts(&update, &previous_keys), (0, 7, 13));
        let clean = ProjectAnalysis::build(Arc::clone(update.current().snapshot())).unwrap();
        assert_eq!(all_node_keys(update.current()), all_node_keys(&clean));
        assert_eq!(
            update
                .current()
                .file(Path::new("partial.ts"))
                .unwrap()
                .arena,
            clean.file(Path::new("partial.ts")).unwrap().arena
        );

        let empty_previous = ProjectAnalysis::build(snapshot(
            repository("empty-transition-repository"),
            &[("empty.rs", b"")],
            None,
        ))
        .unwrap();
        let empty_keys = all_node_keys(&empty_previous);
        assert_eq!(empty_keys.len(), 1);
        let filled = "fn alpha() { one(); }\n";
        let empty_update = empty_previous
            .successor_with_edits(
                snapshot(
                    repository("empty-transition-repository"),
                    &[("empty.rs", filled.as_bytes())],
                    None,
                ),
                &[FileSourceEdits::new(
                    "empty.rs",
                    vec![SourceReplacement::new(0..0, filled)],
                )],
            )
            .unwrap();
        assert_eq!(empty_update.current().node_count(), 17);
        assert_eq!(
            empty_update.changes()[0].syntax_changed_ranges()[0].byte_range(),
            0..22
        );
        assert_eq!(transition_counts(&empty_update, &empty_keys), (0, 0, 1));
    }

    #[test]
    fn invalid_utf8_and_path_lifecycle_have_explicit_rebuild_and_expiry_semantics() {
        let valid = b"fn alpha() { one(); }\n";
        let previous = ProjectAnalysis::build(snapshot(
            repository("invalid-transition-repository"),
            &[("same.rs", valid)],
            None,
        ))
        .unwrap();
        let previous_keys = all_node_keys(&previous);
        let invalid_snapshot = snapshot(
            repository("invalid-transition-repository"),
            &[("same.rs", &[0xff, 0xfe])],
            None,
        );
        let invalid = previous.successor(invalid_snapshot).unwrap();
        assert_eq!(invalid.changes()[0].kind(), FileAnalysisChangeKind::Rebuilt);
        assert_eq!(
            invalid.changes()[0].rebuild_reason(),
            Some(FileRebuildReason::SyntaxUnavailable)
        );
        assert!(invalid.changes()[0].source_invalidation_edit().is_none());
        assert_eq!(invalid.current().node_count(), 0);
        assert!(previous_keys.iter().all(|key| {
            invalid.reanchor(key).unwrap()
                == NodeReanchor::Expired {
                    reason: NodeExpiryReason::SyntaxUnavailable,
                }
        }));
        assert_eq!(
            *invalid.current().parse_counts().values().next().unwrap(),
            FileParseCount {
                requested: 1,
                owners: 1,
                parser_invocations: 0,
                reused: 0,
            }
        );
        let invalid_no_op = invalid
            .current()
            .successor(Arc::clone(invalid.current().snapshot()))
            .unwrap();
        assert_eq!(
            invalid_no_op.changes()[0].kind(),
            FileAnalysisChangeKind::Reused
        );
        assert_eq!(
            *invalid_no_op
                .current()
                .parse_counts()
                .values()
                .next()
                .unwrap(),
            FileParseCount {
                requested: 1,
                owners: 1,
                parser_invocations: 0,
                reused: 1,
            }
        );
        let recovered = invalid
            .current()
            .successor(snapshot(
                repository("invalid-transition-repository"),
                &[("same.rs", valid)],
                None,
            ))
            .unwrap();
        assert_eq!(
            recovered.changes()[0].rebuild_reason(),
            Some(FileRebuildReason::SyntaxUnavailable)
        );
        assert_eq!(recovered.current().node_count(), 17);
        assert_eq!(
            recovered
                .current()
                .parse_counts()
                .values()
                .next()
                .unwrap()
                .parser_invocations,
            1
        );

        let renamed_previous = ProjectAnalysis::build(snapshot(
            repository("path-lifecycle-repository"),
            &[("old.rs", valid)],
            None,
        ))
        .unwrap();
        let renamed_keys = all_node_keys(&renamed_previous);
        let renamed = renamed_previous
            .successor(snapshot(
                repository("path-lifecycle-repository"),
                &[("new.rs", valid)],
                None,
            ))
            .unwrap();
        assert_eq!(
            renamed
                .changes()
                .iter()
                .map(|change| (change.path(), change.kind()))
                .collect::<Vec<_>>(),
            vec![
                (Path::new("new.rs"), FileAnalysisChangeKind::Added),
                (Path::new("old.rs"), FileAnalysisChangeKind::Removed),
            ]
        );
        assert!(renamed_keys.iter().all(|key| {
            renamed.reanchor(key).unwrap()
                == NodeReanchor::Expired {
                    reason: NodeExpiryReason::FileRemoved,
                }
        }));
    }

    #[test]
    fn derived_edits_are_utf8_safe_minimal_and_tree_sitter_bounded() {
        let edit = derive_source_edit("fn é() {}", "fn è() {}").unwrap();
        assert_eq!(edit.old_range(), 3..5);
        assert_eq!(edit.new_range(), 3..5);
        assert_eq!(edit.start_point(), SourcePoint::new(0, 3));
        assert_eq!(edit.old_end_point(), SourcePoint::new(0, 5));
        assert_eq!(edit.new_end_point(), SourcePoint::new(0, 5));

        let old = "fn value() { \u{80}(); }";
        let new = "fn value() { \u{1080}(); }";
        let edit = derive_source_edit(old, new).unwrap();
        assert!(old.is_char_boundary(edit.old_range().start));
        assert!(old.is_char_boundary(edit.old_range().end));
        assert!(new.is_char_boundary(edit.new_range().start));
        assert!(new.is_char_boundary(edit.new_range().end));
        let mut reconstructed = Vec::new();
        reconstructed.extend_from_slice(&old.as_bytes()[..edit.old_range().start]);
        reconstructed.extend_from_slice(&new.as_bytes()[edit.new_range()]);
        reconstructed.extend_from_slice(&old.as_bytes()[edit.old_range().end..]);
        assert_eq!(reconstructed, new.as_bytes());

        #[cfg(target_pointer_width = "64")]
        assert!(validate_tree_sitter_size(u32::MAX as usize + 1).is_err());
    }

    #[test]
    fn no_op_successor_reanchors_every_key_with_fresh_node_ids() {
        let snapshot = snapshot(
            repository("no-op-repository"),
            &[("same.rs", b"fn same() { value(); }\n")],
            None,
        );
        let previous = ProjectAnalysis::build(Arc::clone(&snapshot)).unwrap();
        let previous_ids = previous.node_ids().collect::<Vec<_>>();
        let previous_keys = all_node_keys(&previous);
        let update = previous.successor(snapshot).unwrap();
        assert_eq!(update.changes().len(), 1);
        assert_eq!(update.changes()[0].kind(), FileAnalysisChangeKind::Reused);
        for (id, key) in previous_ids.into_iter().zip(previous_keys) {
            let NodeReanchor::Retained {
                node: current_id,
                key: current_key,
            } = update.reanchor(&key).unwrap()
            else {
                panic!("unchanged key must re-anchor");
            };
            assert_ne!(id, current_id);
            assert_eq!(key, current_key);
        }
    }

    #[test]
    fn successor_rejects_repository_mismatch_before_reuse() {
        let previous = ProjectAnalysis::build(snapshot(
            repository("previous-repository"),
            &[("same.rs", b"fn same() {}\n")],
            None,
        ))
        .unwrap();
        let current = snapshot(
            repository("current-repository"),
            &[("same.rs", b"fn same() {}\n")],
            None,
        );
        assert_eq!(
            previous.successor(current).unwrap_err(),
            ProjectAnalysisUpdateError::RepositoryMismatch {
                previous: repository("previous-repository"),
                current: repository("current-repository"),
            }
        );
    }
}
