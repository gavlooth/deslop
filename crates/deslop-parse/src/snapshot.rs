use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::hash::Hash;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result, bail};
use deslop_core::{AnalysisDiagnostic, AnalysisProvenance, Lang};
use deslop_lang::{
    LangPack, LanguageAdapterCapabilityManifest, LanguageConstructPolicy, LanguageLexicalPolicy,
    LanguageQueryPack, Registry,
};
use ignore::WalkBuilder;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tree_sitter::{Parser, Tree};

use crate::aggregation::{
    InclusiveSyntaxPolicy, SyntaxAggregateOwner, SyntaxAggregateParts, SyntaxAggregateProjection,
    SyntaxAggregates, SyntaxAggregationError,
};
use crate::analysis_provenance_for_tree;
use crate::arena::{
    ArenaNodeIndex, ArenaSegmentIndex, RAW_ARENA_SCHEMA, SyntaxArena, SyntaxSegmentKind,
    SyntaxSegmentOwner,
};
use crate::identity::{
    NodeBaselineFingerprint, NodeId, NodeKey, NodeKeyLookupError, NodeLookupError,
    baseline_fingerprint, build_node_keys,
};
use crate::instrumentation::{
    AnalysisMemoryInstrumentation, AnalysisStructureInstrumentation, ParseOwnershipInstrumentation,
    ProjectAnalysisInstrumentation,
};

