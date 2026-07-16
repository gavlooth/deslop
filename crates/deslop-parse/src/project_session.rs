//! Portable persistent project snapshots shared by CLI/MCP/LSP/evaluator/agent sessions (M9.7).

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::snapshot::restore_project_snapshot;
use crate::{
    CacheSemanticVersions, ProjectSnapshot, RepositoryId, ScopeEntry, ScopeEntryKind,
    SnapshotEntryKind, SourceRevision, SourceStore,
};

pub const PROJECT_SESSION_MANIFEST_SCHEMA: &str = "deslop.project-session/1";
const SESSION_ID_DOMAIN: &str = "deslop persistent project session v1";
const CACHE_DIRECTORY_ENV: &str = "DESLOP_CACHE_DIR";
const SESSION_ID_ENV: &str = "DESLOP_SESSION_ID";
const PROJECT_GRAPH_VERSION: &str = "deslop.project-graph-suite/9";
const PROJECT_RECIPE_VERSION: &str = "deslop.transformation-recipe/1";
const PROJECT_MODEL_VERSION: &str = "deslop.metrics/6:evidence-only";
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ProjectSessionId(String);

impl ProjectSessionId {
    pub fn parse(value: impl Into<String>) -> Result<Self, ProjectSessionError> {
        let value = value.into();
        if !is_session_id(&value) {
            return Err(ProjectSessionError::Invalid(
                "invalid persistent project session identity".into(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ProjectSessionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

impl fmt::Display for ProjectSessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectSessionCapture {
    Stored(ProjectSessionId),
    Reused(ProjectSessionId),
}

impl ProjectSessionCapture {
    pub fn id(&self) -> &ProjectSessionId {
        match self {
            Self::Stored(id) | Self::Reused(id) => id,
        }
    }

    pub fn was_reused(&self) -> bool {
        matches!(self, Self::Reused(_))
    }
}

#[derive(Debug)]
pub struct RestoredProjectSession {
    id: ProjectSessionId,
    snapshot: Arc<ProjectSnapshot>,
    versions: CacheSemanticVersions,
}

impl RestoredProjectSession {
    pub fn id(&self) -> &ProjectSessionId {
        &self.id
    }

    pub fn snapshot(&self) -> &Arc<ProjectSnapshot> {
        &self.snapshot
    }

    pub fn versions(&self) -> &CacheSemanticVersions {
        &self.versions
    }

    pub fn into_snapshot(self) -> Arc<ProjectSnapshot> {
        self.snapshot
    }
}

#[derive(Debug, Clone)]
pub struct ProjectSessionStore {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PersistedEntryKind {
    Source,
    AnalysisInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PersistedScopeKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedScopeEntry {
    path: String,
    kind: PersistedScopeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedSnapshotEntry {
    path: String,
    kind: PersistedEntryKind,
    revision: SourceRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectSessionManifest {
    schema: String,
    id: ProjectSessionId,
    snapshot_id: String,
    repository: RepositoryId,
    root: String,
    requested_scope: Vec<PersistedScopeEntry>,
    entries: Vec<PersistedSnapshotEntry>,
    versions: CacheSemanticVersions,
}

impl ProjectSessionStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ProjectSessionError> {
        let root = root.into();
        for directory in [root.join("sessions"), root.join("sources")] {
            fs::create_dir_all(&directory)
                .map_err(|error| ProjectSessionError::Io(directory, error))?;
        }
        Ok(Self { root })
    }

    pub fn from_environment() -> Result<Option<Self>, ProjectSessionError> {
        let Some(root) = std::env::var_os(CACHE_DIRECTORY_ENV) else {
            return Ok(None);
        };
        if root.is_empty() {
            return Err(ProjectSessionError::Invalid(format!(
                "{CACHE_DIRECTORY_ENV} must not be empty"
            )));
        }
        Self::open(PathBuf::from(root)).map(Some)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn capture(
        &self,
        snapshot: &ProjectSnapshot,
        versions: CacheSemanticVersions,
    ) -> Result<ProjectSessionCapture, ProjectSessionError> {
        let root = path_string(snapshot.root(), "repository root")?;
        let requested_scope = snapshot
            .requested_scope()
            .iter()
            .map(|entry| {
                Ok(PersistedScopeEntry {
                    path: path_string(&entry.path, "scope path")?,
                    kind: match entry.kind {
                        ScopeEntryKind::File => PersistedScopeKind::File,
                        ScopeEntryKind::Directory => PersistedScopeKind::Directory,
                    },
                })
            })
            .collect::<Result<Vec<_>, ProjectSessionError>>()?;
        let mut entries = Vec::new();
        for entry in snapshot.entries() {
            self.publish_source(entry.revision(), entry.bytes())?;
            entries.push(PersistedSnapshotEntry {
                path: path_string(entry.path(), "snapshot entry path")?,
                kind: match entry.kind() {
                    SnapshotEntryKind::Source => PersistedEntryKind::Source,
                    SnapshotEntryKind::AnalysisInput => PersistedEntryKind::AnalysisInput,
                },
                revision: entry.revision().clone(),
            });
        }
        let id = expected_session_id(snapshot.id().as_str(), &versions)?;
        let manifest = ProjectSessionManifest {
            schema: PROJECT_SESSION_MANIFEST_SCHEMA.into(),
            id: id.clone(),
            snapshot_id: snapshot.id().as_str().into(),
            repository: snapshot.repository().clone(),
            root,
            requested_scope,
            entries,
            versions,
        };
        validate_manifest(&manifest)?;
        let bytes = serde_json::to_vec(&manifest)
            .map_err(|error| ProjectSessionError::Serialization(error.to_string()))?;
        let stored = publish_immutable(&self.manifest_path(&id), &bytes)?;
        Ok(if stored {
            ProjectSessionCapture::Stored(id)
        } else {
            ProjectSessionCapture::Reused(id)
        })
    }

    pub fn restore(
        &self,
        id: &ProjectSessionId,
    ) -> Result<RestoredProjectSession, ProjectSessionError> {
        let path = self.manifest_path(id);
        let bytes =
            fs::read(&path).map_err(|error| ProjectSessionError::Io(path.clone(), error))?;
        let manifest: ProjectSessionManifest = serde_json::from_slice(&bytes)
            .map_err(|error| ProjectSessionError::Corrupt(path.clone(), error.to_string()))?;
        validate_manifest(&manifest)?;
        if &manifest.id != id {
            return Err(ProjectSessionError::Corrupt(
                path,
                "manifest identity does not match requested session".into(),
            ));
        }

        let requested_scope = manifest
            .requested_scope
            .into_iter()
            .map(|entry| ScopeEntry {
                path: PathBuf::from(entry.path),
                kind: match entry.kind {
                    PersistedScopeKind::File => ScopeEntryKind::File,
                    PersistedScopeKind::Directory => ScopeEntryKind::Directory,
                },
            })
            .collect();
        let mut entries = Vec::new();
        for entry in manifest.entries {
            let bytes = self.read_source(&entry.revision)?;
            entries.push((
                PathBuf::from(entry.path),
                match entry.kind {
                    PersistedEntryKind::Source => SnapshotEntryKind::Source,
                    PersistedEntryKind::AnalysisInput => SnapshotEntryKind::AnalysisInput,
                },
                bytes,
            ));
        }
        let snapshot = restore_project_snapshot(
            &manifest.snapshot_id,
            manifest.repository,
            PathBuf::from(manifest.root),
            requested_scope,
            entries,
            Arc::new(SourceStore::default()),
        )
        .map_err(|error| ProjectSessionError::Corrupt(self.manifest_path(id), error.to_string()))?;
        Ok(RestoredProjectSession {
            id: id.clone(),
            snapshot,
            versions: manifest.versions,
        })
    }

    fn publish_source(
        &self,
        revision: &SourceRevision,
        bytes: &[u8],
    ) -> Result<(), ProjectSessionError> {
        if SourceRevision::for_bytes(bytes) != *revision {
            return Err(ProjectSessionError::Invalid(
                "snapshot entry bytes disagree with their source revision".into(),
            ));
        }
        let path = self.source_path(revision);
        let _ = publish_immutable(&path, bytes)?;
        Ok(())
    }

    fn read_source(&self, revision: &SourceRevision) -> Result<Vec<u8>, ProjectSessionError> {
        let path = self.source_path(revision);
        let bytes =
            fs::read(&path).map_err(|error| ProjectSessionError::Io(path.clone(), error))?;
        if SourceRevision::for_bytes(&bytes) != *revision {
            return Err(ProjectSessionError::Corrupt(
                path,
                "source blob checksum does not match revision".into(),
            ));
        }
        Ok(bytes)
    }

    fn manifest_path(&self, id: &ProjectSessionId) -> PathBuf {
        self.root
            .join("sessions")
            .join(format!("{}.json", id.as_str()))
    }

    fn source_path(&self, revision: &SourceRevision) -> PathBuf {
        let hex = revision.as_str().strip_prefix("sr1_").unwrap_or("invalid");
        self.root
            .join("sources")
            .join(&hex[..hex.len().min(2)])
            .join(format!("{}.bin", revision.as_str()))
    }
}

/// Capture a snapshot in the shared cache namespace when configured. If a pinned
/// `DESLOP_SESSION_ID` is present, restore that exact snapshot and reject stale input.
pub fn capture_snapshot_from_environment(
    snapshot: Arc<ProjectSnapshot>,
    versions: CacheSemanticVersions,
) -> Result<(Arc<ProjectSnapshot>, Option<ProjectSessionId>), ProjectSessionError> {
    let Some(store) = ProjectSessionStore::from_environment()? else {
        return Ok((snapshot, None));
    };
    if let Some(raw_id) = std::env::var_os(SESSION_ID_ENV) {
        let raw_id = raw_id.into_string().map_err(|_| {
            ProjectSessionError::Invalid(format!("{SESSION_ID_ENV} must be valid Unicode"))
        })?;
        let id = ProjectSessionId::parse(raw_id)?;
        let restored = store.restore(&id)?;
        if restored.snapshot().id() != snapshot.id() {
            return Err(ProjectSessionError::Stale {
                expected_snapshot: snapshot.id().as_str().into(),
                restored_snapshot: restored.snapshot().id().as_str().into(),
            });
        }
        if restored.versions() != &versions {
            return Err(ProjectSessionError::Invalid(
                "pinned project session semantic versions do not match this consumer".into(),
            ));
        }
        return Ok((restored.into_snapshot(), Some(id)));
    }
    let capture = store.capture(&snapshot, versions)?;
    Ok((snapshot, Some(capture.id().clone())))
}

/// The single session version vector used by every first-party consumer.
pub fn project_session_semantic_versions(
    snapshot: &ProjectSnapshot,
) -> Result<CacheSemanticVersions, ProjectSessionError> {
    CacheSemanticVersions::for_snapshot(
        snapshot,
        PROJECT_GRAPH_VERSION,
        PROJECT_RECIPE_VERSION,
        PROJECT_MODEL_VERSION,
    )
    .map_err(|error| ProjectSessionError::Invalid(error.to_string()))
}

pub fn project_file_semantic_versions(
    snapshot: &ProjectSnapshot,
    path: &Path,
) -> Result<CacheSemanticVersions, ProjectSessionError> {
    CacheSemanticVersions::for_file(
        snapshot,
        path,
        PROJECT_GRAPH_VERSION,
        PROJECT_RECIPE_VERSION,
        PROJECT_MODEL_VERSION,
    )
    .map_err(|error| ProjectSessionError::Invalid(error.to_string()))
}

#[derive(Debug)]
pub enum ProjectSessionError {
    Invalid(String),
    Serialization(String),
    Io(PathBuf, io::Error),
    Corrupt(PathBuf, String),
    Conflict(PathBuf),
    Stale {
        expected_snapshot: String,
        restored_snapshot: String,
    },
}

impl fmt::Display for ProjectSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "invalid project session: {message}"),
            Self::Serialization(message) => {
                write!(formatter, "project session serialization failed: {message}")
            }
            Self::Io(path, error) => write!(
                formatter,
                "project session I/O failed at {}: {error}",
                path.display()
            ),
            Self::Corrupt(path, message) => write!(
                formatter,
                "corrupt project session {}: {message}",
                path.display()
            ),
            Self::Conflict(path) => write!(
                formatter,
                "immutable project session record conflicts at {}",
                path.display()
            ),
            Self::Stale {
                expected_snapshot,
                restored_snapshot,
            } => write!(
                formatter,
                "pinned project session is stale: input snapshot {expected_snapshot}, restored snapshot {restored_snapshot}"
            ),
        }
    }
}

impl std::error::Error for ProjectSessionError {}

fn validate_manifest(manifest: &ProjectSessionManifest) -> Result<(), ProjectSessionError> {
    if manifest.schema != PROJECT_SESSION_MANIFEST_SCHEMA {
        return Err(ProjectSessionError::Invalid(format!(
            "unsupported project session schema {}",
            manifest.schema
        )));
    }
    let expected = expected_session_id(&manifest.snapshot_id, &manifest.versions)?;
    if manifest.id != expected {
        return Err(ProjectSessionError::Invalid(
            "project session identity does not match snapshot and semantic versions".into(),
        ));
    }
    if manifest.root.trim().is_empty() || manifest.snapshot_id.trim().is_empty() {
        return Err(ProjectSessionError::Invalid(
            "project session root and snapshot identity must not be empty".into(),
        ));
    }
    if manifest
        .requested_scope
        .windows(2)
        .any(|window| window[0].path >= window[1].path)
        || manifest
            .entries
            .windows(2)
            .any(|window| window[0].path >= window[1].path)
    {
        return Err(ProjectSessionError::Invalid(
            "project session scope and entries must be sorted and unique".into(),
        ));
    }
    Ok(())
}

fn expected_session_id(
    snapshot_id: &str,
    versions: &CacheSemanticVersions,
) -> Result<ProjectSessionId, ProjectSessionError> {
    let bytes = serde_json::to_vec(&(PROJECT_SESSION_MANIFEST_SCHEMA, snapshot_id, versions))
        .map_err(|error| ProjectSessionError::Serialization(error.to_string()))?;
    let digest = blake3::derive_key(SESSION_ID_DOMAIN, &bytes);
    Ok(ProjectSessionId(format!(
        "pss1_{}",
        blake3::Hash::from_bytes(digest).to_hex()
    )))
}

fn publish_immutable(path: &Path, bytes: &[u8]) -> Result<bool, ProjectSessionError> {
    let parent = path
        .parent()
        .expect("project session paths always have parents");
    fs::create_dir_all(parent)
        .map_err(|error| ProjectSessionError::Io(parent.to_path_buf(), error))?;
    if path.exists() {
        let existing =
            fs::read(path).map_err(|error| ProjectSessionError::Io(path.to_path_buf(), error))?;
        if existing == bytes {
            return Ok(false);
        }
        return Err(ProjectSessionError::Conflict(path.to_path_buf()));
    }
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        std::process::id(),
        NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|error| ProjectSessionError::Io(temp.clone(), error))?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|error| ProjectSessionError::Io(temp.clone(), error))?;
        drop(file);
        match fs::hard_link(&temp, path) {
            Ok(()) => {
                fs::remove_file(&temp)
                    .map_err(|error| ProjectSessionError::Io(temp.clone(), error))?;
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                fs::remove_file(&temp)
                    .map_err(|remove| ProjectSessionError::Io(temp.clone(), remove))?;
                let existing = fs::read(path)
                    .map_err(|read| ProjectSessionError::Io(path.to_path_buf(), read))?;
                if existing == bytes {
                    Ok(false)
                } else {
                    Err(ProjectSessionError::Conflict(path.to_path_buf()))
                }
            }
            Err(error) => Err(ProjectSessionError::Io(path.to_path_buf(), error)),
        }
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn path_string(path: &Path, label: &str) -> Result<String, ProjectSessionError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| ProjectSessionError::Invalid(format!("{label} must be valid Unicode")))
}

fn is_session_id(value: &str) -> bool {
    value.strip_prefix("pss1_").is_some_and(|hex| {
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

    fn versions() -> CacheSemanticVersions {
        CacheSemanticVersions::new("adapters/3", "graphs/9", "recipes/7", "evidence-only").unwrap()
    }

    fn snapshot(root: &Path) -> Arc<ProjectSnapshot> {
        ProjectSnapshotBuilder::new(root, RepositoryId::explicit("session-test").unwrap())
            .unwrap()
            .with_scope_spec(crate::ScopeSpec::ExactLogicalFiles(vec![
                PathBuf::from("config.json"),
                PathBuf::from("src/lib.rs"),
            ]))
            .with_overlay("src/lib.rs", b"fn value() -> i32 { 1 }\n".to_vec())
            .unwrap()
            .with_analysis_input("config.json", b"{\"enabled\":true}\n".to_vec())
            .unwrap()
            .build()
            .unwrap()
    }

    #[test]
    fn session_round_trip_preserves_snapshot_identity_and_truthful_reparse() {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let store = ProjectSessionStore::open(cache.path()).unwrap();
        let original = snapshot(root.path());
        let first = store.capture(&original, versions()).unwrap();
        let second = store.capture(&original, versions()).unwrap();
        assert!(!first.was_reused());
        assert!(second.was_reused());
        assert_eq!(first.id(), second.id());

        let restored = store.restore(first.id()).unwrap();
        assert_eq!(restored.snapshot().id(), original.id());
        assert_eq!(restored.snapshot().repository(), original.repository());
        assert_eq!(
            restored.snapshot().requested_scope(),
            original.requested_scope()
        );
        assert!(restored.snapshot().read_counts().is_empty());

        let analysis = ProjectAnalysis::build(Arc::clone(restored.snapshot())).unwrap();
        assert_eq!(analysis.instrumentation().parse.parser_invocations, 1);
    }

    #[test]
    fn session_restore_rejects_tampered_source_blob() {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let store = ProjectSessionStore::open(cache.path()).unwrap();
        let original = snapshot(root.path());
        let capture = store.capture(&original, versions()).unwrap();
        let source = original.entry(Path::new("src/lib.rs")).unwrap();
        fs::write(store.source_path(source.revision()), b"tampered").unwrap();

        assert!(matches!(
            store.restore(capture.id()),
            Err(ProjectSessionError::Corrupt(_, _))
        ));
    }

    #[test]
    fn semantic_version_change_produces_distinct_session_identity() {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let store = ProjectSessionStore::open(cache.path()).unwrap();
        let original = snapshot(root.path());
        let first = store.capture(&original, versions()).unwrap();
        let changed = store
            .capture(
                &original,
                CacheSemanticVersions::new("adapters/4", "graphs/9", "recipes/7", "evidence-only")
                    .unwrap(),
            )
            .unwrap();
        assert_ne!(first.id(), changed.id());
    }
}
