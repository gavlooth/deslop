//! Experimental, vendor-neutral trajectory evidence and bounded minimization.
//!
//! This module deliberately treats an agent export as untrusted data.  It never
//! executes commands found in an export and it does not infer agent intent.
//! `OpenCode` is supported through its documented `opencode export` JSON shape
//! (`{info, messages}` with public message parts); the adapter consumes only
//! edit/write tool inputs and excludes reasoning text.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};
use deslop_protocol::Patch;
use deslop_verify::{ApplyReport, VerifyOptions, VerifyReport, apply_patches, verify_patches};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::TempDir;

#[cfg(test)]
use deslop_verify::{CoverageConfig, MutationConfig};
pub const TRAJECTORY_SCHEMA: &str = "deslop.trajectory/1";
pub const NEUTRAL_TRAJECTORY_FORMAT: &str = "deslop.trajectory-neutral/1";
pub const OPENCODE_EXPORT_FORMAT: &str = "opencode.export/{info,messages}";
pub const TRAJECTORY_INTEGRITY_ALGORITHM: &str = "blake3";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionIdentity {
    pub revision: String,
    pub tree_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub identity: RevisionIdentity,
    /// Repository-relative UTF-8 file contents.  Keeping these bytes in the
    /// evidence object makes replay deterministic and avoids filesystem reads.
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Blob {
    pub hash: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EditKind {
    Modify,
    Create,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditEvent {
    pub id: String,
    /// Zero-based order in the public export, retained exactly during replay.
    pub ordinal: u64,
    pub group: String,
    pub path: String,
    pub kind: EditKind,
    pub before: Option<Blob>,
    pub after: Option<Blob>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckObservationStatus {
    Passed,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckObservation {
    pub id: String,
    pub snapshot_hash: String,
    pub name: String,
    pub status: CheckObservationStatus,
    /// Always false for imported observations. Trusted status is earned only
    /// by rerunning the selected check under a server-owned policy.
    pub trusted: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MissingHistoryKind {
    Unavailable,
    Redacted,
    Unsupported,
    Contradictory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissingHistory {
    pub kind: MissingHistoryKind,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicenseGate {
    pub approved: bool,
    pub spdx: Option<String>,
    pub source_uri: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrityGate {
    pub algorithm: String,
    pub source_digest: String,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trajectory {
    pub schema: String,
    pub format: String,
    pub base: Snapshot,
    pub final_snapshot: Snapshot,
    pub events: Vec<EditEvent>,
    pub observations: Vec<CheckObservation>,
    pub missing_history: Vec<MissingHistory>,
    pub license: LicenseGate,
    pub integrity: IntegrityGate,
    /// Paths that are never eligible for a size/reversion objective.  This
    /// includes tests, checks, error handling, and public contract files.
    pub protected_paths: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReplayStatus {
    Complete,
    Partial,
    Contradictory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayEvidence {
    pub status: ReplayStatus,
    pub files: BTreeMap<String, String>,
    pub applied_events: Vec<String>,
    pub issues: Vec<String>,
    pub final_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MinimizationBudget {
    pub max_candidates: usize,
    pub max_events: usize,
    pub max_groups: usize,
    pub max_validation_calls: usize,
}

impl Default for MinimizationBudget {
    fn default() -> Self {
        Self {
            max_candidates: 64,
            max_events: 512,
            max_groups: 128,
            max_validation_calls: 64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub id: String,
    pub removed_groups: Vec<String>,
    pub files: BTreeMap<String, String>,
    pub state_hash: String,
    pub cache_identity: String,
    pub protected_paths: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckPolicy {
    pub schema: String,
    pub policy_id: String,
    pub selected_checks: Vec<String>,
    pub protected_paths: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateCache {
    pub schema: String,
    pub policy: CheckPolicy,
    pub entries: BTreeMap<String, CandidateCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateCheckStatus {
    TrustedPassed,
    TrustedFailed,
    NotRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateCheck {
    pub candidate_hash: String,
    pub policy_identity: String,
    pub status: CandidateCheckStatus,
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CandidateApplyReport {
    pub candidate_hash: String,
    pub verified: VerifyReport,
    pub applied: ApplyReport,
}

fn digest(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

fn canonical_tree(files: &BTreeMap<String, String>) -> Vec<u8> {
    let mut out = Vec::new();
    for (path, content) in files {
        out.extend_from_slice(path.as_bytes());
        out.push(0);
        out.extend_from_slice(content.as_bytes());
        out.push(0);
    }
    out
}

pub fn snapshot_hash(files: &BTreeMap<String, String>) -> String {
    digest(&canonical_tree(files))
}

fn patch_cost(base: &BTreeMap<String, String>, candidate: &BTreeMap<String, String>) -> usize {
    let paths = base.keys().chain(candidate.keys()).collect::<BTreeSet<_>>();
    paths
        .into_iter()
        .map(|path| {
            let old = base.get(path).map(String::as_str).unwrap_or("");
            let new = candidate.get(path).map(String::as_str).unwrap_or("");
            if old == new {
                return 0;
            }
            let old_lines = old.lines().count();
            let new_lines = new.lines().count();
            old_lines.min(new_lines).saturating_sub(
                old.lines()
                    .zip(new.lines())
                    .filter(|(left, right)| left == right)
                    .count(),
            ) + old_lines.abs_diff(new_lines)
        })
        .sum()
}

pub fn blob(content: impl Into<String>) -> Blob {
    let content = content.into();
    Blob {
        hash: digest(content.as_bytes()),
        content,
    }
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("blake3:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

fn repo_path(raw: &str) -> Result<String> {
    if raw.is_empty() || raw.contains('\0') {
        bail!("trajectory path is empty or contains NUL");
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        bail!("trajectory path `{raw}` is absolute");
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let text = part.to_str().context("trajectory path is not UTF-8")?;
                if text.is_empty() {
                    bail!("trajectory path contains empty component")
                }
                if ignored_metadata(part) {
                    bail!("trajectory path `{raw}` is reserved metadata, not supported source");
                }
                parts.push(text.to_owned());
            }
            Component::CurDir => {}
            Component::ParentDir => bail!("trajectory path `{raw}` escapes repository root"),
            Component::RootDir | Component::Prefix(_) => bail!("trajectory path `{raw}` is rooted"),
        }
    }
    if parts.is_empty() {
        bail!("trajectory path `{raw}` has no normal components")
    }
    Ok(parts.join("/"))
}

impl Snapshot {
    pub fn new(revision: impl Into<String>, files: BTreeMap<String, String>) -> Result<Self> {
        let files = normalize_files(files)?;
        Ok(Self {
            identity: RevisionIdentity {
                revision: revision.into(),
                tree_hash: snapshot_hash(&files),
            },
            files,
        })
    }

    pub fn validate(&self, label: &str) -> Result<()> {
        if self.identity.revision.trim().is_empty() {
            bail!("{label} revision is empty")
        }
        let normalized = normalize_files(self.files.clone())?;
        if normalized != self.files {
            bail!("{label} paths are not normalized")
        }
        let expected = snapshot_hash(&self.files);
        if self.identity.tree_hash != expected {
            bail!("{label} tree hash mismatch: expected {expected}")
        }
        Ok(())
    }
}

fn normalize_files(files: BTreeMap<String, String>) -> Result<BTreeMap<String, String>> {
    let mut normalized = BTreeMap::new();
    for (path, content) in files {
        let path = repo_path(&path)?;
        if normalized.insert(path.clone(), content).is_some() {
            bail!("duplicate normalized path `{path}`")
        }
    }
    Ok(normalized)
}

impl Trajectory {
    pub fn validate(&self) -> Result<()> {
        if self.schema != TRAJECTORY_SCHEMA {
            bail!("unsupported trajectory schema `{}`", self.schema)
        }
        if self.format != NEUTRAL_TRAJECTORY_FORMAT && self.format != OPENCODE_EXPORT_FORMAT {
            bail!("unsupported trajectory source format `{}`", self.format)
        }
        self.base.validate("base snapshot")?;
        self.final_snapshot.validate("final snapshot")?;
        if self.base.identity.revision == self.final_snapshot.identity.revision
            && self.base.files != self.final_snapshot.files
        {
            bail!("base and final share a revision identity but have different content")
        }
        if !self.license.approved {
            bail!("trajectory license gate is not approved")
        }
        if self.license.spdx.as_deref().is_none_or(str::is_empty) {
            bail!("trajectory license gate lacks SPDX identifier")
        }
        if self.integrity.algorithm != TRAJECTORY_INTEGRITY_ALGORITHM
            || !valid_digest(&self.integrity.source_digest)
            || !self.integrity.verified
        {
            bail!("trajectory integrity gate is missing, unsupported, or unverified")
        }
        let mut ids = BTreeSet::new();
        for path in self.protected_paths.iter() {
            let normalized = repo_path(path)?;
            if normalized != *path {
                bail!("protected path `{path}` is not normalized")
            }
        }
        for (index, event) in self.events.iter().enumerate() {
            if event.ordinal != index as u64 {
                bail!(
                    "event {} has ordinal {}, expected {}",
                    index,
                    event.ordinal,
                    index
                )
            }
            if event.id.trim().is_empty() || !ids.insert(event.id.clone()) {
                bail!("duplicate or empty event id")
            }
            let normalized = repo_path(&event.path)?;
            if normalized != event.path {
                bail!("event path `{}` is not normalized", event.path)
            }
            if event.group.trim().is_empty() {
                bail!("event {} has empty dependency group", index)
            }
            validate_blob(event.before.as_ref(), &format!("event {index} before"))?;
            validate_blob(event.after.as_ref(), &format!("event {index} after"))?;
            match event.kind {
                EditKind::Modify if event.before.is_none() || event.after.is_none() => {
                    bail!("modify event {} needs before and after", index)
                }
                EditKind::Create if event.before.is_some() || event.after.is_none() => {
                    bail!("create event {} needs only after", index)
                }
                EditKind::Delete if event.before.is_none() || event.after.is_some() => {
                    bail!("delete event {} needs only before", index)
                }
                _ => {}
            }
        }
        let mut observation_ids = BTreeSet::new();
        let known_snapshot_hashes = BTreeSet::from([
            self.base.identity.tree_hash.clone(),
            self.final_snapshot.identity.tree_hash.clone(),
        ]);
        for observation in &self.observations {
            if observation.id.trim().is_empty() || !observation_ids.insert(observation.id.clone()) {
                bail!("duplicate or empty check observation id")
            }
            if !valid_digest(&observation.snapshot_hash) {
                bail!("observation `{}` has invalid snapshot hash", observation.id)
            }
            if !known_snapshot_hashes.contains(&observation.snapshot_hash) {
                bail!(
                    "observation `{}` is not bound to the declared base or final snapshot",
                    observation.id
                )
            }
            if observation.trusted {
                bail!(
                    "imported observation `{}` cannot claim trusted status",
                    observation.id
                )
            }
            if observation.name.trim().is_empty() {
                bail!("observation `{}` has empty check name", observation.id)
            }
        }
        for gap in &self.missing_history {
            if gap.detail.trim().is_empty() {
                bail!("missing-history entry has empty detail")
            }
        }
        Ok(())
    }
}

fn validate_blob(blob: Option<&Blob>, label: &str) -> Result<()> {
    if let Some(blob) = blob {
        let expected = digest(blob.content.as_bytes());
        if blob.hash != expected {
            bail!("{label} hash mismatch: expected {expected}")
        }
    }
    Ok(())
}
pub fn import_trajectory_with_source(bytes: &[u8], source_bytes: &[u8]) -> Result<Trajectory> {
    let trajectory = import_trajectory(bytes)?;
    if trajectory.integrity.source_digest != digest(source_bytes) {
        bail!("trajectory source integrity digest mismatch")
    }
    Ok(trajectory)
}

/// Strict neutral import rejects incomplete or contradictory histories.
pub fn import_trajectory(bytes: &[u8]) -> Result<Trajectory> {
    let trajectory = import_trajectory_partial(bytes)?;
    replay_trajectory(&trajectory)?;
    Ok(trajectory)
}

/// Structural import retained for review tooling that needs to expose partial
/// evidence; callers must inspect `replay_evidence` before treating it as a
/// replayed state.
pub fn import_trajectory_partial(bytes: &[u8]) -> Result<Trajectory> {
    let trajectory: Trajectory = serde_json::from_slice(bytes).context("parse trajectory JSON")?;
    trajectory.validate()?;
    Ok(trajectory)
}
fn apply_events(trajectory: &Trajectory, excluded: &BTreeSet<String>) -> ReplayEvidence {
    let mut files = trajectory.base.files.clone();
    let mut applied = Vec::new();
    let mut issues = Vec::new();
    for event in &trajectory.events {
        if excluded.contains(&event.group) {
            continue;
        }
        let current = files.get(&event.path).map(|s| s.as_str());
        let current_hash = current.map(|s| digest(s.as_bytes()));
        let expected = event.before.as_ref().map(|b| b.hash.as_str());
        if current_hash.as_deref() != expected {
            issues.push(format!(
                "event `{}` contradicts current `{}` content",
                event.id, event.path
            ));
            continue;
        }
        match &event.after {
            Some(after) => {
                files.insert(event.path.clone(), after.content.clone());
            }
            None => {
                files.remove(&event.path);
            }
        }
        applied.push(event.id.clone());
    }
    let final_hash = snapshot_hash(&files);
    let status = if !issues.is_empty() {
        ReplayStatus::Contradictory
    } else if !trajectory.missing_history.is_empty()
        || final_hash != trajectory.final_snapshot.identity.tree_hash
    {
        ReplayStatus::Partial
    } else {
        ReplayStatus::Complete
    };
    if final_hash != trajectory.final_snapshot.identity.tree_hash {
        issues.push(format!(
            "replayed final hash {final_hash} does not match submitted {}",
            trajectory.final_snapshot.identity.tree_hash
        ));
    }
    ReplayEvidence {
        status,
        files,
        applied_events: applied,
        issues,
        final_hash,
    }
}

/// Strict replay: incomplete or contradictory history is an explicit failure.
pub fn replay_trajectory(trajectory: &Trajectory) -> Result<ReplayEvidence> {
    trajectory.validate()?;
    let evidence = apply_events(trajectory, &BTreeSet::new());
    match evidence.status {
        ReplayStatus::Complete => Ok(evidence),
        ReplayStatus::Partial => {
            let detail = if evidence.issues.is_empty() {
                "missing history declared".to_string()
            } else {
                evidence.issues.join("; ")
            };
            bail!("trajectory replay incomplete: {detail}")
        }
        ReplayStatus::Contradictory => bail!(
            "trajectory replay contradictory: {}",
            evidence.issues.join("; ")
        ),
    }
}

pub fn check_policy_identity(policy: &CheckPolicy) -> Result<String> {
    if policy.schema != "deslop.trajectory-check-policy/1" {
        bail!("unsupported check policy schema `{}`", policy.schema)
    }
    if policy.policy_id.trim().is_empty() || policy.selected_checks.is_empty() {
        bail!("check policy needs id and selected checks")
    }
    if policy.selected_checks.iter().any(|c| c.trim().is_empty()) {
        bail!("check policy contains empty check")
    }
    for path in &policy.protected_paths {
        if repo_path(path)? != *path {
            bail!("check policy protected path `{path}` is not normalized")
        }
    }
    let bytes = serde_json::to_vec(policy)?;
    Ok(digest(&bytes))
}
fn policy_identity(policy: &CheckPolicy) -> Result<String> {
    check_policy_identity(policy)
}
impl CandidateCache {
    pub fn new(policy: CheckPolicy) -> Result<Self> {
        policy_identity(&policy)?;
        Ok(Self {
            schema: "deslop.trajectory-cache/1".to_string(),
            policy,
            entries: BTreeMap::new(),
        })
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema != "deslop.trajectory-cache/1" {
            bail!("unsupported candidate cache schema")
        }
        let expected_policy = policy_identity(&self.policy)?;
        for (key, entry) in &self.entries {
            if key != &entry.candidate_hash || !valid_digest(key) {
                bail!("candidate cache key does not match candidate hash")
            }
            if entry.policy_identity != expected_policy {
                bail!("candidate cache entry uses a different check policy")
            }
        }
        Ok(())
    }

    pub fn record(
        &mut self,
        candidate: &Candidate,
        status: CandidateCheckStatus,
        detail: Option<String>,
    ) -> Result<()> {
        if self.schema != "deslop.trajectory-cache/1" {
            bail!("unsupported candidate cache schema")
        }
        if candidate.state_hash != snapshot_hash(&candidate.files) {
            bail!("candidate state hash does not bind its source inventory")
        }
        let policy_identity = policy_identity(&self.policy)?;
        if candidate.cache_identity != cache_identity(&candidate.state_hash, &policy_identity) {
            bail!("candidate cache identity mismatch")
        }
        self.entries.insert(
            candidate.state_hash.clone(),
            CandidateCheck {
                candidate_hash: candidate.state_hash.clone(),
                policy_identity,
                status,
                detail,
            },
        );
        Ok(())
    }
}

/// Domain-separated identity for a complete candidate state and trusted
/// policy. The caller must hash the complete file inventory into
/// `candidate_state_hash`; policy identity is the canonical hash of every
/// policy field, so changing either invalidates the cache entry.
pub fn cache_identity(candidate_state_hash: &str, policy_identity: &str) -> String {
    digest(
        format!(
            "deslop.trajectory.cache/1\0state\0{candidate_state_hash}\0policy\0{policy_identity}"
        )
        .as_bytes(),
    )
}

pub fn cache_identity_for_policy(
    candidate_state_hash: &str,
    policy: &CheckPolicy,
) -> Result<String> {
    if !valid_digest(candidate_state_hash) {
        bail!("candidate state hash is not a valid blake3 digest")
    }
    Ok(cache_identity(
        candidate_state_hash,
        &check_policy_identity(policy)?,
    ))
}

pub fn generate_candidates(
    trajectory: &Trajectory,
    budget: &MinimizationBudget,
) -> Result<Vec<Candidate>> {
    let policy = CheckPolicy {
        schema: "deslop.trajectory-check-policy/1".to_string(),
        policy_id: "untrusted-generation".to_string(),
        selected_checks: vec!["caller-selected-trusted-checks".to_string()],
        protected_paths: trajectory.protected_paths.clone(),
    };
    generate_candidates_with_policy(trajectory, budget, &policy)
}

pub fn generate_candidates_with_policy(
    trajectory: &Trajectory,
    budget: &MinimizationBudget,
    policy: &CheckPolicy,
) -> Result<Vec<Candidate>> {
    trajectory.validate()?;
    let policy_id = policy_identity(policy)?;
    let protected_paths = trajectory
        .protected_paths
        .union(&policy.protected_paths)
        .cloned()
        .collect::<BTreeSet<_>>();
    if budget.max_candidates == 0
        || budget.max_events == 0
        || budget.max_groups == 0
        || budget.max_validation_calls == 0
    {
        return Ok(Vec::new());
    }
    if trajectory.events.len() > budget.max_events {
        bail!("trajectory exceeds event budget")
    }
    let final_cost = patch_cost(&trajectory.base.files, &trajectory.final_snapshot.files);
    let mut groups = Vec::new();
    let mut seen = BTreeSet::new();
    for event in &trajectory.events {
        if seen.insert(event.group.clone()) {
            if groups.len() == budget.max_groups {
                break;
            }
            if trajectory
                .events
                .iter()
                .filter(|e| e.group == event.group)
                .any(|e| protected_paths.contains(&e.path))
            {
                continue;
            }
            groups.push(event.group.clone());
        }
    }
    let mut candidates = Vec::new();
    let candidate_limit = budget.max_candidates.min(budget.max_validation_calls);
    for group in groups.into_iter().take(candidate_limit) {
        let excluded = BTreeSet::from([group.clone()]);
        let evidence = apply_events(trajectory, &excluded);
        if evidence.status == ReplayStatus::Contradictory {
            continue;
        }
        if patch_cost(&trajectory.base.files, &evidence.files) >= final_cost {
            continue;
        }
        let state_hash = evidence.final_hash;
        candidates.push(Candidate {
            id: String::new(),
            removed_groups: vec![group],
            files: evidence.files,
            state_hash: state_hash.clone(),
            cache_identity: cache_identity(&state_hash, &policy_id),
            protected_paths: protected_paths.clone(),
        });
    }
    candidates.sort_by(|a, b| {
        a.removed_groups
            .cmp(&b.removed_groups)
            .then(a.state_hash.cmp(&b.state_hash))
    });
    for (index, candidate) in candidates.iter_mut().enumerate() {
        candidate.id = format!("cand-{index:04}");
    }
    Ok(candidates)
}
fn ignored_metadata(name: &std::ffi::OsStr) -> bool {
    matches!(name.to_str(), Some(".git" | ".jj" | ".deslop" | "target"))
}

/// Stage the server-supplied patches while excluding VCS/build metadata.
fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in
        fs::read_dir(source).with_context(|| format!("read staging source {}", source.display()))?
    {
        let entry = entry?;
        if ignored_metadata(&entry.file_name()) {
            continue;
        }
        let kind = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            bail!(
                "cannot stage symlink or special file `{}`",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn collect_files(root: &Path, current: &Path, files: &mut BTreeMap<String, String>) -> Result<()> {
    for entry in
        fs::read_dir(current).with_context(|| format!("read staged tree {}", current.display()))?
    {
        let entry = entry?;
        if ignored_metadata(&entry.file_name()) {
            continue;
        }
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            collect_files(root, &path, files)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .context("staged path escaped root")?;
            let relative = relative.to_str().context("staged path is not UTF-8")?;
            let relative = repo_path(relative)?;
            let content = fs::read_to_string(&path)
                .with_context(|| format!("candidate file `{relative}` is not UTF-8 text"))?;
            if files.insert(relative.clone(), content).is_some() {
                bail!("duplicate staged path `{relative}`")
            }
        } else {
            bail!(
                "staged tree contains symlink or special file `{}`",
                path.display()
            );
        }
    }
    Ok(())
}

/// Stage the server-supplied patches, verify/apply there, and compare the
/// complete staged inventory with the candidate before touching the live root.
/// The live apply then repeats the existing verifier's commit-boundary checks.
pub fn verify_and_apply_candidate(
    candidate: &Candidate,
    patches: &[Patch],
    options: &VerifyOptions,
    backup: bool,
) -> Result<CandidateApplyReport> {
    if candidate.state_hash != snapshot_hash(&candidate.files) {
        bail!("candidate state hash mismatch")
    }
    let stage = TempDir::new().context("create isolated candidate staging directory")?;
    copy_tree(&options.root, stage.path())?;
    let mut staged_options = options.clone();
    staged_options.root = stage.path().to_path_buf();
    staged_options.scope = options
        .scope
        .as_ref()
        .map(|scope| {
            scope
                .iter()
                .map(|path| {
                    if path.is_absolute() {
                        path.strip_prefix(&options.root)
                            .map(|relative| stage.path().join(relative))
                            .with_context(|| {
                                format!(
                                    "verification scope `{}` is outside live root",
                                    path.display()
                                )
                            })
                    } else {
                        Ok(path.clone())
                    }
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?;
    let staged_verification = verify_patches(patches, &staged_options)
        .context("trusted candidate verification in staging")?;
    if staged_verification
        .results
        .iter()
        .any(|result| !result.passed)
    {
        bail!("trusted candidate verification rejected candidate in staging")
    }
    let staged_apply = apply_patches(patches, &staged_options, false)
        .context("trusted candidate staging apply")?;
    if staged_apply.verified.failed_count() != 0 {
        bail!("trusted candidate staging apply rejected candidate");
    }
    let mut staged_files = BTreeMap::new();
    collect_files(stage.path(), stage.path(), &mut staged_files)?;
    if staged_files != candidate.files {
        bail!("staged verified state does not exactly match candidate file inventory")
    }
    let applied =
        apply_patches(patches, options, backup).context("trusted candidate live apply")?;
    if applied.verified.failed_count() != 0 {
        bail!("trusted candidate live apply rejected candidate");
    }
    Ok(CandidateApplyReport {
        candidate_hash: snapshot_hash(&staged_files),
        verified: applied.verified.clone(),
        applied,
    })
}

fn required_string(value: &Value, key: &str, context: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .with_context(|| format!("{context} missing string `{key}`"))
}

fn tool_edit(
    part: &Value,
    message_id: &str,
    ordinal: u64,
    files: &mut BTreeMap<String, String>,
    workspace_root: Option<&Path>,
) -> Result<Option<EditEvent>> {
    if part.get("type").and_then(Value::as_str) != Some("tool") {
        return Ok(None);
    }
    let tool = part.get("tool").and_then(Value::as_str).unwrap_or_default();
    if !matches!(tool, "edit" | "write") {
        if tool == "apply_patch" {
            bail!("OpenCode apply_patch event is unsupported without a parsed public patch payload")
        }
        return Ok(None);
    }
    let state = part
        .get("state")
        .context("OpenCode tool part missing state")?;
    if state.get("status").and_then(Value::as_str) != Some("completed") {
        bail!("OpenCode edit/write event is not completed; history is incomplete")
    }
    let input = state
        .get("input")
        .context("OpenCode tool part missing public state.input")?;
    let path_value = input
        .get("filePath")
        .or_else(|| input.get("file"))
        .or_else(|| input.get("path"))
        .or_else(|| input.get("filename"));
    let raw_path = path_value
        .and_then(Value::as_str)
        .context("OpenCode edit/write input missing filePath")?;
    let path = if Path::new(raw_path).is_absolute() {
        let root = workspace_root.context("absolute OpenCode filePath requires workspace root")?;
        let relative = Path::new(raw_path)
            .strip_prefix(root)
            .with_context(|| format!("OpenCode filePath `{raw_path}` is outside workspace root"))?;
        repo_path(
            relative
                .to_str()
                .context("OpenCode filePath is not UTF-8")?,
        )?
    } else {
        repo_path(raw_path)?
    };
    let id = required_string(part, "id", "OpenCode tool part")?;
    let group = format!("message:{message_id}");
    let old_content = files.get(&path).cloned();
    let (before, after) = if tool == "write" {
        let content = input
            .get("content")
            .and_then(Value::as_str)
            .context("OpenCode write input missing content")?
            .to_owned();
        (old_content.as_deref().map(blob), blob(content))
    } else {
        let old = input
            .get("oldString")
            .or_else(|| input.get("old_string"))
            .and_then(Value::as_str)
            .context("OpenCode edit input missing oldString")?;
        let new = input
            .get("newString")
            .or_else(|| input.get("new_string"))
            .and_then(Value::as_str)
            .context("OpenCode edit input missing newString")?;
        let current = old_content.as_deref();
        if old.is_empty() {
            if current.is_some() {
                bail!("OpenCode edit with empty oldString may only create a missing file")
            }
            (None, blob(new))
        } else {
            let current = current
                .context("OpenCode edit targets a file absent from the supplied base snapshot")?;
            let replace_all = input
                .get("replaceAll")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let occurrences = current.match_indices(old).count();
            if occurrences != 1 && !replace_all {
                bail!(
                    "OpenCode edit `{path}` requires exactly one oldString occurrence, found {occurrences}"
                )
            }
            let updated = if replace_all {
                current.replace(old, new)
            } else {
                current.replacen(old, new, 1)
            };
            (Some(blob(current)), blob(updated))
        }
    };
    let kind = if before.is_some() {
        EditKind::Modify
    } else {
        EditKind::Create
    };
    files.insert(path.clone(), after.content.clone());
    Ok(Some(EditEvent {
        id,
        ordinal,
        group,
        path,
        kind,
        before,
        after: Some(after),
    }))
}

/// Adapt the real public OpenCode `opencode export SESSION` JSON.  The export
/// contains message/tool data but not a complete source tree, so callers must
/// provide independently pinned base and final snapshots.
pub fn import_opencode_export(
    bytes: &[u8],
    base: Snapshot,
    final_snapshot: Snapshot,
    license: LicenseGate,
) -> Result<Trajectory> {
    import_opencode_export_with_root(bytes, base, final_snapshot, license, None)
}

pub fn import_opencode_export_with_root(
    bytes: &[u8],
    base: Snapshot,
    final_snapshot: Snapshot,
    license: LicenseGate,
    workspace_root: Option<&Path>,
) -> Result<Trajectory> {
    let root: Value = serde_json::from_slice(bytes).context("parse OpenCode export JSON")?;
    let object = root
        .as_object()
        .context("OpenCode export must be an object")?;
    if object.len() != 2 || !object.contains_key("info") || !object.contains_key("messages") {
        bail!("unsupported or incomplete OpenCode export: expected exactly info and messages")
    }
    let messages = root
        .get("messages")
        .and_then(Value::as_array)
        .context("OpenCode export messages must be an array")?;
    let mut events = Vec::new();
    let mut working_files = base.files.clone();
    for message in messages {
        let info = message
            .get("info")
            .context("OpenCode message missing info")?;
        let message_id = required_string(info, "id", "OpenCode message info")?;
        let parts = message
            .get("parts")
            .and_then(Value::as_array)
            .context("OpenCode message parts must be an array")?;
        for part in parts {
            if let Some(event) = tool_edit(
                part,
                &message_id,
                events.len() as u64,
                &mut working_files,
                workspace_root,
            )? {
                events.push(event);
            }
        }
    }
    if events.is_empty() {
        bail!("OpenCode export contains no public edit/write events; history is incomplete")
    }
    let source_digest = digest(bytes);
    let trajectory = Trajectory {
        schema: TRAJECTORY_SCHEMA.to_string(),
        format: OPENCODE_EXPORT_FORMAT.to_string(),
        base,
        final_snapshot,
        events,
        observations: Vec::new(),
        missing_history: Vec::new(),
        license,
        integrity: IntegrityGate {
            algorithm: TRAJECTORY_INTEGRITY_ALGORITHM.to_string(),
            source_digest,
            verified: true,
        },
        protected_paths: BTreeSet::new(),
    };
    trajectory.validate()?;
    replay_trajectory(&trajectory)?;
    Ok(trajectory)
}

pub fn import_opencode_export_file_with_root(
    path: &Path,
    base: Snapshot,
    final_snapshot: Snapshot,
    license: LicenseGate,
    workspace_root: &Path,
) -> Result<Trajectory> {
    import_opencode_export_with_root(
        &std::fs::read(path).with_context(|| format!("read OpenCode export {}", path.display()))?,
        base,
        final_snapshot,
        license,
        Some(workspace_root),
    )
}

pub fn import_opencode_export_file(
    path: &Path,
    base: Snapshot,
    final_snapshot: Snapshot,
    license: LicenseGate,
) -> Result<Trajectory> {
    import_opencode_export(
        &std::fs::read(path).with_context(|| format!("read OpenCode export {}", path.display()))?,
        base,
        final_snapshot,
        license,
    )
}

pub fn write_trajectory(path: &Path, trajectory: &Trajectory) -> Result<()> {
    trajectory.validate()?;
    std::fs::write(path, serde_json::to_vec_pretty(trajectory)?)
        .with_context(|| format!("write trajectory {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_state_hash_mismatch_rejects_before_any_live_write() {
        let root = tempfile::tempdir().expect("temp root");
        let path = root.path().join("src.rs");
        fs::write(&path, "fn main() {}\n").expect("source");
        let mut files = BTreeMap::new();
        files.insert("src.rs".to_string(), "fn changed() {}\n".to_string());
        let candidate = Candidate {
            id: "candidate".to_string(),
            removed_groups: vec!["g".to_string()],
            state_hash: "blake3:0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            cache_identity: String::new(),
            protected_paths: BTreeSet::new(),
            files,
        };
        let options = VerifyOptions {
            root: root.path().to_path_buf(),
            scope: None,
            check_cmd: None,
            coverage: CoverageConfig::Disabled,
            mutation: MutationConfig::Disabled,
            characterization_tests: Vec::new(),
            allow_non_removable: false,
        };
        let error = verify_and_apply_candidate(&candidate, &[], &options, false)
            .expect_err("mismatched candidate must be rejected");
        assert!(error.to_string().contains("candidate state hash mismatch"));
        assert_eq!(
            fs::read_to_string(path).expect("source remains"),
            "fn main() {}\n"
        );
    }
}