const SOURCE_REVISION_DOMAIN: &str = "deslop source revision v1";
const SNAPSHOT_ID_DOMAIN: &str = "deslop project snapshot v1";
const ANALYSIS_ID_DOMAIN: &str = "deslop project analysis v1";
const PROJECTION_ID_DOMAIN: &str = "deslop analysis projection v1";
const LOCAL_REPOSITORY_DOMAIN: &str = "deslop local repository v1";
const VCS_REPOSITORY_DOMAIN: &str = "deslop vcs repository v1";
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

    pub fn vcs(primary_remote: Option<&str>, root_commits: &[String]) -> Result<Self> {
        let primary_remote = primary_remote
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let mut roots = root_commits
            .iter()
            .map(|root| root.trim())
            .filter(|root| !root.is_empty())
            .collect::<Vec<_>>();
        roots.sort_unstable();
        roots.dedup();
        if primary_remote.is_none() && roots.is_empty() {
            bail!("VCS repository identity requires a remote or root commit");
        }
        let mut parts = Vec::with_capacity(roots.len() + 1);
        parts.push(primary_remote.unwrap_or("").as_bytes());
        parts.extend(roots.into_iter().map(str::as_bytes));
        Ok(Self(format!(
            "repo1_{}",
            domain_digest(VCS_REPOSITORY_DOMAIN, parts)
        )))
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

    pub(crate) fn identity_bytes(&self) -> Vec<u8> {
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

    fn known_payload_bytes(&self) -> usize {
        self.dialect.len()
            + self.selector.len()
            + self.grammar_id.len()
            + self.grammar_version.len()
            + self.parser_build.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageAdapterIdentity {
    name: String,
    schema: String,
    capabilities: LanguageAdapterCapabilityManifest,
    queries: LanguageQueryPack,
    lexical: LanguageLexicalPolicy,
    constructs: LanguageConstructPolicy,
}

impl LanguageAdapterIdentity {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn capabilities(&self) -> &LanguageAdapterCapabilityManifest {
        &self.capabilities
    }

    pub fn queries(&self) -> &LanguageQueryPack {
        &self.queries
    }

    pub fn lexical_policy(&self) -> &LanguageLexicalPolicy {
        &self.lexical
    }

    pub fn construct_policy(&self) -> &LanguageConstructPolicy {
        &self.constructs
    }

    fn identity_bytes(&self) -> Vec<u8> {
        fn push_part(bytes: &mut Vec<u8>, part: &[u8]) {
            bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
            bytes.extend_from_slice(part);
        }

        let mut bytes = Vec::new();
        for part in [
            self.name.as_bytes(),
            self.schema.as_bytes(),
            self.capabilities.schema().as_bytes(),
            self.capabilities.adapter_schema().as_bytes(),
        ] {
            push_part(&mut bytes, part);
        }
        for declaration in self.capabilities.capabilities() {
            push_part(&mut bytes, declaration.capability().as_str().as_bytes());
            push_part(&mut bytes, declaration.support().as_str().as_bytes());
            push_part(
                &mut bytes,
                declaration
                    .authority()
                    .map_or(b"", |authority| authority.as_str().as_bytes()),
            );
        }
        push_part(&mut bytes, self.queries.schema().as_bytes());
        push_part(&mut bytes, self.queries.adapter_schema().as_bytes());
        for declaration in self.queries.queries() {
            push_part(&mut bytes, declaration.family().as_str().as_bytes());
            push_part(&mut bytes, declaration.support().as_str().as_bytes());
            push_part(
                &mut bytes,
                declaration
                    .authority()
                    .map_or(b"", |authority| authority.as_str().as_bytes()),
            );
            push_part(&mut bytes, declaration.source().map_or(b"", str::as_bytes));
            push_part(
                &mut bytes,
                &(declaration.captures().len() as u64).to_le_bytes(),
            );
            for capture in declaration.captures() {
                push_part(&mut bytes, capture.name().as_bytes());
                push_part(&mut bytes, &(capture.roles().len() as u64).to_le_bytes());
                for role in capture.roles().iter() {
                    push_part(&mut bytes, role.as_str().as_bytes());
                }
            }
        }
        push_part(&mut bytes, self.lexical.schema().as_bytes());
        push_part(&mut bytes, self.lexical.adapter_schema().as_bytes());
        push_part(&mut bytes, self.lexical.support().as_str().as_bytes());
        push_part(
            &mut bytes,
            self.lexical
                .authority()
                .map_or(b"", |authority| authority.as_str().as_bytes()),
        );
        push_part(
            &mut bytes,
            self.lexical
                .identifier_case()
                .map_or(b"", |policy| policy.as_str().as_bytes()),
        );
        push_part(
            &mut bytes,
            self.lexical
                .unicode_identifiers()
                .map_or(b"", |enabled| if enabled { b"true" } else { b"false" }),
        );
        for delimiter in self.lexical.line_comments() {
            push_part(&mut bytes, delimiter.as_bytes());
        }
        for delimiter in self.lexical.block_comments() {
            push_part(&mut bytes, delimiter.open().as_bytes());
            push_part(&mut bytes, delimiter.close().as_bytes());
            push_part(&mut bytes, &[u8::from(delimiter.nested())]);
        }
        for rule in self.lexical.rules() {
            push_part(&mut bytes, rule.raw_kind().as_bytes());
            push_part(&mut bytes, rule.text().map_or(b"", str::as_bytes));
            push_part(
                &mut bytes,
                rule.classification().token_class().as_str().as_bytes(),
            );
            push_part(
                &mut bytes,
                rule.classification()
                    .operator_class()
                    .map_or(b"", |operator| operator.as_str().as_bytes()),
            );
        }
        push_part(&mut bytes, self.constructs.schema().as_bytes());
        push_part(&mut bytes, self.constructs.adapter_schema().as_bytes());
        push_part(
            &mut bytes,
            self.constructs
                .parse_recovery()
                .support()
                .as_str()
                .as_bytes(),
        );
        push_part(
            &mut bytes,
            self.constructs
                .parse_recovery()
                .authority()
                .map_or(b"", |authority| authority.as_str().as_bytes()),
        );
        push_part(
            &mut bytes,
            self.constructs
                .parse_recovery()
                .handling()
                .map_or(b"", |handling| handling.as_str().as_bytes()),
        );
        for section in self.constructs.constructs() {
            push_part(&mut bytes, section.kind().as_str().as_bytes());
            push_part(&mut bytes, section.support().as_str().as_bytes());
            push_part(
                &mut bytes,
                section
                    .authority()
                    .map_or(b"", |authority| authority.as_str().as_bytes()),
            );
            for rule in section.rules() {
                push_part(&mut bytes, rule.raw_kind().as_bytes());
                push_part(&mut bytes, rule.text().map_or(b"", str::as_bytes));
                push_part(&mut bytes, rule.handling().as_str().as_bytes());
            }
        }
        push_part(
            &mut bytes,
            self.constructs.dialects().support().as_str().as_bytes(),
        );
        push_part(
            &mut bytes,
            self.constructs
                .dialects()
                .authority()
                .map_or(b"", |authority| authority.as_str().as_bytes()),
        );
        for variant in self.constructs.dialects().variants() {
            push_part(&mut bytes, variant.dialect().as_bytes());
            push_part(&mut bytes, variant.grammar_id().as_bytes());
            push_part(&mut bytes, variant.grammar_version().as_bytes());
        }
        bytes
    }
}

#[derive(Clone)]
struct StoredLangAdapter {
    pack: &'static dyn LangPack,
    identity: LanguageAdapterIdentity,
}

impl fmt::Debug for StoredLangAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("StoredLangAdapter")
            .field(&self.identity)
            .finish()
    }
}

fn resolve_grammar(
    path: &Path,
    registry: &Registry,
) -> Result<(GrammarSelection, tree_sitter::Language, StoredLangAdapter)> {
    let adapter = registry
        .supported_pack_for_path(path)
        .ok_or_else(|| anyhow::anyhow!("no language adapter for {}", path.display()))?;
    let resolved = adapter
        .resolve_grammar(path)
        .ok_or_else(|| anyhow::anyhow!("no grammar artifact for {}", path.display()))?;
    let (descriptor, language) = resolved.into_parts();
    if descriptor.lang() != adapter.lang() {
        bail!(
            "language adapter {} selected {:?} but grammar artifact declares {:?} for {}",
            adapter.name(),
            adapter.lang(),
            descriptor.lang(),
            path.display()
        );
    }
    let capabilities = adapter.capability_manifest();
    capabilities.validate().map_err(|error| {
        anyhow::anyhow!(
            "invalid capability manifest for language adapter {}: {error}",
            adapter.name()
        )
    })?;
    if capabilities.adapter_schema() != adapter.adapter_schema() {
        bail!(
            "language adapter {} capability manifest targets {} but adapter schema is {}",
            adapter.name(),
            capabilities.adapter_schema(),
            adapter.adapter_schema()
        );
    }
    let queries = adapter.query_pack();
    queries.validate().map_err(|error| {
        anyhow::anyhow!(
            "invalid query pack for language adapter {}: {error}",
            adapter.name()
        )
    })?;
    if queries.adapter_schema() != adapter.adapter_schema() {
        bail!(
            "language adapter {} query pack targets {} but adapter schema is {}",
            adapter.name(),
            queries.adapter_schema(),
            adapter.adapter_schema()
        );
    }
    let lexical = adapter.lexical_policy();
    lexical.validate().map_err(|error| {
        anyhow::anyhow!(
            "invalid lexical policy for language adapter {}: {error}",
            adapter.name()
        )
    })?;
    if lexical.adapter_schema() != adapter.adapter_schema() {
        bail!(
            "language adapter {} lexical policy targets {} but adapter schema is {}",
            adapter.name(),
            lexical.adapter_schema(),
            adapter.adapter_schema()
        );
    }
    let constructs = adapter.construct_policy();
    constructs.validate().map_err(|error| {
        anyhow::anyhow!(
            "invalid construct policy for language adapter {}: {error}",
            adapter.name()
        )
    })?;
    if constructs.adapter_schema() != adapter.adapter_schema() {
        bail!(
            "language adapter {} construct policy targets {} but adapter schema is {}",
            adapter.name(),
            constructs.adapter_schema(),
            adapter.adapter_schema()
        );
    }
    Ok((
        GrammarSelection::from_descriptor(descriptor),
        language,
        StoredLangAdapter {
            pack: adapter,
            identity: LanguageAdapterIdentity {
                name: adapter.name().to_string(),
                schema: adapter.adapter_schema().to_string(),
                capabilities,
                queries,
                lexical,
                constructs,
            },
        },
    ))
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

impl FileRevisionKey {
    pub(crate) fn known_payload_bytes(&self) -> usize {
        self.repository.0.len()
            + self
                .path
                .to_str()
                .expect("snapshot paths are validated Unicode")
                .len()
            + self.source.0.len()
            + self.grammar.known_payload_bytes()
    }
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
        adapter: Box<StoredLangAdapter>,
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

    pub(crate) fn grammar_language(&self) -> Option<&tree_sitter::Language> {
        match &self.analysis {
            EntryAnalysis::Source { language, .. } => Some(language),
            EntryAnalysis::AnalysisInput => None,
        }
    }

    pub fn language_adapter(&self) -> Option<&'static dyn LangPack> {
        match &self.analysis {
            EntryAnalysis::Source { adapter, .. } => Some(adapter.pack),
            EntryAnalysis::AnalysisInput => None,
        }
    }

    pub fn language_adapter_identity(&self) -> Option<&LanguageAdapterIdentity> {
        match &self.analysis {
            EntryAnalysis::Source { adapter, .. } => Some(&adapter.identity),
            EntryAnalysis::AnalysisInput => None,
        }
    }

    pub(crate) fn stored_source(&self) -> &Arc<StoredSource> {
        &self.source
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
    ExactLogicalFiles(Vec<PathBuf>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryPolicy {
    Canonical,
    LegacyRespectIgnore,
}

pub struct ProjectSnapshotBuilder {
    root: PathBuf,
    invocation_base: PathBuf,
    repository: RepositoryId,
    requested_scope: ScopeSpec,
    overlays: BTreeMap<PathBuf, Vec<u8>>,
    analysis_inputs: BTreeMap<PathBuf, Vec<u8>>,
    disk_analysis_inputs: BTreeSet<PathBuf>,
    store: Arc<SourceStore>,
    registry: Registry,
    discovery: DiscoveryPolicy,
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
            disk_analysis_inputs: BTreeSet::new(),
            store: Arc::new(SourceStore::default()),
            registry: Registry::default(),
            discovery: DiscoveryPolicy::Canonical,
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

    pub fn with_registry(mut self, registry: Registry) -> Self {
        self.registry = registry;
        self
    }

    pub fn with_discovery_policy(mut self, discovery: DiscoveryPolicy) -> Self {
        self.discovery = discovery;
        self
    }

    pub fn with_disk_analysis_input(mut self, path: impl AsRef<Path>) -> Result<Self> {
        let path = normalize_builder_input_path(&self.root, path.as_ref())?;
        self.disk_analysis_inputs.insert(path);
        Ok(self)
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
        let (requested_scope, exact_files) = match &self.requested_scope {
            ScopeSpec::DefaultAtInvocationBase => (
                normalize_scope(&self.root, &self.invocation_base, &[PathBuf::from(".")])?,
                false,
            ),
            ScopeSpec::Requested(scope) if scope.is_empty() => bail!(
                "requested scope must contain at least one path; use ExactFiles for an exact empty set"
            ),
            ScopeSpec::Requested(scope) => (
                normalize_scope(&self.root, &self.invocation_base, scope)?,
                false,
            ),
            ScopeSpec::ExactFiles(scope) => (
                normalize_scope(&self.root, &self.invocation_base, scope)?,
                true,
            ),
            ScopeSpec::ExactLogicalFiles(scope) => (
                scope
                    .iter()
                    .map(|path| {
                        Ok(ScopeEntry {
                            path: normalize_logical_path(path)?,
                            kind: ScopeEntryKind::File,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
                true,
            ),
        };
        if exact_files
            && requested_scope
                .iter()
                .any(|entry| entry.kind != ScopeEntryKind::File)
        {
            bail!("exact file scope contains a directory");
        }
        let disk_sources =
            collect_disk_sources(&self.root, &requested_scope, &self.registry, self.discovery)?;
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
            if self.registry.supported_pack_for_path(&path).is_none() {
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
        for path in self.disk_analysis_inputs {
            if inputs.contains_key(&path) {
                continue;
            }
            let physical = self.root.join(&path);
            let bytes = std::fs::read(&physical)
                .with_context(|| format!("failed to read analysis input {}", physical.display()))?;
            *read_counts.entry(path.clone()).or_default() += 1;
            inputs.insert(path, (SnapshotEntryKind::AnalysisInput, bytes));
        }
        for (path, bytes) in self.analysis_inputs {
            if let Some((kind, existing)) = inputs.get(&path) {
                if existing != &bytes {
                    bail!("snapshot entry {} has conflicting bytes", path.display());
                }
                if *kind == SnapshotEntryKind::Source {
                    continue;
                }
            }
            inputs.insert(path, (SnapshotEntryKind::AnalysisInput, bytes));
        }

        let mut entries = BTreeMap::new();
        for (path, (kind, bytes)) in inputs {
            let analysis = if kind == SnapshotEntryKind::Source {
                let (selection, language, adapter) = resolve_grammar(&path, &self.registry)?;
                EntryAnalysis::Source {
                    selection,
                    language,
                    adapter: Box::new(adapter),
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
    pub(crate) fn record_requested(&self, key: &FileRevisionKey) {
        let mut counts = self
            .counts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        counts.entry(key.clone()).or_default().requested += 1;
    }

    pub(crate) fn record_owner(&self, key: &FileRevisionKey) {
        let mut counts = self
            .counts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        counts.entry(key.clone()).or_default().owners += 1;
    }

    pub(crate) fn record_invocation(&self, key: &FileRevisionKey) {
        let mut counts = self
            .counts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        counts.entry(key.clone()).or_default().parser_invocations += 1;
    }

    pub(crate) fn record_reuse(&self, key: &FileRevisionKey) {
        let mut counts = self
            .counts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        counts.entry(key.clone()).or_default().reused += 1;
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
    pub(crate) key: FileRevisionKey,
    pub(crate) source: Arc<StoredSource>,
    pub(crate) language: tree_sitter::Language,
    pub(crate) text: Option<Arc<str>>,
    pub(crate) tree: Option<Tree>,
    pub(crate) arena: Option<SyntaxArena>,
    pub(crate) query_node_index: Option<Box<[(usize, u32)]>>,
    pub(crate) provenance: AnalysisProvenance,
    pub(crate) line_starts: Vec<usize>,
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

    pub(crate) fn query_language(&self) -> &tree_sitter::Language {
        &self.language
    }

    pub(crate) fn query_tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    pub(crate) fn query_node_index(&self) -> Option<&[(usize, u32)]> {
        self.query_node_index.as_deref()
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectionId(String);

impl ProjectionId {
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
    node_key_index: Box<[NodeKeyIndexEntry]>,
}

#[derive(Debug)]
struct NodeFileRange {
    path: PathBuf,
    start: u32,
    end: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct NodeKeyIndexEntry {
    digest: [u8; 16],
    index: u32,
}

#[derive(Debug, Clone)]
pub struct NodeIds {
    owner: u64,
    next: u32,
    end: u32,
}

#[derive(Debug, Clone)]
pub struct NodeChildren<'analysis> {
    owner: u64,
    file_start: u32,
    remaining: std::slice::Iter<'analysis, ArenaNodeIndex>,
}

impl NodeChildren<'_> {
    pub fn first(&self) -> Option<NodeId> {
        self.remaining.as_slice().first().map(|child| NodeId {
            owner: self.owner,
            index: self.file_start + child.as_usize() as u32,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.remaining.as_slice().is_empty()
    }

    pub fn contains(&self, id: &NodeId) -> bool {
        id.owner == self.owner
            && self
                .remaining
                .as_slice()
                .iter()
                .any(|child| id.index == self.file_start + child.as_usize() as u32)
    }

    pub fn iter(&self) -> Self {
        self.clone()
    }
}

impl Iterator for NodeChildren<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        let child = *self.remaining.next()?;
        Some(NodeId {
            owner: self.owner,
            index: self.file_start + child.as_usize() as u32,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.remaining.size_hint()
    }
}

impl DoubleEndedIterator for NodeChildren<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let child = *self.remaining.next_back()?;
        Some(NodeId {
            owner: self.owner,
            index: self.file_start + child.as_usize() as u32,
        })
    }
}

impl ExactSizeIterator for NodeChildren<'_> {}
impl std::iter::FusedIterator for NodeChildren<'_> {}

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
    exact_zero_width: ExactZeroWidthNodes<'analysis>,
    before: Option<SyntaxOwner<'analysis>>,
    after: Option<SyntaxOwner<'analysis>>,
}

impl<'analysis> SyntaxPointContext<'analysis> {
    pub fn exact_zero_width(&self) -> ExactZeroWidthNodes<'analysis> {
        self.exact_zero_width.clone()
    }

    pub fn before(&self) -> Option<SyntaxOwner<'analysis>> {
        self.before
    }

    pub fn after(&self) -> Option<SyntaxOwner<'analysis>> {
        self.after
    }

    /// Measure the allocation-free zero-width result view.
    pub fn instrumentation(&self) -> crate::SyntaxPointContextInstrumentation {
        crate::SyntaxPointContextInstrumentation {
            exact_zero_width_nodes: self.exact_zero_width.len(),
            exact_zero_width_bytes: 0,
            known_bytes_lower_bound: std::mem::size_of::<Self>(),
        }
    }
}

/// Allocation-free co-minimal zero-width node views in grammar preorder.
#[derive(Debug, Clone)]
pub struct ExactZeroWidthNodes<'analysis> {
    analysis: &'analysis ProjectAnalysis,
    file: &'analysis ParsedFile,
    arena: &'analysis SyntaxArena,
    file_start: u32,
    entries: &'analysis [(usize, u32)],
}

impl<'analysis> ExactZeroWidthNodes<'analysis> {
    fn view(&self, local: u32) -> NodeView<'analysis> {
        self.analysis.node_view_from_local(
            self.file,
            self.arena,
            self.file_start,
            ArenaNodeIndex::from_u32(local),
        )
    }

    pub fn first(&self) -> Option<NodeView<'analysis>> {
        self.entries.first().map(|(_, local)| self.view(*local))
    }

    pub fn get(&self, index: usize) -> Option<NodeView<'analysis>> {
        self.entries.get(index).map(|(_, local)| self.view(*local))
    }
}

impl<'analysis> Iterator for ExactZeroWidthNodes<'analysis> {
    type Item = NodeView<'analysis>;

    fn next(&mut self) -> Option<Self::Item> {
        let (first, rest) = self.entries.split_first()?;
        self.entries = rest;
        Some(self.view(first.1))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.entries.len(), Some(self.entries.len()))
    }
}

impl DoubleEndedIterator for ExactZeroWidthNodes<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let (last, rest) = self.entries.split_last()?;
        self.entries = rest;
        Some(self.view(last.1))
    }
}

impl ExactSizeIterator for ExactZeroWidthNodes<'_> {}
impl std::iter::FusedIterator for ExactZeroWidthNodes<'_> {}

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
        Self::assemble(snapshot, files, ledger)
    }

    pub(crate) fn assemble(
        snapshot: Arc<ProjectSnapshot>,
        files: BTreeMap<PathBuf, Arc<ParsedFile>>,
        ledger: Arc<ParseLedger>,
    ) -> Result<Arc<Self>> {
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
        let mut node_key_index = node_keys
            .iter()
            .enumerate()
            .map(|(index, key)| NodeKeyIndexEntry {
                digest: key.lookup_digest(),
                index: index as u32,
            })
            .collect::<Vec<_>>();
        node_key_index.sort_unstable();
        Ok(Arc::new(Self {
            id,
            snapshot,
            files,
            ledger,
            owner,
            node_ranges: node_ranges.into_boxed_slice(),
            node_keys: node_keys.into_boxed_slice(),
            node_key_index: node_key_index.into_boxed_slice(),
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

    /// Return the exact language adapter selected when this snapshot entry was built.
    ///
    /// Selection is path- and dialect-sensitive, so consumers must not reconstruct it
    /// from the broader [`Lang`] stored in the grammar descriptor.
    pub fn language_adapter(&self, path: &Path) -> Option<&'static dyn LangPack> {
        self.snapshot.entry(path)?.language_adapter()
    }

    /// Derive a deterministic projection identity from the immutable analysis,
    /// projection policy, declared capabilities, and exact stored adapter schemas.
    pub fn derive_projection_id(
        &self,
        schema: &str,
        policy: &[u8],
        capabilities: &[u8],
    ) -> Result<ProjectionId> {
        if schema.trim().is_empty() {
            bail!("projection schema must not be empty");
        }
        let mut hasher = domain_hasher(PROJECTION_ID_DOMAIN);
        hash_part(&mut hasher, self.id.as_str().as_bytes());
        hash_part(&mut hasher, schema.as_bytes());
        hash_part(&mut hasher, policy);
        hash_part(&mut hasher, capabilities);
        for entry in self.snapshot.entries() {
            let Some(identity) = entry.language_adapter_identity() else {
                continue;
            };
            hash_part(&mut hasher, &path_bytes(entry.path()));
            hash_part(&mut hasher, &identity.identity_bytes());
        }
        Ok(ProjectionId(format!("pj1_{}", hasher.finalize().to_hex())))
    }

    pub(crate) fn file_arc(&self, path: &Path) -> Option<Arc<ParsedFile>> {
        self.files.get(path).cloned()
    }

    pub fn parse_counts(&self) -> BTreeMap<FileRevisionKey, FileParseCount> {
        self.ledger.counts()
    }

    /// Measure deterministic ownership, structure, and visible retained-storage lower bounds.
    ///
    /// Instrumentation is derived after construction and is never part of snapshot, analysis, or
    /// projection identity.
    pub fn instrumentation(&self) -> ProjectAnalysisInstrumentation {
        let counts = self.ledger.counts();
        let mut parse = ParseOwnershipInstrumentation {
            file_revisions: counts.len(),
            ..ParseOwnershipInstrumentation::default()
        };
        for (key, count) in &counts {
            parse.requested += count.requested;
            parse.owners += count.owners;
            parse.parser_invocations += count.parser_invocations;
            parse.reused += count.reused;
            if self.file(&key.path).is_some_and(|file| !file.has_tree()) {
                parse.syntax_unavailable += 1;
            }
            if count.requested != 1
                || count.owners != 1
                || count.parser_invocations > 1
                || count.reused > 1
                || count.parser_invocations + count.reused > 1
            {
                parse.invariant_violations += 1;
            }
        }

        let mut structure = AnalysisStructureInstrumentation {
            files: self.files.len(),
            ..AnalysisStructureInstrumentation::default()
        };
        let mut memory = AnalysisMemoryInstrumentation::default();
        let store = self
            .snapshot
            .store
            .contents
            .read()
            .unwrap_or_else(|poison| poison.into_inner());
        memory.source_store_revisions = store.len();
        memory.source_store_bytes = store.values().map(|source| source.bytes.len()).sum();
        drop(store);

        for file in self.files.values() {
            structure.source_bytes += file.source().len();
            structure.utf8_text_bytes += file.text().map_or(0, str::len);
            structure.line_start_entries += file.line_starts.len();
            memory.parsed_utf8_text_bytes += file.text().map_or(0, str::len);
            memory.line_index_bytes += file.line_starts.len() * std::mem::size_of::<usize>();
            memory.query_node_index_bytes += file
                .query_node_index
                .as_ref()
                .map_or(0, |index| index.len() * std::mem::size_of::<(usize, u32)>());
            memory.opaque_tree_count += usize::from(file.tree.is_some());
            if let Some(arena) = &file.arena {
                let stats = arena.instrumentation();
                structure.nodes += stats.nodes;
                structure.syntax_segments += stats.segments;
                structure.child_edges += stats.child_edges;
                structure.owned_segment_references += stats.owned_segment_references;
                structure.zero_width_nodes += stats.zero_width_nodes;
                memory.arena_bytes_lower_bound += stats.arena_bytes_lower_bound;
                memory.containment_index_bytes += stats.containment_index_bytes;
            }
        }

        memory.node_range_bytes_lower_bound = self.node_ranges.len()
            * std::mem::size_of::<NodeFileRange>()
            + self
                .node_ranges
                .iter()
                .map(|range| {
                    range
                        .path
                        .to_str()
                        .expect("snapshot paths are validated Unicode")
                        .len()
                })
                .sum::<usize>();
        memory.node_key_lookup_index_bytes =
            self.node_key_index.len() * std::mem::size_of::<NodeKeyIndexEntry>();
        let mut order_hasher = domain_hasher("deslop node order instrumentation v1");
        let mut node_key_heap = 0;
        let mut previous_node_key_file: Option<&FileRevisionKey> = None;
        let mut seen_field_paths = BTreeSet::new();
        for key in &self.node_keys {
            let (heap, file_payload, field_bytes, field_entries) = key.instrumentation();
            node_key_heap += heap;
            if previous_node_key_file != Some(key.file()) {
                memory.node_key_file_revision_payload_bytes += file_payload;
                node_key_heap += std::mem::size_of::<FileRevisionKey>() + file_payload;
                previous_node_key_file = Some(key.file());
            }
            if seen_field_paths.insert(key.field_path_allocation_id()) {
                memory.node_key_field_path_bytes += field_bytes;
                node_key_heap += field_bytes;
            }
            structure.node_key_field_path_entries += field_entries;
            structure.max_node_key_field_path_depth =
                structure.max_node_key_field_path_depth.max(field_entries);
            key.update_order_digest(&mut order_hasher);
        }
        memory.node_key_bytes_lower_bound =
            self.node_keys.len() * std::mem::size_of::<NodeKey>() + node_key_heap;
        memory.parse_ledger_bytes_lower_bound = counts.len()
            * (std::mem::size_of::<FileRevisionKey>() + std::mem::size_of::<FileParseCount>())
            + counts
                .keys()
                .map(FileRevisionKey::known_payload_bytes)
                .sum::<usize>();
        memory.known_bytes_lower_bound = memory.source_store_bytes
            + memory.parsed_utf8_text_bytes
            + memory.arena_bytes_lower_bound
            + memory.line_index_bytes
            + memory.query_node_index_bytes
            + memory.node_range_bytes_lower_bound
            + memory.node_key_lookup_index_bytes
            + memory.node_key_bytes_lower_bound
            + memory.parse_ledger_bytes_lower_bound;

        debug_assert_eq!(structure.nodes, self.node_keys.len());
        ProjectAnalysisInstrumentation {
            parse,
            structure,
            memory,
            node_order_digest: format!("pao1_{}", order_hasher.finalize().to_hex()),
        }
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
        let range = self
            .node_ranges
            .binary_search_by(|range| range.path.as_path().cmp(path))
            .ok()
            .map(|index| &self.node_ranges[index])?;
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
        let range = self.node_ranges[..self
            .node_ranges
            .partition_point(|range| range.start <= id.index)]
            .last()
            .filter(|range| id.index < range.end)
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
        let digest = key.lookup_digest();
        let start = self
            .node_key_index
            .partition_point(|entry| entry.digest < digest);
        let index = self.node_key_index[start..]
            .iter()
            .take_while(|entry| entry.digest == digest)
            .find_map(|entry| {
                (self.node_keys[entry.index as usize] == *key).then_some(entry.index as usize)
            })
            .ok_or(NodeKeyLookupError::NotFound)?;
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

    /// Infallible form of [`ProjectAnalysis::try_fold_syntax_aggregates`].
    pub fn fold_syntax_aggregates<'analysis, T, InitLocal, FoldRegion, Merge>(
        &'analysis self,
        path: &Path,
        policy: InclusiveSyntaxPolicy<'_>,
        mut init_local: InitLocal,
        mut fold_region: FoldRegion,
        mut merge: Merge,
    ) -> std::result::Result<
        SyntaxAggregates<'analysis, T>,
        SyntaxAggregationError<std::convert::Infallible>,
    >
    where
        T: Clone,
        InitLocal: FnMut(SyntaxOwner<'analysis>) -> T,
        FoldRegion: FnMut(&mut T, ExclusiveSyntaxRegion<'analysis>),
        Merge: FnMut(&mut T, &T),
    {
        self.try_fold_syntax_aggregates(
            path,
            policy,
            |owner| Ok::<T, std::convert::Infallible>(init_local(owner)),
            |value, region| {
                fold_region(value, region);
                Ok(())
            },
            |parent, child| {
                merge(parent, child);
                Ok(())
            },
        )
    }

    /// Initialize every raw owner once, fold every positive-width exclusive region exactly once,
    /// and derive both full and explicitly declared inclusive values bottom-up.
    ///
    /// `init_local` visits the File pseudo-owner first and then every raw node in grammar preorder,
    /// including internal, anonymous, extra, ERROR, missing, and zero-width nodes. `fold_region`
    /// visits the exact source partition in byte order and mutates only each region's direct owner.
    /// `merge` derives inclusive projections from collapsed owner values; it must be a pure,
    /// associative, and commutative operation. It may be invoked for both the full and declared
    /// projections, so inclusive values deliberately do not promise byte order.
    ///
    /// `full_inclusive()` always contains the full raw subtree. Under `ResetAt`,
    /// `declared_inclusive()` excludes each reset child's projection while retaining that reset
    /// node's own declared value. The File declared value is therefore the residual outside reset
    /// subtrees; the File full-inclusive value remains the total source projection. The caller, not
    /// this raw layer, selects semantic reset nodes.
    ///
    /// File/syntax availability and every normalized reset node are validated before callbacks run.
    /// Callback failures retain exact owner/region/edge context and stop construction without
    /// publishing a partial result.
    ///
    /// After the O(log F) file-range lookup, core work is O(N + S + R) owner, segment, and
    /// reset visits plus O(N) accumulator clones/merges; callback and `T` costs are caller-defined.
    /// Results retain local and full-inclusive values plus a declared projection when resets exist.
    pub fn try_fold_syntax_aggregates<'analysis, T, Error, InitLocal, FoldRegion, Merge>(
        &'analysis self,
        path: &Path,
        policy: InclusiveSyntaxPolicy<'_>,
        mut init_local: InitLocal,
        mut fold_region: FoldRegion,
        mut merge: Merge,
    ) -> std::result::Result<SyntaxAggregates<'analysis, T>, SyntaxAggregationError<Error>>
    where
        T: Clone,
        InitLocal: FnMut(SyntaxOwner<'analysis>) -> std::result::Result<T, Error>,
        FoldRegion:
            FnMut(&mut T, ExclusiveSyntaxRegion<'analysis>) -> std::result::Result<(), Error>,
        Merge: FnMut(&mut T, &T) -> std::result::Result<(), Error>,
    {
        let (file, arena, file_start) =
            self.exclusive_syntax_context(path)
                .map_err(|error| match error {
                    ExclusiveSyntaxLookupError::FileNotFound { path } => {
                        SyntaxAggregationError::FileNotFound { path }
                    }
                    ExclusiveSyntaxLookupError::SyntaxUnavailable { path } => {
                        SyntaxAggregationError::SyntaxUnavailable { path }
                    }
                    ExclusiveSyntaxLookupError::ByteOutOfRange { .. } => {
                        unreachable!("whole-file syntax aggregation does not request a byte")
                    }
                })?;

        let mut resets_parent =
            (!policy.reset_nodes().is_empty()).then(|| vec![false; arena.nodes().len()]);
        let file_end = file_start + arena.nodes().len() as u32;
        for &id in policy.reset_nodes() {
            if id.owner != self.owner {
                return Err(SyntaxAggregationError::InvalidResetNode {
                    node: id,
                    error: NodeLookupError::WrongAnalysis,
                });
            }
            if id.index as usize >= self.node_keys.len() {
                return Err(SyntaxAggregationError::InvalidResetNode {
                    node: id,
                    error: NodeLookupError::OutOfRange {
                        requested: id.index,
                        node_count: self.node_keys.len() as u32,
                    },
                });
            }
            if id.index < file_start || id.index >= file_end {
                return Err(SyntaxAggregationError::ResetNodeOutsideFile {
                    node: id,
                    path: path.to_path_buf(),
                });
            }
            resets_parent
                .as_mut()
                .expect("a declared reset allocates reset flags")
                [(id.index - file_start) as usize] = true;
        }
        let reset_nodes = resets_parent.as_deref().map_or_else(Vec::new, |resets| {
            resets
                .iter()
                .enumerate()
                .filter_map(|(offset, reset)| {
                    reset.then_some(NodeId {
                        owner: self.owner,
                        index: file_start + offset as u32,
                    })
                })
                .collect::<Vec<_>>()
        });

        let initialize_calls = arena.nodes().len() + 1;
        let fold_region_calls = arena.segments().len();
        let mut merge_calls = 0;
        let mut value_clone_calls = 0;
        let mut file_local = init_local(SyntaxOwner::File(&file.key)).map_err(|error| {
            SyntaxAggregationError::InitializeOwner {
                path: path.to_path_buf(),
                owner: SyntaxAggregateOwner::File,
                error,
            }
        })?;
        let mut node_local = Vec::with_capacity(arena.nodes().len());
        for (local, _) in arena.indexed_nodes() {
            let node = self.node_view_from_local(file, arena, file_start, local);
            let id = node.id();
            node_local.push(init_local(SyntaxOwner::Node(node)).map_err(|error| {
                SyntaxAggregationError::InitializeOwner {
                    path: path.to_path_buf(),
                    owner: SyntaxAggregateOwner::Node(id),
                    error,
                }
            })?);
        }
        for (local, segment) in arena.indexed_segments() {
            let region = ExclusiveSyntaxRegion {
                file,
                arena,
                local,
                owner: self.owner,
                file_start,
            };
            let range = segment.byte_range();
            match segment.owner() {
                SyntaxSegmentOwner::File => {
                    fold_region(&mut file_local, region).map_err(|error| {
                        SyntaxAggregationError::FoldRegion {
                            path: path.to_path_buf(),
                            owner: SyntaxAggregateOwner::File,
                            range,
                            error,
                        }
                    })?;
                }
                SyntaxSegmentOwner::Node(owner) => {
                    let id = NodeId {
                        owner: self.owner,
                        index: file_start + owner.as_usize() as u32,
                    };
                    fold_region(&mut node_local[owner.as_usize()], region).map_err(|error| {
                        SyntaxAggregationError::FoldRegion {
                            path: path.to_path_buf(),
                            owner: SyntaxAggregateOwner::Node(id),
                            range,
                            error,
                        }
                    })?;
                }
            }
        }

        let (node_full_inclusive, node_declared_inclusive) = {
            let mut roll_up = |resets: Option<&[bool]>, projection| {
                value_clone_calls += node_local.len();
                let mut values = node_local.clone();
                for parent in (0..arena.nodes().len()).rev() {
                    for child in arena.nodes()[parent].children() {
                        let child = child.as_usize();
                        if resets.is_some_and(|resets| resets[child]) {
                            continue;
                        }
                        let (parents, children) = values.split_at_mut(child);
                        merge_calls += 1;
                        merge(&mut parents[parent], &children[0]).map_err(|error| {
                            SyntaxAggregationError::Merge {
                                path: path.to_path_buf(),
                                projection,
                                parent: SyntaxAggregateOwner::Node(NodeId {
                                    owner: self.owner,
                                    index: file_start + parent as u32,
                                }),
                                child: SyntaxAggregateOwner::Node(NodeId {
                                    owner: self.owner,
                                    index: file_start + child as u32,
                                }),
                                error,
                            }
                        })?;
                    }
                }
                Ok::<Vec<T>, SyntaxAggregationError<Error>>(values)
            };
            let inclusive = roll_up(None, SyntaxAggregateProjection::FullInclusive)?;
            let declared = resets_parent
                .as_deref()
                .map(|resets| roll_up(Some(resets), SyntaxAggregateProjection::DeclaredInclusive))
                .transpose()?;
            (inclusive, declared)
        };

        value_clone_calls += 1;
        let mut file_full_inclusive = file_local.clone();
        let root = arena.root().as_usize();
        let root_owner = SyntaxAggregateOwner::Node(NodeId {
            owner: self.owner,
            index: file_start + root as u32,
        });
        merge_calls += 1;
        merge(&mut file_full_inclusive, &node_full_inclusive[root]).map_err(|error| {
            SyntaxAggregationError::Merge {
                path: path.to_path_buf(),
                projection: SyntaxAggregateProjection::FullInclusive,
                parent: SyntaxAggregateOwner::File,
                child: root_owner,
                error,
            }
        })?;
        let file_declared_inclusive = if let Some(resets_parent) = resets_parent.as_deref() {
            value_clone_calls += 1;
            let mut value = file_local.clone();
            if !resets_parent[root] {
                merge_calls += 1;
                merge(
                    &mut value,
                    &node_declared_inclusive
                        .as_ref()
                        .expect("reset flags own a declared projection")[root],
                )
                .map_err(|error| SyntaxAggregationError::Merge {
                    path: path.to_path_buf(),
                    projection: SyntaxAggregateProjection::DeclaredInclusive,
                    parent: SyntaxAggregateOwner::File,
                    child: root_owner,
                    error,
                })?;
            }
            Some(value)
        } else {
            None
        };

        let reset_node_count = reset_nodes.len();
        let declared_values = usize::from(reset_node_count > 0) * (arena.nodes().len() + 1);
        Ok(SyntaxAggregates::from_parts(SyntaxAggregateParts {
            analysis_id: &self.id,
            file_key: &file.key,
            owner: self.owner,
            file_start,
            file_local,
            file_full_inclusive,
            file_declared_inclusive,
            node_local: node_local.into_boxed_slice(),
            node_full_inclusive: node_full_inclusive.into_boxed_slice(),
            node_declared_inclusive: node_declared_inclusive.map(Vec::into_boxed_slice),
            resets_parent: resets_parent.map(Vec::into_boxed_slice),
            reset_nodes: reset_nodes.into_boxed_slice(),
            instrumentation: crate::aggregation::SyntaxAggregationInstrumentation {
                nodes: arena.nodes().len(),
                reset_nodes: reset_node_count,
                initialize_calls,
                fold_region_calls,
                merge_calls,
                value_clone_calls,
                retained_local_values: arena.nodes().len() + 1,
                retained_full_inclusive_values: arena.nodes().len() + 1,
                retained_declared_inclusive_values: declared_values,
            },
        }))
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
        let exact_zero_width = ExactZeroWidthNodes {
            analysis: self,
            file,
            arena,
            file_start,
            entries: arena.containment().zero_width_nodes_at(point),
        };
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

impl<'analysis> NodeView<'analysis> {
    fn raw(&self) -> &'analysis crate::arena::SyntaxNode {
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

    pub(crate) fn query_parts(
        &self,
    ) -> (
        &'analysis ParsedFile,
        &'analysis SyntaxArena,
        ArenaNodeIndex,
    ) {
        (self.file, self.arena, self.local)
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

    pub fn raw_kind(&self) -> &'analysis str {
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

    pub fn children(&self) -> NodeChildren<'analysis> {
        NodeChildren {
            owner: self.id.owner,
            file_start: self.file_start(),
            remaining: self.raw().children().iter(),
        }
    }

    pub fn child_count(&self) -> usize {
        self.raw().children().len()
    }

    pub fn is_leaf(&self) -> bool {
        self.raw().is_leaf()
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

pub(crate) fn parse_owned_file(
    entry: &SnapshotEntry,
    key: FileRevisionKey,
    ledger: &ParseLedger,
) -> Result<ParsedFile> {
    if entry.bytes().len() > u32::MAX as usize {
        bail!(
            "source {} is {} bytes, exceeding Tree-sitter's {}-byte limit",
            entry.path.display(),
            entry.bytes().len(),
            u32::MAX
        );
    }
    ledger.record_requested(&key);
    ledger.record_owner(&key);
    let line_starts = byte_line_starts(entry.bytes());
    let language = entry.grammar_language().cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "source {} has no stored parser language",
            entry.path.display()
        )
    })?;
    let text = match std::str::from_utf8(entry.bytes()) {
        Ok(text) => Arc::<str>::from(text),
        Err(error) => {
            return Ok(ParsedFile {
                key,
                source: entry.source.clone(),
                language,
                text: None,
                tree: None,
                arena: None,
                query_node_index: None,
                provenance: AnalysisProvenance::failed(vec![AnalysisDiagnostic {
                    code: "invalid-utf8".to_string(),
                    message: format!("source is not valid UTF-8: {error}"),
                    span: None,
                }]),
                line_starts,
            });
        }
    };
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .with_context(|| format!("failed to configure parser for {}", entry.path.display()))?;
    ledger.record_invocation(&key);
    let tree = parser.parse(text.as_ref(), None).with_context(|| {
        format!(
            "Tree-sitter returned no syntax tree for {}",
            entry.path.display()
        )
    })?;
    let provenance = analysis_provenance_for_tree(&tree);
    let arena = SyntaxArena::from_tree(&tree, entry.bytes(), key.grammar.clone())
        .with_context(|| format!("failed to own syntax arena for {}", entry.path.display()))?;
    let query_node_index = crate::query::build_query_node_index(&tree, &arena)
        .with_context(|| format!("failed to index query nodes for {}", entry.path.display()))?;
    Ok(ParsedFile {
        key,
        source: entry.source.clone(),
        language,
        text: Some(text),
        tree: Some(tree),
        arena: Some(arena),
        query_node_index: Some(query_node_index),
        provenance,
        line_starts,
    })
}

pub(crate) fn byte_line_starts(bytes: &[u8]) -> Vec<usize> {
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

fn collect_disk_sources(
    root: &Path,
    scope: &[ScopeEntry],
    registry: &Registry,
    discovery: DiscoveryPolicy,
) -> Result<BTreeMap<PathBuf, PathBuf>> {
    let mut physical_to_logical = BTreeMap::<PathBuf, PathBuf>::new();
    for scope_entry in scope {
        let logical_scope = &scope_entry.path;
        let physical_scope = if logical_scope == Path::new(".") {
            root.to_path_buf()
        } else {
            root.join(logical_scope)
        };
        if scope_entry.kind == ScopeEntryKind::File {
            if !physical_scope.exists() {
                continue;
            }
            insert_disk_source(root, &physical_scope, registry, &mut physical_to_logical)?;
            continue;
        }
        let mut walker = WalkBuilder::new(&physical_scope);
        walker.hidden(false).filter_entry(|entry| {
            !matches!(
                entry.file_name().to_str(),
                Some(".git" | ".jj" | "target" | "__pycache__")
            )
        });
        if discovery == DiscoveryPolicy::Canonical {
            walker
                .parents(false)
                .ignore(false)
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false);
        }
        let walker = walker.build();
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
                insert_disk_source(root, entry.path(), registry, &mut physical_to_logical)?;
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
    registry: &Registry,
    out: &mut BTreeMap<PathBuf, PathBuf>,
) -> Result<()> {
    if registry.supported_pack_for_path(path).is_none() {
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
    use crate::aggregation::SyntaxAggregateLookupError;
    use crate::arena::{RAW_ARENA_SCHEMA, SyntaxSegmentKind, SyntaxSegmentOwner};
    use deslop_core::{AnalysisStatus, Span, revision_guard};
    use deslop_lang::RegionSpan;

    struct NoGrammarTestPack;

    static NO_GRAMMAR_TEST_PACK: NoGrammarTestPack = NoGrammarTestPack;

    impl LangPack for NoGrammarTestPack {
        fn name(&self) -> &'static str {
            "no-grammar-test"
        }

        fn capability_manifest(&self) -> deslop_lang::LanguageAdapterCapabilityManifest {
            deslop_lang::LanguageAdapterCapabilityManifest::unknown(self.adapter_schema())
        }

        fn lang(&self) -> Lang {
            Lang::Generic
        }

        fn extensions(&self) -> &'static [&'static str] {
            &["testpack"]
        }

        fn grammar(&self) -> Option<tree_sitter::Language> {
            None
        }

        fn line_comments(&self) -> &'static [&'static str] {
            &["#"]
        }

        fn metrics_regions(&self) -> &'static [&'static str] {
            &[]
        }

        fn metrics_branches(&self) -> &'static [&'static str] {
            &[]
        }

        fn metrics_nesting(&self) -> &'static [&'static str] {
            &[]
        }

        fn metrics_flow_breaks(&self) -> &'static [&'static str] {
            &[]
        }

        fn halstead_operator_tokens(&self) -> &'static [&'static str] {
            &[]
        }

        fn enclosing_region(
            &self,
            _node: tree_sitter::Node<'_>,
            _text: &str,
        ) -> Option<RegionSpan> {
            None
        }
    }

    fn repository() -> RepositoryId {
        RepositoryId::explicit("test-repository").unwrap()
    }

    #[test]
    fn registered_adapter_without_grammar_is_rejected_before_snapshot_publication() {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::new(&deslop_lang::GENERIC_PACK);
        registry.register(&NO_GRAMMAR_TEST_PACK);
        crate::reset_parse_source_invocations();
        let error = ProjectSnapshotBuilder::new(root.path(), repository())
            .unwrap()
            .with_registry(registry)
            .with_overlay("demo.testpack", b"anything\n".to_vec())
            .unwrap()
            .build()
            .expect_err("grammarless source adapters are not an owned syntax capability");
        assert_eq!(error.to_string(), "no grammar artifact for demo.testpack");
        assert_eq!(crate::parse_source_invocations(), 0);
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

    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    struct SyntaxTally {
        file_owners: usize,
        node_owners: usize,
        missing_nodes: usize,
        leaf_nodes: usize,
        regions: usize,
        bytes: usize,
        token_regions: usize,
        token_bytes: usize,
        trivia_regions: usize,
        trivia_bytes: usize,
    }

    impl SyntaxTally {
        fn initialized(owner: SyntaxOwner<'_>) -> Self {
            match owner {
                SyntaxOwner::File(_) => Self {
                    file_owners: 1,
                    ..Self::default()
                },
                SyntaxOwner::Node(node) => Self {
                    node_owners: 1,
                    missing_nodes: usize::from(node.is_missing()),
                    leaf_nodes: usize::from(node.is_leaf()),
                    ..Self::default()
                },
            }
        }

        fn for_region(region: ExclusiveSyntaxRegion<'_>) -> Self {
            let bytes = region.byte_range().len();
            match region.kind() {
                ExclusiveSyntaxKind::Token => Self {
                    regions: 1,
                    bytes,
                    token_regions: 1,
                    token_bytes: bytes,
                    ..Self::default()
                },
                ExclusiveSyntaxKind::Trivia => Self {
                    regions: 1,
                    bytes,
                    trivia_regions: 1,
                    trivia_bytes: bytes,
                    ..Self::default()
                },
            }
        }

        fn merge(&mut self, other: &Self) {
            self.file_owners += other.file_owners;
            self.node_owners += other.node_owners;
            self.missing_nodes += other.missing_nodes;
            self.leaf_nodes += other.leaf_nodes;
            self.regions += other.regions;
            self.bytes += other.bytes;
            self.token_regions += other.token_regions;
            self.token_bytes += other.token_bytes;
            self.trivia_regions += other.trivia_regions;
            self.trivia_bytes += other.trivia_bytes;
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct AggregateFailure(&'static str);

    impl fmt::Display for AggregateFailure {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.0)
        }
    }

    impl std::error::Error for AggregateFailure {}

    fn assert_syntax_aggregate_oracle(
        analysis: &ProjectAnalysis,
        path: &Path,
        reset_nodes: &[NodeId],
        actual: &SyntaxAggregates<'_, SyntaxTally>,
    ) {
        let ids = analysis.file_node_ids(path).unwrap().collect::<Vec<_>>();
        let first = ids.first().unwrap().index;
        let offset = |id: NodeId| (id.index - first) as usize;
        let resets = reset_nodes.iter().copied().collect::<BTreeSet<_>>();

        let mut file_local =
            SyntaxTally::initialized(SyntaxOwner::File(analysis.file(path).unwrap().key()));
        let mut node_local = ids
            .iter()
            .map(|id| SyntaxTally::initialized(SyntaxOwner::Node(analysis.node(*id).unwrap())))
            .collect::<Vec<_>>();
        for region in analysis.exclusive_syntax_regions(path).unwrap() {
            let contribution = SyntaxTally::for_region(region);
            match region.owner() {
                ExclusiveSyntaxOwner::File(_) => file_local.merge(&contribution),
                ExclusiveSyntaxOwner::Node(owner) => {
                    node_local[offset(owner)].merge(&contribution);
                }
            }
        }

        let mut full = vec![SyntaxTally::default(); ids.len()];
        let mut declared = vec![SyntaxTally::default(); ids.len()];
        let mut file_full = file_local.clone();
        let mut file_declared = file_local.clone();
        for (id, local) in ids.iter().copied().zip(&node_local) {
            let mut current = Some(id);
            while let Some(node) = current {
                full[offset(node)].merge(local);
                current = analysis.node(node).unwrap().parent();
            }
            file_full.merge(local);

            let mut current = Some(id);
            let mut reached_file = true;
            while let Some(node) = current {
                declared[offset(node)].merge(local);
                if resets.contains(&node) {
                    reached_file = false;
                    break;
                }
                current = analysis.node(node).unwrap().parent();
            }
            if reached_file {
                file_declared.merge(local);
            }
        }

        assert_eq!(actual.file_local(), &file_local);
        assert_eq!(actual.file_full_inclusive(), &file_full);
        assert_eq!(actual.file_declared_inclusive(), &file_declared);
        assert_eq!(actual.len(), ids.len());
        assert_eq!(actual.reset_nodes(), reset_nodes);
        for (id, local) in ids.iter().copied().zip(node_local) {
            let aggregate = actual.node(id).unwrap();
            assert_eq!(aggregate.id(), id);
            assert_eq!(aggregate.local(), &local);
            assert_eq!(aggregate.full_inclusive(), &full[offset(id)]);
            assert_eq!(aggregate.declared_inclusive(), &declared[offset(id)]);
            assert_eq!(aggregate.resets_parent(), resets.contains(&id));
        }
        assert_eq!(
            actual
                .nodes()
                .map(|aggregate| aggregate.id())
                .collect::<Vec<_>>(),
            ids
        );
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
                        node.children().map(|child| child.index).collect::<Vec<_>>(),
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
    fn syntax_aggregation_initializes_every_owner_and_conserves_declared_partitions() {
        let temp = tempfile::tempdir().unwrap();
        let source = b"fn outer() {\n    let closure = || { if true { value(); } };\n}\n";
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("nested.rs", source.to_vec())
            .unwrap()
            .with_overlay("peer.rs", b"fn peer() {}\n".to_vec())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let path = Path::new("nested.rs");
        let ids = analysis.file_node_ids(path).unwrap().collect::<Vec<_>>();
        assert_eq!(source.len(), 62);
        assert_eq!(ids.len(), 37);

        let mut initialized = Vec::new();
        let mut ranges = Vec::new();
        let mut byte_visits = vec![0_u8; source.len()];
        let mut merge_calls = 0;
        let all = analysis
            .fold_syntax_aggregates(
                path,
                InclusiveSyntaxPolicy::AllDescendants,
                |owner| {
                    initialized.push(match owner {
                        SyntaxOwner::File(_) => SyntaxAggregateOwner::File,
                        SyntaxOwner::Node(node) => SyntaxAggregateOwner::Node(node.id()),
                    });
                    SyntaxTally::initialized(owner)
                },
                |value, region| {
                    let range = region.byte_range();
                    for byte in range.clone() {
                        byte_visits[byte] += 1;
                    }
                    ranges.push(range);
                    value.merge(&SyntaxTally::for_region(region));
                },
                |parent, child| {
                    merge_calls += 1;
                    parent.merge(child);
                },
            )
            .unwrap();
        assert_eq!(all.analysis_id(), analysis.id());
        assert_eq!(all.file_key(), analysis.file(path).unwrap().key());
        assert_eq!(initialized.len(), ids.len() + 1);
        assert_eq!(initialized[0], SyntaxAggregateOwner::File);
        assert_eq!(
            &initialized[1..],
            &ids.iter()
                .copied()
                .map(SyntaxAggregateOwner::Node)
                .collect::<Vec<_>>()
        );
        assert_eq!(ranges.len(), 37);
        assert!(ranges.windows(2).all(|pair| pair[0].end == pair[1].start));
        assert_eq!(ranges.first().unwrap().start, 0);
        assert_eq!(ranges.last().unwrap().end, source.len());
        assert!(byte_visits.iter().all(|visits| *visits == 1));
        assert_eq!(merge_calls, ids.len());
        assert_eq!(all.reset_nodes(), []);
        assert_eq!(all.policy(), InclusiveSyntaxPolicy::AllDescendants);
        assert_eq!(
            all.instrumentation(),
            crate::aggregation::SyntaxAggregationInstrumentation {
                nodes: 37,
                reset_nodes: 0,
                initialize_calls: 38,
                fold_region_calls: 37,
                merge_calls: 37,
                value_clone_calls: 38,
                retained_local_values: 38,
                retained_full_inclusive_values: 38,
                retained_declared_inclusive_values: 0,
            }
        );
        assert_eq!(
            all.instrumentation()
                .retained_value_bytes_lower_bound::<SyntaxTally>(),
            76 * std::mem::size_of::<SyntaxTally>()
        );
        assert_eq!(all.file_local().regions, 0);
        assert_eq!(all.file_local().bytes, 0);
        assert_eq!(all.file_full_inclusive().regions, 37);
        assert_eq!(all.file_full_inclusive().bytes, 62);
        assert_eq!(all.file_full_inclusive().token_regions, 22);
        assert_eq!(all.file_full_inclusive().token_bytes, 43);
        assert_eq!(all.file_full_inclusive().trivia_regions, 15);
        assert_eq!(all.file_full_inclusive().trivia_bytes, 19);
        assert_eq!(all.file_full_inclusive().file_owners, 1);
        assert_eq!(all.file_full_inclusive().node_owners, 37);
        assert_eq!(all.file_declared_inclusive(), all.file_full_inclusive());
        assert_syntax_aggregate_oracle(&analysis, path, &[], &all);
        let empty_reset = analysis
            .fold_syntax_aggregates(
                path,
                InclusiveSyntaxPolicy::ResetAt(&[]),
                SyntaxTally::initialized,
                |value, region| value.merge(&SyntaxTally::for_region(region)),
                SyntaxTally::merge,
            )
            .unwrap();
        assert_eq!(empty_reset.policy(), InclusiveSyntaxPolicy::AllDescendants);
        assert_eq!(empty_reset.file_local(), all.file_local());
        assert_eq!(empty_reset.file_full_inclusive(), all.file_full_inclusive());
        assert_eq!(
            empty_reset
                .nodes()
                .map(|node| (
                    node.local().clone(),
                    node.full_inclusive().clone(),
                    node.declared_inclusive().clone(),
                ))
                .collect::<Vec<_>>(),
            all.nodes()
                .map(|node| (
                    node.local().clone(),
                    node.full_inclusive().clone(),
                    node.declared_inclusive().clone(),
                ))
                .collect::<Vec<_>>()
        );

        let find = |kind: &str, text: Option<&str>| {
            ids.iter()
                .copied()
                .map(|id| analysis.node(id).unwrap())
                .find(|node| node.raw_kind() == kind && text.is_none_or(|text| node.text() == text))
                .unwrap()
                .id()
        };
        let function = find("function_item", None);
        let closure = find("closure_expression", None);
        let call = find("call_expression", None);
        let reset_input = [call, closure, function, call];
        let mut normalized = vec![function, closure, call];
        normalized.sort_unstable();
        let mut reset_merge_calls = 0;
        let reset = analysis
            .fold_syntax_aggregates(
                path,
                InclusiveSyntaxPolicy::ResetAt(&reset_input),
                SyntaxTally::initialized,
                |value, region| value.merge(&SyntaxTally::for_region(region)),
                |parent, child| {
                    reset_merge_calls += 1;
                    parent.merge(child);
                },
            )
            .unwrap();
        assert_eq!(reset_merge_calls, 2 * ids.len() - normalized.len());
        assert_eq!(reset.reset_nodes(), normalized);
        assert_eq!(reset.policy(), InclusiveSyntaxPolicy::ResetAt(&normalized));
        assert_eq!(
            reset.instrumentation(),
            crate::aggregation::SyntaxAggregationInstrumentation {
                nodes: 37,
                reset_nodes: 3,
                initialize_calls: 38,
                fold_region_calls: 37,
                merge_calls: 71,
                value_clone_calls: 76,
                retained_local_values: 38,
                retained_full_inclusive_values: 38,
                retained_declared_inclusive_values: 38,
            }
        );
        assert_eq!(reset.file_full_inclusive(), all.file_full_inclusive());
        assert_eq!(reset.file_declared_inclusive().regions, 1);
        assert_eq!(reset.file_declared_inclusive().bytes, 1);
        assert_eq!(
            reset.node(function).unwrap().declared_inclusive().regions,
            17
        );
        assert_eq!(reset.node(function).unwrap().declared_inclusive().bytes, 34);
        assert_eq!(
            reset.node(closure).unwrap().declared_inclusive().regions,
            16
        );
        assert_eq!(reset.node(closure).unwrap().declared_inclusive().bytes, 20);
        assert_eq!(reset.node(call).unwrap().declared_inclusive().regions, 3);
        assert_eq!(reset.node(call).unwrap().declared_inclusive().bytes, 7);
        assert_syntax_aggregate_oracle(&analysis, path, &normalized, &reset);
        let mut conserved = reset.file_declared_inclusive().clone();
        for reset_node in &normalized {
            conserved.merge(reset.node(*reset_node).unwrap().declared_inclusive());
        }
        assert_eq!(conserved, *reset.file_full_inclusive());

        let literal = find("boolean_literal", None);
        let token = find("true", Some("true"));
        let mut equal_span_resets = vec![literal, token];
        equal_span_resets.sort_unstable();
        let equal_span = analysis
            .fold_syntax_aggregates(
                path,
                InclusiveSyntaxPolicy::ResetAt(&[token, literal]),
                SyntaxTally::initialized,
                |value, region| value.merge(&SyntaxTally::for_region(region)),
                SyntaxTally::merge,
            )
            .unwrap();
        assert_eq!(equal_span.node(literal).unwrap().local().regions, 0);
        assert_eq!(
            equal_span.node(literal).unwrap().full_inclusive().regions,
            1
        );
        assert_eq!(
            equal_span
                .node(literal)
                .unwrap()
                .declared_inclusive()
                .regions,
            0
        );
        assert_eq!(equal_span.node(token).unwrap().local().bytes, 4);
        assert_eq!(equal_span.file_declared_inclusive().regions, 36);
        assert_eq!(equal_span.file_declared_inclusive().bytes, 58);
        assert_syntax_aggregate_oracle(&analysis, path, &equal_span_resets, &equal_span);
        let mut equal_span_conserved = equal_span.file_declared_inclusive().clone();
        for reset_node in &equal_span_resets {
            equal_span_conserved.merge(equal_span.node(*reset_node).unwrap().declared_inclusive());
        }
        assert_eq!(equal_span_conserved, *equal_span.file_full_inclusive());

        let every_node = analysis
            .fold_syntax_aggregates(
                path,
                InclusiveSyntaxPolicy::ResetAt(&ids),
                SyntaxTally::initialized,
                |value, region| value.merge(&SyntaxTally::for_region(region)),
                SyntaxTally::merge,
            )
            .unwrap();
        let mut every_node_conserved = every_node.file_declared_inclusive().clone();
        for id in &ids {
            let aggregate = every_node.node(*id).unwrap();
            assert_eq!(aggregate.declared_inclusive(), aggregate.local());
            every_node_conserved.merge(aggregate.declared_inclusive());
        }
        assert_eq!(every_node_conserved, *every_node.file_full_inclusive());
        assert_syntax_aggregate_oracle(&analysis, path, &ids, &every_node);

        let peer = analysis
            .file_node_ids(Path::new("peer.rs"))
            .unwrap()
            .next()
            .unwrap();
        assert!(matches!(
            all.node(peer).unwrap_err(),
            SyntaxAggregateLookupError::NodeOutsideFile { .. }
        ));
        assert!(matches!(
            all.node(NodeId {
                owner: analysis.owner,
                index: u32::MAX,
            })
            .unwrap_err(),
            SyntaxAggregateLookupError::NodeOutsideFile {
                requested: u32::MAX,
                ..
            }
        ));
        let second_snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("nested.rs", source.to_vec())
            .unwrap()
            .build()
            .unwrap();
        let second = ProjectAnalysis::build(second_snapshot).unwrap();
        assert_eq!(
            all.node(second.file_node_ids(path).unwrap().next().unwrap())
                .unwrap_err(),
            SyntaxAggregateLookupError::WrongAnalysis
        );
        let second_all = second
            .fold_syntax_aggregates(
                path,
                InclusiveSyntaxPolicy::AllDescendants,
                SyntaxTally::initialized,
                |value, region| value.merge(&SyntaxTally::for_region(region)),
                SyntaxTally::merge,
            )
            .unwrap();
        let keyed = |analysis: &ProjectAnalysis, aggregate: &SyntaxAggregates<'_, SyntaxTally>| {
            aggregate
                .nodes()
                .map(|node| {
                    (
                        analysis.node_key(node.id()).unwrap().clone(),
                        node.local().clone(),
                        node.full_inclusive().clone(),
                        node.declared_inclusive().clone(),
                        node.resets_parent(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(keyed(&analysis, &all), keyed(&second, &second_all));
        assert_eq!(all.file_local(), second_all.file_local());
        assert_eq!(all.file_full_inclusive(), second_all.file_full_inclusive());
    }

    #[test]
    fn syntax_aggregation_validates_resets_and_handles_partial_empty_and_unavailable_syntax() {
        let temp = tempfile::tempdir().unwrap();
        let partition_source = b"\n  fn value() -> &'static str { /* c */ \"h\xc3\xa9\" }\n\t";
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("target.rs", b"fn target() {}\n".to_vec())
            .unwrap()
            .with_overlay("peer.rs", b"fn peer() {}\n".to_vec())
            .unwrap()
            .with_overlay(
                "missing.ts",
                b"function f(a: string { return a; }\n".to_vec(),
            )
            .unwrap()
            .with_overlay("empty.rs", Vec::new())
            .unwrap()
            .with_overlay("whitespace.rs", b"\t \r\n  \n".to_vec())
            .unwrap()
            .with_overlay("partition.rs", partition_source.to_vec())
            .unwrap()
            .with_overlay("broken.rs", vec![0xff, 0xfe])
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let before_counts = analysis.parse_counts();
        let target = Path::new("target.rs");
        let peer = analysis
            .file_node_ids(Path::new("peer.rs"))
            .unwrap()
            .next()
            .unwrap();
        let foreign_snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("target.rs", b"fn target() {}\n".to_vec())
            .unwrap()
            .build()
            .unwrap();
        let foreign = ProjectAnalysis::build(foreign_snapshot).unwrap();
        let foreign_node = foreign.file_node_ids(target).unwrap().next().unwrap();
        let out_of_range = NodeId {
            owner: analysis.owner,
            index: u32::MAX,
        };

        for (reset, expected) in [
            (
                peer,
                SyntaxAggregationError::ResetNodeOutsideFile {
                    node: peer,
                    path: target.to_path_buf(),
                },
            ),
            (
                foreign_node,
                SyntaxAggregationError::InvalidResetNode {
                    node: foreign_node,
                    error: NodeLookupError::WrongAnalysis,
                },
            ),
            (
                out_of_range,
                SyntaxAggregationError::InvalidResetNode {
                    node: out_of_range,
                    error: NodeLookupError::OutOfRange {
                        requested: u32::MAX,
                        node_count: analysis.node_count() as u32,
                    },
                },
            ),
        ] {
            let callbacks = std::cell::Cell::new(0);
            let error = analysis
                .fold_syntax_aggregates(
                    target,
                    InclusiveSyntaxPolicy::ResetAt(&[reset]),
                    |owner| {
                        callbacks.set(callbacks.get() + 1);
                        SyntaxTally::initialized(owner)
                    },
                    |value, region| {
                        callbacks.set(callbacks.get() + 1);
                        value.merge(&SyntaxTally::for_region(region));
                    },
                    |parent, child| {
                        callbacks.set(callbacks.get() + 1);
                        parent.merge(child);
                    },
                )
                .unwrap_err();
            assert_eq!(error, expected);
            assert_eq!(callbacks.get(), 0);
        }

        for (path, expected) in [
            (
                Path::new("broken.rs"),
                SyntaxAggregationError::SyntaxUnavailable {
                    path: PathBuf::from("broken.rs"),
                },
            ),
            (
                Path::new("absent.rs"),
                SyntaxAggregationError::FileNotFound {
                    path: PathBuf::from("absent.rs"),
                },
            ),
        ] {
            let callbacks = std::cell::Cell::new(0);
            let error = analysis
                .fold_syntax_aggregates(
                    path,
                    InclusiveSyntaxPolicy::AllDescendants,
                    |owner| {
                        callbacks.set(callbacks.get() + 1);
                        SyntaxTally::initialized(owner)
                    },
                    |value, region| {
                        callbacks.set(callbacks.get() + 1);
                        value.merge(&SyntaxTally::for_region(region));
                    },
                    |parent, child| {
                        callbacks.set(callbacks.get() + 1);
                        parent.merge(child);
                    },
                )
                .unwrap_err();
            assert_eq!(error, expected);
            assert_eq!(callbacks.get(), 0);
        }

        let missing_path = Path::new("missing.ts");
        let partial = analysis
            .fold_syntax_aggregates(
                missing_path,
                InclusiveSyntaxPolicy::AllDescendants,
                SyntaxTally::initialized,
                |value, region| value.merge(&SyntaxTally::for_region(region)),
                SyntaxTally::merge,
            )
            .unwrap();
        assert_eq!(partial.file_full_inclusive().regions, 18);
        assert_eq!(partial.file_full_inclusive().bytes, 35);
        let missing = analysis
            .file_node_ids(missing_path)
            .unwrap()
            .find(|id| analysis.node(*id).unwrap().is_missing())
            .unwrap();
        assert_eq!(partial.node(missing).unwrap().local().missing_nodes, 1);
        assert_eq!(partial.node(missing).unwrap().local().regions, 0);
        assert_eq!(partial.node(missing).unwrap().local().bytes, 0);
        assert_syntax_aggregate_oracle(&analysis, missing_path, &[], &partial);

        let empty_path = Path::new("empty.rs");
        let mut empty_regions = 0;
        let empty = analysis
            .fold_syntax_aggregates(
                empty_path,
                InclusiveSyntaxPolicy::AllDescendants,
                SyntaxTally::initialized,
                |value, region| {
                    empty_regions += 1;
                    value.merge(&SyntaxTally::for_region(region));
                },
                SyntaxTally::merge,
            )
            .unwrap();
        assert_eq!(empty_regions, 0);
        assert_eq!(empty.len(), 1);
        assert_eq!(empty.file_full_inclusive().regions, 0);
        assert_eq!(empty.file_full_inclusive().bytes, 0);
        assert_eq!(empty.file_full_inclusive().node_owners, 1);
        assert_syntax_aggregate_oracle(&analysis, empty_path, &[], &empty);

        let whitespace_path = Path::new("whitespace.rs");
        let whitespace = analysis
            .fold_syntax_aggregates(
                whitespace_path,
                InclusiveSyntaxPolicy::AllDescendants,
                SyntaxTally::initialized,
                |value, region| value.merge(&SyntaxTally::for_region(region)),
                SyntaxTally::merge,
            )
            .unwrap();
        assert_eq!(whitespace.len(), 1);
        assert_eq!(whitespace.file_local().regions, 1);
        assert_eq!(whitespace.file_local().bytes, 7);
        assert_eq!(whitespace.nodes().next().unwrap().local().regions, 0);
        assert_syntax_aggregate_oracle(&analysis, whitespace_path, &[], &whitespace);

        let partition_path = Path::new("partition.rs");
        let partition = analysis
            .fold_syntax_aggregates(
                partition_path,
                InclusiveSyntaxPolicy::AllDescendants,
                SyntaxTally::initialized,
                |value, region| value.merge(&SyntaxTally::for_region(region)),
                SyntaxTally::merge,
            )
            .unwrap();
        let partition_root = analysis
            .file_node_ids(partition_path)
            .unwrap()
            .next()
            .unwrap();
        assert_eq!(partition_source.len(), 49);
        assert_eq!(partition.file_local().regions, 1);
        assert_eq!(partition.file_local().bytes, 3);
        assert_eq!(partition.file_full_inclusive().regions, 27);
        assert_eq!(partition.file_full_inclusive().bytes, 49);
        assert_eq!(
            partition
                .node(partition_root)
                .unwrap()
                .full_inclusive()
                .regions,
            26
        );
        assert_eq!(
            partition
                .node(partition_root)
                .unwrap()
                .full_inclusive()
                .bytes,
            46
        );
        assert_syntax_aggregate_oracle(&analysis, partition_path, &[], &partition);
        let root_reset = analysis
            .fold_syntax_aggregates(
                partition_path,
                InclusiveSyntaxPolicy::ResetAt(&[partition_root]),
                SyntaxTally::initialized,
                |value, region| value.merge(&SyntaxTally::for_region(region)),
                SyntaxTally::merge,
            )
            .unwrap();
        assert_eq!(root_reset.file_declared_inclusive().regions, 1);
        assert_eq!(root_reset.file_declared_inclusive().bytes, 3);
        assert_eq!(
            root_reset
                .node(partition_root)
                .unwrap()
                .declared_inclusive()
                .regions,
            26
        );
        assert_eq!(
            root_reset
                .node(partition_root)
                .unwrap()
                .declared_inclusive()
                .bytes,
            46
        );
        assert_syntax_aggregate_oracle(&analysis, partition_path, &[partition_root], &root_reset);
        assert_eq!(analysis.parse_counts(), before_counts);
    }

    #[test]
    fn syntax_aggregation_preserves_fallible_callback_context() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository())
            .unwrap()
            .with_overlay("fallible.rs", b"fn value() {}\n".to_vec())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let path = Path::new("fallible.rs");
        let root = analysis.file_node_ids(path).unwrap().next().unwrap();

        let mut initialized = 0;
        let init_error = analysis
            .try_fold_syntax_aggregates(
                path,
                InclusiveSyntaxPolicy::AllDescendants,
                |owner| {
                    initialized += 1;
                    match owner {
                        SyntaxOwner::File(_) => Ok(0_u8),
                        SyntaxOwner::Node(_) => Err(AggregateFailure("init")),
                    }
                },
                |_value, _region| Ok(()),
                |_parent, _child| Ok(()),
            )
            .unwrap_err();
        assert_eq!(initialized, 2);
        assert!(matches!(
            init_error,
            SyntaxAggregationError::InitializeOwner {
                owner: SyntaxAggregateOwner::Node(node),
                error: AggregateFailure("init"),
                ..
            } if node == root
        ));

        let mut folds = 0;
        let fold_error = analysis
            .try_fold_syntax_aggregates(
                path,
                InclusiveSyntaxPolicy::AllDescendants,
                |_owner| Ok::<u8, AggregateFailure>(0),
                |_value, _region| {
                    folds += 1;
                    Err(AggregateFailure("fold"))
                },
                |_parent, _child| Ok(()),
            )
            .unwrap_err();
        assert_eq!(folds, 1);
        assert!(matches!(
            fold_error,
            SyntaxAggregationError::FoldRegion {
                range,
                error: AggregateFailure("fold"),
                ..
            } if range.start == 0 && range.end > 0
        ));

        let merge_error = analysis
            .try_fold_syntax_aggregates(
                path,
                InclusiveSyntaxPolicy::AllDescendants,
                |_owner| Ok::<u8, AggregateFailure>(250),
                |_value, _region| Ok(()),
                |parent, child| {
                    *parent = parent
                        .checked_add(*child)
                        .ok_or(AggregateFailure("overflow"))?;
                    Ok(())
                },
            )
            .unwrap_err();
        assert!(matches!(
            merge_error,
            SyntaxAggregationError::Merge {
                projection: SyntaxAggregateProjection::FullInclusive,
                parent: SyntaxAggregateOwner::Node(_),
                child: SyntaxAggregateOwner::Node(_),
                error: AggregateFailure("overflow"),
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
        assert_eq!(point.exact_zero_width().first().unwrap().id(), missing.id());
        let point_measured = point.instrumentation();
        assert_eq!(point_measured.exact_zero_width_nodes, 1);
        assert_eq!(point_measured.exact_zero_width_bytes, 0);
        assert_eq!(
            point_measured.known_bytes_lower_bound,
            std::mem::size_of::<SyntaxPointContext<'_>>()
        );
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
        assert_eq!(
            empty_point.exact_zero_width().first().unwrap().raw_kind(),
            "source_file"
        );
        assert!(empty_point.before().is_none());
        assert!(empty_point.after().is_none());

        let whitespace_point = analysis
            .syntax_point_context(Path::new("whitespace.rs"), 7)
            .unwrap();
        assert_eq!(whitespace_point.exact_zero_width().len(), 1);
        assert_eq!(
            whitespace_point
                .exact_zero_width()
                .first()
                .unwrap()
                .span()
                .byte_range(),
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
        assert_eq!(
            shared_point.exact_zero_width().first().unwrap().raw_kind(),
            ";"
        );
        assert!(
            shared_point
                .exact_zero_width()
                .first()
                .unwrap()
                .is_missing()
        );
        let Some(SyntaxOwner::Node(after)) = shared_point.after() else {
            panic!("trailing TypeScript space must remain program-owned trivia");
        };
        assert_eq!(after.raw_kind(), "program");
        assert_ne!(
            after.id(),
            shared_point.exact_zero_width().first().unwrap().id()
        );
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

    fn instrumentation_snapshot(root: &Path, reverse: bool) -> Arc<ProjectSnapshot> {
        instrumentation_snapshot_with(
            root,
            reverse,
            None,
            b"fn alpha(value: i32) -> i32 { if value > 0 { value } else { 0 } }\n",
        )
    }

    fn instrumentation_snapshot_with(
        root: &Path,
        reverse: bool,
        store: Option<Arc<SourceStore>>,
        alpha: &[u8],
    ) -> Arc<ProjectSnapshot> {
        let sources = [
            ("alpha.rs", alpha),
            (
                "beta.py",
                b"def beta(value):\n    return value if value else 0\n".as_slice(),
            ),
            (
                "view.tsx",
                b"export const View = ({value}: {value: string}) => <span>{value}</span>;\n"
                    .as_slice(),
            ),
        ];
        let mut builder = ProjectSnapshotBuilder::new(root, repository()).unwrap();
        if let Some(store) = store {
            builder = builder.with_store(store);
        }
        if reverse {
            for (path, source) in sources.into_iter().rev() {
                builder = builder.with_overlay(path, source.to_vec()).unwrap();
            }
        } else {
            for (path, source) in sources {
                builder = builder.with_overlay(path, source.to_vec()).unwrap();
            }
        }
        builder.build().unwrap()
    }

    #[test]
    fn instrumentation_is_deterministic_and_exposes_owned_storage_costs() {
        let root = tempfile::tempdir().unwrap();
        let first = ProjectAnalysis::build(instrumentation_snapshot(root.path(), false)).unwrap();
        let reordered =
            ProjectAnalysis::build(instrumentation_snapshot(root.path(), true)).unwrap();
        let measured = first.instrumentation();

        assert_eq!(measured, reordered.instrumentation());
        assert!(measured.parse.invariant_holds());
        assert_eq!(
            (
                measured.parse.file_revisions,
                measured.parse.requested,
                measured.parse.owners,
                measured.parse.parser_invocations,
                measured.parse.reused,
                measured.parse.syntax_unavailable,
            ),
            (3, 3, 3, 3, 0, 0)
        );
        assert_eq!(measured.structure.files, 3);
        assert_eq!(measured.structure.nodes, first.node_count());
        assert_eq!(
            measured.structure.child_edges,
            measured.structure.nodes - measured.structure.files
        );
        assert!(measured.structure.syntax_segments > measured.structure.files);
        assert!(measured.structure.node_key_field_path_entries > measured.structure.nodes);
        assert!(measured.memory.known_bytes_lower_bound > measured.structure.source_bytes);
        assert!(
            measured.memory.node_key_bytes_lower_bound > measured.memory.arena_bytes_lower_bound
        );
        assert_eq!(measured.memory.opaque_tree_count, 3);
        assert_eq!(
            measured.node_order_digest,
            "pao1_437c1bdc53a43224fde0a0c23fcebbca531996848a87585944f60fe5759c55ed"
        );
    }

    #[test]
    #[ignore = "wall-time and retained-memory probe; run explicitly at M1 instrumentation checkpoints"]
    fn project_analysis_latency_and_memory_probe() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(SourceStore::default());
        let started = std::time::Instant::now();
        let analysis = ProjectAnalysis::build(instrumentation_snapshot_with(
            root.path(),
            false,
            Some(Arc::clone(&store)),
            b"fn alpha(value: i32) -> i32 { if value > 0 { value } else { 0 } }\n",
        ))
        .unwrap();
        let cold = started.elapsed();
        let started = std::time::Instant::now();
        let measured = analysis.instrumentation();
        let instrumentation = started.elapsed();

        let query = analysis
            .compile_syntax_query(Path::new("alpha.rs"), "(identifier) @identifier")
            .unwrap();
        let started = std::time::Instant::now();
        let mut child_edges = 0;
        let mut key_lookups = 0;
        for id in analysis.node_ids() {
            let node = analysis.node(id).unwrap();
            child_edges += node.children().len();
            analysis.node_by_key(node.key()).unwrap();
            key_lookups += 1;
        }
        let query_root = analysis
            .file_node_ids(Path::new("alpha.rs"))
            .unwrap()
            .next()
            .unwrap();
        let captures = analysis.syntax_query_captures(&query, query_root).unwrap();
        let point_context = analysis
            .syntax_point_context(Path::new("alpha.rs"), 0)
            .unwrap();
        let point_measured = point_context.instrumentation();
        let repeated = started.elapsed();
        let query_measured = query.instrumentation();
        let results_measured = query.results_instrumentation(&captures);

        let changed = instrumentation_snapshot_with(
            root.path(),
            false,
            Some(store),
            b"fn alpha(value: i32) -> i32 { if value > 1 { value } else { 0 } }\n",
        );
        let replacement_at = analysis
            .file(Path::new("alpha.rs"))
            .unwrap()
            .source()
            .windows(3)
            .position(|window| window == b"> 0")
            .unwrap()
            + 2;
        let edits = [crate::FileSourceEdits::new(
            "alpha.rs",
            vec![crate::SourceReplacement::new(
                replacement_at..replacement_at + 1,
                "1",
            )],
        )];
        let started = std::time::Instant::now();
        let update = analysis.successor_with_edits(changed, &edits).unwrap();
        let incremental = started.elapsed();
        let update_measured = update.instrumentation();

        eprintln!(
            "m1.11 cold_us={} instrumentation_us={} repeated_us={} incremental_us={} files={} source_bytes={} nodes={} segments={} child_edges={}/{} key_lookups={} point_zero={}/{} query_source={} query_metadata={} query_captures={} query_result_bytes={} node_key_bytes={} node_key_file_payload={} node_key_field_path={} node_key_lookup_index={} query_node_index={} containment_index={} arena_bytes={} known_bytes={} update_files={}/{}/{}/{}/{} update_nodes={}/{}/{} transitions={}/{}/{} transition_bytes={} edit_validation_bytes={} opaque_trees={} order={}",
            cold.as_micros(),
            instrumentation.as_micros(),
            repeated.as_micros(),
            incremental.as_micros(),
            measured.structure.files,
            measured.structure.source_bytes,
            measured.structure.nodes,
            measured.structure.syntax_segments,
            child_edges,
            measured.structure.child_edges,
            key_lookups,
            point_measured.exact_zero_width_nodes,
            point_measured.exact_zero_width_bytes,
            query_measured.source_bytes,
            query_measured.known_bytes_lower_bound,
            results_measured.captures,
            results_measured.known_bytes_lower_bound,
            measured.memory.node_key_bytes_lower_bound,
            measured.memory.node_key_file_revision_payload_bytes,
            measured.memory.node_key_field_path_bytes,
            measured.memory.node_key_lookup_index_bytes,
            measured.memory.query_node_index_bytes,
            measured.memory.containment_index_bytes,
            measured.memory.arena_bytes_lower_bound,
            measured.memory.known_bytes_lower_bound,
            update_measured.reused_files,
            update_measured.incremental_files,
            update_measured.rebuilt_files,
            update_measured.added_files,
            update_measured.removed_files,
            update_measured.previous_nodes,
            update_measured.current_nodes,
            update_measured.incrementally_rebuilt_nodes,
            update_measured.retained_transitions,
            update_measured.reanchored_transitions,
            update_measured.expired_transitions,
            update_measured.transition_bytes_lower_bound,
            update_measured.sequential_edit_validation_bytes_upper_bound,
            measured.memory.opaque_tree_count,
            measured.node_order_digest,
        );
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
