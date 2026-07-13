use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::hash::Hash;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result, bail};
use deslop_core::{AnalysisDiagnostic, AnalysisProvenance, Lang};
use deslop_lang::Registry;
use ignore::WalkBuilder;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tree_sitter::{Parser, Tree};

use crate::analysis_provenance_for_tree;
use crate::arena::{
    ArenaNodeIndex, ArenaSegmentIndex, RAW_ARENA_SCHEMA, SyntaxArena, SyntaxSegmentKind,
    SyntaxSegmentOwner,
};
use crate::identity::{
    NodeBaselineFingerprint, NodeId, NodeKey, NodeKeyLookupError, NodeLookupError,
    baseline_fingerprint, build_node_keys,
};

const SOURCE_REVISION_DOMAIN: &str = "deslop source revision v1";
const SNAPSHOT_ID_DOMAIN: &str = "deslop project snapshot v1";
const ANALYSIS_ID_DOMAIN: &str = "deslop project analysis v1";
const LOCAL_REPOSITORY_DOMAIN: &str = "deslop local repository v1";
const GRAMMAR_SELECTOR: &str = "deslop-grammar-selector/1";
const PARSER_BUILD: &str = concat!(
    "deslop-parse/",
    env!("CARGO_PKG_VERSION"),
    "+tree-sitter/0.25.10"
);
static NEXT_ANALYSIS_OWNER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceRevision(String);

