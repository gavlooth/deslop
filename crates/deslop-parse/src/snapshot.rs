use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hash;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result, bail};
use deslop_core::{AnalysisDiagnostic, AnalysisProvenance, Lang};
use deslop_lang::Registry;
use ignore::WalkBuilder;
use tree_sitter::{Parser, Tree};

use crate::analysis_provenance_for_tree;
#[cfg(test)]
use crate::arena::{ArenaNodeIndex, ArenaSegmentIndex};
use crate::arena::{RAW_ARENA_SCHEMA, SyntaxArena};

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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
}

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
        Ok(Arc::new(Self {
            id,
            snapshot,
            files,
            ledger,
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
            Component::Normal(part) => normalized.push(part),
            _ => bail!("logical path {} is not normalized", path.display()),
        }
    }
    if normalized.as_os_str().is_empty() {
        bail!("logical path {} must name an entry", path.display());
    }
    if normalized.to_str().is_none() {
        bail!("logical path is not valid Unicode");
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
    use deslop_core::AnalysisStatus;

    fn repository() -> RepositoryId {
        RepositoryId::explicit("test-repository").unwrap()
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
