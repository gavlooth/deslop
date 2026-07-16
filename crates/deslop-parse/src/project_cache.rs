//! Persistent, version-complete content-addressed analysis artifacts (M9.1).

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{CloneCandidateIndex, FileRevisionKey, ProjectSnapshot, RepositoryId};

pub const ARTIFACT_CACHE_KEY_SCHEMA: &str = "deslop.artifact-cache-key/1";
pub const ARTIFACT_CACHE_RECORD_SCHEMA: &str = "deslop.artifact-cache-record/1";
const CACHE_KEY_DOMAIN: &str = "deslop artifact cache key v1";
const CACHE_DIRECTORY_ENV: &str = "DESLOP_CACHE_DIR";
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    OwnedSyntax,
    ScopeGraph,
    ControlFlow,
    ProgramDependence,
    CloneBuckets,
    Metrics,
    Candidates,
    ProjectSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheSemanticVersions {
    adapter: String,
    graph_schema: String,
    recipe: String,
    model: String,
}

impl CacheSemanticVersions {
    pub fn new(
        adapter: impl Into<String>,
        graph_schema: impl Into<String>,
        recipe: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, ProjectCacheError> {
        let value = Self {
            adapter: adapter.into(),
            graph_schema: graph_schema.into(),
            recipe: recipe.into(),
            model: model.into(),
        };
        value.validate()?;
        Ok(value)
    }

    /// Bind a semantic version vector to the exact stored adapter identities in
    /// one immutable snapshot. Callers provide the graph/recipe/model schemas;
    /// no consumer may substitute a language name for adapter identity.
    pub fn for_snapshot(
        snapshot: &ProjectSnapshot,
        graph_schema: impl Into<String>,
        recipe: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, ProjectCacheError> {
        let mut hasher = blake3::Hasher::new_derive_key("deslop snapshot adapter set v1");
        for entry in snapshot.entries() {
            let Some(adapter) = entry.language_adapter_identity() else {
                continue;
            };
            hash_part(&mut hasher, entry.path().to_string_lossy().as_bytes());
            hash_part(&mut hasher, &adapter.identity_bytes());
        }
        Self::new(
            format!("adapterset1_{}", hasher.finalize().to_hex()),
            graph_schema,
            recipe,
            model,
        )
    }

    pub fn for_file(
        snapshot: &ProjectSnapshot,
        path: &Path,
        graph_schema: impl Into<String>,
        recipe: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self, ProjectCacheError> {
        let entry = snapshot.entry(path).ok_or_else(|| {
            ProjectCacheError::Invalid(format!(
                "cache version path {} is not in the snapshot",
                path.display()
            ))
        })?;
        let adapter = entry.language_adapter_identity().ok_or_else(|| {
            ProjectCacheError::Invalid(format!(
                "cache version path {} has no language adapter",
                path.display()
            ))
        })?;
        let mut hasher = blake3::Hasher::new_derive_key("deslop file adapter identity v1");
        hash_part(&mut hasher, &adapter.identity_bytes());
        Self::new(
            format!("adapter1_{}", hasher.finalize().to_hex()),
            graph_schema,
            recipe,
            model,
        )
    }

    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    pub fn graph_schema(&self) -> &str {
        &self.graph_schema
    }

    pub fn recipe(&self) -> &str {
        &self.recipe
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    fn validate(&self) -> Result<(), ProjectCacheError> {
        for (name, value) in [
            ("adapter", &self.adapter),
            ("graph schema", &self.graph_schema),
            ("recipe", &self.recipe),
            ("model", &self.model),
        ] {
            if value.trim().is_empty() {
                return Err(ProjectCacheError::Invalid(format!(
                    "cache {name} version must not be empty"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ArtifactCacheKeyId(String);

impl ArtifactCacheKeyId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ArtifactCacheKeyId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if !is_cache_id(&value) {
            return Err(D::Error::custom("invalid artifact cache key identity"));
        }
        Ok(Self(value))
    }
}

impl fmt::Display for ArtifactCacheKeyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactCacheKey {
    schema: String,
    id: ArtifactCacheKeyId,
    kind: ArtifactKind,
    repository: RepositoryId,
    scope: String,
    inputs: Vec<FileRevisionKey>,
    versions: CacheSemanticVersions,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactCacheKeyWire {
    schema: String,
    id: ArtifactCacheKeyId,
    kind: ArtifactKind,
    repository: RepositoryId,
    scope: String,
    inputs: Vec<FileRevisionKey>,
    versions: CacheSemanticVersions,
}

impl<'de> Deserialize<'de> for ArtifactCacheKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ArtifactCacheKeyWire::deserialize(deserializer)?;
        let value = Self {
            schema: wire.schema,
            id: wire.id,
            kind: wire.kind,
            repository: wire.repository,
            scope: wire.scope,
            inputs: wire.inputs,
            versions: wire.versions,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

impl ArtifactCacheKey {
    pub fn new(
        kind: ArtifactKind,
        scope: impl Into<String>,
        mut inputs: Vec<FileRevisionKey>,
        versions: CacheSemanticVersions,
    ) -> Result<Self, ProjectCacheError> {
        if inputs.is_empty() {
            return Err(ProjectCacheError::Invalid(
                "artifact cache key requires at least one content/grammar input".into(),
            ));
        }
        inputs.sort();
        let repository = inputs[0].repository.clone();
        let scope = scope.into();
        let mut value = Self {
            schema: ARTIFACT_CACHE_KEY_SCHEMA.into(),
            id: ArtifactCacheKeyId(String::new()),
            kind,
            repository,
            scope,
            inputs,
            versions,
        };
        value.id = value.expected_id()?;
        value.validate()?;
        Ok(value)
    }

    pub fn id(&self) -> &ArtifactCacheKeyId {
        &self.id
    }

    pub fn kind(&self) -> ArtifactKind {
        self.kind
    }

    pub fn repository(&self) -> &RepositoryId {
        &self.repository
    }

    pub fn scope(&self) -> &str {
        &self.scope
    }

    pub fn inputs(&self) -> &[FileRevisionKey] {
        &self.inputs
    }

    pub fn versions(&self) -> &CacheSemanticVersions {
        &self.versions
    }

    fn validate(&self) -> Result<(), ProjectCacheError> {
        if self.schema != ARTIFACT_CACHE_KEY_SCHEMA {
            return Err(ProjectCacheError::Invalid(format!(
                "unsupported artifact cache key schema {}",
                self.schema
            )));
        }
        if self.scope.trim().is_empty() {
            return Err(ProjectCacheError::Invalid(
                "artifact cache scope must not be empty".into(),
            ));
        }
        if self.inputs.is_empty() {
            return Err(ProjectCacheError::Invalid(
                "artifact cache key has no content inputs".into(),
            ));
        }
        self.versions.validate()?;
        for input in &self.inputs {
            if input.repository != self.repository {
                return Err(ProjectCacheError::Invalid(
                    "artifact cache inputs span repository identities".into(),
                ));
            }
        }
        if self.inputs.windows(2).any(|window| window[0] >= window[1]) {
            return Err(ProjectCacheError::Invalid(
                "artifact cache inputs must be sorted and unique".into(),
            ));
        }
        let expected = self.expected_id()?;
        if self.id != expected {
            return Err(ProjectCacheError::Invalid(
                "artifact cache identity does not match content and semantic versions".into(),
            ));
        }
        Ok(())
    }

    fn expected_id(&self) -> Result<ArtifactCacheKeyId, ProjectCacheError> {
        #[derive(Serialize)]
        struct Identity<'a> {
            schema: &'a str,
            kind: ArtifactKind,
            repository: &'a RepositoryId,
            scope: &'a str,
            inputs: &'a [FileRevisionKey],
            versions: &'a CacheSemanticVersions,
        }
        let bytes = serde_json::to_vec(&Identity {
            schema: &self.schema,
            kind: self.kind,
            repository: &self.repository,
            scope: &self.scope,
            inputs: &self.inputs,
            versions: &self.versions,
        })
        .map_err(|error| ProjectCacheError::Serialization(error.to_string()))?;
        let digest = blake3::derive_key(CACHE_KEY_DOMAIN, &bytes);
        Ok(ArtifactCacheKeyId(format!(
            "pca1_{}",
            blake3::Hash::from_bytes(digest).to_hex()
        )))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheLookup<T> {
    Hit(Arc<T>),
    Miss,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStatistics {
    pub hits: u64,
    pub misses: u64,
    pub writes: u64,
    pub reused_writes: u64,
    pub corruptions: u64,
}

#[derive(Debug, Default)]
struct CacheCounters {
    hits: AtomicU64,
    misses: AtomicU64,
    writes: AtomicU64,
    reused_writes: AtomicU64,
    corruptions: AtomicU64,
}

#[derive(Debug, Clone)]
pub struct PersistentArtifactCache {
    root: PathBuf,
    counters: Arc<CacheCounters>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactRecord {
    schema: String,
    key: ArtifactCacheKey,
    payload_digest: String,
    payload: Vec<u8>,
}

impl PersistentArtifactCache {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ProjectCacheError> {
        let root = root.into();
        fs::create_dir_all(root.join("objects"))
            .map_err(|error| ProjectCacheError::Io(root.clone(), error))?;
        Ok(Self {
            root,
            counters: Arc::default(),
        })
    }

    pub fn from_environment() -> Result<Option<Self>, ProjectCacheError> {
        let Some(root) = std::env::var_os(CACHE_DIRECTORY_ENV) else {
            return Ok(None);
        };
        if root.is_empty() {
            return Err(ProjectCacheError::Invalid(format!(
                "{CACHE_DIRECTORY_ENV} must not be empty"
            )));
        }
        Self::open(PathBuf::from(root)).map(Some)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn statistics(&self) -> CacheStatistics {
        CacheStatistics {
            hits: self.counters.hits.load(Ordering::Relaxed),
            misses: self.counters.misses.load(Ordering::Relaxed),
            writes: self.counters.writes.load(Ordering::Relaxed),
            reused_writes: self.counters.reused_writes.load(Ordering::Relaxed),
            corruptions: self.counters.corruptions.load(Ordering::Relaxed),
        }
    }

    pub fn put_bytes(
        &self,
        key: &ArtifactCacheKey,
        payload: &[u8],
    ) -> Result<(), ProjectCacheError> {
        key.validate()?;
        let record = ArtifactRecord {
            schema: ARTIFACT_CACHE_RECORD_SCHEMA.into(),
            key: key.clone(),
            payload_digest: payload_digest(payload),
            payload: payload.to_vec(),
        };
        let bytes = serde_json::to_vec(&record)
            .map_err(|error| ProjectCacheError::Serialization(error.to_string()))?;
        let path = self.object_path(key.id());
        let parent = path
            .parent()
            .expect("cache object paths always have a parent");
        fs::create_dir_all(parent)
            .map_err(|error| ProjectCacheError::Io(parent.to_path_buf(), error))?;

        if path.exists() {
            self.verify_existing_write(&path, key, payload)?;
            self.counters.reused_writes.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }

        let temp = parent.join(format!(
            ".{}.{}.{}.tmp",
            key.id().as_str(),
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|error| ProjectCacheError::Io(temp.clone(), error))?;
        let publish = (|| {
            file.write_all(&bytes)
                .map_err(|error| ProjectCacheError::Io(temp.clone(), error))?;
            drop(file);
            match fs::hard_link(&temp, &path) {
                Ok(()) => {
                    fs::remove_file(&temp)
                        .map_err(|error| ProjectCacheError::Io(temp.clone(), error))?;
                    self.counters.writes.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    fs::remove_file(&temp)
                        .map_err(|remove| ProjectCacheError::Io(temp.clone(), remove))?;
                    self.verify_existing_write(&path, key, payload)?;
                    self.counters.reused_writes.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }
                Err(error) => Err(ProjectCacheError::Io(path.clone(), error)),
            }
        })();
        if publish.is_err() {
            let _ = fs::remove_file(&temp);
        }
        publish
    }

    pub fn get_bytes(
        &self,
        key: &ArtifactCacheKey,
    ) -> Result<CacheLookup<Vec<u8>>, ProjectCacheError> {
        key.validate()?;
        let path = self.object_path(key.id());
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.counters.misses.fetch_add(1, Ordering::Relaxed);
                return Ok(CacheLookup::Miss);
            }
            Err(error) => return Err(ProjectCacheError::Io(path, error)),
        };
        let record: ArtifactRecord = self.decode_record(&path, &bytes)?;
        if &record.key != key {
            self.record_corruption();
            return Err(ProjectCacheError::Corrupt(
                path,
                "record key does not match requested key".into(),
            ));
        }
        self.counters.hits.fetch_add(1, Ordering::Relaxed);
        Ok(CacheLookup::Hit(Arc::new(record.payload)))
    }

    pub fn put_json<T: Serialize>(
        &self,
        key: &ArtifactCacheKey,
        value: &T,
    ) -> Result<(), ProjectCacheError> {
        let payload = serde_json::to_vec(value)
            .map_err(|error| ProjectCacheError::Serialization(error.to_string()))?;
        self.put_bytes(key, &payload)
    }

    pub fn get_json<T: DeserializeOwned>(
        &self,
        key: &ArtifactCacheKey,
    ) -> Result<CacheLookup<T>, ProjectCacheError> {
        match self.get_bytes(key)? {
            CacheLookup::Miss => Ok(CacheLookup::Miss),
            CacheLookup::Hit(bytes) => match serde_json::from_slice(&bytes) {
                Ok(value) => Ok(CacheLookup::Hit(Arc::new(value))),
                Err(error) => {
                    self.record_corruption();
                    Err(ProjectCacheError::Corrupt(
                        self.object_path(key.id()),
                        format!("artifact payload is invalid: {error}"),
                    ))
                }
            },
        }
    }

    /// Persist the validated M5 clone bucket index without introducing a second
    /// clone lookup representation or an all-project pair enumeration path.
    pub fn put_clone_index(
        &self,
        key: &ArtifactCacheKey,
        index: &CloneCandidateIndex,
    ) -> Result<(), ProjectCacheError> {
        require_kind(key, ArtifactKind::CloneBuckets)?;
        self.put_json(key, index)
    }

    pub fn get_clone_index(
        &self,
        key: &ArtifactCacheKey,
    ) -> Result<CacheLookup<CloneCandidateIndex>, ProjectCacheError> {
        require_kind(key, ArtifactKind::CloneBuckets)?;
        self.get_json(key)
    }

    fn verify_existing_write(
        &self,
        path: &Path,
        key: &ArtifactCacheKey,
        payload: &[u8],
    ) -> Result<(), ProjectCacheError> {
        let bytes =
            fs::read(path).map_err(|error| ProjectCacheError::Io(path.to_path_buf(), error))?;
        let record = self.decode_record(path, &bytes)?;
        if record.key != *key || record.payload != payload {
            return Err(ProjectCacheError::Conflict {
                key: key.id().clone(),
                path: path.to_path_buf(),
            });
        }
        Ok(())
    }

    fn decode_record(
        &self,
        path: &Path,
        bytes: &[u8],
    ) -> Result<ArtifactRecord, ProjectCacheError> {
        let record: ArtifactRecord = serde_json::from_slice(bytes).map_err(|error| {
            self.record_corruption();
            ProjectCacheError::Corrupt(path.to_path_buf(), error.to_string())
        })?;
        if record.schema != ARTIFACT_CACHE_RECORD_SCHEMA {
            self.record_corruption();
            return Err(ProjectCacheError::Corrupt(
                path.to_path_buf(),
                format!("unsupported record schema {}", record.schema),
            ));
        }
        let actual = payload_digest(&record.payload);
        if record.payload_digest != actual {
            self.record_corruption();
            return Err(ProjectCacheError::Corrupt(
                path.to_path_buf(),
                "payload checksum mismatch".into(),
            ));
        }
        Ok(record)
    }

    fn object_path(&self, id: &ArtifactCacheKeyId) -> PathBuf {
        let hex = id.as_str().strip_prefix("pca1_").unwrap_or("invalid");
        self.root
            .join("objects")
            .join(&hex[..hex.len().min(2)])
            .join(format!("{}.json", id.as_str()))
    }

    fn record_corruption(&self) {
        self.counters.corruptions.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub enum ProjectCacheError {
    Invalid(String),
    Serialization(String),
    Io(PathBuf, io::Error),
    Corrupt(PathBuf, String),
    Conflict {
        key: ArtifactCacheKeyId,
        path: PathBuf,
    },
}

impl fmt::Display for ProjectCacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "invalid project cache input: {message}"),
            Self::Serialization(message) => {
                write!(formatter, "project cache serialization failed: {message}")
            }
            Self::Io(path, error) => write!(
                formatter,
                "project cache I/O failed at {}: {error}",
                path.display()
            ),
            Self::Corrupt(path, message) => write!(
                formatter,
                "corrupt project cache record {}: {message}",
                path.display()
            ),
            Self::Conflict { key, path } => write!(
                formatter,
                "artifact {key} conflicts with immutable cache record {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ProjectCacheError {}

fn payload_digest(payload: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(payload).to_hex())
}

fn hash_part(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn require_kind(key: &ArtifactCacheKey, expected: ArtifactKind) -> Result<(), ProjectCacheError> {
    if key.kind() != expected {
        return Err(ProjectCacheError::Invalid(format!(
            "cache operation requires {expected:?}, received {:?}",
            key.kind()
        )));
    }
    Ok(())
}

fn is_cache_id(value: &str) -> bool {
    value.strip_prefix("pca1_").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectAnalysis, ProjectSnapshotBuilder};

    fn input(source: &[u8]) -> FileRevisionKey {
        let temp = tempfile::tempdir().unwrap();
        let repository = RepositoryId::explicit("cache-test").unwrap();
        let snapshot = ProjectSnapshotBuilder::new(temp.path(), repository)
            .unwrap()
            .with_overlay("src/lib.rs", source.to_vec())
            .unwrap()
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        analysis
            .file(Path::new("src/lib.rs"))
            .unwrap()
            .key()
            .clone()
    }

    fn versions(model: &str) -> CacheSemanticVersions {
        CacheSemanticVersions::new(
            "rust-adapter/3",
            "deslop.graph/9",
            "deslop.recipes/7",
            model,
        )
        .unwrap()
    }

    #[test]
    fn cache_key_binds_content_grammar_adapter_graph_recipe_and_model() {
        let base = input(b"fn value() -> i32 { 1 }\n");
        let first = ArtifactCacheKey::new(
            ArtifactKind::Metrics,
            "project",
            vec![base.clone()],
            versions("evidence-only"),
        )
        .unwrap();
        let reordered = ArtifactCacheKey::new(
            ArtifactKind::Metrics,
            "project",
            vec![base],
            versions("evidence-only"),
        )
        .unwrap();
        let changed_content = ArtifactCacheKey::new(
            ArtifactKind::Metrics,
            "project",
            vec![input(b"fn value() -> i32 { 2 }\n")],
            versions("evidence-only"),
        )
        .unwrap();
        let changed_model = ArtifactCacheKey::new(
            ArtifactKind::Metrics,
            "project",
            vec![input(b"fn value() -> i32 { 1 }\n")],
            versions("model-2"),
        )
        .unwrap();

        for changed_versions in [
            CacheSemanticVersions::new(
                "rust-adapter/4",
                "deslop.graph/9",
                "deslop.recipes/7",
                "evidence-only",
            )
            .unwrap(),
            CacheSemanticVersions::new(
                "rust-adapter/3",
                "deslop.graph/10",
                "deslop.recipes/7",
                "evidence-only",
            )
            .unwrap(),
            CacheSemanticVersions::new(
                "rust-adapter/3",
                "deslop.graph/9",
                "deslop.recipes/8",
                "evidence-only",
            )
            .unwrap(),
        ] {
            let changed = ArtifactCacheKey::new(
                ArtifactKind::Metrics,
                "project",
                vec![input(b"fn value() -> i32 { 1 }\n")],
                changed_versions,
            )
            .unwrap();
            assert_ne!(first.id(), changed.id());
        }

        assert_eq!(first.id(), reordered.id());
        assert_ne!(first.id(), changed_content.id());
        assert_ne!(first.id(), changed_model.id());
        assert!(
            ArtifactCacheKey::new(
                ArtifactKind::Metrics,
                "project",
                Vec::new(),
                versions("evidence-only")
            )
            .is_err()
        );

        let mut tampered = serde_json::to_value(&first).unwrap();
        tampered["inputs"][0]["grammar"]["grammar_version"] = serde_json::json!("tampered");
        assert!(serde_json::from_value::<ArtifactCacheKey>(tampered).is_err());
    }

    #[test]
    fn persistent_cache_reuses_exact_records_and_rejects_conflicts() {
        let temp = tempfile::tempdir().unwrap();
        let cache = PersistentArtifactCache::open(temp.path()).unwrap();
        let key = ArtifactCacheKey::new(
            ArtifactKind::ScopeGraph,
            "src/lib.rs",
            vec![input(b"fn main() {}\n")],
            versions("none"),
        )
        .unwrap();

        assert_eq!(cache.get_bytes(&key).unwrap(), CacheLookup::Miss);
        cache.put_bytes(&key, b"scope graph").unwrap();
        cache.put_bytes(&key, b"scope graph").unwrap();
        assert_eq!(
            cache.get_bytes(&key).unwrap(),
            CacheLookup::Hit(Arc::new(b"scope graph".to_vec()))
        );
        assert!(matches!(
            cache.put_bytes(&key, b"different graph"),
            Err(ProjectCacheError::Conflict { .. })
        ));
        assert_eq!(
            cache.statistics(),
            CacheStatistics {
                hits: 1,
                misses: 1,
                writes: 1,
                reused_writes: 1,
                corruptions: 0,
            }
        );
    }

    #[test]
    fn persistent_cache_fails_closed_on_tampered_record() {
        let temp = tempfile::tempdir().unwrap();
        let cache = PersistentArtifactCache::open(temp.path()).unwrap();
        let key = ArtifactCacheKey::new(
            ArtifactKind::Candidates,
            "src/lib.rs",
            vec![input(b"fn main() {}\n")],
            versions("none"),
        )
        .unwrap();
        cache.put_bytes(&key, b"candidate").unwrap();
        let path = cache.object_path(key.id());
        let bytes = fs::read(&path).unwrap();
        let mut record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        record["payload"][0] = serde_json::json!(0);
        fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();

        assert!(matches!(
            cache.get_bytes(&key),
            Err(ProjectCacheError::Corrupt(_, _))
        ));
        assert_eq!(cache.statistics().corruptions, 1);
    }

    #[test]
    fn clone_bucket_index_round_trips_through_typed_persistent_path() {
        let temp = tempfile::tempdir().unwrap();
        let cache = PersistentArtifactCache::open(temp.path()).unwrap();
        let key = ArtifactCacheKey::new(
            ArtifactKind::CloneBuckets,
            "project",
            vec![input(b"fn main() {}\n")],
            versions("none"),
        )
        .unwrap();
        let index = CloneCandidateIndex::build(Vec::new()).unwrap();
        assert_eq!(index.construction_pair_comparisons(), 0);

        cache.put_clone_index(&key, &index).unwrap();
        let CacheLookup::Hit(loaded) = cache.get_clone_index(&key).unwrap() else {
            panic!("persisted clone index must be a hit");
        };
        assert_eq!(loaded.id(), index.id());
        assert_eq!(loaded.bucket_count(), 0);
        assert_eq!(loaded.construction_pair_comparisons(), 0);
    }
}
