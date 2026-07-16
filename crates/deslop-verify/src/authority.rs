use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, PathBuf};

use anyhow::{Result, bail};
use deslop_parse::CapabilityAuthority;
use deslop_protocol::{SharedWorkOrder, WorkOrderImpact, WorkOrderResource, WorkOrderResourceKind};
use serde::{Deserialize, Serialize};

pub const VERIFIER_PLAN_SCHEMA: &str = "deslop.verifier-plan/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationCheckKind {
    Parse,
    Format,
    Build,
    Lint,
    Type,
    TargetedTest,
    Coverage,
    Characterization,
    Differential,
    Mutation,
    GraphDelta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorityProvider {
    Adapter,
    Compiler,
    LanguageServer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorityState {
    Proven,
    Disproven,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JoinedAuthorityState {
    Proven,
    Disproven,
    Unknown,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityRequirement {
    pub key: String,
    pub accepted_providers: Vec<AuthorityProvider>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityObservation {
    pub key: String,
    pub provider: AuthorityProvider,
    pub snapshot: String,
    pub artifact: String,
    pub state: AuthorityState,
    pub detail: String,
}

impl AuthorityObservation {
    pub fn from_capability_authority(
        key: impl Into<String>,
        snapshot: impl Into<String>,
        artifact: impl Into<String>,
        authority: CapabilityAuthority,
        state: AuthorityState,
        detail: impl Into<String>,
    ) -> Result<Self> {
        let provider = match authority {
            CapabilityAuthority::Adapter => AuthorityProvider::Adapter,
            CapabilityAuthority::LanguageServer => AuthorityProvider::LanguageServer,
            CapabilityAuthority::Compiler => AuthorityProvider::Compiler,
            CapabilityAuthority::Syntax | CapabilityAuthority::RuntimeVerification => {
                bail!(
                    "M7 precondition providers must be adapter, language-server, or compiler authority"
                )
            }
        };
        let observation = Self {
            key: key.into(),
            provider,
            snapshot: snapshot.into(),
            artifact: artifact.into(),
            state,
            detail: detail.into(),
        };
        validate_observation(&observation)?;
        Ok(observation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityDecision {
    pub requirement: AuthorityRequirement,
    pub state: JoinedAuthorityState,
    pub observations: Vec<AuthorityObservation>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationCheck {
    pub id: String,
    pub kind: VerificationCheckKind,
    pub command: Option<String>,
    pub covers: Vec<WorkOrderResource>,
    pub dependencies: Vec<String>,
    pub authority: Vec<AuthorityRequirement>,
    pub always_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationCatalog {
    pub snapshot: String,
    pub impact_coverage_complete: bool,
    pub checks: Vec<VerificationCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkPolicy {
    Denied,
    AllowListed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifierExecutionPolicy {
    pub maximum_total_millis: u64,
    pub maximum_command_millis: u64,
    pub maximum_output_bytes: usize,
    pub maximum_files: usize,
    pub maximum_file_bytes: usize,
    pub readable_roots: Vec<PathBuf>,
    pub writable_roots: Vec<PathBuf>,
    pub environment_allowlist: Vec<String>,
    pub network: NetworkPolicy,
    pub allowed_network_hosts: Vec<String>,
}

impl VerifierExecutionPolicy {
    pub fn hermetic_workspace() -> Self {
        Self {
            maximum_total_millis: 300_000,
            maximum_command_millis: 120_000,
            maximum_output_bytes: 1_048_576,
            maximum_files: 10_000,
            maximum_file_bytes: 16 * 1_048_576,
            readable_roots: vec![PathBuf::from(".")],
            writable_roots: vec![PathBuf::from(".")],
            environment_allowlist: vec![
                "CARGO_HOME".into(),
                "HOME".into(),
                "PATH".into(),
                "RUSTUP_HOME".into(),
                "TMPDIR".into(),
            ],
            network: NetworkPolicy::Denied,
            allowed_network_hosts: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.maximum_total_millis == 0
            || self.maximum_command_millis == 0
            || self.maximum_command_millis > self.maximum_total_millis
            || self.maximum_output_bytes == 0
            || self.maximum_files == 0
            || self.maximum_file_bytes == 0
        {
            bail!("verifier execution limits must be nonzero and command time must fit total time");
        }
        validate_relative_roots("readable", &self.readable_roots)?;
        validate_relative_roots("writable", &self.writable_roots)?;
        validate_sorted_unique_text("environment allowlist", &self.environment_allowlist)?;
        validate_sorted_unique_text("allowed network hosts", &self.allowed_network_hosts)?;
        match self.network {
            NetworkPolicy::Denied if !self.allowed_network_hosts.is_empty() => {
                bail!("network-denied policy cannot retain allowed hosts");
            }
            NetworkPolicy::AllowListed if self.allowed_network_hosts.is_empty() => {
                bail!("allow-listed network policy requires at least one host");
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckSelectionMode {
    ImpactCone,
    ConservativeProject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerifierPlanStatus {
    Ready,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerifierPlan {
    schema: String,
    id: String,
    work_order: String,
    snapshot: String,
    selection: CheckSelectionMode,
    impact_resources: Vec<WorkOrderResource>,
    checks: Vec<VerificationCheck>,
    authority: Vec<AuthorityDecision>,
    policy: VerifierExecutionPolicy,
    status: VerifierPlanStatus,
    residual_uncertainty: Vec<String>,
}

impl VerifierPlan {
    pub fn build(
        order: &SharedWorkOrder,
        catalog: VerificationCatalog,
        observations: Vec<AuthorityObservation>,
        policy: VerifierExecutionPolicy,
    ) -> Result<Self> {
        order.validate()?;
        policy.validate()?;
        validate_catalog(&catalog)?;
        let snapshot = order
            .provenance()
            .project_snapshot
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("M7 verification requires a project snapshot"))?;
        if snapshot != catalog.snapshot {
            bail!("verification catalog is stale for the work-order snapshot");
        }
        let impact_resources = impact_resources(order);
        let complete = catalog.impact_coverage_complete
            && order.unknowns().is_empty()
            && !matches!(order.impact(), WorkOrderImpact::Graph { cone } if cone.truncated);
        let selection = if complete {
            CheckSelectionMode::ImpactCone
        } else {
            CheckSelectionMode::ConservativeProject
        };
        let checks = select_checks(&catalog.checks, &impact_resources, selection)?;
        ensure_operational_check_families(&checks)?;
        let requirements = checks
            .iter()
            .flat_map(|check| check.authority.iter().cloned())
            .map(|requirement| (requirement.key.clone(), requirement))
            .collect::<BTreeMap<_, _>>()
            .into_values()
            .collect::<Vec<_>>();
        let authority = join_authority(snapshot, requirements, observations)?;
        let residual_uncertainty = residual_uncertainty(selection, &authority);
        let status = if authority
            .iter()
            .all(|decision| decision.state == JoinedAuthorityState::Proven)
        {
            VerifierPlanStatus::Ready
        } else {
            VerifierPlanStatus::Blocked
        };
        let mut plan = Self {
            schema: VERIFIER_PLAN_SCHEMA.into(),
            id: String::new(),
            work_order: order.id().as_str().into(),
            snapshot: snapshot.into(),
            selection,
            impact_resources,
            checks,
            authority,
            policy,
            status,
            residual_uncertainty,
        };
        plan.id = derive_plan_id(&plan)?;
        plan.validate()?;
        Ok(plan)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn work_order(&self) -> &str {
        &self.work_order
    }

    pub fn snapshot(&self) -> &str {
        &self.snapshot
    }

    pub fn policy(&self) -> &VerifierExecutionPolicy {
        &self.policy
    }

    pub fn selection(&self) -> CheckSelectionMode {
        self.selection
    }

    pub fn checks(&self) -> &[VerificationCheck] {
        &self.checks
    }

    pub fn authority(&self) -> &[AuthorityDecision] {
        &self.authority
    }

    pub fn status(&self) -> VerifierPlanStatus {
        self.status
    }

    pub fn residual_uncertainty(&self) -> &[String] {
        &self.residual_uncertainty
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != VERIFIER_PLAN_SCHEMA {
            bail!("unsupported verifier-plan schema `{}`", self.schema);
        }
        self.policy.validate()?;
        validate_sorted_resources(&self.impact_resources)?;
        validate_selected_checks(&self.checks)?;
        let requirements = self
            .authority
            .iter()
            .map(|decision| {
                (
                    decision.requirement.key.clone(),
                    decision.requirement.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .into_values()
            .collect::<Vec<_>>();
        if requirements.len() != self.authority.len() {
            bail!("verifier-plan authority decisions must be unique by requirement");
        }
        let observations = self
            .authority
            .iter()
            .flat_map(|decision| decision.observations.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if self.authority != join_authority(&self.snapshot, requirements, observations)? {
            bail!("verifier-plan authority decisions are not canonical");
        }
        validate_sorted_unique_text("residual uncertainty", &self.residual_uncertainty)?;
        if self.id != derive_plan_id(self)? {
            bail!("verifier-plan identity is stale");
        }
        let expected_status = if self
            .authority
            .iter()
            .all(|decision| decision.state == JoinedAuthorityState::Proven)
        {
            VerifierPlanStatus::Ready
        } else {
            VerifierPlanStatus::Blocked
        };
        if self.status != expected_status
            || self.residual_uncertainty != residual_uncertainty(self.selection, &self.authority)
        {
            bail!("verifier-plan status diverges from precondition authority");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifierPlanWire {
    schema: String,
    id: String,
    work_order: String,
    snapshot: String,
    selection: CheckSelectionMode,
    impact_resources: Vec<WorkOrderResource>,
    checks: Vec<VerificationCheck>,
    authority: Vec<AuthorityDecision>,
    policy: VerifierExecutionPolicy,
    status: VerifierPlanStatus,
    residual_uncertainty: Vec<String>,
}

impl<'de> Deserialize<'de> for VerifierPlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = VerifierPlanWire::deserialize(deserializer)?;
        let plan = Self {
            schema: wire.schema,
            id: wire.id,
            work_order: wire.work_order,
            snapshot: wire.snapshot,
            selection: wire.selection,
            impact_resources: wire.impact_resources,
            checks: wire.checks,
            authority: wire.authority,
            policy: wire.policy,
            status: wire.status,
            residual_uncertainty: wire.residual_uncertainty,
        };
        plan.validate().map_err(serde::de::Error::custom)?;
        Ok(plan)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerifierStage {
    Planning,
    Precondition,
    Staging,
    Format,
    GraphDelta,
    Command,
    Commit,
    Rollback,
    Demotion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerifierFailureKind {
    InvalidInput,
    StaleRevision,
    AuthorityConflict,
    PolicyViolation,
    CommandFailed,
    Timeout,
    Crash,
    OutputLimit,
    FilesystemViolation,
    NetworkViolation,
    FormatChangedSemantics,
    GraphDeltaMismatch,
    PartialWrite,
    RollbackFailed,
    Counterexample,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifierFailure {
    pub stage: VerifierStage,
    pub kind: VerifierFailureKind,
    pub check: Option<String>,
    pub detail: String,
    pub retryable: bool,
}

fn impact_resources(order: &SharedWorkOrder) -> Vec<WorkOrderResource> {
    let access = order.access();
    let mut resources = access
        .reads
        .iter()
        .chain(&access.writes)
        .chain(&access.requires)
        .chain(&access.invalidates)
        .cloned()
        .collect::<Vec<_>>();
    if let WorkOrderImpact::Graph { cone } = order.impact() {
        resources.extend(cone.entities.iter().map(|entity| WorkOrderResource {
            kind: WorkOrderResourceKind::Node,
            identity: format!("{:?}:{}:{}", entity.layer, entity.graph, entity.entity),
        }));
    }
    resources.sort();
    resources.dedup();
    resources
}

fn select_checks(
    checks: &[VerificationCheck],
    impact: &[WorkOrderResource],
    mode: CheckSelectionMode,
) -> Result<Vec<VerificationCheck>> {
    let by_id = checks
        .iter()
        .map(|check| (check.id.as_str(), check))
        .collect::<BTreeMap<_, _>>();
    let impact = impact.iter().collect::<BTreeSet<_>>();
    let mut selected = checks
        .iter()
        .filter(|check| {
            check.always_required
                || check
                    .covers
                    .iter()
                    .any(|resource| impact.contains(resource))
                || (mode == CheckSelectionMode::ConservativeProject
                    && matches!(
                        check.kind,
                        VerificationCheckKind::Build
                            | VerificationCheckKind::Lint
                            | VerificationCheckKind::Type
                            | VerificationCheckKind::TargetedTest
                    ))
        })
        .map(|check| check.id.clone())
        .collect::<BTreeSet<_>>();
    let mut pending = selected.iter().cloned().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        let check = by_id
            .get(id.as_str())
            .ok_or_else(|| anyhow::anyhow!("selected unknown verification check `{id}`"))?;
        for dependency in &check.dependencies {
            if !by_id.contains_key(dependency.as_str()) {
                bail!("verification check `{id}` depends on unknown check `{dependency}`");
            }
            if selected.insert(dependency.clone()) {
                pending.push(dependency.clone());
            }
        }
    }
    Ok(selected
        .into_iter()
        .map(|id| by_id[id.as_str()].clone())
        .collect())
}

fn join_authority(
    snapshot: &str,
    requirements: Vec<AuthorityRequirement>,
    observations: Vec<AuthorityObservation>,
) -> Result<Vec<AuthorityDecision>> {
    let mut decisions = Vec::new();
    for mut requirement in requirements {
        validate_text("authority requirement", &requirement.key)?;
        requirement.accepted_providers.sort();
        requirement.accepted_providers.dedup();
        if requirement.accepted_providers.is_empty() {
            bail!(
                "authority requirement `{}` accepts no provider",
                requirement.key
            );
        }
        let mut matching = observations
            .iter()
            .filter(|observation| observation.key == requirement.key)
            .cloned()
            .collect::<Vec<_>>();
        matching.sort_by(|left, right| {
            (left.provider, &left.artifact, left.state, &left.detail).cmp(&(
                right.provider,
                &right.artifact,
                right.state,
                &right.detail,
            ))
        });
        matching.dedup();
        for observation in &matching {
            validate_observation(observation)?;
        }
        let accepted = matching
            .iter()
            .filter(|observation| {
                observation.snapshot == snapshot
                    && requirement
                        .accepted_providers
                        .contains(&observation.provider)
            })
            .collect::<Vec<_>>();
        let has_proven = accepted
            .iter()
            .any(|observation| observation.state == AuthorityState::Proven);
        let has_disproven = accepted
            .iter()
            .any(|observation| observation.state == AuthorityState::Disproven);
        let (state, reason) = match (has_proven, has_disproven) {
            (true, true) => (
                JoinedAuthorityState::Conflict,
                "authoritative providers disagree; no precedence winner is allowed".into(),
            ),
            (false, true) => (
                JoinedAuthorityState::Disproven,
                "an accepted current provider disproved the precondition".into(),
            ),
            (true, false) => (
                JoinedAuthorityState::Proven,
                "accepted current provider evidence proves the precondition".into(),
            ),
            (false, false) => (
                JoinedAuthorityState::Unknown,
                "no accepted current provider proves the precondition".into(),
            ),
        };
        decisions.push(AuthorityDecision {
            requirement,
            state,
            observations: matching,
            reason,
        });
    }
    decisions.sort_by(|left, right| left.requirement.key.cmp(&right.requirement.key));
    Ok(decisions)
}

fn residual_uncertainty(
    selection: CheckSelectionMode,
    authority: &[AuthorityDecision],
) -> Vec<String> {
    let mut uncertainty = Vec::new();
    if selection == CheckSelectionMode::ConservativeProject {
        uncertainty.push(
            "impact coverage is incomplete; selected project-wide build/lint/type/test checks"
                .into(),
        );
    }
    for decision in authority {
        if decision.state != JoinedAuthorityState::Proven {
            uncertainty.push(format!(
                "precondition `{}` is {}: {}",
                decision.requirement.key,
                joined_state_name(decision.state),
                decision.reason
            ));
        }
    }
    uncertainty.sort();
    uncertainty.dedup();
    uncertainty
}

fn validate_catalog(catalog: &VerificationCatalog) -> Result<()> {
    validate_text("catalog snapshot", &catalog.snapshot)?;
    validate_selected_checks(&catalog.checks)?;
    let ids = catalog
        .checks
        .iter()
        .map(|check| check.id.as_str())
        .collect::<BTreeSet<_>>();
    for check in &catalog.checks {
        for dependency in &check.dependencies {
            if !ids.contains(dependency.as_str()) {
                bail!(
                    "verification check `{}` has unknown dependency `{dependency}`",
                    check.id
                );
            }
        }
    }
    Ok(())
}

fn validate_selected_checks(checks: &[VerificationCheck]) -> Result<()> {
    let mut prior = None;
    for check in checks {
        validate_text("verification check id", &check.id)?;
        if let Some(previous) = prior
            && previous >= check.id.as_str()
        {
            bail!("verification checks must be strictly sorted by unique id");
        }
        prior = Some(check.id.as_str());
        if check
            .command
            .as_ref()
            .is_some_and(|command| command.trim().is_empty())
        {
            bail!("verification check `{}` has an empty command", check.id);
        }
        validate_sorted_resources(&check.covers)?;
        validate_sorted_unique_text("check dependencies", &check.dependencies)?;
        let keys = check
            .authority
            .iter()
            .map(|requirement| requirement.key.as_str())
            .collect::<Vec<_>>();
        if keys.windows(2).any(|pair| pair[0] >= pair[1]) {
            bail!("verification check authority requirements must be sorted and unique");
        }
    }
    Ok(())
}

fn ensure_operational_check_families(checks: &[VerificationCheck]) -> Result<()> {
    for kind in [
        VerificationCheckKind::Build,
        VerificationCheckKind::Lint,
        VerificationCheckKind::Type,
        VerificationCheckKind::TargetedTest,
    ] {
        if !checks.iter().any(|check| check.kind == kind) {
            bail!("selected verification plan is missing required {kind:?} coverage");
        }
    }
    Ok(())
}

fn validate_observation(observation: &AuthorityObservation) -> Result<()> {
    validate_text("authority observation key", &observation.key)?;
    validate_text("authority observation snapshot", &observation.snapshot)?;
    validate_text("authority observation artifact", &observation.artifact)?;
    validate_text("authority observation detail", &observation.detail)
}

fn validate_sorted_resources(resources: &[WorkOrderResource]) -> Result<()> {
    if resources.windows(2).any(|pair| pair[0] >= pair[1]) {
        bail!("work-order resources must be strictly sorted and unique");
    }
    for resource in resources {
        validate_text("resource identity", &resource.identity)?;
    }
    Ok(())
}

fn validate_relative_roots(label: &str, roots: &[PathBuf]) -> Result<()> {
    if roots.is_empty() {
        bail!("{label} roots cannot be empty");
    }
    let mut normalized = Vec::new();
    for root in roots {
        if root.is_absolute()
            || root
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            bail!("{label} root `{}` escapes the workspace", root.display());
        }
        normalized.push(root.to_string_lossy().to_string());
    }
    validate_sorted_unique_text(label, &normalized)
}

fn validate_sorted_unique_text(label: &str, values: &[String]) -> Result<()> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        bail!("{label} must be strictly sorted and unique");
    }
    for value in values {
        validate_text(label, value)?;
    }
    Ok(())
}

fn validate_text(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        bail!("{label} must be nonempty bounded printable text");
    }
    Ok(())
}

fn joined_state_name(state: JoinedAuthorityState) -> &'static str {
    match state {
        JoinedAuthorityState::Proven => "proven",
        JoinedAuthorityState::Disproven => "disproven",
        JoinedAuthorityState::Unknown => "unknown",
        JoinedAuthorityState::Conflict => "conflict",
    }
}

fn derive_plan_id(plan: &VerifierPlan) -> Result<String> {
    #[derive(Serialize)]
    struct Identity<'a> {
        schema: &'a str,
        work_order: &'a str,
        snapshot: &'a str,
        selection: CheckSelectionMode,
        impact_resources: &'a [WorkOrderResource],
        checks: &'a [VerificationCheck],
        authority: &'a [AuthorityDecision],
        policy: &'a VerifierExecutionPolicy,
        status: VerifierPlanStatus,
        residual_uncertainty: &'a [String],
    }
    let payload = serde_json::to_vec(&Identity {
        schema: &plan.schema,
        work_order: &plan.work_order,
        snapshot: &plan.snapshot,
        selection: plan.selection,
        impact_resources: &plan.impact_resources,
        checks: &plan.checks,
        authority: &plan.authority,
        policy: &plan.policy,
        status: plan.status,
        residual_uncertainty: &plan.residual_uncertainty,
    })?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"deslop verifier plan v1\0");
    hasher.update(&payload);
    Ok(format!("vp1_{}", hasher.finalize().to_hex()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_protocol::SharedWorkOrder;
    use deslop_recipes::detect_rust_recipes;
    use std::path::Path;
    use tempfile::TempDir;

    fn fixture() -> (TempDir, SharedWorkOrder) {
        let root = TempDir::new().unwrap();
        std::fs::write(root.path().join("fixture.rs"), "fn run() { return; 1; }\n").unwrap();
        let candidate = detect_rust_recipes(root.path(), &[PathBuf::from("fixture.rs")])
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        (root, SharedWorkOrder::from_candidate(candidate).unwrap())
    }

    fn requirement() -> AuthorityRequirement {
        AuthorityRequirement {
            key: "binding-preserved".into(),
            accepted_providers: vec![
                AuthorityProvider::Adapter,
                AuthorityProvider::Compiler,
                AuthorityProvider::LanguageServer,
            ],
        }
    }

    fn catalog(order: &SharedWorkOrder, complete: bool) -> VerificationCatalog {
        let resource = order.access().writes[0].clone();
        let authority = vec![requirement()];
        let mut checks = vec![
            VerificationCheck {
                id: "build:workspace".into(),
                kind: VerificationCheckKind::Build,
                command: Some("cargo build --workspace".into()),
                covers: vec![resource.clone()],
                dependencies: vec!["parse".into()],
                authority: authority.clone(),
                always_required: false,
            },
            VerificationCheck {
                id: "lint:workspace".into(),
                kind: VerificationCheckKind::Lint,
                command: Some("cargo clippy --workspace".into()),
                covers: vec![resource.clone()],
                dependencies: vec!["parse".into()],
                authority: authority.clone(),
                always_required: false,
            },
            VerificationCheck {
                id: "parse".into(),
                kind: VerificationCheckKind::Parse,
                command: None,
                covers: vec![],
                dependencies: vec![],
                authority: vec![],
                always_required: true,
            },
            VerificationCheck {
                id: "test:impacted".into(),
                kind: VerificationCheckKind::TargetedTest,
                command: Some("cargo test impacted".into()),
                covers: vec![resource.clone()],
                dependencies: vec!["build:workspace".into()],
                authority: authority.clone(),
                always_required: false,
            },
            VerificationCheck {
                id: "test:unrelated".into(),
                kind: VerificationCheckKind::TargetedTest,
                command: Some("cargo test unrelated".into()),
                covers: vec![WorkOrderResource {
                    kind: WorkOrderResourceKind::Test,
                    identity: "unrelated".into(),
                }],
                dependencies: vec!["build:workspace".into()],
                authority: authority.clone(),
                always_required: false,
            },
            VerificationCheck {
                id: "type:workspace".into(),
                kind: VerificationCheckKind::Type,
                command: Some("cargo check --workspace".into()),
                covers: vec![resource],
                dependencies: vec!["parse".into()],
                authority,
                always_required: false,
            },
        ];
        checks.sort_by(|left, right| left.id.cmp(&right.id));
        VerificationCatalog {
            snapshot: order.provenance().project_snapshot.clone().unwrap(),
            impact_coverage_complete: complete,
            checks,
        }
    }

    fn observation(
        order: &SharedWorkOrder,
        provider: AuthorityProvider,
        state: AuthorityState,
    ) -> AuthorityObservation {
        AuthorityObservation {
            key: "binding-preserved".into(),
            provider,
            snapshot: order.provenance().project_snapshot.clone().unwrap(),
            artifact: format!("artifact-{provider:?}"),
            state,
            detail: format!("{provider:?} result"),
        }
    }

    #[test]
    fn complete_impact_selects_dependency_closed_checks_only() {
        let (_root, order) = fixture();
        let plan = VerifierPlan::build(
            &order,
            catalog(&order, true),
            vec![observation(
                &order,
                AuthorityProvider::Compiler,
                AuthorityState::Proven,
            )],
            VerifierExecutionPolicy::hermetic_workspace(),
        )
        .unwrap();
        assert_eq!(plan.selection(), CheckSelectionMode::ImpactCone);
        assert_eq!(plan.status(), VerifierPlanStatus::Ready);
        assert!(
            !plan
                .checks()
                .iter()
                .any(|check| check.id == "test:unrelated")
        );
        assert!(plan.checks().iter().any(|check| check.id == "parse"));
    }

    #[test]
    fn incomplete_impact_uses_project_fallback_and_conflicts_block() {
        let (_root, order) = fixture();
        let plan = VerifierPlan::build(
            &order,
            catalog(&order, false),
            vec![
                observation(&order, AuthorityProvider::Compiler, AuthorityState::Proven),
                observation(
                    &order,
                    AuthorityProvider::LanguageServer,
                    AuthorityState::Disproven,
                ),
            ],
            VerifierExecutionPolicy::hermetic_workspace(),
        )
        .unwrap();
        assert_eq!(plan.selection(), CheckSelectionMode::ConservativeProject);
        assert_eq!(plan.status(), VerifierPlanStatus::Blocked);
        assert!(
            plan.checks()
                .iter()
                .any(|check| check.id == "test:unrelated")
        );
        assert_eq!(plan.authority()[0].state, JoinedAuthorityState::Conflict);
        assert!(
            plan.residual_uncertainty()
                .iter()
                .any(|reason| reason.contains("disagree"))
        );
    }

    #[test]
    fn stale_provider_and_plan_tampering_fail_closed() {
        let (_root, order) = fixture();
        let mut stale = observation(&order, AuthorityProvider::Compiler, AuthorityState::Proven);
        stale.snapshot = "ps1_stale".into();
        let plan = VerifierPlan::build(
            &order,
            catalog(&order, true),
            vec![stale],
            VerifierExecutionPolicy::hermetic_workspace(),
        )
        .unwrap();
        assert_eq!(plan.status(), VerifierPlanStatus::Blocked);

        let mut wire = serde_json::to_value(&plan).unwrap();
        wire["selection"] = serde_json::json!("conservative-project");
        assert!(serde_json::from_value::<VerifierPlan>(wire).is_err());
    }

    #[test]
    fn policy_rejects_escape_and_network_ambiguity() {
        let mut policy = VerifierExecutionPolicy::hermetic_workspace();
        policy.writable_roots = vec![Path::new("../escape").to_path_buf()];
        assert!(policy.validate().is_err());

        let mut policy = VerifierExecutionPolicy::hermetic_workspace();
        policy.allowed_network_hosts = vec!["example.com".into()];
        assert!(policy.validate().is_err());
    }

    #[test]
    fn adapter_compiler_and_lsp_capability_authorities_map_without_rank_collapse() {
        for (authority, provider) in [
            (CapabilityAuthority::Adapter, AuthorityProvider::Adapter),
            (CapabilityAuthority::Compiler, AuthorityProvider::Compiler),
            (
                CapabilityAuthority::LanguageServer,
                AuthorityProvider::LanguageServer,
            ),
        ] {
            let observation = AuthorityObservation::from_capability_authority(
                "binding",
                "ps1_snapshot",
                "artifact",
                authority,
                AuthorityState::Proven,
                "provider result",
            )
            .unwrap();
            assert_eq!(observation.provider, provider);
        }
        assert!(
            AuthorityObservation::from_capability_authority(
                "binding",
                "ps1_snapshot",
                "artifact",
                CapabilityAuthority::Syntax,
                AuthorityState::Proven,
                "syntax is not semantic precondition authority",
            )
            .is_err()
        );
    }
}