impl SourceRevision {
    pub fn for_bytes(bytes: &[u8]) -> Self {
        Self(format!(
            "sr1_{}",
            domain_digest(SOURCE_REVISION_DOMAIN, [bytes])
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepositoryId(String);

impl RepositoryId {
    pub fn explicit(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            bail!("repository identity must not be empty");
        }
        Ok(Self(value))
    }

    pub fn local(root: &Path) -> Result<Self> {
        let root = root
            .canonicalize()
            .with_context(|| format!("failed to resolve repository root {}", root.display()))?;
        if root.to_str().is_none() {
            bail!("repository root is not valid Unicode");
        }
        let digest = domain_digest(
            LOCAL_REPOSITORY_DOMAIN,
            [root.to_str().expect("validated Unicode root").as_bytes()],
        );
        Ok(Self(format!("repo1_{digest}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrammarSelection {
    lang: Lang,
    dialect: String,
    selector: String,
    grammar_id: String,
    grammar_version: String,
    parser_build: String,
}

impl PartialOrd for GrammarSelection {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GrammarSelection {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        grammar_lang_key(self.lang)
            .cmp(&grammar_lang_key(other.lang))
            .then(self.dialect.cmp(&other.dialect))
            .then(self.selector.cmp(&other.selector))
            .then(self.grammar_id.cmp(&other.grammar_id))
            .then(self.grammar_version.cmp(&other.grammar_version))
            .then(self.parser_build.cmp(&other.parser_build))
    }
}

impl Hash for GrammarSelection {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        grammar_lang_key(self.lang).hash(state);
        self.dialect.hash(state);
        self.selector.hash(state);
        self.grammar_id.hash(state);
        self.grammar_version.hash(state);
        self.parser_build.hash(state);
    }
}

impl GrammarSelection {
    fn from_descriptor(descriptor: deslop_lang::GrammarDescriptor) -> Self {
        Self {
            lang: descriptor.lang(),
            dialect: descriptor.dialect().to_string(),
            selector: GRAMMAR_SELECTOR.to_string(),
            grammar_id: descriptor.grammar_id().to_string(),
            grammar_version: descriptor.grammar_version().to_string(),
            parser_build: PARSER_BUILD.to_string(),
        }
    }

    fn identity_bytes(&self) -> Vec<u8> {
        format!(
            "{:?}\0{}\0{}\0{}\0{}\0{}",
            self.lang,
            self.dialect,
            self.selector,
            self.grammar_id,
            self.grammar_version,
            self.parser_build
        )
        .into_bytes()
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    pub fn dialect(&self) -> &str {
        &self.dialect
    }

    pub fn selector(&self) -> &str {
        &self.selector
    }

    pub fn grammar_id(&self) -> &str {
        &self.grammar_id
    }

    pub fn grammar_version(&self) -> &str {
        &self.grammar_version
    }

    pub fn parser_build(&self) -> &str {
        &self.parser_build
    }
}

fn resolve_grammar(path: &Path) -> Result<(GrammarSelection, tree_sitter::Language)> {
    let registry = Registry::default();
    let resolved = registry
        .resolve_grammar(path)
        .ok_or_else(|| anyhow::anyhow!("no grammar artifact for {}", path.display()))?;
    let (descriptor, language) = resolved.into_parts();
    Ok((GrammarSelection::from_descriptor(descriptor), language))
}

fn grammar_lang_key(lang: Lang) -> u8 {
    match lang {
        Lang::Clojure => 0,
        Lang::Julia => 1,
        Lang::Python => 2,
        Lang::JavaScript => 3,
        Lang::TypeScript => 4,
        Lang::Rust => 5,
        Lang::Generic => 6,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileRevisionKey {
    pub repository: RepositoryId,
    pub path: PathBuf,
    pub source: SourceRevision,
    pub grammar: GrammarSelection,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRevisionKeyWire {
    repository: RepositoryId,
    path: String,
    source: SourceRevision,
    grammar: GrammarSelection,
}

impl Serialize for FileRevisionKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        FileRevisionKeyWire {
            repository: self.repository.clone(),
            path: encode_wire_repo_path(&self.path).map_err(serde::ser::Error::custom)?,
            source: self.source.clone(),
            grammar: self.grammar.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FileRevisionKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FileRevisionKeyWire::deserialize(deserializer)?;
        if wire.repository.as_str().trim().is_empty() {
            return Err(D::Error::custom(
                "file revision repository identity is empty",
            ));
        }
        if !is_lower_prefixed_hex(wire.source.as_str(), "sr1_") {
            return Err(D::Error::custom(
                "file revision source must be lowercase sr1_ plus 64 hex digits",
            ));
        }
        if wire.grammar.dialect().is_empty()
            || wire.grammar.selector().is_empty()
            || wire.grammar.grammar_id().is_empty()
            || wire.grammar.grammar_version().is_empty()
            || wire.grammar.parser_build().is_empty()
        {
            return Err(D::Error::custom(
                "file revision grammar contains an empty identity field",
            ));
        }
        Ok(Self {
            repository: wire.repository,
            path: decode_wire_repo_path(&wire.path).map_err(D::Error::custom)?,
            source: wire.source,
            grammar: wire.grammar,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SnapshotEntryKind {
    Source,
    AnalysisInput,
}

#[derive(Debug, Clone)]
pub struct SnapshotEntry {
    path: PathBuf,
    source: Arc<StoredSource>,
    analysis: EntryAnalysis,
}

#[derive(Debug, Clone)]
enum EntryAnalysis {
    Source {
        selection: GrammarSelection,
        language: tree_sitter::Language,
    },
    AnalysisInput,
}

impl SnapshotEntry {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn revision(&self) -> &SourceRevision {
        self.source.revision()
    }

    pub fn bytes(&self) -> &[u8] {
        self.source.bytes()
    }

    pub fn kind(&self) -> SnapshotEntryKind {
        match self.analysis {
            EntryAnalysis::Source { .. } => SnapshotEntryKind::Source,
            EntryAnalysis::AnalysisInput => SnapshotEntryKind::AnalysisInput,
        }
    }

    pub fn grammar(&self) -> Option<&GrammarSelection> {
        match &self.analysis {
            EntryAnalysis::Source { selection, .. } => Some(selection),
            EntryAnalysis::AnalysisInput => None,
        }
    }

    fn grammar_language(&self) -> Option<&tree_sitter::Language> {
        match &self.analysis {
            EntryAnalysis::Source { language, .. } => Some(language),
            EntryAnalysis::AnalysisInput => None,
        }
    }
}

#[derive(Debug)]
pub struct StoredSource {
    revision: SourceRevision,
    bytes: Arc<[u8]>,
}

impl StoredSource {
    pub fn revision(&self) -> &SourceRevision {
        &self.revision
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Debug, Default)]
pub struct SourceStore {
    contents: RwLock<BTreeMap<SourceRevision, Arc<StoredSource>>>,
}

impl SourceStore {
    pub fn intern(&self, bytes: impl Into<Vec<u8>>) -> Arc<StoredSource> {
        let bytes = bytes.into();
        let revision = SourceRevision::for_bytes(&bytes);
        let mut contents = self
            .contents
            .write()
            .unwrap_or_else(|poison| poison.into_inner());
        contents
            .entry(revision.clone())
            .or_insert_with(|| {
                Arc::new(StoredSource {
                    revision,
                    bytes: Arc::<[u8]>::from(bytes),
                })
            })
            .clone()
    }

    pub fn get(&self, revision: &SourceRevision) -> Option<Arc<StoredSource>> {
        self.contents
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(revision)
            .cloned()
    }

    pub fn len(&self) -> usize {
        self.contents
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.contents
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectSnapshotId(String);

impl ProjectSnapshotId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
pub struct ProjectSnapshot {
    id: ProjectSnapshotId,
    repository: RepositoryId,
    root: PathBuf,
    requested_scope: Vec<ScopeEntry>,
    entries: BTreeMap<PathBuf, SnapshotEntry>,
    store: Arc<SourceStore>,
    read_counts: BTreeMap<PathBuf, usize>,
}

impl ProjectSnapshot {
    pub fn id(&self) -> &ProjectSnapshotId {
        &self.id
    }

    pub fn repository(&self) -> &RepositoryId {
        &self.repository
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn requested_scope(&self) -> &[ScopeEntry] {
        &self.requested_scope
    }

    pub fn entries(&self) -> impl Iterator<Item = &SnapshotEntry> {
        self.entries.values()
    }

    pub fn entry(&self, path: &Path) -> Option<&SnapshotEntry> {
        self.entries.get(path)
    }

    pub fn store(&self) -> &Arc<SourceStore> {
        &self.store
    }

    pub fn read_counts(&self) -> &BTreeMap<PathBuf, usize> {
        &self.read_counts
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopeEntry {
    pub path: PathBuf,
    pub kind: ScopeEntryKind,
}

#[derive(Debug, Clone)]
pub enum ScopeSpec {
    DefaultAtInvocationBase,
    Requested(Vec<PathBuf>),
    ExactFiles(Vec<PathBuf>),
}

pub struct ProjectSnapshotBuilder {
    root: PathBuf,
    invocation_base: PathBuf,
    repository: RepositoryId,
    requested_scope: ScopeSpec,
    overlays: BTreeMap<PathBuf, Vec<u8>>,
    analysis_inputs: BTreeMap<PathBuf, Vec<u8>>,
    store: Arc<SourceStore>,
}

impl ProjectSnapshotBuilder {
    pub fn new(root: impl AsRef<Path>, repository: RepositoryId) -> Result<Self> {
        let root = root.as_ref().canonicalize().with_context(|| {
            format!(
                "failed to resolve repository root {}",
                root.as_ref().display()
            )
        })?;
        if !root.is_dir() {
            bail!("repository root {} is not a directory", root.display());
        }
        Ok(Self {
            invocation_base: root.clone(),
            root,
            repository,
            requested_scope: ScopeSpec::DefaultAtInvocationBase,
            overlays: BTreeMap::new(),
            analysis_inputs: BTreeMap::new(),
            store: Arc::new(SourceStore::default()),
        })
    }

    pub fn with_scope(mut self, scope: &[PathBuf]) -> Self {
        self.requested_scope = ScopeSpec::Requested(scope.to_vec());
        self
    }

    pub fn with_exact_files(mut self, files: &[PathBuf]) -> Self {
        self.requested_scope = ScopeSpec::ExactFiles(files.to_vec());
        self
    }

    pub fn with_scope_spec(mut self, scope: ScopeSpec) -> Self {
        self.requested_scope = scope;
        self
    }

    pub fn with_invocation_base(mut self, base: impl AsRef<Path>) -> Result<Self> {
        self.invocation_base = base.as_ref().canonicalize().with_context(|| {
            format!(
                "failed to resolve invocation base {}",
                base.as_ref().display()
            )
        })?;
        Ok(self)
    }

    pub fn with_store(mut self, store: Arc<SourceStore>) -> Self {
        self.store = store;
        self
    }

    pub fn with_overlay(
        mut self,
        path: impl AsRef<Path>,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<Self> {
        let path = normalize_builder_input_path(&self.root, path.as_ref())?;
        let bytes = bytes.into();
        if let Some(existing) = self.overlays.get(&path) {
            if existing != &bytes {
                bail!("snapshot overlay {} has conflicting bytes", path.display());
            }
            return Ok(self);
        }
        self.overlays.insert(path, bytes);
        Ok(self)
    }

    pub fn with_analysis_input(
        mut self,
        path: impl AsRef<Path>,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<Self> {
        let path = normalize_builder_input_path(&self.root, path.as_ref())?;
        let bytes = bytes.into();
        if let Some(existing) = self.analysis_inputs.get(&path) {
            if existing != &bytes {
                bail!(
                    "snapshot analysis input {} has conflicting bytes",
                    path.display()
                );
            }
            return Ok(self);
        }
        self.analysis_inputs.insert(path, bytes);
        Ok(self)
    }

    pub fn build(self) -> Result<Arc<ProjectSnapshot>> {
        let (scope, exact_files) = match &self.requested_scope {
            ScopeSpec::DefaultAtInvocationBase => (vec![PathBuf::from(".")], false),
            ScopeSpec::Requested(scope) if scope.is_empty() => bail!(
                "requested scope must contain at least one path; use ExactFiles for an exact empty set"
            ),
            ScopeSpec::Requested(scope) => (scope.clone(), false),
            ScopeSpec::ExactFiles(scope) => (scope.clone(), true),
        };
        let requested_scope = normalize_scope(&self.root, &self.invocation_base, &scope)?;
        if exact_files
            && requested_scope
                .iter()
                .any(|entry| entry.kind != ScopeEntryKind::File)
        {
            bail!("exact file scope contains a directory");
        }
        let disk_sources = collect_disk_sources(&self.root, &requested_scope)?;
        let mut inputs = BTreeMap::<PathBuf, (SnapshotEntryKind, Vec<u8>)>::new();
        let mut read_counts = BTreeMap::<PathBuf, usize>::new();
        for (logical, physical) in disk_sources {
            if self.overlays.contains_key(&logical) {
                continue;
            }
            let bytes = std::fs::read(&physical)
                .with_context(|| format!("failed to read {}", physical.display()))?;
            *read_counts.entry(logical.clone()).or_default() += 1;
            inputs.insert(logical, (SnapshotEntryKind::Source, bytes));
        }
        for (path, bytes) in self.overlays {
            if !crate::is_supported_source(&path) {
                bail!("overlay {} is not a supported source", path.display());
            }
            if let Some((kind, _)) = inputs.insert(path.clone(), (SnapshotEntryKind::Source, bytes))
                && kind != SnapshotEntryKind::Source
            {
                bail!(
                    "snapshot entry {} has conflicting input kinds",
                    path.display()
                );
            }
        }
        for (path, bytes) in self.analysis_inputs {
            if inputs.contains_key(&path) {
                bail!(
                    "snapshot entry {} has conflicting input kinds",
                    path.display()
                );
            }
            inputs.insert(path, (SnapshotEntryKind::AnalysisInput, bytes));
        }

        let mut entries = BTreeMap::new();
        for (path, (kind, bytes)) in inputs {
            let analysis = if kind == SnapshotEntryKind::Source {
                let (selection, language) = resolve_grammar(&path)?;
                EntryAnalysis::Source {
                    selection,
                    language,
                }
            } else {
                EntryAnalysis::AnalysisInput
            };
            let source = self.store.intern(bytes);
            entries.insert(
                path.clone(),
                SnapshotEntry {
                    path,
                    source,
                    analysis,
                },
            );
        }
        let id = snapshot_id(&self.repository, &requested_scope, &entries);
        Ok(Arc::new(ProjectSnapshot {
            id,
            repository: self.repository,
            root: self.root,
            requested_scope,
            entries,
            store: self.store,
            read_counts,
        }))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileParseCount {
    pub requested: usize,
    pub owners: usize,
    pub parser_invocations: usize,
    pub reused: usize,
}

#[derive(Debug, Default)]
pub struct ParseLedger {
    counts: Mutex<BTreeMap<FileRevisionKey, FileParseCount>>,
}

impl ParseLedger {
    fn record_requested(&self, key: &FileRevisionKey) {
        let mut counts = self
            .counts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        counts.entry(key.clone()).or_default().requested += 1;
    }

    fn record_owner(&self, key: &FileRevisionKey) {
        let mut counts = self
            .counts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        counts.entry(key.clone()).or_default().owners += 1;
    }

    fn record_invocation(&self, key: &FileRevisionKey) {
        let mut counts = self
            .counts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        counts.entry(key.clone()).or_default().parser_invocations += 1;
    }

    pub fn counts(&self) -> BTreeMap<FileRevisionKey, FileParseCount> {
        self.counts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }
}

#[derive(Debug)]
pub struct ParsedFile {
    key: FileRevisionKey,
    source: Arc<StoredSource>,
    text: Option<Arc<str>>,
    tree: Option<Tree>,
    arena: Option<SyntaxArena>,
    provenance: AnalysisProvenance,
    line_starts: Vec<usize>,
}

impl ParsedFile {
    pub fn key(&self) -> &FileRevisionKey {
        &self.key
    }

    pub fn source(&self) -> &[u8] {
        self.source.bytes()
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn grammar(&self) -> &GrammarSelection {
        &self.key.grammar
    }

    pub fn provenance(&self) -> &AnalysisProvenance {
        &self.provenance
    }

    pub fn has_tree(&self) -> bool {
        self.tree.is_some()
    }

    pub fn has_arena(&self) -> bool {
        self.arena.is_some()
    }

    #[cfg(test)]
    pub(crate) fn arena(&self) -> Option<&SyntaxArena> {
        self.arena.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn node_source(&self, index: ArenaNodeIndex) -> Option<&[u8]> {
        self.arena.as_ref()?.node_source(self.source(), index)
    }

    #[cfg(test)]
    pub(crate) fn segment_source(&self, index: ArenaSegmentIndex) -> Option<&[u8]> {
        self.arena.as_ref()?.segment_source(self.source(), index)
    }

    pub fn line_starts(&self) -> &[usize] {
        &self.line_starts
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectAnalysisId(String);

impl ProjectAnalysisId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
pub struct ProjectAnalysis {
    id: ProjectAnalysisId,
    snapshot: Arc<ProjectSnapshot>,
    files: BTreeMap<PathBuf, Arc<ParsedFile>>,
    ledger: Arc<ParseLedger>,
    owner: u64,
    node_ranges: Box<[NodeFileRange]>,
    node_keys: Box<[NodeKey]>,
}

#[derive(Debug)]
struct NodeFileRange {
    path: PathBuf,
    start: u32,
    end: u32,
}

#[derive(Debug, Clone)]
pub struct NodeIds {
    owner: u64,
    next: u32,
    end: u32,
}

impl Iterator for NodeIds {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.end {
            return None;
        }
        let id = NodeId {
            owner: self.owner,
            index: self.next,
        };
        self.next += 1;
        Some(id)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.end - self.next) as usize;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for NodeIds {}

#[derive(Debug, Clone, Copy)]
pub struct NodeView<'analysis> {
    analysis: &'analysis ProjectAnalysis,
    file: &'analysis ParsedFile,
    arena: &'analysis SyntaxArena,
    local: ArenaNodeIndex,
    id: NodeId,
}

/// The raw byte class of one smallest exclusive syntax region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExclusiveSyntaxKind {
    Token,
    Trivia,
}

/// The unique raw owner of one positive-width exclusive syntax region.
///
/// File ownership carries the exact file revision so owners cannot alias across project files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExclusiveSyntaxOwner<'analysis> {
    File(&'analysis FileRevisionKey),
    Node(NodeId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExclusiveSyntaxLookupError {
    FileNotFound { path: PathBuf },
    SyntaxUnavailable { path: PathBuf },
    ByteOutOfRange { requested: usize, source_len: usize },
}

impl fmt::Display for ExclusiveSyntaxLookupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileNotFound { path } => {
                write!(formatter, "analysis has no source file {}", path.display())
            }
            Self::SyntaxUnavailable { path } => {
                write!(
                    formatter,
                    "source file {} has no syntax arena",
                    path.display()
                )
            }
            Self::ByteOutOfRange {
                requested,
                source_len,
            } => write!(
                formatter,
                "byte {requested} is outside source byte range 0..{source_len}"
            ),
        }
    }
}

impl std::error::Error for ExclusiveSyntaxLookupError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeRangeLookupError {
    FileNotFound {
        path: PathBuf,
    },
    SyntaxUnavailable {
        path: PathBuf,
    },
    ReversedRange {
        start: usize,
        end: usize,
    },
    EmptyRangeRequiresPointLookup {
        byte: usize,
    },
    RangeOutOfBounds {
        start: usize,
        end: usize,
        source_len: usize,
    },
    PointOutOfBounds {
        byte: usize,
        source_len: usize,
    },
}

impl fmt::Display for NodeRangeLookupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileNotFound { path } => {
                write!(formatter, "analysis has no source file {}", path.display())
            }
            Self::SyntaxUnavailable { path } => {
                write!(
                    formatter,
                    "source file {} has no syntax arena",
                    path.display()
                )
            }
            Self::ReversedRange { start, end } => {
                write!(formatter, "syntax byte range {start}..{end} is reversed")
            }
            Self::EmptyRangeRequiresPointLookup { byte } => write!(
                formatter,
                "syntax byte range {byte}..{byte} is empty; use syntax_point_context"
            ),
            Self::RangeOutOfBounds {
                start,
                end,
                source_len,
            } => write!(
                formatter,
                "syntax byte range {start}..{end} is outside source range 0..{source_len}"
            ),
            Self::PointOutOfBounds { byte, source_len } => write!(
                formatter,
                "syntax point {byte} is outside source point range 0..={source_len}"
            ),
        }
    }
}

impl std::error::Error for NodeRangeLookupError {}

/// A revision-local raw syntax owner. `File` denotes bytes outside the grammar root.
#[derive(Debug, Clone, Copy)]
pub enum SyntaxOwner<'analysis> {
    File(&'analysis FileRevisionKey),
    Node(NodeView<'analysis>),
}

/// Unbiased context at a byte boundary or insertion point.
///
/// Exact zero-width nodes are the co-minimal structural nodes in grammar preorder. `before` and
/// `after` remain separate so callers cannot accidentally hide a sibling-boundary choice.
#[derive(Debug, Clone)]
pub struct SyntaxPointContext<'analysis> {
    exact_zero_width: Vec<NodeView<'analysis>>,
    before: Option<SyntaxOwner<'analysis>>,
    after: Option<SyntaxOwner<'analysis>>,
}

impl<'analysis> SyntaxPointContext<'analysis> {
    pub fn exact_zero_width(&self) -> &[NodeView<'analysis>] {
        &self.exact_zero_width
    }

    pub fn before(&self) -> Option<SyntaxOwner<'analysis>> {
        self.before
    }

    pub fn after(&self) -> Option<SyntaxOwner<'analysis>> {
        self.after
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ExclusiveSyntaxRegion<'analysis> {
    file: &'analysis ParsedFile,
    arena: &'analysis SyntaxArena,
    local: ArenaSegmentIndex,
    owner: u64,
    file_start: u32,
}

impl<'analysis> ExclusiveSyntaxRegion<'analysis> {
    fn raw(&self) -> &crate::arena::SyntaxSegment {
        self.arena
            .segment(self.local)
            .expect("exclusive syntax region belongs to its arena")
    }

    pub fn file_key(&self) -> &FileRevisionKey {
        &self.file.key
    }

    pub fn path(&self) -> &Path {
        &self.file.key.path
    }

    pub fn kind(&self) -> ExclusiveSyntaxKind {
        match self.raw().kind() {
            SyntaxSegmentKind::Token => ExclusiveSyntaxKind::Token,
            SyntaxSegmentKind::Trivia => ExclusiveSyntaxKind::Trivia,
        }
    }

    pub fn owner(&self) -> ExclusiveSyntaxOwner<'analysis> {
        match self.raw().owner() {
            SyntaxSegmentOwner::File => ExclusiveSyntaxOwner::File(&self.file.key),
            SyntaxSegmentOwner::Node(local) => ExclusiveSyntaxOwner::Node(NodeId {
                owner: self.owner,
                index: self.file_start + local.as_usize() as u32,
            }),
        }
    }

    pub fn byte_range(&self) -> Range<usize> {
        self.raw().byte_range()
    }

    pub fn bytes(&self) -> &[u8] {
        self.file
            .source()
            .get(self.byte_range())
            .expect("exclusive syntax region belongs to its exact source")
    }

    pub fn text(&self) -> &str {
        std::str::from_utf8(self.bytes())
            .expect("a syntax arena exists only for valid UTF-8 source")
    }
}

#[derive(Debug, Clone)]
pub struct ExclusiveSyntaxRegions<'analysis> {
    file: &'analysis ParsedFile,
    arena: &'analysis SyntaxArena,
    owner: u64,
    file_start: u32,
    next: usize,
}

impl<'analysis> Iterator for ExclusiveSyntaxRegions<'analysis> {
    type Item = ExclusiveSyntaxRegion<'analysis>;

    fn next(&mut self) -> Option<Self::Item> {
        let local = ArenaSegmentIndex::from_usize(self.next);
        self.arena.segment(local)?;
        self.next += 1;
        Some(ExclusiveSyntaxRegion {
            file: self.file,
            arena: self.arena,
            local,
            owner: self.owner,
            file_start: self.file_start,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.arena.segments().len() - self.next;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for ExclusiveSyntaxRegions<'_> {}

#[derive(Debug, Clone)]
pub struct NodeExclusiveSyntaxRegions<'analysis> {
    file: &'analysis ParsedFile,
    arena: &'analysis SyntaxArena,
    owner: u64,
    file_start: u32,
    remaining: std::slice::Iter<'analysis, ArenaSegmentIndex>,
}

impl<'analysis> Iterator for NodeExclusiveSyntaxRegions<'analysis> {
    type Item = ExclusiveSyntaxRegion<'analysis>;

    fn next(&mut self) -> Option<Self::Item> {
        let local = *self.remaining.next()?;
        Some(ExclusiveSyntaxRegion {
            file: self.file,
            arena: self.arena,
            local,
            owner: self.owner,
            file_start: self.file_start,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.remaining.size_hint()
    }
}

impl ExactSizeIterator for NodeExclusiveSyntaxRegions<'_> {}

impl ProjectAnalysis {
    pub fn build(snapshot: Arc<ProjectSnapshot>) -> Result<Arc<Self>> {
        Self::build_with_ledger(snapshot, Arc::new(ParseLedger::default()))
    }

    fn build_with_ledger(
        snapshot: Arc<ProjectSnapshot>,
        ledger: Arc<ParseLedger>,
    ) -> Result<Arc<Self>> {
        let mut files = BTreeMap::new();
        for entry in snapshot.entries.values() {
            if entry.kind() != SnapshotEntryKind::Source {
                continue;
            }
            let selection = entry.grammar().cloned().ok_or_else(|| {
                anyhow::anyhow!(
                    "source {} has no stored grammar selection",
                    entry.path.display()
                )
            })?;
            let key = FileRevisionKey {
                repository: snapshot.repository.clone(),
                path: entry.path.clone(),
                source: entry.revision().clone(),
                grammar: selection,
            };
            let parsed = parse_owned_file(entry, key, &ledger)?;
            files.insert(entry.path.clone(), Arc::new(parsed));
        }
        let id = analysis_id(&snapshot.id, files.values().map(|file| &file.key));
        let owner = NEXT_ANALYSIS_OWNER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| anyhow::anyhow!("project analysis owner tag space exhausted"))?;
        let mut node_ranges = Vec::new();
        let mut node_keys = Vec::new();
        for (path, file) in &files {
            let start = u32::try_from(node_keys.len())
                .map_err(|_| anyhow::anyhow!("project analysis exceeds {} nodes", u32::MAX))?;
            if let Some(arena) = &file.arena {
                node_keys.extend(build_node_keys(&file.key, arena)?);
            }
            let end = u32::try_from(node_keys.len())
                .map_err(|_| anyhow::anyhow!("project analysis exceeds {} nodes", u32::MAX))?;
            node_ranges.push(NodeFileRange {
                path: path.clone(),
                start,
                end,
            });
        }
        Ok(Arc::new(Self {
            id,
            snapshot,
            files,
            ledger,
            owner,
            node_ranges: node_ranges.into_boxed_slice(),
            node_keys: node_keys.into_boxed_slice(),
        }))
    }

    pub fn id(&self) -> &ProjectAnalysisId {
        &self.id
    }

    pub fn snapshot(&self) -> &Arc<ProjectSnapshot> {
        &self.snapshot
    }

    pub fn files(&self) -> impl Iterator<Item = &ParsedFile> {
        self.files.values().map(Arc::as_ref)
    }

    pub fn file(&self, path: &Path) -> Option<&ParsedFile> {
        self.files.get(path).map(Arc::as_ref)
    }

    pub fn parse_counts(&self) -> BTreeMap<FileRevisionKey, FileParseCount> {
        self.ledger.counts()
    }

    pub fn node_count(&self) -> usize {
        self.node_keys.len()
    }

    pub fn node_ids(&self) -> NodeIds {
        NodeIds {
            owner: self.owner,
            next: 0,
            end: self.node_keys.len() as u32,
        }
    }

    pub fn file_node_ids(&self, path: &Path) -> Option<NodeIds> {
        let range = self.node_ranges.iter().find(|range| range.path == path)?;
        Some(NodeIds {
            owner: self.owner,
            next: range.start,
            end: range.end,
        })
    }

    pub fn node(&self, id: NodeId) -> Result<NodeView<'_>, NodeLookupError> {
        if id.owner != self.owner {
            return Err(NodeLookupError::WrongAnalysis);
        }
        if id.index as usize >= self.node_keys.len() {
            return Err(NodeLookupError::OutOfRange {
                requested: id.index,
                node_count: self.node_keys.len() as u32,
            });
        }
        let range = self
            .node_ranges
            .iter()
            .find(|range| range.start <= id.index && id.index < range.end)
            .expect("every global node index belongs to one file range");
        let file = self
            .files
            .get(&range.path)
            .expect("node range path belongs to analysis file map");
        let arena = file
            .arena
            .as_ref()
            .expect("non-empty node range belongs to parsed arena");
        let local = ArenaNodeIndex::from_usize((id.index - range.start) as usize)
            .expect("global and local node indices fit u32");
        Ok(NodeView {
            analysis: self,
            file,
            arena,
            local,
            id,
        })
    }

    pub fn node_key(&self, id: NodeId) -> Result<&NodeKey, NodeLookupError> {
        self.node(id)?;
        Ok(&self.node_keys[id.index as usize])
    }

    pub fn node_by_key(&self, key: &NodeKey) -> Result<NodeView<'_>, NodeKeyLookupError> {
        if !key.is_supported() {
            return Err(NodeKeyLookupError::UnsupportedSchema);
        }
        let file = self
            .files
            .get(&key.file().path)
            .ok_or(NodeKeyLookupError::FileRevisionExpired)?;
        if &file.key != key.file() {
            return Err(NodeKeyLookupError::FileRevisionExpired);
        }
        let range = self
            .node_ranges
            .iter()
            .find(|range| range.path == key.file().path)
            .expect("analysis file has a node range");
        let offset = self.node_keys[range.start as usize..range.end as usize]
            .iter()
            .position(|candidate| candidate == key)
            .ok_or(NodeKeyLookupError::NotFound)?;
        let index = range.start as usize + offset;
        self.node(NodeId {
            owner: self.owner,
            index: index as u32,
        })
        .map_err(|_| NodeKeyLookupError::NotFound)
    }

    /// Return the deterministic preorder subtree rooted at `id`, including `id` itself.
    pub fn subtree_node_ids(&self, id: NodeId) -> Result<NodeIds, NodeLookupError> {
        let node = self.node(id)?;
        let end = node
            .arena
            .containment()
            .subtree_end(node.local)
            .expect("node view belongs to the containment index");
        Ok(NodeIds {
            owner: self.owner,
            next: id.index,
            end: node.file_start() + end.as_usize() as u32,
        })
    }

    /// Return strict descendants of `id` in deterministic preorder.
    pub fn descendant_node_ids(&self, id: NodeId) -> Result<NodeIds, NodeLookupError> {
        let mut subtree = self.subtree_node_ids(id)?;
        subtree.next += 1;
        Ok(subtree)
    }

    /// Test structural CST containment. A node contains itself; different files never contain.
    pub fn node_contains(
        &self,
        ancestor: NodeId,
        descendant: NodeId,
    ) -> Result<bool, NodeLookupError> {
        let ancestor = self.node(ancestor)?;
        let descendant = self.node(descendant)?;
        if ancestor.path() != descendant.path() {
            return Ok(false);
        }
        Ok(ancestor
            .arena
            .containment()
            .contains(ancestor.local, descendant.local))
    }

    /// Iterate the positive-width token/trivia regions that partition one exact source revision.
    pub fn exclusive_syntax_regions(
        &self,
        path: &Path,
    ) -> Result<ExclusiveSyntaxRegions<'_>, ExclusiveSyntaxLookupError> {
        let (file, arena, file_start) = self.exclusive_syntax_context(path)?;
        Ok(ExclusiveSyntaxRegions {
            file,
            arena,
            owner: self.owner,
            file_start,
            next: 0,
        })
    }

    /// Find the unique smallest exclusive token/trivia region owning `byte`.
    ///
    /// `byte` addresses an existing byte, so `source_len` is out of range. Zero-width recovery
    /// nodes are available through structural containment but own no exclusive byte region.
    pub fn smallest_exclusive_syntax_region(
        &self,
        path: &Path,
        byte: usize,
    ) -> Result<ExclusiveSyntaxRegion<'_>, ExclusiveSyntaxLookupError> {
        let (file, arena, file_start) = self.exclusive_syntax_context(path)?;
        if byte >= file.source().len() {
            return Err(ExclusiveSyntaxLookupError::ByteOutOfRange {
                requested: byte,
                source_len: file.source().len(),
            });
        }
        let local = arena
            .containment()
            .exclusive_region_at(arena.segments(), byte)
            .expect("non-empty validated source partition owns every byte");
        Ok(ExclusiveSyntaxRegion {
            file,
            arena,
            local,
            owner: self.owner,
            file_start,
        })
    }

    /// Resolve a strict positive byte range to its smallest raw CST owner.
    ///
    /// Equal-span wrappers are disambiguated structurally. A range touching bytes outside the
    /// grammar root returns the exact file revision rather than a syntax node with a lying span.
    pub fn smallest_containing_syntax(
        &self,
        path: &Path,
        range: Range<usize>,
    ) -> Result<SyntaxOwner<'_>, NodeRangeLookupError> {
        let (file, arena, file_start) = self.node_range_context(path)?;
        if range.start > range.end {
            return Err(NodeRangeLookupError::ReversedRange {
                start: range.start,
                end: range.end,
            });
        }
        if range.end > file.source().len() {
            return Err(NodeRangeLookupError::RangeOutOfBounds {
                start: range.start,
                end: range.end,
                source_len: file.source().len(),
            });
        }
        if range.start == range.end {
            return Err(NodeRangeLookupError::EmptyRangeRequiresPointLookup { byte: range.start });
        }
        Ok(
            match arena.containment().smallest_containing_node(
                arena.nodes(),
                arena.segments(),
                range.start,
                range.end,
            ) {
                Some(local) => {
                    SyntaxOwner::Node(self.node_view_from_local(file, arena, file_start, local))
                }
                None => SyntaxOwner::File(&file.key),
            },
        )
    }

    /// Resolve a strict positive byte range and explicitly promote a raw owner to its nearest named
    /// ancestor. File ownership is preserved.
    pub fn smallest_containing_named_syntax(
        &self,
        path: &Path,
        range: Range<usize>,
    ) -> Result<SyntaxOwner<'_>, NodeRangeLookupError> {
        let owner = self.smallest_containing_syntax(path, range)?;
        Ok(self.promote_named_owner(owner))
    }

    /// Return unbiased raw ownership context at a byte boundary or insertion point.
    ///
    /// Exact zero-width nodes contain no bytes. Co-minimal unrelated nodes are all returned in
    /// grammar preorder; byte owners before and after the point remain separate.
    pub fn syntax_point_context(
        &self,
        path: &Path,
        point: usize,
    ) -> Result<SyntaxPointContext<'_>, NodeRangeLookupError> {
        let (file, arena, file_start) = self.node_range_context(path)?;
        let source_len = file.source().len();
        if point > source_len {
            return Err(NodeRangeLookupError::PointOutOfBounds {
                byte: point,
                source_len,
            });
        }
        let exact_zero_width = arena
            .containment()
            .zero_width_nodes_at(point)
            .iter()
            .map(|(_, index)| {
                self.node_view_from_local(file, arena, file_start, ArenaNodeIndex::from_u32(*index))
            })
            .collect();
        let before =
            (point > 0).then(|| self.syntax_owner_at_byte(file, arena, file_start, point - 1));
        let after =
            (point < source_len).then(|| self.syntax_owner_at_byte(file, arena, file_start, point));
        Ok(SyntaxPointContext {
            exact_zero_width,
            before,
            after,
        })
    }

    fn syntax_owner_at_byte<'analysis>(
        &'analysis self,
        file: &'analysis ParsedFile,
        arena: &'analysis SyntaxArena,
        file_start: u32,
        byte: usize,
    ) -> SyntaxOwner<'analysis> {
        let region = arena
            .containment()
            .exclusive_region_at(arena.segments(), byte)
            .expect("validated source partition owns every existing byte");
        match arena
            .segment(region)
            .expect("containment region belongs to arena")
            .owner()
        {
            SyntaxSegmentOwner::File => SyntaxOwner::File(&file.key),
            SyntaxSegmentOwner::Node(local) => {
                SyntaxOwner::Node(self.node_view_from_local(file, arena, file_start, local))
            }
        }
    }

    fn node_view_from_local<'analysis>(
        &'analysis self,
        file: &'analysis ParsedFile,
        arena: &'analysis SyntaxArena,
        file_start: u32,
        local: ArenaNodeIndex,
    ) -> NodeView<'analysis> {
        NodeView {
            analysis: self,
            file,
            arena,
            local,
            id: NodeId {
                owner: self.owner,
                index: file_start + local.as_usize() as u32,
            },
        }
    }

    fn promote_named_owner<'analysis>(
        &'analysis self,
        mut owner: SyntaxOwner<'analysis>,
    ) -> SyntaxOwner<'analysis> {
        while let SyntaxOwner::Node(node) = owner {
            if node.is_named() {
                return owner;
            }
            owner = match node.parent() {
                Some(parent) => SyntaxOwner::Node(
                    self.node(parent)
                        .expect("node parent belongs to the same project analysis"),
                ),
                None => return owner,
            };
        }
        owner
    }

    fn node_range_context(
        &self,
        path: &Path,
    ) -> Result<(&ParsedFile, &SyntaxArena, u32), NodeRangeLookupError> {
        let file = self
            .files
            .get(path)
            .ok_or_else(|| NodeRangeLookupError::FileNotFound {
                path: path.to_path_buf(),
            })?;
        let arena = file
            .arena
            .as_ref()
            .ok_or_else(|| NodeRangeLookupError::SyntaxUnavailable {
                path: path.to_path_buf(),
            })?;
        let file_start = self
            .node_ranges
            .iter()
            .find(|range| range.path == path)
            .expect("analysis file has a node range")
            .start;
        Ok((file, arena, file_start))
    }

    fn exclusive_syntax_context(
        &self,
        path: &Path,
    ) -> Result<(&ParsedFile, &SyntaxArena, u32), ExclusiveSyntaxLookupError> {
        let file =
            self.files
                .get(path)
                .ok_or_else(|| ExclusiveSyntaxLookupError::FileNotFound {
                    path: path.to_path_buf(),
                })?;
        let arena =
            file.arena
                .as_ref()
                .ok_or_else(|| ExclusiveSyntaxLookupError::SyntaxUnavailable {
                    path: path.to_path_buf(),
                })?;
        let file_start = self
            .node_ranges
            .iter()
            .find(|range| range.path == path)
            .expect("analysis file has a node range")
            .start;
        Ok((file, arena, file_start))
    }
}

impl NodeView<'_> {
    fn raw(&self) -> &crate::arena::SyntaxNode {
        self.arena
            .node(self.local)
            .expect("node view local index belongs to its arena")
    }

    fn file_start(&self) -> u32 {
        self.analysis
            .node_ranges
            .iter()
            .find(|range| range.path == self.file.key.path)
            .expect("node view file has a global range")
            .start
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    pub fn key(&self) -> &NodeKey {
        &self.analysis.node_keys[self.id.index as usize]
    }

    pub fn file_key(&self) -> &FileRevisionKey {
        &self.file.key
    }

    pub fn path(&self) -> &Path {
        &self.file.key.path
    }

    pub fn grammar(&self) -> &GrammarSelection {
        self.file.grammar()
    }

    pub fn raw_kind(&self) -> &str {
        self.raw().raw_kind()
    }

    pub fn raw_kind_id(&self) -> u16 {
        self.raw().raw_kind_id()
    }

    pub fn raw_grammar_kind(&self) -> &str {
        self.raw().raw_grammar_kind()
    }

    pub fn raw_grammar_kind_id(&self) -> u16 {
        self.raw().raw_grammar_kind_id()
    }

    pub fn field(&self) -> Option<&str> {
        self.raw().field()
    }

    pub fn span(&self) -> crate::arena::SyntaxSpan {
        self.raw().span()
    }

    pub fn is_named(&self) -> bool {
        self.raw().is_named()
    }

    pub fn is_extra(&self) -> bool {
        self.raw().is_extra()
    }

    pub fn is_error(&self) -> bool {
        self.raw().is_error()
    }

    pub fn is_missing(&self) -> bool {
        self.raw().is_missing()
    }

    pub fn has_error(&self) -> bool {
        self.raw().has_error()
    }

    pub fn parent(&self) -> Option<NodeId> {
        self.raw().parent().map(|parent| NodeId {
            owner: self.id.owner,
            index: self.file_start() + parent.as_usize() as u32,
        })
    }

    pub fn children(&self) -> Vec<NodeId> {
        let start = self.file_start();
        self.raw()
            .children()
            .iter()
            .map(|child| NodeId {
                owner: self.id.owner,
                index: start + child.as_usize() as u32,
            })
            .collect()
    }

    /// Iterate only the positive-width raw regions owned directly by this node.
    ///
    /// Descendant ownership is intentionally excluded; M1.6 declares inclusive aggregation.
    pub fn exclusive_syntax_regions(&self) -> NodeExclusiveSyntaxRegions<'_> {
        NodeExclusiveSyntaxRegions {
            file: self.file,
            arena: self.arena,
            owner: self.id.owner,
            file_start: self.file_start(),
            remaining: self.raw().owned_segment_indices().iter(),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        self.arena
            .node_source(self.file.source(), self.local)
            .expect("node span belongs to its exact source")
    }

    pub fn text(&self) -> &str {
        std::str::from_utf8(self.bytes()).expect("an arena exists only for valid UTF-8 source")
    }

    /// Return collision-prone, read-only comparison evidence.
    ///
    /// This value never authorizes lookup, re-anchoring, a revision guard, or a write.
    pub fn baseline_fingerprint(&self) -> NodeBaselineFingerprint {
        baseline_fingerprint(self.key(), self.text())
    }
}

fn parse_owned_file(
    entry: &SnapshotEntry,
    key: FileRevisionKey,
    ledger: &ParseLedger,
) -> Result<ParsedFile> {
    ledger.record_requested(&key);
    ledger.record_owner(&key);
    let line_starts = byte_line_starts(entry.bytes());
    let text = match std::str::from_utf8(entry.bytes()) {
        Ok(text) => Arc::<str>::from(text),
        Err(error) => {
            return Ok(ParsedFile {
                key,
                source: entry.source.clone(),
                text: None,
                tree: None,
                arena: None,
                provenance: AnalysisProvenance::failed(vec![AnalysisDiagnostic {
                    code: "invalid-utf8".to_string(),
                    message: format!("source is not valid UTF-8: {error}"),
                    span: None,
                }]),
                line_starts,
            });
        }
    };
    let language = entry.grammar_language().cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "source {} has no stored parser language",
            entry.path.display()
        )
    })?;
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .with_context(|| format!("failed to configure parser for {}", entry.path.display()))?;
    ledger.record_invocation(&key);
    let tree = parser.parse(text.as_ref(), None);
    let provenance = tree.as_ref().map_or_else(
        || {
            AnalysisProvenance::failed(vec![AnalysisDiagnostic {
                code: "parser-no-tree".to_string(),
                message: "tree-sitter returned no syntax tree".to_string(),
                span: None,
            }])
        },
        analysis_provenance_for_tree,
    );
    let arena = tree
        .as_ref()
        .map(|tree| SyntaxArena::from_tree(tree, entry.bytes(), key.grammar.clone()))
        .transpose()
        .with_context(|| format!("failed to own syntax arena for {}", entry.path.display()))?;
    Ok(ParsedFile {
        key,
        source: entry.source.clone(),
        text: Some(text),
        tree,
        arena,
        provenance,
        line_starts,
    })
}

fn byte_line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(
        bytes
            .iter()
            .enumerate()
            .filter_map(|(index, byte)| (*byte == b'\n').then_some(index + 1)),
    );
    starts
}

fn normalize_scope(
    root: &Path,
    invocation_base: &Path,
    scope: &[PathBuf],
) -> Result<Vec<ScopeEntry>> {
    let mut out = BTreeSet::new();
    for path in scope {
        let physical = if path.is_absolute() {
            path.clone()
        } else {
            invocation_base.join(path)
        };
        let physical = physical
            .canonicalize()
            .with_context(|| format!("failed to resolve scope {}", path.display()))?;
        let relative = physical.strip_prefix(root).with_context(|| {
            format!(
                "scope {} resolves outside repository root {}",
                path.display(),
                root.display()
            )
        })?;
        let path = if relative.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            normalize_logical_path(relative)?
        };
        let kind = if physical.is_file() {
            ScopeEntryKind::File
        } else if physical.is_dir() {
            ScopeEntryKind::Directory
        } else {
            bail!(
                "scope {} is neither a file nor a directory",
                physical.display()
            );
        };
        out.insert(ScopeEntry { path, kind });
    }
    let mut collapsed = Vec::<ScopeEntry>::new();
    for entry in out {
        if collapsed.iter().any(|existing| {
            existing.kind == ScopeEntryKind::Directory
                && (existing.path == Path::new(".") || entry.path.starts_with(&existing.path))
        }) {
            continue;
        }
        collapsed.push(entry);
    }
    Ok(collapsed)
}

fn collect_disk_sources(root: &Path, scope: &[ScopeEntry]) -> Result<BTreeMap<PathBuf, PathBuf>> {
    let mut physical_to_logical = BTreeMap::<PathBuf, PathBuf>::new();
    for scope_entry in scope {
        let logical_scope = &scope_entry.path;
        let physical_scope = if logical_scope == Path::new(".") {
            root.to_path_buf()
        } else {
            root.join(logical_scope)
        };
        if scope_entry.kind == ScopeEntryKind::File {
            insert_disk_source(root, &physical_scope, &mut physical_to_logical)?;
            continue;
        }
        let walker = WalkBuilder::new(&physical_scope)
            .hidden(false)
            .parents(false)
            .ignore(false)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .filter_entry(|entry| {
                !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | ".jj" | "target" | "__pycache__")
                )
            })
            .build();
        for entry in walker {
            let entry =
                entry.with_context(|| format!("failed to walk {}", physical_scope.display()))?;
            let metadata = std::fs::symlink_metadata(entry.path())
                .with_context(|| format!("failed to inspect {}", entry.path().display()))?;
            if metadata.file_type().is_symlink() {
                let target = entry.path().canonicalize().with_context(|| {
                    format!("failed to resolve symlink {}", entry.path().display())
                })?;
                if !target.starts_with(root) {
                    bail!(
                        "source alias {} resolves outside repository root {}",
                        entry.path().display(),
                        root.display()
                    );
                }
                continue;
            }
            if entry.file_type().is_some_and(|kind| kind.is_file()) {
                insert_disk_source(root, entry.path(), &mut physical_to_logical)?;
            }
        }
    }
    Ok(physical_to_logical
        .into_iter()
        .map(|(physical, logical)| (logical, physical))
        .collect())
}

fn insert_disk_source(
    root: &Path,
    path: &Path,
    out: &mut BTreeMap<PathBuf, PathBuf>,
) -> Result<()> {
    if !crate::is_supported_source(path) {
        return Ok(());
    }
    let physical = path
        .canonicalize()
        .with_context(|| format!("failed to resolve source {}", path.display()))?;
    let relative = physical.strip_prefix(root).with_context(|| {
        format!(
            "source {} resolves outside repository root {}",
            path.display(),
            root.display()
        )
    })?;
    let logical = normalize_logical_path(relative)?;
    out.entry(physical)
        .and_modify(|current| {
            if logical < *current {
                *current = logical.clone();
            }
        })
        .or_insert(logical);
    Ok(())
}

fn normalize_logical_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() || path.as_os_str().is_empty() {
        bail!(
            "logical path {} must be non-empty and relative",
            path.display()
        );
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => {
                let part = part
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("logical path is not valid Unicode"))?;
                if part.contains('\\') {
                    bail!(
                        "logical path {} contains a literal backslash component",
                        path.display()
                    );
                }
                normalized.push(part);
            }
            _ => bail!("logical path {} is not normalized", path.display()),
        }
    }
    if normalized.as_os_str().is_empty() {
        bail!("logical path {} must name an entry", path.display());
    }
    Ok(normalized)
}

