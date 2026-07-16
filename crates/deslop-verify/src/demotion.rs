use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{VerifierFailureKind, VerifierStage};

pub const RECIPE_DEMOTION_SCHEMA: &str = "deslop.recipe-demotion/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeDemotionRecord {
    pub schema: String,
    pub id: String,
    pub recipe: String,
    pub candidate: String,
    pub snapshot: String,
    pub stage: VerifierStage,
    pub failure: VerifierFailureKind,
    pub counterexample: String,
    pub detail: String,
}

impl RecipeDemotionRecord {
    pub fn counterexample(
        recipe: impl Into<String>,
        candidate: impl Into<String>,
        snapshot: impl Into<String>,
        stage: VerifierStage,
        failure: VerifierFailureKind,
        counterexample_bytes: &[u8],
        detail: impl Into<String>,
    ) -> Result<Self> {
        let mut record = Self {
            schema: RECIPE_DEMOTION_SCHEMA.into(),
            id: String::new(),
            recipe: recipe.into(),
            candidate: candidate.into(),
            snapshot: snapshot.into(),
            stage,
            failure,
            counterexample: digest_bytes(counterexample_bytes),
            detail: detail.into(),
        };
        record.id = derive_id(&record)?;
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != RECIPE_DEMOTION_SCHEMA {
            bail!("unsupported recipe-demotion schema `{}`", self.schema);
        }
        for (label, value) in [
            ("demotion recipe", self.recipe.as_str()),
            ("demotion candidate", self.candidate.as_str()),
            ("demotion snapshot", self.snapshot.as_str()),
            ("demotion detail", self.detail.as_str()),
        ] {
            validate_text(label, value)?;
        }
        if !valid_digest(&self.counterexample, "ce1_") {
            bail!("invalid counterexample digest");
        }
        if self.id != derive_id(self)? {
            bail!("recipe-demotion identity is stale");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemotionSupersession {
    pub record: String,
    pub authority: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct RecipeDemotionStore {
    records: BTreeMap<String, RecipeDemotionRecord>,
    superseded: BTreeMap<String, DemotionSupersession>,
}

impl RecipeDemotionStore {
    pub fn load(root: &Path, relative: &Path) -> Result<Self> {
        let path = journal_path(root, relative)?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut store = Self::default();
        for (index, line) in fs::read_to_string(&path)?.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(line)
                .with_context(|| format!("invalid demotion journal line {}", index + 1))?;
            match value.get("kind").and_then(serde_json::Value::as_str) {
                Some("demotion") => {
                    let record: RecipeDemotionRecord = serde_json::from_value(
                        value
                            .get("record")
                            .cloned()
                            .context("demotion line lacks record")?,
                    )?;
                    record.validate()?;
                    if store.records.insert(record.id.clone(), record).is_some() {
                        bail!("duplicate demotion record at line {}", index + 1);
                    }
                }
                Some("supersession") => {
                    let supersession: DemotionSupersession = serde_json::from_value(
                        value
                            .get("supersession")
                            .cloned()
                            .context("supersession line lacks payload")?,
                    )?;
                    validate_text("supersession record", &supersession.record)?;
                    validate_text("supersession authority", &supersession.authority)?;
                    validate_text("supersession reason", &supersession.reason)?;
                    if store
                        .superseded
                        .insert(supersession.record.clone(), supersession)
                        .is_some()
                    {
                        bail!("duplicate demotion supersession at line {}", index + 1);
                    }
                }
                _ => bail!("demotion journal line {} has unknown kind", index + 1),
            }
        }
        for record in store.superseded.keys() {
            if !store.records.contains_key(record) {
                bail!("demotion journal supersedes unknown record `{record}`");
            }
        }
        Ok(store)
    }

    pub fn append(root: &Path, relative: &Path, record: RecipeDemotionRecord) -> Result<Self> {
        record.validate()?;
        let path = journal_path(root, relative)?;
        append_durable(
            &path,
            &serde_json::json!({"kind":"demotion", "record":record}),
        )?;
        Self::load(root, relative)
    }

    pub fn supersede(
        root: &Path,
        relative: &Path,
        record: &str,
        authority: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<Self> {
        let store = Self::load(root, relative)?;
        if !store.records.contains_key(record) {
            bail!("cannot supersede unknown demotion `{record}`");
        }
        let supersession = DemotionSupersession {
            record: record.into(),
            authority: authority.into(),
            reason: reason.into(),
        };
        validate_text("supersession authority", &supersession.authority)?;
        validate_text("supersession reason", &supersession.reason)?;
        let path = journal_path(root, relative)?;
        append_durable(
            &path,
            &serde_json::json!({"kind":"supersession", "supersession":supersession}),
        )?;
        Self::load(root, relative)
    }

    pub fn active_for_recipe(&self, recipe: &str) -> Vec<&RecipeDemotionRecord> {
        self.records
            .values()
            .filter(|record| {
                record.recipe == recipe && !self.superseded.contains_key(record.id.as_str())
            })
            .collect()
    }

    pub fn is_demoted(&self, recipe: &str) -> bool {
        !self.active_for_recipe(recipe).is_empty()
    }
}

fn journal_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    validate_relative_path(relative)?;
    let root = root.canonicalize()?;
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        let canonical_parent = parent.canonicalize()?;
        if canonical_parent.strip_prefix(&root).is_err() {
            bail!("demotion journal escapes project root through a symlink");
        }
    }
    Ok(path)
}

fn append_durable(path: &Path, value: &serde_json::Value) -> Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn derive_id(record: &RecipeDemotionRecord) -> Result<String> {
    #[derive(Serialize)]
    struct Identity<'a> {
        schema: &'a str,
        recipe: &'a str,
        candidate: &'a str,
        snapshot: &'a str,
        stage: VerifierStage,
        failure: VerifierFailureKind,
        counterexample: &'a str,
        detail: &'a str,
    }
    let payload = serde_json::to_vec(&Identity {
        schema: &record.schema,
        recipe: &record.recipe,
        candidate: &record.candidate,
        snapshot: &record.snapshot,
        stage: record.stage,
        failure: record.failure,
        counterexample: &record.counterexample,
        detail: &record.detail,
    })?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"deslop recipe demotion v1\0");
    hasher.update(&payload);
    Ok(format!("rd1_{}", hasher.finalize().to_hex()))
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"deslop counterexample v1\0");
    hasher.update(bytes);
    format!("ce1_{}", hasher.finalize().to_hex())
}

