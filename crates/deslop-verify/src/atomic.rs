use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const UNDO_MANIFEST_SCHEMA: &str = "deslop.undo-manifest/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UndoState {
    Prepared,
    Committing,
    Committed,
    RolledBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UndoFile {
    pub path: PathBuf,
    pub original_digest: String,
    pub replacement_digest: String,
    pub original_artifact: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UndoManifest {
    pub schema: String,
    pub id: String,
    pub transaction: String,
    pub state: UndoState,
    pub files: Vec<UndoFile>,
}

impl UndoManifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema != UNDO_MANIFEST_SCHEMA {
            bail!("unsupported undo-manifest schema `{}`", self.schema);
        }
        validate_identity("transaction identity", &self.transaction)?;
        let mut prior = None;
        for file in &self.files {
            validate_relative_path(&file.path)?;
            validate_relative_path(&file.original_artifact)?;
            validate_digest(&file.original_digest)?;
            validate_digest(&file.replacement_digest)?;
            if let Some(previous) = prior
                && previous >= file.path.as_path()
            {
                bail!("undo files must be strictly sorted and unique");
            }
            prior = Some(file.path.as_path());
        }
        if self.files.is_empty() {
            bail!("undo manifest requires at least one file");
        }
        if self.id != derive_manifest_id(&self.transaction, &self.files)? {
            bail!("undo-manifest identity is stale");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicFailurePoint {
    AfterUndoDurable,
    AfterTemporaryFiles,
    AfterRename(usize),
    AfterCommitMarker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicFailureMode {
    Error,
    Crash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtomicFailureInjection {
    pub point: AtomicFailurePoint,
    pub mode: AtomicFailureMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtomicCommitReceipt {
    pub transaction: String,
    pub manifest: PathBuf,
    pub written: Vec<PathBuf>,
    pub state: UndoState,
}

pub fn commit_atomic_sources(
    root: &Path,
    undo_root: &Path,
    expected: &BTreeMap<PathBuf, Vec<u8>>,
    replacements: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<AtomicCommitReceipt> {
    commit_atomic_sources_with_injection(root, undo_root, expected, replacements, None)
}

pub fn commit_atomic_sources_with_injection(
    root: &Path,
    undo_root: &Path,
    expected: &BTreeMap<PathBuf, Vec<u8>>,
    replacements: &BTreeMap<PathBuf, Vec<u8>>,
    injection: Option<AtomicFailureInjection>,
) -> Result<AtomicCommitReceipt> {
    validate_source_maps(expected, replacements)?;
    validate_relative_path(undo_root)?;
    let root = root
        .canonicalize()
        .with_context(|| format!("failed to resolve transaction root {}", root.display()))?;
    validate_live_sources(&root, expected)?;
    let transaction = derive_transaction_id(expected, replacements)?;
    let directory = root.join(undo_root).join(&transaction);
    fs::create_dir_all(directory.join("originals"))?;
    let mut files = Vec::new();
    for (index, (path, original)) in expected.iter().enumerate() {
        let artifact = PathBuf::from(format!("originals/{index}.bin"));
        write_durable_file(&directory.join(&artifact), original)?;
        files.push(UndoFile {
            path: path.clone(),
            original_digest: digest_bytes(original),
            replacement_digest: digest_bytes(&replacements[path]),
            original_artifact: artifact,
        });
    }
    let mut manifest = UndoManifest {
        schema: UNDO_MANIFEST_SCHEMA.into(),
        id: derive_manifest_id(&transaction, &files)?,
        transaction: transaction.clone(),
        state: UndoState::Prepared,
        files,
    };
    let manifest_path = directory.join("manifest.json");
    write_manifest(&manifest_path, &manifest)?;
    fsync_directory(&directory)?;
    maybe_inject(injection, AtomicFailurePoint::AfterUndoDurable)?;

    let mut temporary = Vec::new();
    for (index, (path, replacement)) in replacements.iter().enumerate() {
        let live = live_path(&root, path)?;
        let temp = live.with_extension(format!(
            "{}deslop-tx-{index}.tmp",
            live.extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| format!("{extension}."))
                .unwrap_or_default()
        ));
        write_durable_file(&temp, replacement)?;
        if let Ok(metadata) = fs::metadata(&live) {
            fs::set_permissions(&temp, metadata.permissions())?;
        }
        temporary.push((path.clone(), temp));
    }
    maybe_inject(injection, AtomicFailurePoint::AfterTemporaryFiles)?;
    manifest.state = UndoState::Committing;
    write_manifest(&manifest_path, &manifest)?;

    for (index, (path, temp)) in temporary.iter().enumerate() {
        let live = live_path(&root, path)?;
        if let Err(error) = fs::rename(temp, &live) {
            rollback_from_manifest(&root, &directory, &mut manifest, &manifest_path)?;
            return Err(error).with_context(|| format!("failed to replace {}", path.display()));
        }
        fsync_parent(&live)?;
        if let Err(error) = maybe_inject(injection, AtomicFailurePoint::AfterRename(index)) {
            if injection.is_some_and(|injection| injection.mode == AtomicFailureMode::Crash) {
                return Err(error);
            }
            rollback_from_manifest(&root, &directory, &mut manifest, &manifest_path)?;
            return Err(error);
        }
    }
    manifest.state = UndoState::Committed;
    write_manifest(&manifest_path, &manifest)?;
    fsync_directory(&directory)?;
    maybe_inject(injection, AtomicFailurePoint::AfterCommitMarker)?;
    validate_replacements(&root, replacements)?;
    Ok(AtomicCommitReceipt {
        transaction,
        manifest: manifest_path,
        written: replacements.keys().cloned().collect(),
        state: UndoState::Committed,
    })
}

pub fn recover_incomplete_transactions(root: &Path, undo_root: &Path) -> Result<Vec<String>> {
    let root = root
        .canonicalize()
        .with_context(|| format!("failed to resolve recovery root {}", root.display()))?;
    validate_relative_path(undo_root)?;
    let directory = root.join(undo_root);
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut recovered = Vec::new();
    let mut entries = fs::read_dir(&directory)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let transaction_dir = entry.path();
        let manifest_path = transaction_dir.join("manifest.json");
        if !manifest_path.is_file() {
            continue;
        }
        let mut manifest: UndoManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        manifest.validate()?;
        if matches!(manifest.state, UndoState::Prepared | UndoState::Committing) {
            rollback_from_manifest(&root, &transaction_dir, &mut manifest, &manifest_path)?;
            recovered.push(manifest.transaction);
        }
    }
    Ok(recovered)
}

pub fn restore_committed_transaction(root: &Path, manifest_path: &Path) -> Result<()> {
    let root = root.canonicalize()?;
    let directory = manifest_path
        .parent()
        .context("undo manifest has no parent directory")?;
    let mut manifest: UndoManifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
    manifest.validate()?;
    if manifest.state != UndoState::Committed {
        bail!("only a committed transaction can be explicitly undone");
    }
    for file in &manifest.files {
        let current = fs::read(live_path(&root, &file.path)?)?;
        if digest_bytes(&current) != file.replacement_digest {
            bail!(
                "cannot undo `{}` after subsequent source drift",
                file.path.display()
            );
        }
    }
    rollback_from_manifest(&root, directory, &mut manifest, manifest_path)
}

fn rollback_from_manifest(
    root: &Path,
    directory: &Path,
    manifest: &mut UndoManifest,
    manifest_path: &Path,
) -> Result<()> {
    for file in &manifest.files {
        let original = fs::read(directory.join(&file.original_artifact))?;
        if digest_bytes(&original) != file.original_digest {
            bail!("undo artifact for `{}` is corrupted", file.path.display());
        }
        let live = live_path(root, &file.path)?;
        let temp = live.with_extension(format!(
            "{}deslop-rollback.tmp",
            live.extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| format!("{extension}."))
                .unwrap_or_default()
        ));
        write_durable_file(&temp, &original)?;
        fs::rename(&temp, &live)?;
        fsync_parent(&live)?;
    }
    manifest.state = UndoState::RolledBack;
    write_manifest(manifest_path, manifest)?;
    fsync_directory(directory)
}

fn validate_source_maps(
    expected: &BTreeMap<PathBuf, Vec<u8>>,
    replacements: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<()> {
    if expected.is_empty() || expected.keys().ne(replacements.keys()) {
        bail!("atomic source transaction requires identical nonempty expected/replacement paths");
    }
    for path in expected.keys() {
        validate_relative_path(path)?;
    }
    Ok(())
}

fn validate_live_sources(root: &Path, expected: &BTreeMap<PathBuf, Vec<u8>>) -> Result<()> {
    for (path, bytes) in expected {
        if fs::read(live_path(root, path)?).ok().as_deref() != Some(bytes.as_slice()) {
            bail!(
                "stale exact bytes for `{}` before atomic commit",
                path.display()
            );
        }
    }
    Ok(())
}

fn validate_replacements(root: &Path, replacements: &BTreeMap<PathBuf, Vec<u8>>) -> Result<()> {
    for (path, bytes) in replacements {
        if fs::read(live_path(root, path)?).ok().as_deref() != Some(bytes.as_slice()) {
            bail!(
                "atomic commit did not retain exact replacement for `{}`",
                path.display()
            );
        }
    }
    Ok(())
}

fn write_durable_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn write_manifest(path: &Path, manifest: &UndoManifest) -> Result<()> {
    manifest.validate()?;
    let temp = path.with_extension("json.tmp");
    write_durable_file(&temp, &serde_json::to_vec_pretty(manifest)?)?;
    fs::rename(&temp, path)?;
    fsync_parent(path)
}

fn fsync_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fsync_directory(parent)?;
    }
    Ok(())
}

fn fsync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn maybe_inject(
    injection: Option<AtomicFailureInjection>,
    point: AtomicFailurePoint,
) -> Result<()> {
    if injection.is_some_and(|injection| injection.point == point) {
        bail!("injected {:?} at {point:?}", injection.unwrap().mode);
    }
    Ok(())
}

fn derive_transaction_id(
    expected: &BTreeMap<PathBuf, Vec<u8>>,
    replacements: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<String> {
    #[derive(Serialize)]
    struct Entry<'a> {
        path: &'a Path,
        original: String,
        replacement: String,
    }
    let entries = expected
        .iter()
        .map(|(path, original)| Entry {
            path,
            original: digest_bytes(original),
            replacement: digest_bytes(&replacements[path]),
        })
        .collect::<Vec<_>>();
    digest_json("deslop atomic source transaction v1", &entries, "tx1_")
}

fn derive_manifest_id(transaction: &str, files: &[UndoFile]) -> Result<String> {
    #[derive(Serialize)]
    struct Identity<'a> {
        transaction: &'a str,
        files: &'a [UndoFile],
    }
    digest_json(
        "deslop undo manifest v1",
        &Identity { transaction, files },
        "um1_",
    )
}

fn digest_json(domain: &str, value: &impl Serialize, prefix: &str) -> Result<String> {
    let payload = serde_json::to_vec(value)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain.as_bytes());
    hasher.update(&[0]);
    hasher.update(&payload);
    Ok(format!("{prefix}{}", hasher.finalize().to_hex()))
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"deslop source bytes v1\0");
    hasher.update(bytes);
    format!("sb1_{}", hasher.finalize().to_hex())
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!(
            "transaction path `{}` must stay relative to its root",
            path.display()
        );
    }
    Ok(())
}