fn normalize_builder_input_path(root: &Path, path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        let physical = if path.exists() {
            path.canonicalize()
                .with_context(|| format!("failed to resolve input {}", path.display()))?
        } else {
            path.to_path_buf()
        };
        let relative = physical.strip_prefix(root).with_context(|| {
            format!(
                "input {} resolves outside repository root {}",
                path.display(),
                root.display()
            )
        })?;
        return normalize_logical_path(relative);
    }
    let logical = normalize_logical_path(path)?;
    let physical = root.join(&logical);
    if !physical.exists() {
        return Ok(logical);
    }
    let physical = physical
        .canonicalize()
        .with_context(|| format!("failed to resolve input {}", path.display()))?;
    let relative = physical.strip_prefix(root).with_context(|| {
        format!(
            "input {} resolves outside repository root {}",
            path.display(),
            root.display()
        )
    })?;
    normalize_logical_path(relative)
}

fn snapshot_id(
    repository: &RepositoryId,
    scope: &[ScopeEntry],
    entries: &BTreeMap<PathBuf, SnapshotEntry>,
) -> ProjectSnapshotId {
    let mut hasher = domain_hasher(SNAPSHOT_ID_DOMAIN);
    hash_part(&mut hasher, repository.as_str().as_bytes());
    for entry in scope {
        hash_part(&mut hasher, &path_bytes(&entry.path));
        hash_part(&mut hasher, &[scope_kind_byte(entry.kind)]);
    }
    for (path, entry) in entries {
        hash_part(&mut hasher, &path_bytes(path));
        hash_part(&mut hasher, entry.revision().as_str().as_bytes());
        hash_part(&mut hasher, &[snapshot_kind_byte(entry.kind())]);
    }
    ProjectSnapshotId(format!("ps1_{}", hasher.finalize().to_hex()))
}