fn valid_digest(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == 64 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
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
        bail!("demotion journal path must stay relative to the project root");
    }
    Ok(())
}

fn validate_text(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 16_384 || value.chars().any(char::is_control) {
        bail!("{label} must be nonempty bounded printable text");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn counterexample_immediately_demotes_until_explicit_supersession() {
        let root = TempDir::new().unwrap();
        let relative = Path::new("negative/recipes.jsonl");
        let record = RecipeDemotionRecord::counterexample(
            "recipe-1",
            "candidate-1",
            "ps1_snapshot",
            VerifierStage::GraphDelta,
            VerifierFailureKind::GraphDeltaMismatch,
            b"counterexample source",
            "expected and actual graph delta differ",
        )
        .unwrap();
        let store = RecipeDemotionStore::append(root.path(), relative, record.clone()).unwrap();
        assert!(store.is_demoted("recipe-1"));
        let store = RecipeDemotionStore::supersede(
            root.path(),
            relative,
            &record.id,
            "review-board",
            "counterexample incorporated in recipe version 2",
        )
        .unwrap();
        assert!(!store.is_demoted("recipe-1"));
    }

    #[test]
    fn corrupted_record_identity_and_unknown_supersession_fail_closed() {
        let mut record = RecipeDemotionRecord::counterexample(
            "recipe-1",
            "candidate-1",
            "ps1_snapshot",
            VerifierStage::Command,
            VerifierFailureKind::Counterexample,
            b"source",
            "differential mismatch",
        )
        .unwrap();
        record.detail = "tampered".into();
        assert!(record.validate().is_err());

        let root = TempDir::new().unwrap();
        assert!(
            RecipeDemotionStore::supersede(
                root.path(),
                Path::new("journal.jsonl"),
                "rd1_unknown",
                "reviewer",
                "reason"
            )
            .is_err()
        );
    }
}