fn live_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    validate_relative_path(relative)?;
    let live = root.join(relative);
    let canonical = live
        .canonicalize()
        .with_context(|| format!("failed to resolve transaction path {}", live.display()))?;
    if canonical.strip_prefix(root).is_err() {
        bail!(
            "transaction path `{}` escapes its root through a symlink",
            relative.display()
        );
    }
    Ok(canonical)
}

fn validate_identity(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        bail!("{label} is invalid");
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<()> {
    let valid = value.strip_prefix("sb1_").is_some_and(|suffix| {
        suffix.len() == 64 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    if !valid {
        bail!("invalid source-byte digest `{value}`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    type SourceMap = BTreeMap<PathBuf, Vec<u8>>;

    fn fixture() -> (TempDir, SourceMap, SourceMap) {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("a.rs"), b"fn a() {}\n").unwrap();
        fs::write(root.path().join("b.rs"), b"fn b() {}\n").unwrap();
        let expected = [
            (PathBuf::from("a.rs"), b"fn a() {}\n".to_vec()),
            (PathBuf::from("b.rs"), b"fn b() {}\n".to_vec()),
        ]
        .into_iter()
        .collect();
        let replacements = [
            (PathBuf::from("a.rs"), b"fn a() { work(); }\n".to_vec()),
            (PathBuf::from("b.rs"), b"fn b() { work(); }\n".to_vec()),
        ]
        .into_iter()
        .collect();
        (root, expected, replacements)
    }

    #[test]
    fn commit_is_durable_and_explicit_undo_restores_exact_bytes() {
        let (root, expected, replacements) = fixture();
        let receipt = commit_atomic_sources(
            root.path(),
            Path::new(".deslop/undo"),
            &expected,
            &replacements,
        )
        .unwrap();
        assert_eq!(receipt.state, UndoState::Committed);
        assert_eq!(
            fs::read(root.path().join("a.rs")).unwrap(),
            replacements[&PathBuf::from("a.rs")]
        );
        restore_committed_transaction(root.path(), &receipt.manifest).unwrap();
        assert_eq!(
            fs::read(root.path().join("a.rs")).unwrap(),
            expected[&PathBuf::from("a.rs")]
        );
        let manifest: UndoManifest =
            serde_json::from_slice(&fs::read(receipt.manifest).unwrap()).unwrap();
        assert_eq!(manifest.state, UndoState::RolledBack);
    }

    #[test]
    fn every_partial_rename_error_rolls_back_all_files() {
        for index in 0..2 {
            let (root, expected, replacements) = fixture();
            let result = commit_atomic_sources_with_injection(
                root.path(),
                Path::new(".deslop/undo"),
                &expected,
                &replacements,
                Some(AtomicFailureInjection {
                    point: AtomicFailurePoint::AfterRename(index),
                    mode: AtomicFailureMode::Error,
                }),
            );
            assert!(result.is_err());
            for (path, bytes) in &expected {
                assert_eq!(&fs::read(root.path().join(path)).unwrap(), bytes);
            }
        }
    }

    #[test]
    fn simulated_crash_is_recovered_from_durable_manifest() {
        let (root, expected, replacements) = fixture();
        let result = commit_atomic_sources_with_injection(
            root.path(),
            Path::new(".deslop/undo"),
            &expected,
            &replacements,
            Some(AtomicFailureInjection {
                point: AtomicFailurePoint::AfterRename(0),
                mode: AtomicFailureMode::Crash,
            }),
        );
        assert!(result.is_err());
        assert_eq!(
            fs::read(root.path().join("a.rs")).unwrap(),
            replacements[&PathBuf::from("a.rs")]
        );
        let recovered =
            recover_incomplete_transactions(root.path(), Path::new(".deslop/undo")).unwrap();
        assert_eq!(recovered.len(), 1);
        for (path, bytes) in &expected {
            assert_eq!(&fs::read(root.path().join(path)).unwrap(), bytes);
        }
    }

    #[test]
    fn stale_source_and_corrupt_undo_refuse_commit_or_restore() {
        let (root, expected, replacements) = fixture();
        fs::write(root.path().join("a.rs"), b"drift\n").unwrap();
        assert!(
            commit_atomic_sources(
                root.path(),
                Path::new(".deslop/undo"),
                &expected,
                &replacements
            )
            .is_err()
        );

        fs::write(root.path().join("a.rs"), &expected[&PathBuf::from("a.rs")]).unwrap();
        let receipt = commit_atomic_sources(
            root.path(),
            Path::new(".deslop/undo"),
            &expected,
            &replacements,
        )
        .unwrap();
        let manifest: UndoManifest =
            serde_json::from_slice(&fs::read(&receipt.manifest).unwrap()).unwrap();
        let directory = receipt.manifest.parent().unwrap();
        fs::write(
            directory.join(&manifest.files[0].original_artifact),
            b"corrupt",
        )
        .unwrap();
        assert!(restore_committed_transaction(root.path(), &receipt.manifest).is_err());
    }
}