fn analysis_id<'a>(
    snapshot: &ProjectSnapshotId,
    files: impl Iterator<Item = &'a FileRevisionKey>,
) -> ProjectAnalysisId {
    let mut hasher = domain_hasher(ANALYSIS_ID_DOMAIN);
    hash_part(&mut hasher, snapshot.as_str().as_bytes());
    hash_part(&mut hasher, RAW_ARENA_SCHEMA.as_bytes());
    for file in files {
        hash_part(&mut hasher, &path_bytes(&file.path));
        hash_part(&mut hasher, file.source.as_str().as_bytes());
        hash_part(&mut hasher, &file.grammar.identity_bytes());
    }
    ProjectAnalysisId(format!("pa1_{}", hasher.finalize().to_hex()))
}

fn domain_digest<'a>(domain: &str, parts: impl IntoIterator<Item = &'a [u8]>) -> String {
    let mut hasher = domain_hasher(domain);
    for part in parts {
        hash_part(&mut hasher, part);
    }
    hasher.finalize().to_hex().to_string()
}

fn domain_hasher(domain: &str) -> blake3::Hasher {
    let mut hasher = blake3::Hasher::new();
    hash_part(&mut hasher, domain.as_bytes());
    hasher
}

fn hash_part(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn scope_kind_byte(kind: ScopeEntryKind) -> u8 {
    match kind {
        ScopeEntryKind::File => 0,
        ScopeEntryKind::Directory => 1,
    }
}

fn snapshot_kind_byte(kind: SnapshotEntryKind) -> u8 {
    match kind {
        SnapshotEntryKind::Source => 0,
        SnapshotEntryKind::AnalysisInput => 1,
    }
}

fn encode_wire_repo_path(path: &Path) -> std::result::Result<String, String> {
    let mut encoded = Vec::new();
    for component in path.components() {
        let Component::Normal(component) = component else {
            return Err("file revision path must contain only normal components".to_string());
        };
        let component = component
            .to_str()
            .ok_or_else(|| "file revision path must be Unicode".to_string())?;
        if component.is_empty() || component.contains(['\0', '\\']) {
            return Err(
                "file revision path component is empty or contains NUL or backslash".to_string(),
            );
        }
        encoded.push(component.replace('%', "%25"));
    }
    if encoded.is_empty() {
        return Err("file revision path must not be empty".to_string());
    }
    Ok(encoded.join("/"))
}

fn decode_wire_repo_path(encoded: &str) -> std::result::Result<PathBuf, String> {
    if encoded.is_empty()
        || encoded.starts_with('/')
        || encoded.ends_with('/')
        || encoded.contains("//")
        || encoded.contains('\\')
        || encoded.contains('\0')
    {
        return Err("file revision path is not canonical root-relative wire form".to_string());
    }
    let mut path = PathBuf::new();
    for component in encoded.split('/') {
        let component = decode_wire_component(component)?;
        if component.is_empty() || component == "." || component == ".." {
            return Err("file revision path contains a non-normal component".to_string());
        }
        path.push(component);
    }
    Ok(path)
}

fn decode_wire_component(encoded: &str) -> std::result::Result<String, String> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let escape = bytes
            .get(index + 1..index + 3)
            .ok_or_else(|| "truncated file revision path escape".to_string())?;
        match escape {
            b"25" => decoded.push(b'%'),
            _ => return Err("unsupported file revision path escape".to_string()),
        }
        index += 3;
    }
    String::from_utf8(decoded).map_err(|_| "file revision path escape is not UTF-8".to_string())
}

fn is_lower_prefixed_hex(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn path_bytes(path: &Path) -> Vec<u8> {
    if path == Path::new(".") {
        return b".".to_vec();
    }
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(
                part.to_str()
                    .expect("snapshot paths are validated as Unicode"),
            ),
            Component::CurDir => None,
            _ => panic!("snapshot path is not normalized and relative"),
        })
        .collect::<Vec<_>>()
        .join("/")
        .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::{RAW_ARENA_SCHEMA, SyntaxSegmentKind, SyntaxSegmentOwner};
    use deslop_core::{AnalysisStatus, Span, revision_guard};

    fn repository() -> RepositoryId {
        RepositoryId::explicit("test-repository").unwrap()
    }

    fn node_by_kind<'analysis>(
        analysis: &'analysis ProjectAnalysis,
        kind: &str,
    ) -> NodeView<'analysis> {
        analysis
            .node_ids()
            .map(|id| analysis.node(id).unwrap())
            .find(|node| node.raw_kind() == kind)
            .unwrap()
    }

    fn node_depth(analysis: &ProjectAnalysis, mut id: NodeId) -> usize {
        let mut depth = 0;
        while let Some(parent) = analysis.node(id).unwrap().parent() {
            depth += 1;
            id = parent;
        }
        depth
    }

    #[test]
    fn source_revision_hashes_exact_raw_bytes_only() {
        assert_eq!(
            SourceRevision::for_bytes(b"abc").as_str(),
            "sr1_9aa7a4e8572b05920922c56f310d77531a645f496765b4a8875ff4715a0cfe61"
        );
        let revisions = [
            b"line\n".as_slice(),
            b"line\r\n".as_slice(),
            b"\xef\xbb\xbfline\n".as_slice(),
            b"line".as_slice(),
            b"line\0\n".as_slice(),
            b"Line\n".as_slice(),
        ]
        .map(SourceRevision::for_bytes)
        .into_iter()
        .collect::<BTreeSet<_>>();
        assert_eq!(revisions.len(), 6);
    }

    #[test]
    fn source_store_deduplicates_content_without_collapsing_paths() {
        let store = SourceStore::default();
        let first = store.intern(b"same".to_vec());
        let second = store.intern(b"same".to_vec());
        assert_eq!(first.revision(), second.revision());
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn snapshot_is_deterministic_and_distinguishes_logical_paths() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("a.rs"), "fn same() {}\n").unwrap();
        std::fs::write(temp.path().join("b.rs"), "fn same() {}\n").unwrap();
        let first = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_scope(&[PathBuf::from("b.rs"), PathBuf::from("a.rs")])
            .build()
            .unwrap();
        let second = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_scope(&[PathBuf::from("a.rs"), PathBuf::from("b.rs")])
            .build()
            .unwrap();
        assert_eq!(first.id(), second.id());
        assert_eq!(first.entries().count(), 2);
        assert_eq!(first.store().len(), 1);
        let revisions = first
            .entries()
            .map(SnapshotEntry::revision)
            .collect::<BTreeSet<_>>();
        assert_eq!(revisions.len(), 1);
        let paths = first
            .entries()
            .map(|entry| entry.path().to_path_buf())
            .collect::<Vec<_>>();
        assert_eq!(paths, [PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
        let analysis = ProjectAnalysis::build(first).unwrap();
        assert_eq!(analysis.parse_counts().len(), 2);
        assert!(analysis.parse_counts().values().all(|count| {
            count.requested == 1 && count.owners == 1 && count.parser_invocations == 1
        }));
    }

    #[test]
    fn project_analysis_owns_one_parse_per_file_revision_and_variant() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("typed.ts"), "const value: number = 1;\n").unwrap();
        std::fs::write(
            temp.path().join("view.tsx"),
            "const view = <div>{value}</div>;\n",
        )
        .unwrap();
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let counts = analysis.parse_counts();
        assert_eq!(counts.len(), 2);
        assert!(counts.values().all(|count| {
            count
                == &FileParseCount {
                    requested: 1,
                    owners: 1,
                    parser_invocations: 1,
                    reused: 0,
                }
        }));
        assert_eq!(
            analysis
                .file(Path::new("typed.ts"))
                .unwrap()
                .grammar()
                .dialect,
            "typescript"
        );
        assert_eq!(
            analysis
                .file(Path::new("view.tsx"))
                .unwrap()
                .grammar()
                .dialect,
            "tsx"
        );
        assert!(analysis.files().all(ParsedFile::has_tree));
    }

    #[test]
    fn grammar_selection_matrix_is_path_authoritative_and_versioned() {
        let temp = tempfile::tempdir().unwrap();
        let fixtures = [
            ("sample.clj", "clojure", "tree-sitter-clojure", "0.1.0"),
            ("sample.jl", "julia", "tree-sitter-julia", "0.23.1"),
            ("sample.py", "python", "tree-sitter-python", "0.25.0"),
            (
                "sample.js",
                "javascript",
                "tree-sitter-javascript",
                "0.25.0",
            ),
            ("sample.jsx", "jsx", "tree-sitter-javascript", "0.25.0"),
            (
                "sample.ts",
                "typescript",
                "tree-sitter-typescript/typescript",
                "0.23.2",
            ),
            (
                "sample.mts",
                "typescript",
                "tree-sitter-typescript/typescript",
                "0.23.2",
            ),
            (
                "sample.cts",
                "typescript",
                "tree-sitter-typescript/typescript",
                "0.23.2",
            ),
            ("sample.tsx", "tsx", "tree-sitter-typescript/tsx", "0.23.2"),
            ("sample.rs", "rust", "tree-sitter-rust", "0.24.2"),
        ];
        let mut builder = ProjectSnapshotBuilder::new(temp.path(), repository()).unwrap();
        for (path, _, _, _) in fixtures {
            builder = builder.with_overlay(path, b"value\n".to_vec()).unwrap();
        }
        let analysis = ProjectAnalysis::build(builder.build().unwrap()).unwrap();
        for (path, dialect, grammar_id, grammar_version) in fixtures {
            let grammar = analysis.file(Path::new(path)).unwrap().grammar();
            assert_eq!(grammar.dialect(), dialect, "{path}");
            assert_eq!(grammar.grammar_id(), grammar_id, "{path}");
            assert_eq!(grammar.grammar_version(), grammar_version, "{path}");
            assert_eq!(grammar.selector(), "deslop-grammar-selector/1");
            assert_eq!(
                grammar.parser_build(),
                "deslop-parse/0.1.0+tree-sitter/0.25.10"
            );
        }
    }

    #[test]
    fn stored_typescript_variant_controls_the_actual_parser() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = b"const view = <div>ok</div>;\n".to_vec();
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("view.ts", bytes.clone())
            .unwrap()
            .with_overlay("view.tsx", bytes)
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        assert_eq!(
            analysis
                .file(Path::new("view.ts"))
                .unwrap()
                .provenance()
                .status,
            AnalysisStatus::Partial
        );
        assert_eq!(
            analysis
                .file(Path::new("view.tsx"))
                .unwrap()
                .provenance()
                .status,
            AnalysisStatus::Complete
        );
        for path in ["view.ts", "view.tsx"] {
            let file = analysis.file(Path::new(path)).unwrap();
            assert_eq!(file.arena().unwrap().grammar(), file.grammar());
            assert_eq!(file.arena().unwrap().grammar(), &file.key().grammar);
        }
        let counts = analysis.parse_counts();
        let keys = counts.keys().collect::<Vec<_>>();
        assert_eq!(keys[0].source, keys[1].source);
        assert_ne!(keys[0].grammar, keys[1].grammar);
        assert!(counts.values().all(|count| {
            count
                == &FileParseCount {
                    requested: 1,
                    owners: 1,
                    parser_invocations: 1,
                    reused: 0,
                }
        }));
        for file in analysis.files() {
            let arena = file.arena().unwrap();
            for (index, _) in arena.indexed_nodes() {
                let _ = file.node_source(index);
            }
            for (index, _) in arena.indexed_segments() {
                let _ = file.segment_source(index);
            }
        }
        assert_eq!(analysis.parse_counts(), counts);
    }

    #[test]
    fn exact_empty_scope_stays_empty_and_overlapping_scope_collapses() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("a.rs"), "fn a() {}\n").unwrap();
        let empty = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_exact_files(&[])
            .build()
            .unwrap();
        assert_eq!(empty.entries().count(), 0);
        assert!(empty.requested_scope().is_empty());

        let root_only = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .build()
            .unwrap();
        let redundant = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_scope(&[PathBuf::from("."), PathBuf::from("a.rs")])
            .build()
            .unwrap();
        assert_eq!(root_only.id(), redundant.id());
        assert_eq!(redundant.requested_scope().len(), 1);
        assert_eq!(redundant.read_counts().get(Path::new("a.rs")), Some(&1));
    }

    #[test]
    fn overlay_shadows_disk_before_the_read_plan() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("shadow.rs");
        std::fs::write(&path, "fn disk() {}\n").unwrap();
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("shadow.rs", b"fn overlay() {}\n".to_vec())
            .unwrap()
            .build()
            .unwrap();
        assert!(snapshot.read_counts().is_empty());
        assert_eq!(
            snapshot.entry(Path::new("shadow.rs")).unwrap().bytes(),
            b"fn overlay() {}\n"
        );
    }

    #[test]
    fn invocation_base_resolves_relative_scope_without_changing_authority_root() {
        let temp = tempfile::tempdir().unwrap();
        let subdir = temp.path().join("nested");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(subdir.join("file.rs"), "fn nested() {}\n").unwrap();
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_invocation_base(&subdir)
            .unwrap()
            .with_scope(&[PathBuf::from("file.rs")])
            .build()
            .unwrap();
        assert!(snapshot.entry(Path::new("nested/file.rs")).is_some());
        assert_eq!(
            snapshot.requested_scope(),
            &[ScopeEntry {
                path: PathBuf::from("nested/file.rs"),
                kind: ScopeEntryKind::File,
            }]
        );
    }

    #[test]
    fn reusable_store_shares_content_across_snapshots() {
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        std::fs::write(first_root.path().join("same.rs"), "fn same() {}\n").unwrap();
        std::fs::write(second_root.path().join("same.rs"), "fn same() {}\n").unwrap();
        let store = Arc::new(SourceStore::default());
        let first = ProjectSnapshotBuilder::new(first_root.path(), repository())
            .unwrap()
            .with_store(store.clone())
            .build()
            .unwrap();
        let second = ProjectSnapshotBuilder::new(second_root.path(), repository())
            .unwrap()
            .with_store(store.clone())
            .build()
            .unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(first.id(), second.id());
        assert!(Arc::ptr_eq(
            &first.entry(Path::new("same.rs")).unwrap().source,
            &second.entry(Path::new("same.rs")).unwrap().source
        ));
        assert_eq!(
            first.entry(Path::new("same.rs")).unwrap().revision(),
            second.entry(Path::new("same.rs")).unwrap().revision()
        );
    }

    #[test]
    fn absolute_in_root_overlay_normalizes_and_conflicts_fail() {
        let temp = tempfile::tempdir().unwrap();
        let absolute = temp.path().join("overlay.rs");
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay(&absolute, b"fn overlay() {}\n".to_vec())
            .unwrap()
            .build()
            .unwrap();
        assert!(snapshot.entry(Path::new("overlay.rs")).is_some());

        let error = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("same.rs", b"first".to_vec())
            .unwrap()
            .with_overlay("same.rs", b"second".to_vec())
            .err()
            .expect("conflicting overlay must fail");
        assert!(error.to_string().contains("conflicting bytes"));
    }

    #[test]
    fn malformed_source_keeps_one_owner_and_one_parser_invocation() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("broken.ts", b"function broken(: {\n".to_vec())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let file = analysis.file(Path::new("broken.ts")).unwrap();
        assert_eq!(file.provenance().status, AnalysisStatus::Partial);
        assert!(file.has_tree());
        let arena = file.arena().expect("partial source retains owned arena");
        assert!(
            arena
                .nodes()
                .iter()
                .any(|node| node.is_error() || node.is_missing())
        );
        assert!(arena.node(arena.root()).unwrap().has_error());
        assert_eq!(file.line_starts(), &[0, 20]);
        assert_eq!(
            analysis.parse_counts().values().next().copied(),
            Some(FileParseCount {
                requested: 1,
                owners: 1,
                parser_invocations: 1,
                reused: 0,
            })
        );
    }

    #[test]
    fn zero_width_recovery_nodes_remain_owned_without_claiming_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let source = b"function f(a: string { return a; }\n";
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("missing.ts", source.to_vec())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let file = analysis.file(Path::new("missing.ts")).unwrap();
        assert_eq!(file.provenance().status, AnalysisStatus::Partial);
        assert!(
            file.provenance()
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "tree-sitter-missing-node")
        );
        let arena = file.arena().unwrap();
        let missing = arena
            .indexed_nodes()
            .filter(|(_, node)| node.is_missing())
            .collect::<Vec<_>>();
        assert_eq!(arena.nodes().len(), 20);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].1.parent().map(ArenaNodeIndex::as_usize), Some(4));
        assert_eq!(
            arena
                .node(missing[0].1.parent().unwrap())
                .unwrap()
                .raw_kind(),
            "formal_parameters"
        );
        assert!(missing.iter().all(|(_, node)| {
            node.raw_kind() == ")"
                && node.span().byte_range() == (20..20)
                && node.span().start_point().row() == 0
                && node.span().start_point().column() == 20
                && file.node_source(missing[0].0) == Some(b"".as_slice())
                && node.owned_segment_indices().is_empty()
        }));
        assert_eq!(arena.segments().len(), 18);
        assert_eq!(
            arena
                .segments()
                .iter()
                .filter(|segment| segment.kind() == SyntaxSegmentKind::Token)
                .map(|segment| segment.byte_range().len())
                .sum::<usize>(),
            28
        );
        assert_eq!(
            arena
                .segments()
                .iter()
                .filter(|segment| segment.kind() == SyntaxSegmentKind::Trivia)
                .map(|segment| segment.byte_range().len())
                .sum::<usize>(),
            7
        );
        assert_eq!(
            arena
                .segments()
                .iter()
                .filter(|segment| segment.kind() == SyntaxSegmentKind::Token)
                .count(),
            11
        );
        assert_eq!(
            arena
                .segments()
                .iter()
                .filter(|segment| segment.kind() == SyntaxSegmentKind::Trivia)
                .count(),
            7
        );
        assert_eq!(
            arena
                .segments()
                .iter()
                .map(|segment| segment.byte_range().len())
                .sum::<usize>(),
            file.source().len()
        );
        assert_eq!(
            analysis
                .parse_counts()
                .values()
                .next()
                .unwrap()
                .parser_invocations,
            1
        );
    }

    #[test]
    fn typed_partial_arenas_retain_exact_error_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let malformed_ts = include_bytes!("../../../tests/fixtures/typescript/malformed.ts");
        let malformed_tsx = include_bytes!("../../../tests/fixtures/typescript/malformed.tsx");
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("malformed.ts", malformed_ts.to_vec())
            .unwrap()
            .with_overlay("malformed.tsx", malformed_tsx.to_vec())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let cases = [
            (
                "malformed.ts",
                malformed_ts.as_slice(),
                27,
                24,
                15,
                55,
                9,
                11,
                62..63,
            ),
            (
                "malformed.tsx",
                malformed_tsx.as_slice(),
                52,
                46,
                35,
                84,
                11,
                13,
                0..96,
            ),
        ];
        for (
            path,
            source,
            node_count,
            segment_count,
            token_count,
            token_bytes,
            trivia_count,
            trivia_bytes,
            error_range,
        ) in cases
        {
            let file = analysis.file(Path::new(path)).unwrap();
            assert_eq!(file.provenance().status, AnalysisStatus::Partial, "{path}");
            assert_eq!(file.provenance().diagnostics.len(), 1, "{path}");
            assert_eq!(
                file.provenance().diagnostics[0]
                    .span
                    .as_ref()
                    .map(|span| span.start_byte..span.end_byte),
                Some(error_range.clone()),
                "{path}"
            );
            let arena = file.arena().unwrap();
            assert_eq!(arena.nodes().len(), node_count, "{path}");
            assert_eq!(arena.segments().len(), segment_count, "{path}");
            let errors = arena
                .nodes()
                .iter()
                .filter(|node| node.is_error())
                .collect::<Vec<_>>();
            assert_eq!(errors.len(), 1, "{path}");
            assert_eq!(errors[0].span().byte_range(), error_range, "{path}");
            assert_eq!(
                arena
                    .nodes()
                    .iter()
                    .filter(|node| node.is_missing())
                    .count(),
                0,
                "{path}"
            );
            let count_bytes = |kind| {
                arena
                    .segments()
                    .iter()
                    .filter(|segment| segment.kind() == kind)
                    .fold((0, 0), |(count, bytes), segment| {
                        (count + 1, bytes + segment.byte_range().len())
                    })
            };
            assert_eq!(
                count_bytes(SyntaxSegmentKind::Token),
                (token_count, token_bytes)
            );
            assert_eq!(
                count_bytes(SyntaxSegmentKind::Trivia),
                (trivia_count, trivia_bytes)
            );
            let reconstructed = arena
                .indexed_segments()
                .flat_map(|(index, _)| file.segment_source(index).unwrap())
                .copied()
                .collect::<Vec<_>>();
            assert_eq!(reconstructed, source, "{path}");
        }
        assert!(analysis.parse_counts().values().all(|count| {
            count.requested == 1
                && count.owners == 1
                && count.parser_invocations == 1
                && count.reused == 0
        }));
    }

    #[test]
    fn snapshot_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SourceStore>();
        assert_send_sync::<ProjectSnapshot>();
        assert_send_sync::<GrammarSelection>();
        assert_send_sync::<ParseLedger>();
        assert_send_sync::<ParsedFile>();
        assert_send_sync::<ProjectAnalysis>();
        assert_send_sync::<SyntaxArena>();
    }

    #[test]
    fn owned_arena_is_deterministic_reciprocal_and_source_bound() {
        let temp = tempfile::tempdir().unwrap();
        let source = b"fn greet(name: &str) {\n  // note\n  println!(\"h\xc3\xa9, {name}\");\n}\n";
        let build = || {
            let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
                .unwrap()
                .with_overlay("arena.rs", source.to_vec())
                .unwrap()
                .build()
                .unwrap();
            ProjectAnalysis::build(snapshot).unwrap()
        };
        let first = build();
        let second = build();
        assert_eq!(first.id(), second.id());
        let file = first.file(Path::new("arena.rs")).unwrap();
        let arena = file.arena().expect("valid source owns an arena");
        assert_eq!(arena.schema(), RAW_ARENA_SCHEMA);
        assert_eq!(arena.schema(), "deslop-raw-arena/1");
        assert_eq!(arena.grammar(), file.grammar());
        assert_eq!(arena.source_len(), source.len());
        assert_eq!(
            arena,
            second.file(Path::new("arena.rs")).unwrap().arena().unwrap()
        );

        let root = arena.node(arena.root()).unwrap();
        assert!(root.parent().is_none());
        assert_eq!(root.span().byte_range(), 0..source.len());
        assert_eq!(file.node_source(arena.root()), Some(source.as_slice()));

        let mut saw_field = false;
        let mut saw_anonymous = false;
        let mut saw_unicode_slice = false;
        let mut saw_byte_column_after_unicode = false;
        for (index, node) in arena.indexed_nodes() {
            let raw_index = index.as_usize();
            if node.field().is_some() {
                saw_field = true;
            }
            if !node.is_named() {
                saw_anonymous = true;
            }
            if file
                .node_source(index)
                .is_some_and(|bytes| bytes.windows(2).any(|pair| pair == "é".as_bytes()))
            {
                saw_unicode_slice = true;
            }
            if file.node_source(index) == Some(b"name".as_slice())
                && node.span().start_point().row() == 0
            {
                let byte_column = source
                    .windows(b"name".len())
                    .position(|window| window == b"name")
                    .unwrap();
                assert_eq!(node.span().start_point().column(), byte_column);
            }
            assert!(!node.raw_kind().is_empty());
            assert!(!node.raw_grammar_kind().is_empty());
            for child_index in node.children() {
                assert!(child_index.as_usize() > raw_index, "preorder child index");
                let child = arena.node(*child_index).unwrap();
                assert_eq!(child.parent(), Some(index));
                assert!(child.span().start_byte() >= node.span().start_byte());
                assert!(child.span().end_byte() <= node.span().end_byte());
            }
            if let Some(parent_index) = node.parent() {
                assert!(
                    arena
                        .node(parent_index)
                        .unwrap()
                        .children()
                        .contains(&index)
                );
            }
            for segment_index in node.owned_segment_indices() {
                assert_eq!(
                    arena.segment(*segment_index).unwrap().owner(),
                    SyntaxSegmentOwner::Node(index)
                );
            }
        }
        assert!(saw_field);
        assert!(saw_anonymous);
        assert!(saw_unicode_slice);
        let unicode_line = b"fn f() { let text = \"h\xc3\xa9\"; let after = 1; }\n";
        let unicode_snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("unicode.rs", unicode_line.to_vec())
            .unwrap()
            .build()
            .unwrap();
        let unicode_analysis = ProjectAnalysis::build(unicode_snapshot).unwrap();
        let unicode_file = unicode_analysis.file(Path::new("unicode.rs")).unwrap();
        for (index, node) in unicode_file.arena().unwrap().indexed_nodes() {
            if unicode_file.node_source(index) == Some(b"after".as_slice()) {
                let expected = unicode_line
                    .windows(b"after".len())
                    .position(|window| window == b"after")
                    .unwrap();
                assert_eq!(node.span().start_point().column(), expected);
                assert!(
                    expected
                        > String::from_utf8_lossy(&unicode_line[..expected])
                            .chars()
                            .count()
                );
                saw_byte_column_after_unicode = true;
            }
        }
        assert!(saw_byte_column_after_unicode);
    }

    #[test]
    fn owned_arena_matches_private_tree_node_for_node() {
        let temp = tempfile::tempdir().unwrap();
        let source = b"\t// caf\xc3\xa9 \xf0\x9f\x98\x80\r\nfn caf\xc3\xa9(\xcf\x80: i32) -> i32 {\n    \xcf\x80 + 1  \n}\n";
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("mirror.rs", source.to_vec())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let file = analysis.file(Path::new("mirror.rs")).unwrap();
        let tree = file.tree.as_ref().unwrap();
        let arena = file.arena().unwrap();
        assert_eq!(source.len(), 58);
        assert_eq!(file.line_starts(), &[0, 16, 43, 56, 58]);
        assert_eq!(arena.nodes().len(), 22);
        assert_eq!(arena.segments().len(), 28);
        assert_eq!(
            arena
                .nodes()
                .iter()
                .map(|node| node.raw_kind())
                .collect::<Vec<_>>(),
            [
                "source_file",
                "line_comment",
                "//",
                "function_item",
                "fn",
                "identifier",
                "parameters",
                "(",
                "parameter",
                "identifier",
                ":",
                "primitive_type",
                ")",
                "->",
                "primitive_type",
                "block",
                "{",
                "binary_expression",
                "identifier",
                "+",
                "integer_literal",
                "}",
            ]
        );
        let (token_count, token_bytes, trivia_count, trivia_bytes) =
            arena
                .segments()
                .iter()
                .fold((0, 0, 0, 0), |mut totals, segment| {
                    let len = segment.byte_range().len();
                    match segment.kind() {
                        SyntaxSegmentKind::Token => {
                            totals.0 += 1;
                            totals.1 += len;
                        }
                        SyntaxSegmentKind::Trivia => {
                            totals.2 += 1;
                            totals.3 += len;
                        }
                    }
                    totals
                });
        assert_eq!((token_count, token_bytes), (14, 26));
        assert_eq!((trivia_count, trivia_bytes), (14, 32));

        let mut expected = Vec::new();
        let mut expected_children = Vec::<Vec<usize>>::new();
        let mut pending: Vec<(tree_sitter::Node<'_>, Option<usize>, Option<&'static str>)> =
            vec![(tree.root_node(), None, None)];
        while let Some((node, parent, field)) = pending.pop() {
            let index = expected.len();
            expected.push((node, parent, field));
            expected_children.push(Vec::new());
            if let Some(parent) = parent {
                expected_children[parent].push(index);
            }
            let mut cursor = node.walk();
            let children = node
                .children(&mut cursor)
                .enumerate()
                .map(|(child_index, child)| {
                    (
                        child,
                        Some(index),
                        node.field_name_for_child(child_index as u32),
                    )
                })
                .collect::<Vec<_>>();
            pending.extend(children.into_iter().rev());
        }

        assert_eq!(arena.nodes().len(), expected.len());
        for (index, arena_node) in arena.indexed_nodes() {
            let (tree_node, parent, field) = expected[index.as_usize()];
            assert_eq!(arena_node.raw_kind(), tree_node.kind());
            assert_eq!(arena_node.raw_kind_id(), tree_node.kind_id());
            assert_eq!(arena_node.raw_grammar_kind(), tree_node.grammar_name());
            assert_eq!(arena_node.raw_grammar_kind_id(), tree_node.grammar_id());
            assert_eq!(arena_node.field(), field);
            assert_eq!(arena_node.parent().map(ArenaNodeIndex::as_usize), parent);
            assert_eq!(
                arena_node
                    .children()
                    .iter()
                    .map(|child| child.as_usize())
                    .collect::<Vec<_>>(),
                expected_children[index.as_usize()]
            );
            assert_eq!(arena_node.span().start_byte(), tree_node.start_byte());
            assert_eq!(arena_node.span().end_byte(), tree_node.end_byte());
            assert_eq!(
                arena_node.span().start_point().row(),
                tree_node.start_position().row
            );
            assert_eq!(
                arena_node.span().start_point().column(),
                tree_node.start_position().column
            );
            assert_eq!(
                arena_node.span().end_point().row(),
                tree_node.end_position().row
            );
            assert_eq!(
                arena_node.span().end_point().column(),
                tree_node.end_position().column
            );
            assert_eq!(arena_node.is_named(), tree_node.is_named());
            assert_eq!(arena_node.is_extra(), tree_node.is_extra());
            assert_eq!(arena_node.is_error(), tree_node.is_error());
            assert_eq!(arena_node.is_missing(), tree_node.is_missing());
            assert_eq!(arena_node.has_error(), tree_node.has_error());
            assert_eq!(file.node_source(index), source.get(tree_node.byte_range()));
        }
    }

    #[test]
    fn arena_preserves_alias_kinds_and_repeated_fields() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("alias.rs", b"type A = Vec<String>;\n".to_vec())
            .unwrap()
            .with_overlay("fields.py", b"from pkg import a, b, c as d\n".to_vec())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();

        let rust = analysis.file(Path::new("alias.rs")).unwrap();
        let rust_arena = rust.arena().unwrap();
        let aliased = rust_arena
            .indexed_nodes()
            .find(|(index, node)| {
                node.raw_kind() == "type_identifier"
                    && node.raw_grammar_kind() == "identifier"
                    && rust.node_source(*index) == Some(b"A".as_slice())
            })
            .expect("Rust type alias must retain visible and grammar kind identities");
        assert_ne!(aliased.1.raw_kind_id(), aliased.1.raw_grammar_kind_id());

        let python = analysis.file(Path::new("fields.py")).unwrap();
        let python_arena = python.arena().unwrap();
        let repeated_name_fields = python_arena
            .nodes()
            .iter()
            .map(|parent| {
                parent
                    .children()
                    .iter()
                    .filter(|child| python_arena.node(**child).unwrap().field() == Some("name"))
                    .count()
            })
            .max()
            .unwrap();
        assert_eq!(repeated_name_fields, 3);
    }

    #[test]
    fn token_and_trivia_segments_partition_every_source_byte_once() {
        let temp = tempfile::tempdir().unwrap();
        let source = b"\n  fn value() -> &'static str { /* c */ \"h\xc3\xa9\" }\n\t";
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("partition.rs", source.to_vec())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let file = analysis.file(Path::new("partition.rs")).unwrap();
        let arena = file.arena().unwrap();

        let mut cursor = 0;
        let mut reconstructed = Vec::new();
        let mut token_count = 0;
        let mut trivia_count = 0;
        for (index, segment) in arena.indexed_segments() {
            let range = segment.byte_range();
            assert_eq!(range.start, cursor);
            assert!(range.end > range.start);
            reconstructed.extend_from_slice(file.segment_source(index).unwrap());
            cursor = range.end;

            let owner_index = match segment.owner() {
                SyntaxSegmentOwner::File => {
                    assert_eq!(segment.kind(), SyntaxSegmentKind::Trivia);
                    let root = arena.node(arena.root()).unwrap().span();
                    assert!(range.end <= root.start_byte() || range.start >= root.end_byte());
                    continue;
                }
                SyntaxSegmentOwner::Node(owner) => owner,
            };
            let owner = arena.node(owner_index).unwrap();
            assert!(owner.owned_segment_indices().contains(&index));
            assert!(range.start >= owner.span().start_byte());
            assert!(range.end <= owner.span().end_byte());
            match segment.kind() {
                SyntaxSegmentKind::Token => {
                    token_count += 1;
                    assert!(owner.is_leaf());
                    assert_eq!(owner.span().byte_range(), range);
                }
                SyntaxSegmentKind::Trivia => {
                    trivia_count += 1;
                    if owner.is_leaf() {
                        assert!(
                            owner.is_extra() || {
                                let mut ancestor = owner.parent();
                                let mut within_extra = false;
                                while let Some(index) = ancestor {
                                    let node = arena.node(index).unwrap();
                                    within_extra |= node.is_extra();
                                    ancestor = node.parent();
                                }
                                within_extra
                            }
                        );
                    } else {
                        assert!(owner.children().iter().all(|child| {
                            let child = arena.node(*child).unwrap().span();
                            range.end <= child.start_byte() || range.start >= child.end_byte()
                        }));
                    }
                }
            }
        }
        assert_eq!(cursor, source.len());
        assert_eq!(reconstructed, source);
        assert!(token_count > 0);
        assert!(trivia_count > 0);
        assert_eq!(
            analysis.parse_counts().values().next().copied(),
            Some(FileParseCount {
                requested: 1,
                owners: 1,
                parser_invocations: 1,
                reused: 0,
            })
        );
    }

    #[test]
    fn empty_source_has_a_root_and_an_empty_byte_partition() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("empty.rs", Vec::new())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let file = analysis.file(Path::new("empty.rs")).unwrap();
        let arena = file.arena().unwrap();
        assert_eq!(arena.nodes().len(), 1);
        assert_eq!(arena.node(arena.root()).unwrap().span().byte_range(), 0..0);
        assert!(arena.segments().is_empty());
        assert_eq!(arena.source_len(), 0);
    }

    #[test]
    fn whitespace_only_source_is_root_owned_trivia() {
        let temp = tempfile::tempdir().unwrap();
        let source = b"\t \r\n  \n";
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("whitespace.rs", source.to_vec())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let file = analysis.file(Path::new("whitespace.rs")).unwrap();
        let arena = file.arena().unwrap();
        let root = arena.node(arena.root()).unwrap();
        assert_eq!(source.len(), 7);
        assert_eq!(arena.nodes().len(), 1);
        assert_eq!(root.span().byte_range(), 7..7);
        assert_eq!(root.span().start_point().row(), 2);
        assert_eq!(root.span().start_point().column(), 0);
        assert_eq!(file.node_source(arena.root()), Some(b"".as_slice()));
        assert_eq!(arena.segments().len(), 1);
        assert_eq!(arena.segments()[0].kind(), SyntaxSegmentKind::Trivia);
        assert_eq!(arena.segments()[0].byte_range(), 0..7);
        assert_eq!(arena.segments()[0].owner(), SyntaxSegmentOwner::File);
        assert!(root.owned_segment_indices().is_empty());
        let segment_index = arena.indexed_segments().next().unwrap().0;
        assert_eq!(file.segment_source(segment_index), Some(source.as_slice()));
        assert_eq!(
            analysis.parse_counts().values().next().copied(),
            Some(FileParseCount {
                requested: 1,
                owners: 1,
                parser_invocations: 1,
                reused: 0,
            })
        );
    }

    #[test]
    fn invalid_utf8_is_revisioned_but_never_parsed() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("broken.rs", vec![0xff, 0xfe])
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let file = analysis.file(Path::new("broken.rs")).unwrap();
        assert_eq!(file.provenance().status, AnalysisStatus::Failed);
        assert_eq!(file.provenance().diagnostics[0].code, "invalid-utf8");
        assert_eq!(file.source(), &[0xff, 0xfe]);
        assert_eq!(file.key().source, SourceRevision::for_bytes(&[0xff, 0xfe]));
        assert!(file.text().is_none());
        assert!(!file.has_tree());
        assert!(!file.has_arena());
        assert_eq!(
            analysis.parse_counts().values().next().copied(),
            Some(FileParseCount {
                requested: 1,
                owners: 1,
                parser_invocations: 0,
                reused: 0,
            })
        );
    }

    #[test]
    fn project_global_node_ids_are_dense_deterministic_and_owner_checked() {
        let temp = tempfile::tempdir().unwrap();
        let build = |reverse_input: bool, with_prefix: bool| {
            let mut builder = ProjectSnapshotBuilder::new(temp.path(), repository()).unwrap();
            let overlays: [(&str, &[u8]); 3] = if reverse_input {
                [
                    ("a.rs", b"fn a() {}\n"),
                    ("b.rs", b"const B: i32 = 1;\n"),
                    ("c.rs", b"fn c(x: i32) -> i32 { x }\n"),
                ]
            } else {
                [
                    ("c.rs", b"fn c(x: i32) -> i32 { x }\n"),
                    ("b.rs", b"const B: i32 = 1;\n"),
                    ("a.rs", b"fn a() {}\n"),
                ]
            };
            for (path, source) in overlays {
                builder = builder.with_overlay(path, source.to_vec()).unwrap();
            }
            if with_prefix {
                builder = builder
                    .with_overlay("0.rs", b"fn zero() {}\n".to_vec())
                    .unwrap();
            }
            ProjectAnalysis::build(builder.build().unwrap()).unwrap()
        };
        let first = build(false, false);
        let second = build(true, false);
        assert_eq!(first.id(), second.id());
        assert_eq!(first.node_count(), 36);
        assert_eq!(
            first.node_ids().map(|id| id.index).collect::<Vec<_>>(),
            (0..36).collect::<Vec<_>>()
        );
        assert_eq!(
            first
                .file_node_ids(Path::new("a.rs"))
                .unwrap()
                .map(|id| id.index)
                .collect::<Vec<_>>(),
            (0..10).collect::<Vec<_>>()
        );
        assert_eq!(
            first
                .file_node_ids(Path::new("b.rs"))
                .unwrap()
                .map(|id| id.index)
                .collect::<Vec<_>>(),
            (10..19).collect::<Vec<_>>()
        );
        assert_eq!(
            first
                .file_node_ids(Path::new("c.rs"))
                .unwrap()
                .map(|id| id.index)
                .collect::<Vec<_>>(),
            (19..36).collect::<Vec<_>>()
        );

        let first_root = first.node_ids().next().unwrap();
        let second_root = second.node_ids().next().unwrap();
        assert_ne!(first_root, second_root);
        assert_eq!(
            second
                .node_by_key(first.node_key(first_root).unwrap())
                .unwrap()
                .id(),
            second_root
        );
        let sequence = |analysis: &ProjectAnalysis| {
            analysis
                .node_ids()
                .map(|id| {
                    let node = analysis.node(id).unwrap();
                    (
                        id.index,
                        node.path().to_path_buf(),
                        node.raw_kind().to_string(),
                        node.key().clone(),
                        node.parent().map(|parent| parent.index),
                        node.children()
                            .into_iter()
                            .map(|child| child.index)
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(sequence(&first), sequence(&second));
        let mut roots = Vec::new();
        let mut child_edges = 0;
        for id in first.node_ids() {
            let node = first.node(id).unwrap();
            match node.parent() {
                Some(parent) => {
                    assert_eq!(first.node(parent).unwrap().path(), node.path());
                    assert!(first.node(parent).unwrap().children().contains(&id));
                }
                None => roots.push(id.index),
            }
            for child in node.children() {
                child_edges += 1;
                assert_eq!(first.node(child).unwrap().parent(), Some(id));
                assert_eq!(first.node(child).unwrap().path(), node.path());
            }
        }
        assert_eq!(roots, [0, 10, 19]);
        assert_eq!(child_edges, 33);
        assert_eq!(
            second.node(first_root).unwrap_err(),
            NodeLookupError::WrongAnalysis
        );
        assert_eq!(
            first
                .node(NodeId {
                    owner: first.owner,
                    index: 35
                })
                .unwrap()
                .path(),
            Path::new("c.rs")
        );
        assert_eq!(
            first
                .node(NodeId {
                    owner: first.owner,
                    index: 36,
                })
                .unwrap_err(),
            NodeLookupError::OutOfRange {
                requested: 36,
                node_count: 36,
            }
        );
        assert_eq!(
            first
                .node(NodeId {
                    owner: second.owner,
                    index: u32::MAX,
                })
                .unwrap_err(),
            NodeLookupError::WrongAnalysis
        );
        assert_eq!(
            first
                .node(NodeId {
                    owner: first.owner,
                    index: u32::MAX,
                })
                .unwrap_err(),
            NodeLookupError::OutOfRange {
                requested: u32::MAX,
                node_count: 36,
            }
        );

        let before_keys = first
            .node_ids()
            .map(|id| first.node(id).unwrap())
            .filter(|node| node.path() == Path::new("a.rs"))
            .map(|node| node.key().clone())
            .collect::<Vec<_>>();
        let prefixed = build(true, true);
        let after_keys = prefixed
            .file_node_ids(Path::new("a.rs"))
            .unwrap()
            .map(|id| prefixed.node_key(id).unwrap().clone())
            .collect::<Vec<_>>();
        assert_eq!(before_keys, after_keys);
        assert_eq!(
            prefixed
                .file_node_ids(Path::new("a.rs"))
                .unwrap()
                .next()
                .unwrap()
                .index,
            10
        );
    }

    #[test]
    fn containment_indices_match_parent_oracle_and_disambiguate_equal_spans() {
        let temp = tempfile::tempdir().unwrap();
        let nested = b"fn outer() {\n    let closure = || { if true { value(); } };\n}\n";
        let build = || {
            let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
                .unwrap()
                .with_overlay("nested.rs", nested.to_vec())
                .unwrap()
                .with_overlay("peer.rs", b"fn peer() {}\n".to_vec())
                .unwrap()
                .build()
                .unwrap();
            ProjectAnalysis::build(snapshot).unwrap()
        };
        let first = build();
        let second = build();
        let nested_ids = first
            .file_node_ids(Path::new("nested.rs"))
            .unwrap()
            .collect::<Vec<_>>();
        assert_eq!(nested.len(), 62);
        assert_eq!(nested_ids.len(), 37);
        assert_eq!(nested_ids.first().unwrap().index, 0);
        assert_eq!(nested_ids.last().unwrap().index, 36);

        let parent_oracle = |ancestor: NodeId, mut descendant: NodeId| loop {
            if ancestor == descendant {
                break true;
            }
            let Some(parent) = first.node(descendant).unwrap().parent() else {
                break false;
            };
            descendant = parent;
        };
        let mut containment_pairs = 0;
        for ancestor in &nested_ids {
            let expected = nested_ids
                .iter()
                .copied()
                .filter(|descendant| parent_oracle(*ancestor, *descendant))
                .collect::<Vec<_>>();
            assert_eq!(
                first
                    .subtree_node_ids(*ancestor)
                    .unwrap()
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(
                first
                    .descendant_node_ids(*ancestor)
                    .unwrap()
                    .collect::<Vec<_>>(),
                expected[1..]
            );
            for descendant in &nested_ids {
                let indexed = first.node_contains(*ancestor, *descendant).unwrap();
                assert_eq!(indexed, parent_oracle(*ancestor, *descendant));
                containment_pairs += usize::from(indexed);
            }
        }
        assert_eq!(containment_pairs, 254);
        assert_eq!(containment_pairs - nested_ids.len(), 217);

        let find = |kind: &str, range: Range<usize>| {
            nested_ids
                .iter()
                .copied()
                .map(|id| first.node(id).unwrap())
                .find(|node| node.raw_kind() == kind && node.span().byte_range() == range)
                .unwrap()
        };
        let statement = find("expression_statement", 36..56);
        let conditional = find("if_expression", 36..56);
        assert!(
            first
                .node_contains(statement.id(), conditional.id())
                .unwrap()
        );
        assert!(
            !first
                .node_contains(conditional.id(), statement.id())
                .unwrap()
        );

        let literal = find("boolean_literal", 39..43);
        let token = find("true", 39..43);
        assert!(first.node_contains(literal.id(), token.id()).unwrap());
        assert!(!first.node_contains(token.id(), literal.id()).unwrap());
        for byte in 39..43 {
            let region = first
                .smallest_exclusive_syntax_region(Path::new("nested.rs"), byte)
                .unwrap();
            assert_eq!(region.byte_range(), 39..43);
            assert_eq!(region.owner(), ExclusiveSyntaxOwner::Node(token.id()));
            assert_eq!(region.kind(), ExclusiveSyntaxKind::Token);
            assert_eq!(region.text(), "true");
        }
        let SyntaxOwner::Node(range_owner) = first
            .smallest_containing_syntax(Path::new("nested.rs"), 36..56)
            .unwrap()
        else {
            panic!("equal-span conditional range must have a raw syntax owner");
        };
        assert_eq!(range_owner.id(), conditional.id());
        let SyntaxOwner::Node(range_owner) = first
            .smallest_containing_syntax(Path::new("nested.rs"), 39..43)
            .unwrap()
        else {
            panic!("boolean token range must have a raw syntax owner");
        };
        assert_eq!(range_owner.id(), token.id());
        let SyntaxOwner::Node(named_owner) = first
            .smallest_containing_named_syntax(Path::new("nested.rs"), 39..43)
            .unwrap()
        else {
            panic!("boolean token range must promote to named syntax");
        };
        assert_eq!(named_owner.id(), literal.id());

        let value = nested_ids
            .iter()
            .copied()
            .map(|id| first.node(id).unwrap())
            .find(|node| node.raw_kind() == "identifier" && node.text() == "value")
            .unwrap();
        let mut ancestors = Vec::new();
        let mut parent = value.parent();
        while let Some(id) = parent {
            let node = first.node(id).unwrap();
            ancestors.push(node.raw_kind().to_string());
            parent = node.parent();
        }
        assert_eq!(
            ancestors,
            [
                "call_expression",
                "expression_statement",
                "block",
                "if_expression",
                "expression_statement",
                "block",
                "closure_expression",
                "let_declaration",
                "block",
                "function_item",
                "source_file",
            ]
        );

        let nested_nodes = nested_ids
            .iter()
            .copied()
            .map(|id| first.node(id).unwrap())
            .collect::<Vec<_>>();
        let mut checked_ranges = 0;
        for start in 0..nested.len() {
            for end in start + 1..=nested.len() {
                let expected = nested_nodes
                    .iter()
                    .copied()
                    .filter(|node| {
                        node.span().start_byte() <= start && end <= node.span().end_byte()
                    })
                    .max_by_key(|node| node_depth(&first, node.id()))
                    .unwrap();
                let SyntaxOwner::Node(indexed) = first
                    .smallest_containing_syntax(Path::new("nested.rs"), start..end)
                    .unwrap()
                else {
                    panic!("nested fixture range {start}..{end} must be syntax owned");
                };
                assert_eq!(indexed.id(), expected.id(), "range {start}..{end}");
                checked_ranges += 1;
            }
        }
        assert_eq!(checked_ranges, 1_953);

        let peer_root = first
            .file_node_ids(Path::new("peer.rs"))
            .unwrap()
            .next()
            .unwrap();
        assert!(!first.node_contains(nested_ids[0], peer_root).unwrap());
        assert_eq!(
            first
                .subtree_node_ids(second.node_ids().next().unwrap())
                .unwrap_err(),
            NodeLookupError::WrongAnalysis
        );
        assert_eq!(
            first
                .node_contains(nested_ids[0], second.node_ids().next().unwrap())
                .unwrap_err(),
            NodeLookupError::WrongAnalysis
        );
        assert_eq!(
            first
                .node_contains(second.node_ids().next().unwrap(), nested_ids[0])
                .unwrap_err(),
            NodeLookupError::WrongAnalysis
        );
        let out_of_range = NodeId {
            owner: first.owner,
            index: u32::MAX,
        };
        assert!(matches!(
            first.subtree_node_ids(out_of_range).unwrap_err(),
            NodeLookupError::OutOfRange {
                requested: u32::MAX,
                ..
            }
        ));
        assert!(matches!(
            first.descendant_node_ids(out_of_range).unwrap_err(),
            NodeLookupError::OutOfRange {
                requested: u32::MAX,
                ..
            }
        ));
        assert!(matches!(
            first
                .node_contains(out_of_range, nested_ids[0])
                .unwrap_err(),
            NodeLookupError::OutOfRange {
                requested: u32::MAX,
                ..
            }
        ));
        assert!(matches!(
            first
                .node_contains(nested_ids[0], out_of_range)
                .unwrap_err(),
            NodeLookupError::OutOfRange {
                requested: u32::MAX,
                ..
            }
        ));
    }

    #[test]
    fn exclusive_syntax_index_is_total_strict_and_partial_safe() {
        let temp = tempfile::tempdir().unwrap();
        let source = b"\n  fn value() -> &'static str { /* c */ \"h\xc3\xa9\" }\n\t";
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("partition.rs", source.to_vec())
            .unwrap()
            .with_overlay(
                "missing.ts",
                b"function f(a: string { return a; }\n".to_vec(),
            )
            .unwrap()
            .with_overlay("broken.rs", vec![0xff, 0xfe])
            .unwrap()
            .with_overlay("empty.rs", Vec::new())
            .unwrap()
            .with_overlay("whitespace.rs", b"\t \r\n  \n".to_vec())
            .unwrap()
            .with_overlay("point.ts", b"if (value) ".to_vec())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();

        let regions = analysis
            .exclusive_syntax_regions(Path::new("partition.rs"))
            .unwrap()
            .collect::<Vec<_>>();
        assert_eq!(source.len(), 49);
        assert_eq!(regions.len(), 27);
        assert_eq!(
            regions
                .iter()
                .filter(|region| region.kind() == ExclusiveSyntaxKind::Token)
                .count(),
            14
        );
        assert_eq!(
            regions
                .iter()
                .filter(|region| region.kind() == ExclusiveSyntaxKind::Trivia)
                .count(),
            13
        );
        assert!(matches!(
            regions.first().unwrap().owner(),
            ExclusiveSyntaxOwner::File(key) if key.path == Path::new("partition.rs")
        ));
        let ExclusiveSyntaxOwner::Node(last_owner) = regions.last().unwrap().owner() else {
            panic!("trailing newline must remain inside the Rust source root");
        };
        assert_eq!(analysis.node(last_owner).unwrap().raw_kind(), "source_file");
        let mut reconstructed = Vec::new();
        let mut cursor = 0;
        for region in &regions {
            assert_eq!(region.path(), Path::new("partition.rs"));
            assert_eq!(region.byte_range().start, cursor);
            assert!(region.byte_range().end > cursor);
            reconstructed.extend_from_slice(region.bytes());
            cursor = region.byte_range().end;
        }
        assert_eq!(cursor, source.len());
        assert_eq!(reconstructed, source);
        for byte in 0..source.len() {
            let indexed = analysis
                .smallest_exclusive_syntax_region(Path::new("partition.rs"), byte)
                .unwrap();
            let linear = regions
                .iter()
                .find(|region| region.byte_range().contains(&byte))
                .unwrap();
            assert_eq!(indexed.byte_range(), linear.byte_range());
            assert_eq!(indexed.kind(), linear.kind());
            assert_eq!(indexed.owner(), linear.owner());
            assert_eq!(indexed.bytes(), linear.bytes());
            if let ExclusiveSyntaxOwner::Node(owner) = indexed.owner() {
                let owner = analysis.node(owner).unwrap();
                assert_eq!(owner.path(), Path::new("partition.rs"));
                assert!(
                    owner
                        .exclusive_syntax_regions()
                        .any(|owned| owned.byte_range() == indexed.byte_range())
                );
            }
        }
        let partition_nodes = analysis
            .file_node_ids(Path::new("partition.rs"))
            .unwrap()
            .map(|id| analysis.node(id).unwrap())
            .collect::<Vec<_>>();
        for byte in 0..source.len() {
            let indexed = analysis
                .smallest_exclusive_syntax_region(Path::new("partition.rs"), byte)
                .unwrap();
            let expected = partition_nodes
                .iter()
                .copied()
                .filter(|node| node.span().start_byte() <= byte && byte < node.span().end_byte())
                .max_by_key(|node| node_depth(&analysis, node.id()));
            match (expected, indexed.owner()) {
                (None, ExclusiveSyntaxOwner::File(key)) => {
                    assert_eq!(key.path, Path::new("partition.rs"));
                    assert_eq!(indexed.kind(), ExclusiveSyntaxKind::Trivia);
                }
                (Some(expected), ExclusiveSyntaxOwner::Node(owner)) => {
                    assert_eq!(owner, expected.id(), "byte {byte}");
                    let mut within_non_error_extra = false;
                    let mut current = Some(expected);
                    while let Some(node) = current {
                        within_non_error_extra |= node.is_extra() && !node.is_error();
                        current = node.parent().map(|parent| analysis.node(parent).unwrap());
                    }
                    let expected_kind = if expected.children().is_empty() && !within_non_error_extra
                    {
                        ExclusiveSyntaxKind::Token
                    } else {
                        ExclusiveSyntaxKind::Trivia
                    };
                    assert_eq!(indexed.kind(), expected_kind, "byte {byte}");
                }
                pair => panic!("exclusive owner mismatch at byte {byte}: {pair:?}"),
            }
        }
        let SyntaxOwner::File(key) = analysis
            .smallest_containing_syntax(Path::new("partition.rs"), 0..1)
            .unwrap()
        else {
            panic!("root-external prefix must remain file owned");
        };
        assert_eq!(key.path, Path::new("partition.rs"));
        let SyntaxOwner::Node(fn_token) = analysis
            .smallest_containing_syntax(Path::new("partition.rs"), 3..5)
            .unwrap()
        else {
            panic!("fn token must have a raw syntax owner");
        };
        assert_eq!(fn_token.raw_kind(), "fn");
        assert_eq!(
            analysis
                .smallest_exclusive_syntax_region(Path::new("partition.rs"), 2)
                .unwrap()
                .byte_range(),
            0..3
        );
        assert_eq!(
            analysis
                .smallest_exclusive_syntax_region(Path::new("partition.rs"), 3)
                .unwrap()
                .byte_range(),
            3..5
        );
        assert_eq!(
            analysis
                .smallest_exclusive_syntax_region(Path::new("partition.rs"), 5)
                .unwrap()
                .byte_range(),
            5..6
        );
        for byte in [47, 48] {
            assert_eq!(
                analysis
                    .smallest_exclusive_syntax_region(Path::new("partition.rs"), byte)
                    .unwrap()
                    .byte_range(),
                47..49
            );
        }
        assert_eq!(
            analysis
                .smallest_exclusive_syntax_region(Path::new("partition.rs"), source.len())
                .unwrap_err(),
            ExclusiveSyntaxLookupError::ByteOutOfRange {
                requested: source.len(),
                source_len: source.len(),
            }
        );
        assert_eq!(
            analysis
                .smallest_exclusive_syntax_region(Path::new("partition.rs"), usize::MAX)
                .unwrap_err(),
            ExclusiveSyntaxLookupError::ByteOutOfRange {
                requested: usize::MAX,
                source_len: source.len(),
            }
        );

        let missing = analysis
            .file_node_ids(Path::new("missing.ts"))
            .unwrap()
            .map(|id| analysis.node(id).unwrap())
            .find(|node| node.is_missing())
            .unwrap();
        assert_eq!(missing.span().byte_range(), 20..20);
        assert_eq!(
            analysis
                .subtree_node_ids(missing.id())
                .unwrap()
                .collect::<Vec<_>>(),
            [missing.id()]
        );
        assert!(
            analysis
                .node_contains(missing.parent().unwrap(), missing.id())
                .unwrap()
        );
        assert!(
            analysis
                .exclusive_syntax_regions(Path::new("missing.ts"))
                .unwrap()
                .all(|region| region.owner() != ExclusiveSyntaxOwner::Node(missing.id()))
        );
        assert_ne!(
            analysis
                .smallest_exclusive_syntax_region(Path::new("missing.ts"), 20)
                .unwrap()
                .owner(),
            ExclusiveSyntaxOwner::Node(missing.id())
        );
        let point = analysis
            .syntax_point_context(Path::new("missing.ts"), 20)
            .unwrap();
        assert_eq!(point.exact_zero_width().len(), 1);
        assert_eq!(point.exact_zero_width()[0].id(), missing.id());
        let SyntaxOwner::Node(after) = point.after().unwrap() else {
            panic!("byte after the missing token must remain syntax owned");
        };
        assert_eq!(after.raw_kind(), "function_declaration");
        assert_eq!(
            analysis
                .smallest_exclusive_syntax_region(Path::new("missing.ts"), 20)
                .unwrap()
                .byte_range(),
            20..21
        );

        assert_eq!(
            analysis
                .smallest_containing_syntax(Path::new("partition.rs"), 5..5)
                .unwrap_err(),
            NodeRangeLookupError::EmptyRangeRequiresPointLookup { byte: 5 }
        );
        assert_eq!(
            analysis
                .smallest_containing_syntax(Path::new("partition.rs"), Range { start: 6, end: 5 },)
                .unwrap_err(),
            NodeRangeLookupError::ReversedRange { start: 6, end: 5 }
        );
        assert_eq!(
            analysis
                .smallest_containing_syntax(
                    Path::new("partition.rs"),
                    Range {
                        start: source.len() + 1,
                        end: source.len() + 1,
                    },
                )
                .unwrap_err(),
            NodeRangeLookupError::RangeOutOfBounds {
                start: source.len() + 1,
                end: source.len() + 1,
                source_len: source.len(),
            }
        );
        assert_eq!(
            analysis
                .smallest_containing_syntax(
                    Path::new("partition.rs"),
                    Range {
                        start: usize::MAX,
                        end: usize::MAX,
                    },
                )
                .unwrap_err(),
            NodeRangeLookupError::RangeOutOfBounds {
                start: usize::MAX,
                end: usize::MAX,
                source_len: source.len(),
            }
        );
        assert_eq!(
            analysis
                .smallest_containing_syntax(Path::new("partition.rs"), 48..50)
                .unwrap_err(),
            NodeRangeLookupError::RangeOutOfBounds {
                start: 48,
                end: 50,
                source_len: source.len(),
            }
        );
        assert_eq!(
            analysis
                .syntax_point_context(Path::new("partition.rs"), usize::MAX)
                .unwrap_err(),
            NodeRangeLookupError::PointOutOfBounds {
                byte: usize::MAX,
                source_len: source.len(),
            }
        );

        assert!(
            analysis
                .exclusive_syntax_regions(Path::new("empty.rs"))
                .unwrap()
                .next()
                .is_none()
        );
        assert_eq!(
            analysis
                .smallest_exclusive_syntax_region(Path::new("empty.rs"), 0)
                .unwrap_err(),
            ExclusiveSyntaxLookupError::ByteOutOfRange {
                requested: 0,
                source_len: 0,
            }
        );
        let empty_point = analysis
            .syntax_point_context(Path::new("empty.rs"), 0)
            .unwrap();
        assert_eq!(empty_point.exact_zero_width().len(), 1);
        assert_eq!(empty_point.exact_zero_width()[0].raw_kind(), "source_file");
        assert!(empty_point.before().is_none());
        assert!(empty_point.after().is_none());

        let whitespace_point = analysis
            .syntax_point_context(Path::new("whitespace.rs"), 7)
            .unwrap();
        assert_eq!(whitespace_point.exact_zero_width().len(), 1);
        assert_eq!(
            whitespace_point.exact_zero_width()[0].span().byte_range(),
            7..7
        );
        assert!(matches!(
            whitespace_point.before(),
            Some(SyntaxOwner::File(key)) if key.path == Path::new("whitespace.rs")
        ));
        assert!(whitespace_point.after().is_none());
        let whitespace_regions = analysis
            .exclusive_syntax_regions(Path::new("whitespace.rs"))
            .unwrap()
            .collect::<Vec<_>>();
        assert_eq!(whitespace_regions.len(), 1);
        assert_eq!(whitespace_regions[0].byte_range(), 0..7);
        assert!(matches!(
            whitespace_regions[0].owner(),
            ExclusiveSyntaxOwner::File(key) if key.path == Path::new("whitespace.rs")
        ));
        let shared_point = analysis
            .syntax_point_context(Path::new("point.ts"), 10)
            .unwrap();
        assert_eq!(shared_point.exact_zero_width().len(), 1);
        assert_eq!(shared_point.exact_zero_width()[0].raw_kind(), ";");
        assert!(shared_point.exact_zero_width()[0].is_missing());
        let Some(SyntaxOwner::Node(after)) = shared_point.after() else {
            panic!("trailing TypeScript space must remain program-owned trivia");
        };
        assert_eq!(after.raw_kind(), "program");
        assert_ne!(after.id(), shared_point.exact_zero_width()[0].id());
        assert_eq!(
            analysis
                .exclusive_syntax_regions(Path::new("broken.rs"))
                .unwrap_err(),
            ExclusiveSyntaxLookupError::SyntaxUnavailable {
                path: PathBuf::from("broken.rs"),
            }
        );
        assert_eq!(
            analysis
                .exclusive_syntax_regions(Path::new("absent.rs"))
                .unwrap_err(),
            ExclusiveSyntaxLookupError::FileNotFound {
                path: PathBuf::from("absent.rs"),
            }
        );
    }

    #[test]
    fn node_keys_round_trip_and_expire_with_file_revision() {
        let temp = tempfile::tempdir().unwrap();
        let build = |source: &[u8]| {
            let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
                .unwrap()
                .with_overlay("key.rs", source.to_vec())
                .unwrap()
                .build()
                .unwrap();
            ProjectAnalysis::build(snapshot).unwrap()
        };
        let first = build(b"fn stable() { value(); }\n");
        let function = first
            .node_ids()
            .map(|id| first.node(id).unwrap())
            .find(|node| node.raw_kind() == "function_item")
            .unwrap();
        let key = function.key().clone();
        assert_eq!(key.schema(), "deslop.node-key/1");
        assert_eq!(key.arena_schema(), "deslop-raw-arena/1");
        assert_eq!(key.file(), function.file_key());
        assert_eq!(key.raw_grammar_kind(), "function_item");
        assert_eq!(key.anchor().start_byte(), 0);
        assert!(key.anchor().structural_digest().starts_with("nsa1_"));
        assert_eq!(key.collision_ordinal(), 0);
        let json = serde_json::to_string(&key).unwrap();
        let decoded: NodeKey = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, key);
        assert_eq!(first.node_by_key(&decoded).unwrap().id(), function.id());

        let mut value = serde_json::to_value(&key).unwrap();
        let fields = value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        assert_eq!(
            fields,
            [
                "anchor",
                "arena_schema",
                "collision_ordinal",
                "field_path",
                "file",
                "raw_grammar_kind",
                "raw_grammar_kind_id",
                "schema",
            ]
            .into_iter()
            .map(str::to_string)
            .collect()
        );
        assert!(!json.contains("owner"));
        assert!(!json.contains("canonical_role"));
        value
            .as_object_mut()
            .unwrap()
            .insert("canonical_role".to_string(), serde_json::json!("callable"));
        assert!(serde_json::from_value::<NodeKey>(value).is_err());

        let mut wrong_schema = serde_json::to_value(&key).unwrap();
        wrong_schema["schema"] = serde_json::json!("deslop.node-key/999");
        assert!(serde_json::from_value::<NodeKey>(wrong_schema).is_err());

        let mut wrong_arena = serde_json::to_value(&key).unwrap();
        wrong_arena["arena_schema"] = serde_json::json!("deslop-raw-arena/999");
        assert!(serde_json::from_value::<NodeKey>(wrong_arena).is_err());

        let mut uppercase_source = serde_json::to_value(&key).unwrap();
        uppercase_source["file"]["source"] = serde_json::json!(format!("sr1_{}", "A".repeat(64)));
        assert!(serde_json::from_value::<NodeKey>(uppercase_source).is_err());

        let mut forged_source = serde_json::to_value(&key).unwrap();
        forged_source["file"]["source"] = serde_json::json!(format!("sr1_{}", "0".repeat(64)));
        let forged_source: NodeKey = serde_json::from_value(forged_source).unwrap();
        assert_eq!(
            first.node_by_key(&forged_source).unwrap_err(),
            NodeKeyLookupError::FileRevisionExpired
        );

        let mut absolute_path = serde_json::to_value(&key).unwrap();
        absolute_path["file"]["path"] = serde_json::json!("/key.rs");
        assert!(serde_json::from_value::<NodeKey>(absolute_path).is_err());

        let mut reversed_anchor = serde_json::to_value(&key).unwrap();
        reversed_anchor["anchor"]["start_byte"] = serde_json::json!(10);
        reversed_anchor["anchor"]["end_byte"] = serde_json::json!(1);
        assert!(serde_json::from_value::<NodeKey>(reversed_anchor).is_err());

        let mut wrong_ordinal = serde_json::to_value(&key).unwrap();
        wrong_ordinal["collision_ordinal"] = serde_json::json!(99);
        let wrong_ordinal: NodeKey = serde_json::from_value(wrong_ordinal).unwrap();
        assert_eq!(
            first.node_by_key(&wrong_ordinal).unwrap_err(),
            NodeKeyLookupError::NotFound
        );

        let changed = build(b"fn stable() { changed(); }\n");
        assert_eq!(
            changed.node_by_key(&key).unwrap_err(),
            NodeKeyLookupError::FileRevisionExpired
        );
        assert_ne!(
            changed
                .node_ids()
                .map(|id| changed.node_key(id).unwrap())
                .find(|candidate| candidate.raw_grammar_kind() == "function_item")
                .unwrap(),
            &key
        );

        let peer_changed = build(b"fn stable() { value(); }\n \t");
        let peer_function = node_by_kind(&peer_changed, "function_item");
        assert_eq!(
            function.baseline_fingerprint(),
            peer_function.baseline_fingerprint()
        );
        assert_ne!(function.key(), peer_function.key());
        assert_eq!(
            peer_changed.node_by_key(&key).unwrap_err(),
            NodeKeyLookupError::FileRevisionExpired
        );
        let guard_for = |node: NodeView<'_>| {
            let span = node.span();
            revision_guard(
                node.path(),
                Span::new(
                    span.start_point().row() + 1,
                    span.end_point().row() + 1,
                    span.start_byte(),
                    span.end_byte(),
                ),
                node.text(),
            )
        };
        assert_eq!(guard_for(function), guard_for(peer_function));
    }

    #[test]
    fn baseline_fingerprints_are_fuzzy_ambiguous_and_never_node_keys() {
        let temp = tempfile::tempdir().unwrap();
        let build = |source: &[u8]| {
            let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
                .unwrap()
                .with_overlay("baseline.rs", source.to_vec())
                .unwrap()
                .build()
                .unwrap();
            ProjectAnalysis::build(snapshot).unwrap()
        };
        let original = build(b"fn stable() { value(); }\n");
        let relocated = build(b"\n\nfn stable() { value(); }\n");
        let changed = build(b"fn stable() { changed(); }\n");
        let original_node = node_by_kind(&original, "function_item");
        let relocated_node = node_by_kind(&relocated, "function_item");
        let changed_node = node_by_kind(&changed, "function_item");

        assert_eq!(
            original_node.baseline_fingerprint(),
            relocated_node.baseline_fingerprint()
        );
        assert_ne!(original_node.key(), relocated_node.key());
        assert_ne!(
            original_node.baseline_fingerprint(),
            changed_node.baseline_fingerprint()
        );

        let duplicates = build(b"fn same() {}\nfn same() {}\n");
        let duplicate_fingerprints = duplicates
            .node_ids()
            .map(|id| duplicates.node(id).unwrap())
            .filter(|node| node.raw_kind() == "function_item")
            .map(|node| node.baseline_fingerprint())
            .collect::<Vec<_>>();
        assert_eq!(duplicate_fingerprints.len(), 2);
        assert_eq!(duplicate_fingerprints[0], duplicate_fingerprints[1]);
        let duplicate_keys = duplicates
            .node_ids()
            .map(|id| duplicates.node(id).unwrap())
            .filter(|node| node.raw_kind() == "function_item")
            .map(|node| node.key().clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(duplicate_keys.len(), 2);
    }

    #[test]
    fn node_key_structural_anchor_has_a_pinned_raw_grammar_vector() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("anchor.rs", b"fn a(){same();}\n".to_vec())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        assert_eq!(analysis.node_count(), 17);
        let call = node_by_kind(&analysis, "call_expression");
        assert_eq!(call.key().raw_grammar_kind_id(), 256);
        assert_eq!(
            call.key().field_path(),
            &[None, Some("body".to_string()), None, None]
        );
        assert_eq!(call.key().anchor().start_byte(), 7);
        assert_eq!(call.key().anchor().end_byte(), 13);
        assert_eq!(call.key().anchor().start_row(), 0);
        assert_eq!(call.key().anchor().start_column(), 7);
        assert_eq!(call.key().anchor().end_row(), 0);
        assert_eq!(call.key().anchor().end_column(), 13);
        assert_eq!(
            call.key().anchor().structural_digest(),
            "nsa1_2e71d4d3ed08b9955a5d305e4d79667b5933bdd90860055902470563646d464c"
        );
    }

    #[test]
    fn file_revision_wire_paths_are_portable_and_strict() {
        assert_eq!(
            encode_wire_repo_path(Path::new("nested/file.rs")).unwrap(),
            "nested/file.rs"
        );
        assert_eq!(
            decode_wire_repo_path("nested/file.rs").unwrap(),
            PathBuf::from("nested/file.rs")
        );
        assert_eq!(
            encode_wire_repo_path(Path::new("percent%file.rs")).unwrap(),
            "percent%25file.rs"
        );
        assert_eq!(
            decode_wire_repo_path("percent%25file.rs").unwrap(),
            PathBuf::from("percent%file.rs")
        );
        #[cfg(unix)]
        {
            assert!(encode_wire_repo_path(Path::new("a\\b.rs")).is_err());
            assert!(normalize_logical_path(Path::new("a\\b.rs")).is_err());
            assert!(
                ProjectSnapshotBuilder::new(tempfile::tempdir().unwrap().path(), repository())
                    .unwrap()
                    .with_overlay("a\\b.rs", b"fn ambiguous() {}\n".to_vec())
                    .is_err()
            );
        }
        for invalid in [
            "",
            "/abs.rs",
            "./a.rs",
            "a/../b.rs",
            "a//b.rs",
            "a\\b.rs",
            "a%5cb.rs",
            "a%5c..%5csecret.rs",
        ] {
            assert!(decode_wire_repo_path(invalid).is_err(), "{invalid}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("outside.rs");
        std::fs::write(&target, "fn outside() {}\n").unwrap();
        symlink(&target, root.path().join("escape.rs")).unwrap();
        let error = ProjectSnapshotBuilder::new(root.path(), repository())
            .unwrap()
            .with_scope(&[PathBuf::from("escape.rs")])
            .build()
            .unwrap_err();
        assert!(error.to_string().contains("outside repository root"));
    }

    #[cfg(unix)]
    #[test]
    fn recursively_discovered_symlink_escape_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("outside.rs");
        std::fs::write(&target, "fn outside() {}\n").unwrap();
        symlink(&target, root.path().join("escape.rs")).unwrap();
        let error = ProjectSnapshotBuilder::new(root.path(), repository())
            .unwrap()
            .build()
            .unwrap_err();
        assert!(error.to_string().contains("outside repository root"));
    }

    #[cfg(unix)]
    #[test]
    fn explicitly_requested_in_root_symlink_collapses_to_target() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("target.rs"), "fn target() {}\n").unwrap();
        symlink(root.path().join("target.rs"), root.path().join("alias.rs")).unwrap();
        let target = ProjectSnapshotBuilder::new(root.path(), repository())
            .unwrap()
            .with_scope(&[PathBuf::from("target.rs")])
            .build()
            .unwrap();
        let alias = ProjectSnapshotBuilder::new(root.path(), repository())
            .unwrap()
            .with_scope(&[PathBuf::from("alias.rs")])
            .build()
            .unwrap();
        assert_eq!(target.id(), alias.id());
        assert_eq!(alias.entries().count(), 1);
        assert_eq!(alias.read_counts().get(Path::new("target.rs")), Some(&1));
    }
}
