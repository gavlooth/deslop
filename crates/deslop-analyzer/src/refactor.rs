//! Refactor-defect accumulation detection over a revision window.
//!
//! Design: `docs/REFACTOR_DEFECT_ACCUMULATION.md`. All eleven detector
//! families run here over [`ContractChangeHistory`] facts extracted from
//! exact `ProjectAnalysis` snapshots:
//!
//! - migration families over direct dependents
//!   (`owner-moved-consumer-stale`, `producer-verifier-schema-drift`) and
//!   over the syntactic reference closure (`mechanism-live-gate-retired`,
//!   `telemetry-not-bound-to-claim`, `operational-identity-stale`), where
//!   the dependency-path split — reaching the retired representation while
//!   never reaching the new one — is the firing condition and surface
//!   classification is lexical supporting evidence only;
//! - `confidence-provenance-lost`: a formerly bound dependent re-derives
//!   its output through a lossy operation without following the owner to
//!   the new evidence source;
//! - `scope-collapse-after-refactor`: loop structure lost plus a
//!   flatten/concatenate-family gain; the finding requests a metamorphic
//!   independence test because syntax cannot prove the partition axis;
//! - `accepted-config-inert`: a formerly live config key loses its final
//!   behavioral read while an acceptance surface still carries it;
//! - `test-oracle-lag`: a migration or scope collapse fired while an
//!   unchanged test still exercises the former representation; the finding
//!   states what remains unproved, never that the implementation is wrong;
//! - `hot-path-work-duplicated`: an introduced structurally equivalent
//!   composite call on one reachable path; cost stays an explicit gap;
//! - `adoption-chain-incomplete` summaries: when several families share one
//!   owner migration, one summary is emitted in
//!   [`RefactorRiskReport::summaries`] — never in `findings` — so baselines
//!   and severity counts do not double-count.
//!
//! All findings are `NeverAuto`: the analysis diagnoses an incomplete
//! contract migration and suggests a verification; it never proposes a
//! rewrite. Optional revision-bound semantic-provider artifacts join as
//! supporting or conflicting evidence without changing syntax authority.
//!
//! Detection discipline:
//! - an owner change requires a function whose contract-token set both lost
//!   and gained tokens between the revisions (a pure removal is a deletion,
//!   not a migration);
//! - a removed token only counts as a *former representation* when it is
//!   still referenced somewhere in the after revision — otherwise the change
//!   is a rewrite or rename, which must not fire (rename/move negative
//!   cases);
//! - a stale consumer must be matched by name and have an *unchanged* token
//!   set still containing the removed token. A consumer that moved anywhere
//!   else — including to a compatibility adapter — does not fire;
//! - narrowing filters only ever suppress candidates: type-constructor
//!   references, revision-wide shared utilities, and self-recursion are
//!   not ownership evidence, and hot-path candidates must be composite
//!   non-constructor calls.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use deslop_core::SafetyClass;
use deslop_core::refactor_defect::{
    CapabilityLevel, ContractEdgeKind, ContractNodeRef, ContractRole, ContractStep, CoverageGap,
    EntityMatchEvidence, EntityMatchKind, EvidenceItem, FactProvider, OwnerMigration, Persistence,
    REFACTOR_DEFECT_SCHEMA, RefactorDefect, RevisionPair, rule_names,
};
use deslop_parse::{
    ContractChangeHistory, ContractFunction, DiscoveryPolicy, FactCoverage, FileContracts,
    ProjectAnalysis, ProjectSnapshotPlanner, ProjectSnapshotRequest, RepositorySpec,
    RevisionContracts, RootSpec, ScopeSpec,
};
use serde::Serialize;

/// Wire schema identifier for a refactor-risk report over one revision window.
pub const REFACTOR_RISK_SCHEMA: &str = "deslop.refactor-risk/1";

/// Project a refactor-risk report's findings into scan-path file reports so
/// the existing text and SARIF renderers apply with no format changes.
/// Summaries stay out: they must never enter finding counts or baselines.
pub fn to_file_reports(report: &RefactorRiskReport) -> Vec<deslop_core::FileReport> {
    let mut by_path: BTreeMap<PathBuf, Vec<deslop_core::Finding>> = BTreeMap::new();
    for defect in &report.findings {
        let finding = defect.to_finding();
        by_path
            .entry(finding.path.clone())
            .or_default()
            .push(finding);
    }
    by_path
        .into_iter()
        .map(|(path, findings)| deslop_core::FileReport {
            lang: deslop_lang::detect_lang(&path),
            path,
            analysis: deslop_core::AnalysisProvenance::complete(),
            findings,
        })
        .collect()
}

/// The output of one refactor-risk comparison over an ordered window.
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RefactorRiskReport {
    pub schema: String,
    /// First revision label (== `revisions[0]`).
    pub before: String,
    /// Last revision label.
    pub after: String,
    /// All revision labels in window order, oldest first.
    pub revisions: Vec<String>,
    pub coverage: FactCoverage,
    pub coverage_reasons: Vec<String>,
    /// Detector-family findings, one per detected condition.
    pub findings: Vec<RefactorDefect>,
    /// `adoption-chain-incomplete` summaries. Kept out of `findings` so
    /// baselines and severity counts never double-count the underlying
    /// findings.
    pub summaries: Vec<RefactorDefect>,
}

/// Compare two directory snapshots (`--from`, `--to`) and report
/// refactor-defect findings.
pub fn refactor_risk_paths(from: &Path, to: &Path) -> Result<RefactorRiskReport> {
    refactor_risk_window_paths(&[from.to_path_buf(), to.to_path_buf()])
}

/// Compare an ordered window of directory snapshots (`--from`, `--to`,
/// `--then ...`) and report refactor-defect findings.
pub fn refactor_risk_window_paths(roots: &[PathBuf]) -> Result<RefactorRiskReport> {
    let labeled: Vec<(String, PathBuf)> = roots
        .iter()
        .map(|root| (root.display().to_string(), root.clone()))
        .collect();
    refactor_risk_window_labeled(&labeled)
}

/// Compare an ordered window of directory snapshots under caller-supplied
/// revision labels (a VCS provider labels materialized revisions by their
/// revision spec, not by the extraction directory).
pub fn refactor_risk_window_labeled(roots: &[(String, PathBuf)]) -> Result<RefactorRiskReport> {
    let mut revisions = Vec::with_capacity(roots.len());
    for (label, root) in roots {
        revisions.push((label.clone(), analysis_for(root)?));
    }
    analyze_refactor_window(revisions)
}

/// Wire schema identifier for the semantic-provider facts payload accepted
/// inside a `deslop.refactor-history/1` provider artifact: per-function
/// reference tokens a provider attests for the revision carrying the
/// artifact.
pub const SEMANTIC_PROVIDER_FACTS_SCHEMA: &str = "deslop.semantic-provider-facts/1";

/// Rules whose stale-edge tokens are reference tokens a semantic provider
/// can attest or dispute. Literal and config tokens are outside the
/// provider-facts contract.
const PROVIDER_JOIN_RULES: &[&str] = &[
    rule_names::OWNER_MOVED_CONSUMER_STALE,
    rule_names::MECHANISM_LIVE_GATE_RETIRED,
    rule_names::TELEMETRY_NOT_BOUND_TO_CLAIM,
    rule_names::OPERATIONAL_IDENTITY_STALE,
    rule_names::CONFIDENCE_PROVENANCE_LOST,
];

/// Deserialized `deslop.semantic-provider-facts/1` payload.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderFacts {
    schema: String,
    functions: Vec<ProviderFunctionFacts>,
}

/// One function's provider-attested reference tokens, located by path and
/// starting line so the join never parses detail strings.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderFunctionFacts {
    path: PathBuf,
    start_line: usize,
    references: BTreeSet<String>,
}

/// Analyze an ordered `deslop.refactor-history/1` bundle: build exact-byte
/// analyses for every revision, run the window detection, then join
/// revision-bound semantic-provider artifacts as supporting or conflicting
/// evidence. Provider facts never change the authority of syntax facts and
/// never promote or suppress a finding; disagreement stays visible in
/// `counter_evidence`, and payloads the analysis cannot understand become
/// explicit coverage reasons.
pub fn analyze_refactor_bundle(
    bundle: &deslop_core::refactor_defect::RefactorHistoryBundle,
) -> Result<RefactorRiskReport> {
    bundle
        .validate()
        .map_err(|error| anyhow::anyhow!("invalid refactor-history bundle: {error}"))?;
    // An empty anchor directory: overlay bytes are pinned in memory, the
    // root only anchors path resolution and repository identity.
    let anchor =
        std::env::temp_dir().join(format!("deslop-refactor-bundle-{}", std::process::id()));
    std::fs::create_dir_all(&anchor).context("create bundle anchor directory")?;
    let mut revisions = Vec::with_capacity(bundle.revisions.len());
    for snapshot in &bundle.revisions {
        let mut builder = deslop_parse::ProjectSnapshotBuilder::new(
            &anchor,
            deslop_parse::RepositoryId::explicit("refactor-history-bundle")
                .map_err(|error| anyhow::anyhow!("{error}"))?,
        )?;
        for file in &snapshot.files {
            builder = builder.with_overlay(&file.path, file.contents.clone().into_bytes())?;
        }
        let analysis = ProjectAnalysis::build(builder.build()?)?;
        revisions.push((snapshot.revision.clone(), analysis));
    }
    let mut report = analyze_refactor_window(revisions)?;
    join_provider_artifacts(&mut report, bundle);
    Ok(report)
}

/// Join revision-bound provider artifacts into an existing report.
fn join_provider_artifacts(
    report: &mut RefactorRiskReport,
    bundle: &deslop_core::refactor_defect::RefactorHistoryBundle,
) {
    for snapshot in &bundle.revisions {
        for artifact in &snapshot.provider_artifacts {
            let facts: ProviderFacts = match serde_json::from_str(&artifact.payload) {
                Ok(facts) => facts,
                Err(_) => {
                    report.coverage = FactCoverage::Partial;
                    report.coverage_reasons.push(format!(
                        "provider artifact ({:?}, revision {}) payload not understood; \
                         expected {SEMANTIC_PROVIDER_FACTS_SCHEMA}",
                        artifact.provider, snapshot.revision
                    ));
                    continue;
                }
            };
            if facts.schema != SEMANTIC_PROVIDER_FACTS_SCHEMA {
                report.coverage = FactCoverage::Partial;
                report.coverage_reasons.push(format!(
                    "provider artifact ({:?}, revision {}) carries schema `{}`; expected \
                     {SEMANTIC_PROVIDER_FACTS_SCHEMA}",
                    artifact.provider, snapshot.revision, facts.schema
                ));
                continue;
            }
            for finding in report.findings.iter_mut() {
                if finding.revisions.after != snapshot.revision
                    || !PROVIDER_JOIN_RULES.contains(&finding.rule.as_str())
                {
                    continue;
                }
                let mut supporting = Vec::new();
                let mut conflicting = Vec::new();
                for step in &finding.stale_edges {
                    let Some(token) = &step.token else {
                        continue;
                    };
                    let Some(fact) = facts.functions.iter().find(|fact| {
                        fact.path == step.node.path && fact.start_line == step.node.span.start_line
                    }) else {
                        // Missing provider facts stay unknown; they are not
                        // negative facts.
                        continue;
                    };
                    if fact.references.contains(token) {
                        supporting.push(EvidenceItem {
                            provider: artifact.provider,
                            detail: format!(
                                "provider attests `{token}` is referenced at {}:{} in \
                                 revision {}",
                                step.node.path.display(),
                                step.node.span.start_line,
                                snapshot.revision
                            ),
                            node: Some(step.node.clone()),
                        });
                    } else {
                        conflicting.push(EvidenceItem {
                            provider: artifact.provider,
                            detail: format!(
                                "provider does not list `{token}` at {}:{} in revision {}; \
                                 disagreement retained, syntax authority unchanged",
                                step.node.path.display(),
                                step.node.span.start_line,
                                snapshot.revision
                            ),
                            node: Some(step.node.clone()),
                        });
                    }
                }
                finding.evidence.extend(supporting);
                finding.counter_evidence.extend(conflicting);
            }
        }
    }
}

/// Compare two exact analyses under caller-supplied revision labels.
pub fn analyze_refactor_risk(
    before: (String, Arc<ProjectAnalysis>),
    after: (String, Arc<ProjectAnalysis>),
) -> Result<RefactorRiskReport> {
    analyze_refactor_window(vec![before, after])
}

/// Compare an ordered window of exact analyses under caller-supplied labels.
///
/// Detection runs over each adjacent pair; persistence and co-change triage
/// inputs are then tracked across the whole window. Two-revision windows
/// behave exactly like Phase 1 and carry the explicit
/// accumulation-not-analyzed coverage gap.
pub fn analyze_refactor_window(
    revisions: Vec<(String, Arc<ProjectAnalysis>)>,
) -> Result<RefactorRiskReport> {
    if revisions.len() < 2 {
        bail!(
            "refactor-risk requires at least two revisions (got {})",
            revisions.len()
        );
    }
    let labels: Vec<String> = revisions.iter().map(|(label, _)| label.clone()).collect();
    let history = ContractChangeHistory::from_analyses(&revisions)
        .context("build contract change history")?;
    let mut drafts = Vec::new();
    for index in 0..history.revisions.len() - 1 {
        detect_pair(
            index,
            &labels,
            &history.revisions[index],
            &history.revisions[index + 1],
            &mut drafts,
        );
    }
    for draft in &mut drafts {
        track_persistence(&history, draft);
    }
    let window = history.revisions.len();
    let findings: Vec<RefactorDefect> = drafts
        .iter()
        .map(|draft| build_finding(draft, window))
        .collect();
    let summaries = build_summaries(&drafts, window);
    // Dynamic/reflective access leaves consumer resolution incomplete: an
    // explicit capability gap, never a clean result.
    let mut coverage = history.coverage;
    let dynamic_reasons = dynamic_access_reasons(&history);
    let mut coverage_reasons = history.reasons;
    if !dynamic_reasons.is_empty() {
        coverage = FactCoverage::Partial;
        coverage_reasons.extend(dynamic_reasons);
    }
    Ok(RefactorRiskReport {
        schema: REFACTOR_RISK_SCHEMA.to_string(),
        before: labels.first().cloned().unwrap_or_default(),
        after: labels.last().cloned().unwrap_or_default(),
        revisions: labels,
        coverage,
        coverage_reasons,
        findings,
        summaries,
    })
}

/// Build an exact analysis from one on-disk snapshot directory, mirroring
/// the planner flow used by `scan_paths_with_context` and `deslop-graph`.
/// The root is pinned to the directory itself so file paths are comparable
/// across the two revisions (`before/dir/x.py` and `after/dir/x.py` must
/// identify the same contract file). Public so the LSP can build its
/// configured base revision through the same entry point.
pub fn snapshot_analysis(root: &Path) -> Result<Arc<ProjectAnalysis>> {
    analysis_for(root)
}

fn analysis_for(root: &Path) -> Result<Arc<ProjectAnalysis>> {
    let invocation_base = std::env::current_dir().context("resolve invocation base")?;
    let planner = ProjectSnapshotPlanner::resolve(ProjectSnapshotRequest {
        invocation_base,
        root: RootSpec::Explicit(root.to_path_buf()),
        repository: RepositorySpec::Auto,
        scope: ScopeSpec::Requested(vec![root.to_path_buf()]),
        discovery: DiscoveryPolicy::LegacyRespectIgnore,
    })?;
    let built = planner.build()?;
    ProjectAnalysis::build(built.snapshot)
}

/// A contract-token domain. Detector families are one structural query over
/// different domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenDomain {
    /// Callee/attribute reference tokens.
    References,
    /// String-literal (schema field) tokens.
    Literals,
    /// Config-key read tokens.
    ConfigKeys,
}

impl TokenDomain {
    fn tokens(self, function: &ContractFunction) -> &BTreeSet<String> {
        match self {
            Self::References => &function.references,
            Self::Literals => &function.literals,
            Self::ConfigKeys => &function.config_keys,
        }
    }

    /// Module-level tokens of this domain (tokens outside any function).
    fn module_tokens(self, file: &FileContracts) -> Vec<&str> {
        match self {
            Self::References => Vec::new(),
            Self::Literals => file.module_literals.keys().map(String::as_str).collect(),
            Self::ConfigKeys => file.module_config_keys.keys().map(String::as_str).collect(),
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::References => "reference",
            Self::Literals => "schema field",
            Self::ConfigKeys => "config key",
        }
    }
}

/// The two migration detector families shipped in Phase 1. Both are the same
/// structural query — a changed token set with an unchanged dependent still
/// holding a removed token — over different token domains and roles.
#[derive(Debug, Clone, Copy)]
enum Family {
    OwnerMovedConsumerStale,
    ProducerVerifierSchemaDrift,
}

impl Family {
    fn rule(self) -> &'static str {
        match self {
            Self::OwnerMovedConsumerStale => rule_names::OWNER_MOVED_CONSUMER_STALE,
            Self::ProducerVerifierSchemaDrift => rule_names::PRODUCER_VERIFIER_SCHEMA_DRIFT,
        }
    }

    fn domain(self) -> TokenDomain {
        match self {
            Self::OwnerMovedConsumerStale => TokenDomain::References,
            Self::ProducerVerifierSchemaDrift => TokenDomain::Literals,
        }
    }

    fn roles(self) -> (ContractRole, ContractRole) {
        match self {
            Self::OwnerMovedConsumerStale => (ContractRole::Owner, ContractRole::Consumer),
            Self::ProducerVerifierSchemaDrift => (ContractRole::Producer, ContractRole::Verifier),
        }
    }

    fn edges(self) -> (ContractEdgeKind, ContractEdgeKind) {
        match self {
            Self::OwnerMovedConsumerStale => {
                (ContractEdgeKind::Produces, ContractEdgeKind::Consumes)
            }
            Self::ProducerVerifierSchemaDrift => {
                (ContractEdgeKind::Produces, ContractEdgeKind::Verifies)
            }
        }
    }

    fn suggested_verification(
        self,
        owner: &str,
        consumer: &str,
        added: &BTreeSet<String>,
    ) -> String {
        let added = added
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        match self {
            Self::OwnerMovedConsumerStale => format!(
                "decide whether `{consumer}` should follow `{owner}` to the new owner ({added}), \
                 or pin the old representation behind a tested compatibility invariant"
            ),
            Self::ProducerVerifierSchemaDrift => format!(
                "reconcile `{consumer}` with `{owner}`'s new schema ({added}): update the \
                 verifier's required fields or restore the removed ones"
            ),
        }
    }
}

/// The contract node a stale condition is attached to: a function, or a
/// module-level token (acceptance surfaces such as defaults dicts live
/// outside functions).
#[derive(Debug, Clone)]
enum Holder {
    Function {
        path: PathBuf,
        function: ContractFunction,
    },
    Module {
        path: PathBuf,
        span: deslop_core::Span,
        token: String,
    },
}

impl Holder {
    fn path(&self) -> &Path {
        match self {
            Self::Function { path, .. } | Self::Module { path, .. } => path,
        }
    }

    fn display_name(&self) -> String {
        match self {
            Self::Function { function, .. } => function.name.clone(),
            Self::Module { token, .. } => format!("module level ({token})"),
        }
    }

    /// Identity used to detect edits across revisions.
    fn identity(&self) -> String {
        match self {
            Self::Function { function, .. } => function.fingerprint.clone(),
            Self::Module { span, .. } => format!("{span:?}"),
        }
    }

    fn node_ref(&self, role: ContractRole) -> ContractNodeRef {
        let (span, fingerprint) = match self {
            Self::Function { function, .. } => (function.span, function.fingerprint.clone()),
            Self::Module { span, token, .. } => (*span, format!("module-token:{token}")),
        };
        ContractNodeRef {
            role,
            path: self.path().to_path_buf(),
            span,
            fingerprint,
            provider: FactProvider::TreeSitter,
            capability: CapabilityLevel::Complete,
        }
    }
}

/// One detected condition before it is assembled into a `RefactorDefect`.
/// Drafts keep the function-level facts persistence tracking needs; the
/// finding payload itself does not carry names outside `detail` strings.
#[derive(Debug, Clone)]
struct FindingDraft {
    rule: &'static str,
    pair_index: usize,
    before_label: String,
    after_label: String,
    /// Owner file path (config-inert findings use the holder's path).
    path: PathBuf,
    owner_before: Option<ContractFunction>,
    owner_after: Option<ContractFunction>,
    dependent: Holder,
    domain: TokenDomain,
    /// Removed tokens the dependent still holds (config-inert: the key).
    stale: BTreeSet<String>,
    /// Removed tokens that survive somewhere in the after revision.
    surviving: BTreeSet<String>,
    /// Tokens the owner gained (config-inert: live replacement keys).
    added: BTreeSet<String>,
    note: Option<String>,
    persistence: Persistence,
}

impl FindingDraft {
    fn owner_name(&self) -> Option<&str> {
        self.owner_after.as_ref().map(|owner| owner.name.as_str())
    }
}

/// Index a file's functions by name.
fn functions_by_name(file: &FileContracts) -> BTreeMap<&str, &ContractFunction> {
    file.functions
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect()
}

/// Reference-token leaves that indicate a partition axis was flattened or
/// merged. Classification evidence for scope-collapse nomination; the
/// semantic axis itself remains unproved and the finding requests a
/// metamorphic independence test.
const FLATTEN_LEAVES: &[&str] = &[
    "flatten",
    "ravel",
    "flat",
    "concat",
    "concatenate",
    "chain",
    "vcat",
    "hcat",
    "vstack",
    "hstack",
    "stack",
];

/// Reference-token leaves that indicate a lossy commit (argmax, threshold,
/// rounding). Classification evidence for provenance-loss nomination.
const LOSSY_LEAVES: &[&str] = &[
    "argmax",
    "argmin",
    "round",
    "floor",
    "ceil",
    "clip",
    "clamp",
    "quantize",
    "sign",
    "threshold",
    "onehot",
];

/// Observation-surface object names whose method calls classify a function
/// as a telemetry producer. Lexical classification is supporting evidence
/// only; the sufficient condition is the dependency-path split shown by the
/// contract facts.
const TELEMETRY_SURFACES: &[&str] = &[
    "metrics",
    "metric",
    "statsd",
    "prometheus",
    "telemetry",
    "logger",
    "log",
    "gauge",
    "counter",
    "histogram",
];

/// Publication-surface object names whose method calls classify a function
/// as an operational status/identity publisher.
const STATUS_SURFACES: &[&str] = &["status", "health", "heartbeat", "watchdog", "registry"];

/// Reflection/dynamic-dispatch callees that leave consumer resolution
/// incomplete. Their presence is a capability gap, never a clean result.
const DYNAMIC_LEAVES: &[&str] = &[
    "getattr",
    "setattr",
    "eval",
    "exec",
    "globals",
    "locals",
    "vars",
    "__import__",
    "getfield",
    "invokelatest",
];

/// The last dotted segment of a reference token.
fn token_leaf(token: &str) -> &str {
    token.rsplit('.').next().unwrap_or(token)
}

/// Whether a reference token names a type constructor or exception class by
/// the uppercase-initial convention (`Int`, `Float32`, `ValueError`,
/// `Dense`). Conversion and construction churn is not ownership migration;
/// this filter only narrows candidates, it never promotes one.
fn is_type_like(token: &str) -> bool {
    token_leaf(token)
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_uppercase())
}

/// Minimum normalized length for a hot-path duplication candidate. Shorter
/// calls (`one(T)`, `size(x, 1)`) are ubiquitous cheap accessors whose
/// nomination would drown review.
const HOT_PATH_MIN_CALL_LEN: usize = 24;

/// A reference token held by more than this many distinct functions in its
/// revision is a shared utility (`throw`, `zeros`, `log`), not an owned
/// representation a consumer could be left attached to. Narrowing only.
const SHARED_UTILITY_FUNCTIONS: usize = 3;

/// Distinct-function reference counts per token for one revision.
fn reference_popularity(rev: &RevisionContracts) -> BTreeMap<&str, usize> {
    let mut popularity: BTreeMap<&str, usize> = BTreeMap::new();
    for file in &rev.files {
        for function in &file.functions {
            for token in &function.references {
                *popularity.entry(token.as_str()).or_insert(0) += 1;
            }
        }
    }
    popularity
}

/// Whether a reference token is a method call on one of the named
/// observation surfaces (`metrics.gauge`, `app.status.publish`, ...).
fn observation_surface(token: &str, surfaces: &[&str]) -> bool {
    let Some((object, _leaf)) = token.rsplit_once('.') else {
        return false;
    };
    let object_leaf = token_leaf(object).to_ascii_lowercase();
    surfaces.contains(&object_leaf.as_str())
}

/// How a dependent function participates in the adoption chain. Families
/// are mutually exclusive per dependent so one stale edge is reported by
/// exactly one detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DependentClass {
    Test,
    Gate,
    StatusPublisher,
    TelemetryProducer,
    Consumer,
}

fn classify_dependent(path: &Path, function: &ContractFunction) -> DependentClass {
    if is_test_function(path, &function.name) {
        DependentClass::Test
    } else if function.assertions > 0 {
        DependentClass::Gate
    } else if function
        .references
        .iter()
        .any(|token| observation_surface(token, STATUS_SURFACES))
    {
        DependentClass::StatusPublisher
    } else if function
        .references
        .iter()
        .any(|token| observation_surface(token, TELEMETRY_SURFACES))
    {
        DependentClass::TelemetryProducer
    } else {
        DependentClass::Consumer
    }
}

/// One owner change extracted from a matched file pair: the owner both lost
/// and gained tokens, and at least one removed token survives elsewhere in
/// the after revision (a pure removal is a deletion and a vanished token is
/// a rename/rewrite; neither is a migration).
struct Migration<'f> {
    path: &'f Path,
    owner_before: &'f ContractFunction,
    owner_after: &'f ContractFunction,
    surviving: BTreeSet<String>,
    added: BTreeSet<String>,
}

/// Extract every owner migration in one token domain from a matched file
/// pair. `popularity_before`/`popularity_after` are revision-wide reference
/// counts used to exclude shared utilities from the reference domain.
fn find_migrations<'f>(
    domain: TokenDomain,
    file_before: &'f FileContracts,
    file_after: &'f FileContracts,
    popularity_before: &BTreeMap<&str, usize>,
    popularity_after: &BTreeMap<&str, usize>,
) -> Vec<Migration<'f>> {
    let before_by_name = functions_by_name(file_before);
    let after_by_name = functions_by_name(file_after);
    let global_after: BTreeSet<&str> = file_after
        .functions
        .iter()
        .flat_map(|function| domain.tokens(function).iter().map(String::as_str))
        .chain(domain.module_tokens(file_after))
        .collect();
    let mut migrations = Vec::new();
    for (name, owner_before) in &before_by_name {
        let Some(owner_after) = after_by_name.get(name) else {
            continue;
        };
        let tokens_before = domain.tokens(owner_before);
        let tokens_after = domain.tokens(owner_after);
        // In the reference domain, type-constructor/exception references
        // are conversion churn and revision-wide shared utilities are not
        // an owned representation; both narrow candidates only.
        let counts = |popularity: &BTreeMap<&str, usize>, token: &String| {
            domain != TokenDomain::References
                || (!is_type_like(token.as_str())
                    && popularity.get(token.as_str()).copied().unwrap_or(0)
                        <= SHARED_UTILITY_FUNCTIONS)
        };
        let removed: BTreeSet<String> = tokens_before
            .iter()
            .filter(|token| !tokens_after.contains(*token))
            .filter(|token| counts(popularity_before, token))
            .cloned()
            .collect();
        let added: BTreeSet<String> = tokens_after
            .iter()
            .filter(|token| !tokens_before.contains(*token))
            .filter(|token| counts(popularity_after, token))
            .cloned()
            .collect();
        if removed.is_empty() || added.is_empty() {
            continue;
        }
        let surviving: BTreeSet<String> = removed
            .iter()
            .filter(|token| global_after.contains(token.as_str()))
            .cloned()
            .collect();
        if surviving.is_empty() {
            continue;
        }
        migrations.push(Migration {
            path: &file_before.path,
            owner_before,
            owner_after,
            surviving,
            added,
        });
    }
    migrations
}

/// Run all detectors over one adjacent revision pair.
fn detect_pair(
    pair_index: usize,
    labels: &[String],
    rev_before: &RevisionContracts,
    rev_after: &RevisionContracts,
    drafts: &mut Vec<FindingDraft>,
) {
    let pair_start = drafts.len();
    let popularity_before = reference_popularity(rev_before);
    let popularity_after = reference_popularity(rev_after);
    for file_before in &rev_before.files {
        let Some(file_after) = rev_after
            .files
            .iter()
            .find(|file| file.path == file_before.path)
        else {
            continue;
        };
        let reference_migrations = find_migrations(
            TokenDomain::References,
            file_before,
            file_after,
            &popularity_before,
            &popularity_after,
        );
        let literal_migrations = find_migrations(
            TokenDomain::Literals,
            file_before,
            file_after,
            &popularity_before,
            &popularity_after,
        );
        detect_family(
            Family::OwnerMovedConsumerStale,
            &reference_migrations,
            pair_index,
            labels,
            file_before,
            file_after,
            drafts,
        );
        detect_family(
            Family::ProducerVerifierSchemaDrift,
            &literal_migrations,
            pair_index,
            labels,
            file_before,
            file_after,
            drafts,
        );
        detect_provenance_lost(
            &reference_migrations,
            pair_index,
            labels,
            file_before,
            file_after,
            drafts,
        );
        detect_reach_families(
            &reference_migrations,
            pair_index,
            labels,
            rev_before,
            rev_after,
            drafts,
        );
        detect_scope_collapse(pair_index, labels, file_before, file_after, drafts);
        detect_hot_path(pair_index, labels, file_before, file_after, drafts);
    }
    detect_config_inert(pair_index, labels, rev_before, rev_after, drafts);
    detect_test_oracle_lag(
        pair_index, labels, rev_before, rev_after, pair_start, drafts,
    );
}

/// Run one direct-dependent migration detector family over the migrations
/// extracted from one matched file pair.
fn detect_family(
    family: Family,
    migrations: &[Migration<'_>],
    pair_index: usize,
    labels: &[String],
    file_before: &FileContracts,
    file_after: &FileContracts,
    drafts: &mut Vec<FindingDraft>,
) {
    let domain = family.domain();
    let before_by_name = functions_by_name(file_before);
    let after_by_name = functions_by_name(file_after);
    for migration in migrations {
        // Stale dependents: matched by name, token set unchanged, still
        // holding at least one surviving removed token.
        for (dependent_name, dependent_before) in &before_by_name {
            if **dependent_name == migration.owner_before.name {
                continue;
            }
            let Some(dependent_after) = after_by_name.get(dependent_name) else {
                continue;
            };
            if domain.tokens(dependent_before) != domain.tokens(dependent_after) {
                continue;
            }
            // Dependents claimed by a more specific family are skipped here:
            // tests belong to `test-oracle-lag`; in the reference domain,
            // gates belong to `mechanism-live-gate-retired` and observation
            // surfaces to the telemetry/identity families.
            let class = classify_dependent(&file_after.path, dependent_after);
            if class == DependentClass::Test {
                continue;
            }
            if domain == TokenDomain::References && class != DependentClass::Consumer {
                continue;
            }
            // A dependent's reference to its own name is recursion, not an
            // attachment to the former owner's representation.
            let stale: BTreeSet<String> = migration
                .surviving
                .iter()
                .filter(|token| domain.tokens(dependent_before).contains(*token))
                .filter(|token| token_leaf(token) != *dependent_name)
                .cloned()
                .collect();
            if stale.is_empty() {
                continue;
            }
            drafts.push(FindingDraft {
                rule: family.rule(),
                pair_index,
                before_label: labels[pair_index].clone(),
                after_label: labels[pair_index + 1].clone(),
                path: migration.path.to_path_buf(),
                owner_before: Some(migration.owner_before.clone()),
                owner_after: Some(migration.owner_after.clone()),
                dependent: Holder::Function {
                    path: file_after.path.clone(),
                    function: (*dependent_after).clone(),
                },
                domain,
                stale,
                surviving: migration.surviving.clone(),
                added: migration.added.clone(),
                note: None,
                persistence: Persistence {
                    revisions: 1,
                    independent_edits: 0,
                },
            });
        }
    }
}

/// `accepted-config-inert`: a config key read in the before revision has no
/// read anywhere in the after revision, yet the acceptance surface (a
/// literal, often a defaults dict) still carries it. A key that vanished
/// from reads *and* literals is a clean removal and must not fire; a key
/// still read is live and must not fire. Unknown dynamic uses suppress
/// promotion rather than being treated as inert — they are simply outside
/// the extracted domains.
fn detect_config_inert(
    pair_index: usize,
    labels: &[String],
    rev_before: &RevisionContracts,
    rev_after: &RevisionContracts,
    drafts: &mut Vec<FindingDraft>,
) {
    let reads = |rev: &RevisionContracts| -> BTreeSet<String> {
        rev.files
            .iter()
            .flat_map(|file| {
                file.functions
                    .iter()
                    .flat_map(|function| function.config_keys.iter().cloned())
                    .chain(file.module_config_keys.keys().cloned())
            })
            .collect()
    };
    let before_reads = reads(rev_before);
    let after_reads = reads(rev_after);
    let replacements: BTreeSet<String> = after_reads.difference(&before_reads).cloned().collect();

    for key in before_reads.difference(&after_reads) {
        // The acceptance surface in the after revision: any function literal
        // or module-level literal still carrying the key.
        let mut holders: Vec<Holder> = Vec::new();
        for file in &rev_after.files {
            for function in &file.functions {
                if function.literals.contains(key) {
                    holders.push(Holder::Function {
                        path: file.path.clone(),
                        function: function.clone(),
                    });
                }
            }
            if let Some(span) = file.module_literals.get(key) {
                holders.push(Holder::Module {
                    path: file.path.clone(),
                    span: *span,
                    token: key.clone(),
                });
            }
        }
        if holders.is_empty() {
            continue;
        }
        let note = if replacements.is_empty() {
            None
        } else {
            Some(format!(
                "replacement key(s) live in the after revision: {}",
                replacements
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        };
        let dependent = holders.into_iter().next().expect("holders is non-empty");
        drafts.push(FindingDraft {
            rule: rule_names::ACCEPTED_CONFIG_INERT,
            pair_index,
            before_label: labels[pair_index].clone(),
            after_label: labels[pair_index + 1].clone(),
            path: dependent.path().to_path_buf(),
            owner_before: None,
            owner_after: None,
            dependent,
            domain: TokenDomain::ConfigKeys,
            stale: BTreeSet::from([key.clone()]),
            surviving: BTreeSet::from([key.clone()]),
            added: replacements.clone(),
            note,
            persistence: Persistence {
                revisions: 1,
                independent_edits: 0,
            },
        });
    }
}

/// `confidence-provenance-lost`: after an owner migration, a dependent that
/// was bound to the former evidence source now re-derives its output through
/// a lossy operation (argmax/round/threshold family) without following the
/// owner to the new evidence source. The dependent *changed* — the unchanged
/// case is `owner-moved-consumer-stale` — but changed to a reconstruction
/// instead of an adoption. Review candidate: the lossy-op set is
/// classification evidence, and whether the information loss matters needs a
/// behavioral oracle.
fn detect_provenance_lost(
    migrations: &[Migration<'_>],
    pair_index: usize,
    labels: &[String],
    file_before: &FileContracts,
    file_after: &FileContracts,
    drafts: &mut Vec<FindingDraft>,
) {
    let before_by_name = functions_by_name(file_before);
    let after_by_name = functions_by_name(file_after);
    for migration in migrations {
        for (name, dependent_before) in &before_by_name {
            if **name == migration.owner_before.name {
                continue;
            }
            let Some(dependent_after) = after_by_name.get(name) else {
                continue;
            };
            // The dependent was bound to the former evidence source...
            let formerly_bound: BTreeSet<String> = migration
                .surviving
                .iter()
                .filter(|token| dependent_before.references.contains(*token))
                .cloned()
                .collect();
            if formerly_bound.is_empty() {
                continue;
            }
            // ...changed in this pair...
            if dependent_before.fingerprint == dependent_after.fingerprint {
                continue;
            }
            // ...now applies a lossy operation...
            let lossy: BTreeSet<String> = dependent_after
                .references
                .iter()
                .filter(|token| LOSSY_LEAVES.contains(&token_leaf(token)))
                .cloned()
                .collect();
            if lossy.is_empty() {
                continue;
            }
            // ...and did not follow the owner to the new evidence source.
            // A dependent referencing any added token retained provenance:
            // counter-evidence, do not fire.
            if migration
                .added
                .iter()
                .any(|token| dependent_after.references.contains(token))
            {
                continue;
            }
            drafts.push(FindingDraft {
                rule: rule_names::CONFIDENCE_PROVENANCE_LOST,
                pair_index,
                before_label: labels[pair_index].clone(),
                after_label: labels[pair_index + 1].clone(),
                path: migration.path.to_path_buf(),
                owner_before: Some(migration.owner_before.clone()),
                owner_after: Some(migration.owner_after.clone()),
                dependent: Holder::Function {
                    path: file_after.path.clone(),
                    function: (*dependent_after).clone(),
                },
                domain: TokenDomain::References,
                stale: lossy,
                surviving: migration.surviving.clone(),
                added: migration.added.clone(),
                note: Some(format!(
                    "`{name}` was bound to {} before the migration",
                    formerly_bound
                        .iter()
                        .map(|token| format!("`{token}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                persistence: Persistence {
                    revisions: 1,
                    independent_edits: 0,
                },
            });
        }
    }
}

/// The syntactic reference closure of one function within a revision:
/// direct reference tokens plus the tokens of every function transitively
/// reachable by leaf-name expansion across files. Same-named functions are
/// merged by union, which is deliberately conservative: reaching the new
/// owner through *any* candidate suppresses a finding. Syntactic evidence,
/// never resolution proof.
fn reference_closure(rev: &RevisionContracts, start: &ContractFunction) -> BTreeSet<String> {
    let mut by_name: BTreeMap<&str, Vec<&ContractFunction>> = BTreeMap::new();
    for file in &rev.files {
        for function in &file.functions {
            by_name
                .entry(function.name.as_str())
                .or_default()
                .push(function);
        }
    }
    let mut closure: BTreeSet<String> = BTreeSet::new();
    let mut visited: BTreeSet<&str> = BTreeSet::from([start.name.as_str()]);
    let mut queue: Vec<&ContractFunction> = vec![start];
    while let Some(function) = queue.pop() {
        for token in &function.references {
            closure.insert(token.clone());
            let leaf = token_leaf(token);
            if visited.contains(leaf) {
                continue;
            }
            if let Some(callees) = by_name.get(leaf) {
                visited.insert(leaf);
                queue.extend(callees.iter().copied());
            }
        }
    }
    closure
}

/// The three dependency-path families over one migration: a gate
/// (`mechanism-live-gate-retired`), telemetry producer
/// (`telemetry-not-bound-to-claim`), or status/identity publisher
/// (`operational-identity-stale`) that is unchanged through the migration
/// and whose reference closure still reaches the former owner's surviving
/// tokens without reaching any token the owner gained. The dependency-path
/// split is the structural condition; the surface classification is
/// supporting evidence and is recorded as such.
fn detect_reach_families(
    migrations: &[Migration<'_>],
    pair_index: usize,
    labels: &[String],
    rev_before: &RevisionContracts,
    rev_after: &RevisionContracts,
    drafts: &mut Vec<FindingDraft>,
) {
    for migration in migrations {
        for file_after in &rev_after.files {
            for dependent_after in &file_after.functions {
                if dependent_after.name == migration.owner_before.name {
                    continue;
                }
                let rule = match classify_dependent(&file_after.path, dependent_after) {
                    DependentClass::Gate => rule_names::MECHANISM_LIVE_GATE_RETIRED,
                    DependentClass::TelemetryProducer => rule_names::TELEMETRY_NOT_BOUND_TO_CLAIM,
                    DependentClass::StatusPublisher => rule_names::OPERATIONAL_IDENTITY_STALE,
                    DependentClass::Test | DependentClass::Consumer => continue,
                };
                // Unchanged through the migration (exact bytes); a dependent
                // updated in the same revision is counter-evidence.
                let unchanged = rev_before
                    .files
                    .iter()
                    .find(|file| file.path == file_after.path)
                    .and_then(|file| {
                        file.functions
                            .iter()
                            .find(|before_fn| before_fn.name == dependent_after.name)
                    })
                    .is_some_and(|before_fn| before_fn.fingerprint == dependent_after.fingerprint);
                if !unchanged {
                    continue;
                }
                let closure = reference_closure(rev_after, dependent_after);
                // The graph must show the split: the dependency path reaches
                // the retired representation and does not reach the new one.
                // Self-references are recursion, not attachment.
                let stale: BTreeSet<String> = migration
                    .surviving
                    .iter()
                    .filter(|token| closure.contains(*token))
                    .filter(|token| token_leaf(token) != dependent_after.name)
                    .cloned()
                    .collect();
                if stale.is_empty() {
                    continue;
                }
                if migration.added.iter().any(|token| closure.contains(token)) {
                    continue;
                }
                drafts.push(FindingDraft {
                    rule,
                    pair_index,
                    before_label: labels[pair_index].clone(),
                    after_label: labels[pair_index + 1].clone(),
                    path: migration.path.to_path_buf(),
                    owner_before: Some(migration.owner_before.clone()),
                    owner_after: Some(migration.owner_after.clone()),
                    dependent: Holder::Function {
                        path: file_after.path.clone(),
                        function: dependent_after.clone(),
                    },
                    domain: TokenDomain::References,
                    stale,
                    surviving: migration.surviving.clone(),
                    added: migration.added.clone(),
                    note: None,
                    persistence: Persistence {
                        revisions: 1,
                        independent_edits: 0,
                    },
                });
            }
        }
    }
}

/// `scope-collapse-after-refactor`: a function that iterated a partition
/// axis lost loop structure while gaining a flatten/concatenate-family
/// reference. Tree-sitter cannot prove the semantic axis, so the finding
/// requests a metamorphic independence test instead of asserting a defect.
fn detect_scope_collapse(
    pair_index: usize,
    labels: &[String],
    file_before: &FileContracts,
    file_after: &FileContracts,
    drafts: &mut Vec<FindingDraft>,
) {
    let after_by_name = functions_by_name(file_after);
    for function_before in &file_before.functions {
        let Some(function_after) = after_by_name.get(function_before.name.as_str()) else {
            continue;
        };
        if function_after.loops >= function_before.loops {
            continue;
        }
        let gained_flatten: BTreeSet<String> = function_after
            .references
            .iter()
            .filter(|token| {
                !function_before.references.contains(*token)
                    && FLATTEN_LEAVES.contains(&token_leaf(token))
            })
            .cloned()
            .collect();
        if gained_flatten.is_empty() {
            continue;
        }
        drafts.push(FindingDraft {
            rule: rule_names::SCOPE_COLLAPSE_AFTER_REFACTOR,
            pair_index,
            before_label: labels[pair_index].clone(),
            after_label: labels[pair_index + 1].clone(),
            path: file_before.path.clone(),
            owner_before: Some(function_before.clone()),
            owner_after: Some((*function_after).clone()),
            dependent: Holder::Function {
                path: file_after.path.clone(),
                function: (*function_after).clone(),
            },
            domain: TokenDomain::References,
            stale: gained_flatten.clone(),
            // The oracle-lag join uses `surviving`: a test still exercising
            // the collapsed function has no multi-partition oracle.
            surviving: BTreeSet::from([function_before.name.clone()]),
            added: gained_flatten,
            note: Some(format!(
                "loop structure fell from {} to {}",
                function_before.loops, function_after.loops
            )),
            persistence: Persistence {
                revisions: 1,
                independent_edits: 0,
            },
        });
    }
}

/// Whether a normalized call text is a nominable duplication candidate: it
/// carries at least one argument (a shared reachable input), composes at
/// least one nested call (composite work rather than a bare accessor), and
/// is at least [`HOT_PATH_MIN_CALL_LEN`] bytes long.
fn call_is_nominable(text: &str) -> bool {
    if text.starts_with("blake3:") {
        // Digested texts exceed the storage limit and necessarily satisfy
        // every size-based bound.
        return true;
    }
    if text.len() < HOT_PATH_MIN_CALL_LEN {
        return false;
    }
    let Some(open) = text.find('(') else {
        return false;
    };
    let Some(close) = text.rfind(')') else {
        return false;
    };
    // A repeated constructor builds two distinct objects; reusing one is
    // not a safe suggestion, so type-like callees are not nominable.
    if is_type_like(text[..open].trim()) {
        return false;
    }
    close > open + 1 && !text[open + 1..close].trim().is_empty() && text[open + 1..].contains('(')
}

/// `hot-path-work-duplicated`: a refactor introduced two structurally
/// equivalent calls with arguments inside one function where the before
/// revision had at most one. Cost and safe reuse remain unproved without
/// profiling or effect facts, so the family is review-only with an explicit
/// gap.
fn detect_hot_path(
    pair_index: usize,
    labels: &[String],
    file_before: &FileContracts,
    file_after: &FileContracts,
    drafts: &mut Vec<FindingDraft>,
) {
    let before_by_name = functions_by_name(file_before);
    for function_after in &file_after.functions {
        let Some(function_before) = before_by_name.get(function_after.name.as_str()) else {
            continue;
        };
        let duplicated: Vec<&String> = function_after
            .call_texts
            .iter()
            .filter(|(text, count)| {
                **count >= 2
                    && function_before.call_texts.get(*text).copied().unwrap_or(0) <= 1
                    && call_is_nominable(text)
            })
            .map(|(text, _)| text)
            .collect();
        // Report only maximal duplicated texts: a duplicated subexpression
        // of a duplicated call is the same duplication, not a second one.
        for text in &duplicated {
            if duplicated
                .iter()
                .any(|other| *other != *text && other.contains(*text))
            {
                continue;
            }
            drafts.push(FindingDraft {
                rule: rule_names::HOT_PATH_WORK_DUPLICATED,
                pair_index,
                before_label: labels[pair_index].clone(),
                after_label: labels[pair_index + 1].clone(),
                path: file_after.path.clone(),
                owner_before: None,
                owner_after: None,
                dependent: Holder::Function {
                    path: file_after.path.clone(),
                    function: function_after.clone(),
                },
                domain: TokenDomain::References,
                stale: BTreeSet::from([(*text).clone()]),
                surviving: BTreeSet::from([(*text).clone()]),
                added: BTreeSet::new(),
                note: None,
                persistence: Persistence {
                    revisions: 1,
                    independent_edits: 0,
                },
            });
        }
    }
}

/// Coverage reasons for dynamic/reflective access anywhere in the window:
/// consumer resolution is incomplete, so the analysis must not present a
/// clean result.
fn dynamic_access_reasons(history: &ContractChangeHistory) -> Vec<String> {
    let mut reasons = Vec::new();
    for rev in &history.revisions {
        for file in &rev.files {
            for function in &file.functions {
                let dynamic: Vec<&str> = function
                    .references
                    .iter()
                    .filter(|token| DYNAMIC_LEAVES.contains(&token_leaf(token)))
                    .map(String::as_str)
                    .collect();
                if !dynamic.is_empty() {
                    reasons.push(format!(
                        "{}: `{}` uses dynamic access ({}) in revision {}; consumer \
                         resolution incomplete",
                        file.path.display(),
                        function.name,
                        dynamic.join(", "),
                        rev.revision
                    ));
                }
            }
        }
    }
    reasons.sort();
    reasons.dedup();
    reasons
}

/// Whether a function looks like a test: its name starts with `test_`, or
/// its file is a `test_*`/`*_test` module or lives under a `test`/`tests`
/// directory. Classification evidence only.
fn is_test_function(path: &Path, name: &str) -> bool {
    if name.starts_with("test_") {
        return true;
    }
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if stem.starts_with("test_") || stem.ends_with("_test") {
        return true;
    }
    path.components().any(|component| {
        let text = component.as_os_str().to_str().unwrap_or_default();
        text == "test" || text == "tests"
    })
}

/// `test-oracle-lag`: a migration fired in this pair while an unchanged test
/// still exercises the former representation. Reports what remains unproved
/// (no oracle covers the new owner); a test that changed in the same revision
/// is counter-evidence and does not fire.
fn detect_test_oracle_lag(
    pair_index: usize,
    labels: &[String],
    rev_before: &RevisionContracts,
    rev_after: &RevisionContracts,
    pair_start: usize,
    drafts: &mut Vec<FindingDraft>,
) {
    // Migrations detected in this pair so far (owned copies: drafts grows below).
    let migrations: Vec<FindingDraft> = drafts[pair_start..]
        .iter()
        .filter(|draft| {
            draft.rule == rule_names::OWNER_MOVED_CONSUMER_STALE
                || draft.rule == rule_names::PRODUCER_VERIFIER_SCHEMA_DRIFT
                || draft.rule == rule_names::SCOPE_COLLAPSE_AFTER_REFACTOR
        })
        .cloned()
        .collect();
    for migration in migrations {
        // For a scope collapse, the unproved contract is partition
        // independence: any unchanged test still exercising the collapsed
        // function (its name is `surviving`) is a singleton oracle. For
        // owner/schema migrations the join is on the surviving tokens.
        let scope_collapse = migration.rule == rule_names::SCOPE_COLLAPSE_AFTER_REFACTOR;
        for file in &rev_after.files {
            for function in &file.functions {
                if !is_test_function(&file.path, &function.name) {
                    continue;
                }
                // Unchanged since the before revision (exact bytes).
                let unchanged = rev_before
                    .files
                    .iter()
                    .find(|candidate| candidate.path == file.path)
                    .and_then(|candidate| {
                        candidate
                            .functions
                            .iter()
                            .find(|before_fn| before_fn.name == function.name)
                    })
                    .is_some_and(|before_fn| before_fn.fingerprint == function.fingerprint);
                if !unchanged {
                    continue;
                }
                let held: &BTreeSet<String> = if scope_collapse {
                    &function.references
                } else {
                    migration.domain.tokens(function)
                };
                let stale: BTreeSet<String> = migration
                    .surviving
                    .iter()
                    .filter(|token| {
                        held.contains(*token)
                            || (scope_collapse
                                && held
                                    .iter()
                                    .any(|reference| token_leaf(reference) == token.as_str()))
                    })
                    .cloned()
                    .collect();
                if stale.is_empty() {
                    continue;
                }
                let note = scope_collapse.then(|| {
                    format!(
                        "partition independence has no multi-partition oracle: \
                         `{}` exercises the collapsed function unchanged",
                        function.name
                    )
                });
                drafts.push(FindingDraft {
                    rule: rule_names::TEST_ORACLE_LAG,
                    pair_index,
                    before_label: labels[pair_index].clone(),
                    after_label: labels[pair_index + 1].clone(),
                    path: migration.path.clone(),
                    owner_before: migration.owner_before.clone(),
                    owner_after: migration.owner_after.clone(),
                    dependent: Holder::Function {
                        path: file.path.clone(),
                        function: function.clone(),
                    },
                    domain: migration.domain,
                    stale,
                    surviving: migration.surviving.clone(),
                    added: migration.added.clone(),
                    note,
                    persistence: Persistence {
                        revisions: 1,
                        independent_edits: 0,
                    },
                });
            }
        }
    }
}

/// The holder in one revision matching `holder` by path and name/token.
fn locate(rev: &RevisionContracts, holder: &Holder) -> Option<Holder> {
    match holder {
        Holder::Function { path, function } => rev
            .files
            .iter()
            .find(|file| file.path == *path)?
            .functions
            .iter()
            .find(|candidate| candidate.name == function.name)
            .map(|candidate| Holder::Function {
                path: path.clone(),
                function: candidate.clone(),
            }),
        Holder::Module { path, token, .. } => rev
            .files
            .iter()
            .find(|file| file.path == *path)
            .and_then(|file| {
                file.module_literals.get(token).map(|span| Holder::Module {
                    path: path.clone(),
                    span: *span,
                    token: token.clone(),
                })
            }),
    }
}

/// Whether the holder's identity changed between two revisions. Appearing or
/// disappearing counts as a change.
fn changed_between(before: &RevisionContracts, after: &RevisionContracts, holder: &Holder) -> bool {
    match (locate(before, holder), locate(after, holder)) {
        (Some(left), Some(right)) => left.identity() != right.identity(),
        (None, None) => false,
        _ => true,
    }
}

/// Whether the draft's stale condition still holds in one revision.
fn stale_holds(draft: &FindingDraft, rev: &RevisionContracts) -> bool {
    if draft.rule == rule_names::ACCEPTED_CONFIG_INERT {
        let Some(key) = draft.stale.iter().next() else {
            return false;
        };
        let accepted = rev.files.iter().any(|file| {
            file.functions
                .iter()
                .any(|function| function.literals.contains(key))
                || file.module_literals.contains_key(key)
        });
        let read = rev.files.iter().any(|file| {
            file.functions
                .iter()
                .any(|function| function.config_keys.contains(key))
                || file.module_config_keys.contains_key(key)
        });
        return accepted && !read;
    }
    let Some(holder) = locate(rev, &draft.dependent) else {
        return false;
    };
    match draft.rule {
        rule_names::MECHANISM_LIVE_GATE_RETIRED
        | rule_names::TELEMETRY_NOT_BOUND_TO_CLAIM
        | rule_names::OPERATIONAL_IDENTITY_STALE => {
            // The dependency-path split must persist: the closure still
            // reaches a stale token and none of the owner's gained tokens.
            let Holder::Function { function, .. } = &holder else {
                return false;
            };
            let closure = reference_closure(rev, function);
            draft.stale.iter().any(|token| closure.contains(token))
                && !draft.added.iter().any(|token| closure.contains(token))
        }
        rule_names::SCOPE_COLLAPSE_AFTER_REFACTOR => {
            let Holder::Function { function, .. } = &holder else {
                return false;
            };
            let before_loops = draft
                .owner_before
                .as_ref()
                .map(|owner| owner.loops)
                .unwrap_or(0);
            function.loops < before_loops
                && draft
                    .stale
                    .iter()
                    .any(|token| function.references.contains(token))
        }
        rule_names::HOT_PATH_WORK_DUPLICATED => {
            let Holder::Function { function, .. } = &holder else {
                return false;
            };
            draft
                .stale
                .iter()
                .any(|text| function.call_texts.get(text).copied().unwrap_or(0) >= 2)
        }
        rule_names::CONFIDENCE_PROVENANCE_LOST => {
            let Holder::Function { function, .. } = &holder else {
                return false;
            };
            draft
                .stale
                .iter()
                .any(|token| function.references.contains(token))
                && !draft
                    .added
                    .iter()
                    .any(|token| function.references.contains(token))
        }
        _ => match &holder {
            Holder::Function { function, .. } => draft
                .stale
                .iter()
                .any(|token| draft.domain.tokens(function).contains(token)),
            Holder::Module { token, .. } => draft.stale.contains(token),
        },
    }
}

/// Fill in `draft.persistence` by tracking the stale condition across the
/// revisions after the pair that detected it: how many revisions the stale
/// edge survives, and how often the owner and the stale dependent were
/// edited independently (co-change triage evidence from contract
/// fingerprints).
fn track_persistence(history: &ContractChangeHistory, draft: &mut FindingDraft) {
    let window = history.revisions.len();
    let mut survived = 1_u64;
    let mut independent = 0_u64;
    for index in draft.pair_index + 1..window.saturating_sub(1) {
        let rev = &history.revisions[index + 1];
        if !stale_holds(draft, rev) {
            break;
        }
        survived += 1;
        let dependent_changed = changed_between(&history.revisions[index], rev, &draft.dependent);
        let owner_changed = match &draft.owner_after {
            Some(owner) => {
                let holder = Holder::Function {
                    path: draft.path.clone(),
                    function: owner.clone(),
                };
                changed_between(&history.revisions[index], rev, &holder)
            }
            None => false,
        };
        if dependent_changed ^ owner_changed {
            independent += 1;
        }
    }
    draft.persistence = Persistence {
        revisions: survived,
        independent_edits: independent,
    };
}

/// The adoption-chain stage a rule's dependent represents, for summary
/// findings.
fn chain_stage(rule: &str) -> Option<(ContractEdgeKind, ContractRole)> {
    match rule {
        rule_names::OWNER_MOVED_CONSUMER_STALE => {
            Some((ContractEdgeKind::Consumes, ContractRole::Consumer))
        }
        rule_names::PRODUCER_VERIFIER_SCHEMA_DRIFT => {
            Some((ContractEdgeKind::Verifies, ContractRole::Verifier))
        }
        rule_names::TEST_ORACLE_LAG => {
            Some((ContractEdgeKind::Exercises, ContractRole::TestEntryPoint))
        }
        rule_names::MECHANISM_LIVE_GATE_RETIRED => {
            Some((ContractEdgeKind::Verifies, ContractRole::Verifier))
        }
        rule_names::TELEMETRY_NOT_BOUND_TO_CLAIM => {
            Some((ContractEdgeKind::Observes, ContractRole::TelemetrySurface))
        }
        rule_names::OPERATIONAL_IDENTITY_STALE => {
            Some((ContractEdgeKind::Publishes, ContractRole::RuntimeIdentity))
        }
        rule_names::CONFIDENCE_PROVENANCE_LOST => {
            Some((ContractEdgeKind::Consumes, ContractRole::Consumer))
        }
        _ => None,
    }
}

/// The two-revision coverage gap: accumulation evidence requires a longer
/// window.
fn window_gap() -> CoverageGap {
    CoverageGap {
        provider: FactProvider::VcsHistory,
        capability: CapabilityLevel::Unknown,
        reason: "two-revision window; accumulation, rename/move history, and co-change \
                 evidence not analyzed"
            .to_string(),
    }
}

/// Assemble one `deslop.refactor-defect/1` finding from a draft. Every
/// finding is validated against the core invariants before it leaves the
/// analyzer.
fn build_finding(draft: &FindingDraft, window: usize) -> RefactorDefect {
    let mut coverage_gaps = Vec::new();
    if window == 2 {
        coverage_gaps.push(window_gap());
    }
    let stale_list = draft
        .stale
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let added_list = draft
        .added
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let domain = draft.domain.noun();
    let dependent_name = draft.dependent.display_name();

    let owner = draft
        .owner_before
        .as_ref()
        .zip(draft.owner_after.as_ref())
        .map(|(before, after)| OwnerMigration {
            before: ContractNodeRef {
                role: owner_role(draft.rule),
                path: draft.path.clone(),
                span: before.span,
                fingerprint: before.fingerprint.clone(),
                provider: FactProvider::TreeSitter,
                capability: CapabilityLevel::Complete,
            },
            after: ContractNodeRef {
                role: owner_role(draft.rule),
                path: draft.path.clone(),
                span: after.span,
                fingerprint: after.fingerprint.clone(),
                provider: FactProvider::TreeSitter,
                capability: CapabilityLevel::Complete,
            },
            match_evidence: vec![EntityMatchEvidence {
                kind: EntityMatchKind::PathSpan,
                detail: format!(
                    "function `{}` matched by name within {}",
                    after.name,
                    draft.path.display()
                ),
            }],
        });

    let (stale_edges, causal_path, evidence, suggested_verification, mut priority_inputs) =
        match draft.rule {
            rule_names::OWNER_MOVED_CONSUMER_STALE | rule_names::PRODUCER_VERIFIER_SCHEMA_DRIFT => {
                let family = if draft.rule == rule_names::OWNER_MOVED_CONSUMER_STALE {
                    Family::OwnerMovedConsumerStale
                } else {
                    Family::ProducerVerifierSchemaDrift
                };
                let (owner_role, dependent_role) = family.roles();
                let (owner_edge, dependent_edge) = family.edges();
                let owner_after = draft
                    .owner_after
                    .as_ref()
                    .expect("migration draft has owner");
                let owner_node = ContractNodeRef {
                    role: owner_role,
                    path: draft.path.clone(),
                    span: owner_after.span,
                    fingerprint: owner_after.fingerprint.clone(),
                    provider: FactProvider::TreeSitter,
                    capability: CapabilityLevel::Complete,
                };
                let dependent_node = draft.dependent.node_ref(dependent_role);
                (
                    draft
                        .stale
                        .iter()
                        .map(|token| ContractStep {
                            token: Some(token.clone()),
                            edge: dependent_edge,
                            node: dependent_node.clone(),
                            detail: format!(
                                "`{dependent_name}` still {domain}s `{token}` from the former owner"
                            ),
                        })
                        .collect::<Vec<_>>(),
                    vec![
                        ContractStep {
                            token: None,
                            edge: owner_edge,
                            node: owner_node.clone(),
                            detail: format!(
                                "`{}` moved from {stale_list} to {added_list}",
                                owner_after.name
                            ),
                        },
                        ContractStep {
                            token: None,
                            edge: dependent_edge,
                            node: dependent_node.clone(),
                            detail: format!(
                                "`{dependent_name}` is unchanged and still {domain}s {stale_list}"
                            ),
                        },
                    ],
                    vec![
                        EvidenceItem {
                            provider: FactProvider::TreeSitter,
                            detail: format!(
                                "`{}` {domain} set changed between revisions (removed {stale_list}, added {added_list})",
                                owner_after.name
                            ),
                            node: Some(owner_node),
                        },
                        EvidenceItem {
                            provider: FactProvider::TreeSitter,
                            detail: format!(
                                "`{dependent_name}` {domain} set is unchanged and contains {stale_list}"
                            ),
                            node: Some(dependent_node),
                        },
                    ],
                    family.suggested_verification(&owner_after.name, &dependent_name, &draft.added),
                    BTreeMap::from([
                        ("owner-change".to_string(), 1),
                        ("stale-edges".to_string(), draft.stale.len() as i64),
                    ]),
                )
            }
            rule_names::TEST_ORACLE_LAG => {
                let owner_after = draft.owner_after.as_ref().expect("oracle draft has owner");
                let test_node = draft.dependent.node_ref(ContractRole::TestEntryPoint);
                (
                    draft
                        .stale
                        .iter()
                        .map(|token| ContractStep {
                            token: Some(token.clone()),
                            edge: ContractEdgeKind::Exercises,
                            node: test_node.clone(),
                            detail: format!(
                                "test `{dependent_name}` is unchanged and still exercises `{token}` \
                                 from the former owner"
                            ),
                        })
                        .collect::<Vec<_>>(),
                    vec![
                        ContractStep {
                            token: None,
                            edge: ContractEdgeKind::Produces,
                            node: draft.dependent.node_ref(ContractRole::Owner),
                            detail: format!(
                                "`{}` moved from {stale_list} to {added_list}",
                                owner_after.name
                            ),
                        },
                        ContractStep {
                            token: None,
                            edge: ContractEdgeKind::Exercises,
                            node: test_node.clone(),
                            detail: format!(
                                "`{dependent_name}` is unchanged through the migration and still \
                                 {domain}s {stale_list}"
                            ),
                        },
                    ],
                    vec![
                        EvidenceItem {
                            provider: FactProvider::TreeSitter,
                            detail: format!(
                                "production contract changed while test `{dependent_name}` kept \
                                 identical bytes and still {domain}s {stale_list}"
                            ),
                            node: Some(test_node),
                        },
                    ],
                    match &draft.note {
                        Some(note) => format!(
                            "{note}: add a multi-partition metamorphic oracle (hold one \
                             partition fixed, vary its companions, compare the fixed \
                             partition's result)"
                        ),
                        None => format!(
                            "no oracle covers `{}`'s new representation ({added_list}): update \
                             `{dependent_name}` to exercise it, or pin the former representation \
                             as an explicit compatibility test",
                            owner_after.name
                        ),
                    },
                    BTreeMap::from([
                        ("owner-change".to_string(), 1),
                        ("stale-edges".to_string(), draft.stale.len() as i64),
                        ("missing-oracle".to_string(), 1),
                    ]),
                )
            }
            rule_names::MECHANISM_LIVE_GATE_RETIRED
            | rule_names::TELEMETRY_NOT_BOUND_TO_CLAIM
            | rule_names::OPERATIONAL_IDENTITY_STALE => {
                let (dependent_role, dependent_edge, surface_noun) = match draft.rule {
                    rule_names::MECHANISM_LIVE_GATE_RETIRED => {
                        (ContractRole::Verifier, ContractEdgeKind::Verifies, "gate")
                    }
                    rule_names::TELEMETRY_NOT_BOUND_TO_CLAIM => (
                        ContractRole::TelemetrySurface,
                        ContractEdgeKind::Observes,
                        "telemetry producer",
                    ),
                    _ => (
                        ContractRole::RuntimeIdentity,
                        ContractEdgeKind::Publishes,
                        "status publisher",
                    ),
                };
                let owner_after = draft.owner_after.as_ref().expect("reach draft has owner");
                let owner_node = ContractNodeRef {
                    role: ContractRole::Owner,
                    path: draft.path.clone(),
                    span: owner_after.span,
                    fingerprint: owner_after.fingerprint.clone(),
                    provider: FactProvider::TreeSitter,
                    capability: CapabilityLevel::Complete,
                };
                let dependent_node = draft.dependent.node_ref(dependent_role);
                let mut evidence = vec![
                    EvidenceItem {
                        provider: FactProvider::TreeSitter,
                        detail: format!(
                            "`{}` moved from {stale_list} to {added_list}",
                            owner_after.name
                        ),
                        node: Some(owner_node.clone()),
                    },
                    EvidenceItem {
                        provider: FactProvider::TreeSitter,
                        detail: format!(
                            "`{dependent_name}` is unchanged and its reference closure \
                             reaches {stale_list} without reaching {added_list} (syntactic \
                             leaf-name expansion; not resolution proof)"
                        ),
                        node: Some(dependent_node.clone()),
                    },
                ];
                if draft.rule != rule_names::MECHANISM_LIVE_GATE_RETIRED {
                    evidence.push(EvidenceItem {
                        provider: FactProvider::TreeSitter,
                        detail: format!(
                            "`{dependent_name}` is classified as a {surface_noun} by its \
                             observation-surface calls; the classification is lexical \
                             supporting evidence, not the firing condition"
                        ),
                        node: Some(dependent_node.clone()),
                    });
                    coverage_gaps.push(CoverageGap {
                        provider: FactProvider::TreeSitter,
                        capability: CapabilityLevel::Partial,
                        reason: format!(
                            "the {surface_noun} surface classification is lexical; the \
                             semantic binding between the surface and the claimed mechanism \
                             is unproved"
                        ),
                    });
                }
                (
                    draft
                        .stale
                        .iter()
                        .map(|token| ContractStep {
                            token: Some(token.clone()),
                            edge: dependent_edge,
                            node: dependent_node.clone(),
                            detail: format!(
                                "`{dependent_name}`'s dependency path still terminates at \
                                 `{token}` from the former owner"
                            ),
                        })
                        .collect::<Vec<_>>(),
                    vec![
                        ContractStep {
                            token: None,
                            edge: ContractEdgeKind::Produces,
                            node: owner_node.clone(),
                            detail: format!(
                                "`{}` moved from {stale_list} to {added_list}",
                                owner_after.name
                            ),
                        },
                        ContractStep {
                            token: None,
                            edge: dependent_edge,
                            node: dependent_node.clone(),
                            detail: format!(
                                "the {surface_noun} `{dependent_name}` is unchanged and its \
                                 dependency path terminates at {stale_list}, not {added_list}"
                            ),
                        },
                    ],
                    evidence,
                    match draft.rule {
                        rule_names::MECHANISM_LIVE_GATE_RETIRED => format!(
                            "`{dependent_name}` can certify a mechanism it no longer governs: \
                             point the gate's dependency path at {added_list}, or record why \
                             the retired value {stale_list} still gates the release"
                        ),
                        rule_names::TELEMETRY_NOT_BOUND_TO_CLAIM => format!(
                            "`{dependent_name}` observes {stale_list}, which no longer \
                             carries the claimed mechanism ({added_list}): rebind the metric \
                             to the live mechanism or rename the claim it reports"
                        ),
                        _ => format!(
                            "`{dependent_name}` still publishes the retired identity \
                             {stale_list}: publish the live identity ({added_list}) or \
                             record the compatibility window explicitly"
                        ),
                    },
                    BTreeMap::from([
                        ("owner-change".to_string(), 1),
                        ("stale-edges".to_string(), draft.stale.len() as i64),
                    ]),
                )
            }
            rule_names::SCOPE_COLLAPSE_AFTER_REFACTOR => {
                let owner_after = draft.owner_after.as_ref().expect("scope draft has owner");
                let owner_node = ContractNodeRef {
                    role: ContractRole::Owner,
                    path: draft.path.clone(),
                    span: owner_after.span,
                    fingerprint: owner_after.fingerprint.clone(),
                    provider: FactProvider::TreeSitter,
                    capability: CapabilityLevel::Complete,
                };
                coverage_gaps.push(CoverageGap {
                    provider: FactProvider::TreeSitter,
                    capability: CapabilityLevel::Partial,
                    reason: "the semantic axis of the removed loop (request, document, \
                             tenant, batch member, ...) cannot be proved from syntax"
                        .to_string(),
                });
                let note = draft.note.clone().unwrap_or_default();
                (
                    vec![ContractStep {
                        token: None,
                        edge: ContractEdgeKind::Transforms,
                        node: owner_node.clone(),
                        detail: format!(
                            "`{}` now merges across the former partition boundary via \
                             {stale_list}",
                            owner_after.name
                        ),
                    }],
                    vec![
                        ContractStep {
                            token: None,
                            edge: ContractEdgeKind::Transforms,
                            node: owner_node.clone(),
                            detail: format!("`{}`: {note}", owner_after.name),
                        },
                        ContractStep {
                            token: None,
                            edge: ContractEdgeKind::Consumes,
                            node: owner_node.clone(),
                            detail: format!(
                                "flatten/concatenate-family reference(s) gained: {stale_list}"
                            ),
                        },
                    ],
                    vec![EvidenceItem {
                        provider: FactProvider::TreeSitter,
                        detail: format!(
                            "`{}` lost loop structure ({note}) while gaining {stale_list}",
                            owner_after.name
                        ),
                        node: Some(owner_node),
                    }],
                    format!(
                        "run a metamorphic independence test on `{}`: hold one partition \
                         fixed, vary its companions, and compare the fixed partition's \
                         result before and after",
                        owner_after.name
                    ),
                    BTreeMap::from([
                        ("owner-change".to_string(), 1),
                        ("stale-edges".to_string(), draft.stale.len() as i64),
                    ]),
                )
            }
            rule_names::CONFIDENCE_PROVENANCE_LOST => {
                let owner_after = draft
                    .owner_after
                    .as_ref()
                    .expect("provenance draft has owner");
                let owner_node = ContractNodeRef {
                    role: ContractRole::Owner,
                    path: draft.path.clone(),
                    span: owner_after.span,
                    fingerprint: owner_after.fingerprint.clone(),
                    provider: FactProvider::TreeSitter,
                    capability: CapabilityLevel::Complete,
                };
                let dependent_node = draft.dependent.node_ref(ContractRole::Consumer);
                let bound_note = draft.note.clone().unwrap_or_default();
                coverage_gaps.push(CoverageGap {
                    provider: FactProvider::TreeSitter,
                    capability: CapabilityLevel::Partial,
                    reason: "whether the information loss matters needs a behavioral \
                             oracle; the lossy-operation set is classification evidence"
                        .to_string(),
                });
                (
                    vec![ContractStep {
                        token: None,
                        edge: ContractEdgeKind::Transforms,
                        node: dependent_node.clone(),
                        detail: format!(
                            "`{dependent_name}` re-derives its output through {stale_list} \
                             instead of the evidence that governed the decision"
                        ),
                    }],
                    vec![
                        ContractStep {
                            token: None,
                            edge: ContractEdgeKind::Produces,
                            node: owner_node.clone(),
                            detail: format!("`{}` now decides from {added_list}", owner_after.name),
                        },
                        ContractStep {
                            token: None,
                            edge: ContractEdgeKind::Transforms,
                            node: dependent_node.clone(),
                            detail: format!(
                                "`{dependent_name}` ({bound_note}) now reconstructs through \
                                 {stale_list} and does not reach {added_list}"
                            ),
                        },
                    ],
                    vec![
                        EvidenceItem {
                            provider: FactProvider::TreeSitter,
                            detail: format!(
                                "`{}` decision evidence moved to {added_list}",
                                owner_after.name
                            ),
                            node: Some(owner_node),
                        },
                        EvidenceItem {
                            provider: FactProvider::TreeSitter,
                            detail: format!(
                                "`{dependent_name}` changed in the same pair and now applies \
                                 lossy operation(s) {stale_list}; {bound_note}"
                            ),
                            node: Some(dependent_node),
                        },
                    ],
                    format!(
                        "compare `{dependent_name}`'s public output against the evidence \
                         that governed the decision ({added_list}); retain that evidence or \
                         document the lossy reconstruction as intentional"
                    ),
                    BTreeMap::from([
                        ("owner-change".to_string(), 1),
                        ("stale-edges".to_string(), draft.stale.len() as i64),
                    ]),
                )
            }
            rule_names::HOT_PATH_WORK_DUPLICATED => {
                let call = draft
                    .stale
                    .iter()
                    .next()
                    .expect("hot-path draft has a call");
                let dependent_node = draft.dependent.node_ref(ContractRole::Consumer);
                coverage_gaps.push(CoverageGap {
                    provider: FactProvider::Runtime,
                    capability: CapabilityLevel::Unknown,
                    reason: "cost and safe reuse are unproved without profiling or effect \
                             facts"
                        .to_string(),
                });
                (
                    vec![ContractStep {
                        token: Some(call.clone()),
                        edge: ContractEdgeKind::Consumes,
                        node: dependent_node.clone(),
                        detail: format!(
                            "`{dependent_name}` now evaluates `{call}` more than once on one \
                             reachable path"
                        ),
                    }],
                    vec![ContractStep {
                        token: None,
                        edge: ContractEdgeKind::Transforms,
                        node: dependent_node.clone(),
                        detail: format!(
                            "this pair introduced a second structurally equivalent \
                                 evaluation of `{call}`"
                        ),
                    }],
                    vec![EvidenceItem {
                        provider: FactProvider::TreeSitter,
                        detail: format!(
                            "`{call}` occurs at least twice in `{dependent_name}` in \
                             revision {}, at most once in revision {}",
                            draft.after_label, draft.before_label
                        ),
                        node: Some(dependent_node),
                    }],
                    format!(
                        "if profiling confirms `{call}` is expensive, evaluate it once and \
                         reuse the result; otherwise record why the recomputation is \
                         intentional"
                    ),
                    BTreeMap::from([("stale-edges".to_string(), 1)]),
                )
            }
            rule_names::ACCEPTED_CONFIG_INERT => {
                let key = draft.stale.iter().next().expect("config draft has a key");
                let holder_node = draft.dependent.node_ref(ContractRole::ConfigParameter);
                let mut verification = format!(
                    "remove `{key}` from the acceptance surface (`{dependent_name}`) or restore a \
                     behavioral consumer"
                );
                if !draft.added.is_empty() {
                    verification.push_str(&format!("; replacement key(s) {added_list} are live"));
                }
                let mut evidence = vec![
                    EvidenceItem {
                        provider: FactProvider::TreeSitter,
                        detail: format!(
                            "config key `{key}` was read in revision {} and has no config read in \
                             revision {}",
                            draft.before_label, draft.after_label
                        ),
                        node: None,
                    },
                    EvidenceItem {
                        provider: FactProvider::TreeSitter,
                        detail: format!(
                            "`{dependent_name}` still carries `{key}` in its literal surface"
                        ),
                        node: Some(holder_node.clone()),
                    },
                ];
                if let Some(note) = &draft.note {
                    evidence.push(EvidenceItem {
                        provider: FactProvider::TreeSitter,
                        detail: note.clone(),
                        node: None,
                    });
                }
                (
                    vec![ContractStep {
                        token: Some(key.clone()),
                        edge: ContractEdgeKind::Configures,
                        node: holder_node.clone(),
                        detail: format!(
                            "`{dependent_name}` still accepts config key `{key}`, but nothing in \
                             revision {} reads it",
                            draft.after_label
                        ),
                    }],
                    vec![
                        ContractStep {
                            token: None,
                            edge: ContractEdgeKind::Reads,
                            node: holder_node.clone(),
                            detail: format!(
                                "revision {} read `{key}`; revision {} does not",
                                draft.before_label, draft.after_label
                            ),
                        },
                        ContractStep {
                            token: None,
                            edge: ContractEdgeKind::Configures,
                            node: holder_node,
                            detail: format!("the acceptance surface still carries `{key}`"),
                        },
                    ],
                    evidence,
                    verification,
                    BTreeMap::from([("stale-edges".to_string(), 1)]),
                )
            }
            _ => unreachable!("unknown draft rule {}", draft.rule),
        };

    priority_inputs.insert(
        "persistence".to_string(),
        draft.persistence.revisions as i64,
    );
    priority_inputs.insert(
        "independent-churn".to_string(),
        draft.persistence.independent_edits as i64,
    );
    priority_inputs.insert(
        "boundary-distance".to_string(),
        i64::from(draft.dependent.path() != draft.path),
    );

    let finding = RefactorDefect {
        schema: REFACTOR_DEFECT_SCHEMA.to_string(),
        rule: draft.rule.to_string(),
        revisions: RevisionPair {
            before: draft.before_label.clone(),
            after: draft.after_label.clone(),
        },
        owner,
        stale_edges,
        causal_path,
        evidence,
        counter_evidence: Vec::new(),
        coverage_gaps,
        persistence: draft.persistence,
        priority_inputs,
        safety: SafetyClass::NeverAuto,
        suggested_verification,
    };
    debug_assert!(finding.validate().is_ok());
    finding
}

/// The owner role for a rule's owner migration node.
fn owner_role(rule: &str) -> ContractRole {
    match rule {
        rule_names::PRODUCER_VERIFIER_SCHEMA_DRIFT | rule_names::OPERATIONAL_IDENTITY_STALE => {
            ContractRole::Producer
        }
        _ => ContractRole::Owner,
    }
}

/// Emit one `adoption-chain-incomplete` summary per owner migration shared
/// by at least two detector families. Summaries present the missing chain
/// stages; they travel in [`RefactorRiskReport::summaries`] so they never
/// duplicate the underlying findings in baselines or severity counts.
fn build_summaries(drafts: &[FindingDraft], window: usize) -> Vec<RefactorDefect> {
    let mut groups: BTreeMap<(usize, String, PathBuf), Vec<&FindingDraft>> = BTreeMap::new();
    for draft in drafts {
        let Some(owner_name) = draft.owner_name() else {
            continue;
        };
        groups
            .entry((draft.pair_index, owner_name.to_string(), draft.path.clone()))
            .or_default()
            .push(draft);
    }
    let mut summaries = Vec::new();
    for ((_pair_index, owner_name, path), members) in groups {
        let rules: BTreeSet<&str> = members.iter().map(|member| member.rule).collect();
        if rules.len() < 2 {
            continue;
        }
        let first = members[0];
        let owner_before = first.owner_before.as_ref().expect("summary owner");
        let owner_after = first.owner_after.as_ref().expect("summary owner");
        let role = owner_role(first.rule);
        let owner_node = |function: &ContractFunction| ContractNodeRef {
            role,
            path: path.clone(),
            span: function.span,
            fingerprint: function.fingerprint.clone(),
            provider: FactProvider::TreeSitter,
            capability: CapabilityLevel::Complete,
        };
        let mut stale_edges = Vec::new();
        for rule in &rules {
            let Some((edge, stage_role)) = chain_stage(rule) else {
                continue;
            };
            let member = members
                .iter()
                .find(|member| member.rule == *rule)
                .expect("rule came from a member");
            stale_edges.push(ContractStep {
                token: None,
                edge,
                node: member.dependent.node_ref(stage_role),
                detail: format!(
                    "chain stage `{rule}` incomplete: `{}` is still bound to the former owner",
                    member.dependent.display_name()
                ),
            });
        }
        let mut coverage_gaps = Vec::new();
        if window == 2 {
            coverage_gaps.push(window_gap());
        }
        let persistence = members
            .iter()
            .map(|member| member.persistence)
            .max_by_key(|persistence| persistence.revisions)
            .unwrap_or(Persistence {
                revisions: 1,
                independent_edits: 0,
            });
        let summary = RefactorDefect {
            schema: REFACTOR_DEFECT_SCHEMA.to_string(),
            rule: rule_names::ADOPTION_CHAIN_INCOMPLETE.to_string(),
            revisions: RevisionPair {
                before: first.before_label.clone(),
                after: first.after_label.clone(),
            },
            owner: Some(OwnerMigration {
                before: owner_node(owner_before),
                after: owner_node(owner_after),
                match_evidence: vec![EntityMatchEvidence {
                    kind: EntityMatchKind::PathSpan,
                    detail: format!(
                        "function `{owner_name}` matched by name within {}",
                        path.display()
                    ),
                }],
            }),
            stale_edges,
            causal_path: vec![ContractStep {
                token: None,
                edge: ContractEdgeKind::Produces,
                node: owner_node(owner_after),
                detail: format!(
                    "`{owner_name}` changed owners; {} adoption-chain stages did not follow",
                    rules.len()
                ),
            }],
            evidence: vec![EvidenceItem {
                provider: FactProvider::TreeSitter,
                detail: format!(
                    "summarizes {} findings across {} detector families over owner \
                     `{owner_name}`; emitted once and excluded from finding counts",
                    members.len(),
                    rules.len()
                ),
                node: Some(owner_node(owner_after)),
            }],
            counter_evidence: Vec::new(),
            coverage_gaps,
            persistence,
            priority_inputs: BTreeMap::from([
                ("families".to_string(), rules.len() as i64),
                (
                    "stale-edges".to_string(),
                    members.iter().map(|member| member.stale.len() as i64).sum(),
                ),
                ("persistence".to_string(), persistence.revisions as i64),
                (
                    "independent-churn".to_string(),
                    persistence.independent_edits as i64,
                ),
            ]),
            safety: SafetyClass::NeverAuto,
            suggested_verification: format!(
                "complete the adoption chain for `{owner_name}`: reconcile {}",
                rules
                    .iter()
                    .map(|rule| format!("`{rule}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        debug_assert!(summary.validate().is_ok());
        summaries.push(summary);
    }
    summaries
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_parse::{ProjectSnapshotBuilder, RepositoryId};

    fn analysis(files: &[(&str, &[u8])]) -> Arc<ProjectAnalysis> {
        let temp = tempfile::tempdir().unwrap();
        let mut builder = ProjectSnapshotBuilder::new(
            temp.path(),
            RepositoryId::explicit("refactor-test").unwrap(),
        )
        .unwrap();
        for (path, source) in files {
            builder = builder.with_overlay(path, source.to_vec()).unwrap();
        }
        ProjectAnalysis::build(builder.build().unwrap()).unwrap()
    }

    fn compare(before: &[(&str, &[u8])], after: &[(&str, &[u8])]) -> RefactorRiskReport {
        analyze_refactor_risk(
            ("before".to_string(), analysis(before)),
            ("after".to_string(), analysis(after)),
        )
        .unwrap()
    }

    const SCORER_BEFORE: &[u8] = br#"class Scorer:
    def __init__(self, model):
        self.model = model

    def decide(self, candidates):
        return max(candidates, key=lambda c: self.model.raw_score(c))

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;

    #[test]
    fn owner_moved_consumer_stale_fires() {
        let after = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;
        let report = compare(&[("scoring.py", SCORER_BEFORE)], &[("scoring.py", after)]);
        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert_eq!(finding.rule, rule_names::OWNER_MOVED_CONSUMER_STALE);
        assert_eq!(finding.safety, SafetyClass::NeverAuto);
        finding.validate().unwrap();
        assert_eq!(finding.stale_edges.len(), 1);
        assert!(finding.stale_edges[0].detail.contains("model.raw_score"));
        assert!(!finding.coverage_gaps.is_empty());
    }

    #[test]
    fn full_adoption_does_not_fire() {
        let after = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.posterior.committed_score(candidate)
"#;
        let report = compare(&[("scoring.py", SCORER_BEFORE)], &[("scoring.py", after)]);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn pure_rename_does_not_fire() {
        let before = br#"def decide(candidates):
    return max(candidates)


def rank(candidates):
    return decide(candidates)
"#;
        let after = br#"def choose(candidates):
    return max(candidates)


def rank(candidates):
    return choose(candidates)
"#;
        let report = compare(&[("scoring.py", before)], &[("scoring.py", after)]);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn compatibility_adapter_does_not_fire() {
        let after = br#"class ScoreAdapter:
    def __init__(self, posterior):
        self.posterior = posterior

    def legacy_score(self, candidate):
        return self.posterior.committed_score(candidate)


class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.adapter.legacy_score(candidate)
"#;
        let report = compare(&[("scoring.py", SCORER_BEFORE)], &[("scoring.py", after)]);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn producer_verifier_schema_drift_fires() {
        let before = br#"def build_manifest(run):
    return {"run_id": run.id, "epochs": run.epochs, "metric": run.final_loss}


def validate_manifest(manifest):
    required = {"run_id", "epochs", "metric"}
    return required <= manifest.keys()
"#;
        let after = br#"def build_manifest(run):
    return {"run_id": run.id, "epochs": run.epochs, "seed": run.seed, "metrics": {"loss": run.final_loss}}


def validate_manifest(manifest):
    required = {"run_id", "epochs", "metric"}
    return required <= manifest.keys()
"#;
        let report = compare(&[("manifest.py", before)], &[("manifest.py", after)]);
        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert_eq!(finding.rule, rule_names::PRODUCER_VERIFIER_SCHEMA_DRIFT);
        finding.validate().unwrap();
        assert!(finding.stale_edges[0].detail.contains("metric"));
    }

    #[test]
    fn coherent_schema_migration_does_not_fire() {
        let before = br#"def build_manifest(run):
    return {"run_id": run.id, "metric": run.final_loss}


def validate_manifest(manifest):
    required = {"run_id", "metric"}
    return required <= manifest.keys()
"#;
        let after = br#"def build_manifest(run):
    return {"run_id": run.id, "metrics": {"loss": run.final_loss}}


def validate_manifest(manifest):
    required = {"run_id", "metrics"}
    return required <= manifest.keys()
"#;
        let report = compare(&[("manifest.py", before)], &[("manifest.py", after)]);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn julia_owner_moved_consumer_stale_fires() {
        let before = b"decide(model, candidates) = argmax(c -> raw_score(model, c), candidates)\n\npublic_score(model, c) = raw_score(model, c)\n";
        let after = b"decide(posterior, candidates) = commit(posterior, candidates)\n\npublic_score(model, c) = raw_score(model, c)\n";
        let report = compare(&[("scoring.jl", before)], &[("scoring.jl", after)]);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(
            report.findings[0].rule,
            rule_names::OWNER_MOVED_CONSUMER_STALE
        );
    }

    const CONFIG_BEFORE: &[u8] = br#"import os

DEFAULTS = {"THRESHOLD": 0.5}


def load_config():
    config = dict(DEFAULTS)
    config["THRESHOLD"] = os.environ["THRESHOLD"]
    return config
"#;

    #[test]
    fn accepted_config_inert_fires_when_retired_key_stays_accepted() {
        let after = br#"import os

DEFAULTS = {"THRESHOLD": 0.5}


def load_config():
    config = dict(DEFAULTS)
    config["THRESHOLD"] = os.environ["THRESHOLD_V2"]
    return config
"#;
        let report = compare(&[("config.py", CONFIG_BEFORE)], &[("config.py", after)]);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::ACCEPTED_CONFIG_INERT)
            .expect("accepted-config-inert should fire");
        finding.validate().unwrap();
        assert!(finding.owner.is_none());
        assert!(finding.stale_edges[0].detail.contains("THRESHOLD"));
        assert!(
            finding
                .evidence
                .iter()
                .any(|item| item.detail.contains("THRESHOLD_V2")),
            "replacement key should be noted: {:?}",
            finding.evidence
        );
        assert!(finding.suggested_verification.contains("THRESHOLD_V2"));
    }

    #[test]
    fn clean_config_removal_does_not_fire() {
        let after = br#"import os

DEFAULTS = {}


def load_config():
    return dict(DEFAULTS)
"#;
        let report = compare(&[("config.py", CONFIG_BEFORE)], &[("config.py", after)]);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.rule != rule_names::ACCEPTED_CONFIG_INERT),
            "clean removal must not fire: {:?}",
            report.findings
        );
    }

    #[test]
    fn module_level_acceptance_fires_config_inert() {
        let before = br#"import os

LIMIT = os.environ["LIMIT"]
"#;
        let after = br#"DEFAULTS = {"LIMIT": 10}
"#;
        let report = compare(&[("settings.py", before)], &[("settings.py", after)]);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::ACCEPTED_CONFIG_INERT)
            .expect("module-level acceptance should fire");
        finding.validate().unwrap();
        assert!(finding.stale_edges[0].detail.contains("LIMIT"));
    }

    #[test]
    fn test_oracle_lag_fires_for_unchanged_test() {
        let test = br#"from scoring import Scorer


def test_public_score(scorer, candidate):
    assert scorer.public_score(candidate) == model.raw_score(candidate)
"#;
        let after = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;
        let report = compare(
            &[("scoring.py", SCORER_BEFORE), ("test_scoring.py", test)],
            &[("scoring.py", after), ("test_scoring.py", test)],
        );
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::TEST_ORACLE_LAG)
            .expect("test-oracle-lag should fire");
        finding.validate().unwrap();
        assert!(finding.suggested_verification.contains("no oracle"));
        assert_eq!(report.summaries.len(), 1);
        assert_eq!(
            report.summaries[0].rule,
            rule_names::ADOPTION_CHAIN_INCOMPLETE
        );
        report.summaries[0].validate().unwrap();
    }

    #[test]
    fn updated_test_does_not_fire_oracle_lag() {
        let test_before = br#"def test_public_score():
    assert public_score(c) == model.raw_score(c)
"#;
        let test_after = br#"def test_public_score():
    assert public_score(c) == posterior.committed_score(c)
"#;
        let after = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;
        let report = compare(
            &[
                ("scoring.py", SCORER_BEFORE),
                ("test_scoring.py", test_before),
            ],
            &[("scoring.py", after), ("test_scoring.py", test_after)],
        );
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.rule != rule_names::TEST_ORACLE_LAG),
            "a coherently updated test is counter-evidence: {:?}",
            report.findings
        );
    }

    #[test]
    fn persistence_tracks_stale_edge_across_window() {
        let rev2 = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;
        // rev3 edits the owner alone (independent churn) while the stale
        // consumer is untouched.
        let rev3 = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        committed = self.posterior.commit(candidates)
        return committed

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;
        let report = analyze_refactor_window(vec![
            (
                "rev1".to_string(),
                analysis(&[("scoring.py", SCORER_BEFORE)]),
            ),
            ("rev2".to_string(), analysis(&[("scoring.py", rev2)])),
            ("rev3".to_string(), analysis(&[("scoring.py", rev3)])),
        ])
        .unwrap();
        assert_eq!(report.revisions, vec!["rev1", "rev2", "rev3"]);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::OWNER_MOVED_CONSUMER_STALE)
            .expect("migration should fire on rev1->rev2");
        assert_eq!(finding.persistence.revisions, 2);
        assert_eq!(finding.persistence.independent_edits, 1);
        assert_eq!(finding.priority_inputs["persistence"], 2);
        assert_eq!(finding.priority_inputs["independent-churn"], 1);
        // A window longer than two revisions analyzes accumulation, so the
        // two-revision coverage gap must be gone.
        assert!(finding.coverage_gaps.is_empty());
    }

    #[test]
    fn repaired_edge_stops_persistence() {
        let rev2 = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;
        let rev3 = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.posterior.committed_score(candidate)
"#;
        let report = analyze_refactor_window(vec![
            (
                "rev1".to_string(),
                analysis(&[("scoring.py", SCORER_BEFORE)]),
            ),
            ("rev2".to_string(), analysis(&[("scoring.py", rev2)])),
            ("rev3".to_string(), analysis(&[("scoring.py", rev3)])),
        ])
        .unwrap();
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::OWNER_MOVED_CONSUMER_STALE)
            .expect("migration should fire on rev1->rev2");
        assert_eq!(finding.persistence.revisions, 1);
        assert_eq!(finding.persistence.independent_edits, 0);
    }

    const RANK_BEFORE: &[u8] = br#"def rank_documents(docs):
    results = []
    for doc in docs:
        results.append(score_document(doc))
    return results
"#;

    const RANK_COLLAPSED: &[u8] = br#"def rank_documents(docs):
    merged = flatten(docs)
    return score_batch(merged)
"#;

    #[test]
    fn scope_collapse_fires_when_partition_loop_is_flattened() {
        let report = compare(&[("rank.py", RANK_BEFORE)], &[("rank.py", RANK_COLLAPSED)]);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::SCOPE_COLLAPSE_AFTER_REFACTOR)
            .expect("scope collapse should fire");
        finding.validate().unwrap();
        assert_eq!(finding.safety, SafetyClass::NeverAuto);
        assert!(finding.suggested_verification.contains("metamorphic"));
        assert!(
            finding
                .coverage_gaps
                .iter()
                .any(|gap| gap.reason.contains("semantic axis")),
            "the semantic axis must stay an explicit gap: {:?}",
            finding.coverage_gaps
        );
    }

    #[test]
    fn flatten_without_loop_removal_does_not_fire_scope_collapse() {
        let after = br#"def rank_documents(docs):
    results = []
    for doc in docs:
        results.append(score_document(doc))
    return flatten(results)
"#;
        let report = compare(&[("rank.py", RANK_BEFORE)], &[("rank.py", after)]);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.rule != rule_names::SCOPE_COLLAPSE_AFTER_REFACTOR),
            "loop structure is unchanged: {:?}",
            report.findings
        );
    }

    #[test]
    fn singleton_oracle_after_scope_collapse_is_test_oracle_lag() {
        let test = br#"def test_rank_documents():
    assert rank_documents([fixture_doc()]) == [expected_result()]
"#;
        let report = compare(
            &[("rank.py", RANK_BEFORE), ("test_rank.py", test)],
            &[("rank.py", RANK_COLLAPSED), ("test_rank.py", test)],
        );
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::TEST_ORACLE_LAG)
            .expect("singleton oracle should be a test-oracle-lag candidate");
        finding.validate().unwrap();
        assert!(
            finding.suggested_verification.contains("multi-partition"),
            "verification should request a multi-partition oracle: {}",
            finding.suggested_verification
        );
    }

    #[test]
    fn updated_metamorphic_oracle_does_not_fire_oracle_lag() {
        let test_before = br#"def test_rank_documents():
    assert rank_documents([fixture_doc()]) == [expected_result()]
"#;
        let test_after = br#"def test_rank_documents():
    fixed = fixture_doc()
    alone = rank_documents([fixed])
    packed = rank_documents([fixed, companion_doc()])
    assert alone[0] == packed[0]
"#;
        let report = compare(
            &[("rank.py", RANK_BEFORE), ("test_rank.py", test_before)],
            &[("rank.py", RANK_COLLAPSED), ("test_rank.py", test_after)],
        );
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.rule != rule_names::TEST_ORACLE_LAG),
            "an updated metamorphic oracle is counter-evidence: {:?}",
            report.findings
        );
    }

    const GATE_BEFORE: &[u8] = br#"def step(trainer):
    return gate_scalar_update(trainer)


def release_check(model):
    value = read_gate(model)
    assert value > 0


def read_gate(model):
    return gate_scalar_update(model)
"#;

    #[test]
    fn mechanism_live_gate_retired_fires_through_dependency_path() {
        let after = br#"def step(trainer):
    return controller_apply(trainer)


def release_check(model):
    value = read_gate(model)
    assert value > 0


def read_gate(model):
    return gate_scalar_update(model)
"#;
        let report = compare(&[("train.py", GATE_BEFORE)], &[("train.py", after)]);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::MECHANISM_LIVE_GATE_RETIRED)
            .expect("retired gate should fire");
        finding.validate().unwrap();
        assert!(finding.suggested_verification.contains("no longer governs"));
        // The direct stale helper is still a plain consumer finding; the two
        // families never claim the same dependent.
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.rule == rule_names::OWNER_MOVED_CONSUMER_STALE)
        );
    }

    #[test]
    fn gate_reaching_the_new_mechanism_does_not_fire() {
        let before = br#"def step(trainer):
    return gate_scalar_update(trainer)


def release_check(model):
    value = read_gate(model)
    assert value > 0


def read_gate(model):
    return gate_scalar_update(model)


def legacy_dump(model):
    return gate_scalar_update(model)
"#;
        let after = br#"def step(trainer):
    return controller_apply(trainer)


def release_check(model):
    value = read_gate(model)
    assert value > 0


def read_gate(model):
    return controller_apply(model)


def legacy_dump(model):
    return gate_scalar_update(model)
"#;
        let report = compare(&[("train.py", before)], &[("train.py", after)]);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.rule != rule_names::MECHANISM_LIVE_GATE_RETIRED),
            "a gate whose path reaches the new mechanism must not fire: {:?}",
            report.findings
        );
    }

    #[test]
    fn telemetry_not_bound_to_claim_fires_for_stale_observation() {
        let before = br#"def train_step(model):
    return legacy_scalar(model)


def report_health(model):
    metrics.gauge("controller_activity", read_activity(model))


def read_activity(model):
    return legacy_scalar(model)
"#;
        let after = br#"def train_step(model):
    return controller_activity(model)


def report_health(model):
    metrics.gauge("controller_activity", read_activity(model))


def read_activity(model):
    return legacy_scalar(model)
"#;
        let report = compare(&[("train.py", before)], &[("train.py", after)]);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::TELEMETRY_NOT_BOUND_TO_CLAIM)
            .expect("stale telemetry should fire");
        finding.validate().unwrap();
        assert!(
            finding
                .coverage_gaps
                .iter()
                .any(|gap| gap.reason.contains("lexical")),
            "the lexical classification must stay an explicit gap: {:?}",
            finding.coverage_gaps
        );
        assert!(
            finding
                .evidence
                .iter()
                .any(|item| item.detail.contains("supporting evidence")),
        );
    }

    #[test]
    fn rebound_telemetry_does_not_fire() {
        let before = br#"def train_step(model):
    return legacy_scalar(model)


def report_health(model):
    metrics.gauge("controller_activity", read_activity(model))


def read_activity(model):
    return legacy_scalar(model)


def legacy_probe(model):
    return legacy_scalar(model)
"#;
        let after = br#"def train_step(model):
    return controller_activity(model)


def report_health(model):
    metrics.gauge("controller_activity", read_activity(model))


def read_activity(model):
    return controller_activity(model)


def legacy_probe(model):
    return legacy_scalar(model)
"#;
        let report = compare(&[("train.py", before)], &[("train.py", after)]);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.rule != rule_names::TELEMETRY_NOT_BOUND_TO_CLAIM),
            "telemetry reaching the live mechanism must not fire: {:?}",
            report.findings
        );
    }

    #[test]
    fn operational_identity_stale_fires_for_status_publisher() {
        let before = br#"def resume_run(state):
    return spawn_process(state)


def publish_status(state):
    status.publish(current_pid(state))


def current_pid(state):
    return spawn_process(state)
"#;
        let after = br#"def resume_run(state):
    return relaunch_supervisor(state)


def publish_status(state):
    status.publish(current_pid(state))


def current_pid(state):
    return spawn_process(state)
"#;
        let report = compare(&[("runner.py", before)], &[("runner.py", after)]);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::OPERATIONAL_IDENTITY_STALE)
            .expect("stale status publisher should fire");
        finding.validate().unwrap();
        assert!(finding.suggested_verification.contains("retired identity"));
    }

    #[test]
    fn confidence_provenance_lost_fires_for_lossy_reconstruction() {
        let before = br#"def decide(model, candidates):
    return best_by_raw_score(model, candidates)


def public_score(model, candidate):
    return best_by_raw_score(model, candidate)


def debug_dump(model, candidate):
    return best_by_raw_score(model, candidate)
"#;
        let after = br#"def decide(model, candidates):
    return posterior_commit(model, candidates)


def public_score(model, candidate):
    committed = posterior_commit_index(model, candidate)
    return round(reconstruct_score(committed), 3)


def debug_dump(model, candidate):
    return best_by_raw_score(model, candidate)
"#;
        let report = compare(&[("scoring.py", before)], &[("scoring.py", after)]);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::CONFIDENCE_PROVENANCE_LOST)
            .expect("lossy reconstruction should fire");
        finding.validate().unwrap();
        assert!(
            finding
                .suggested_verification
                .contains("evidence that governed")
        );
    }

    #[test]
    fn provenance_retained_does_not_fire() {
        let before = br#"def decide(model, candidates):
    return best_by_raw_score(model, candidates)


def public_score(model, candidate):
    return best_by_raw_score(model, candidate)


def debug_dump(model, candidate):
    return best_by_raw_score(model, candidate)
"#;
        let after = br#"def decide(model, candidates):
    return posterior_commit(model, candidates)


def public_score(model, candidate):
    return round(posterior_commit(model, candidate), 3)


def debug_dump(model, candidate):
    return best_by_raw_score(model, candidate)
"#;
        let report = compare(&[("scoring.py", before)], &[("scoring.py", after)]);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.rule != rule_names::CONFIDENCE_PROVENANCE_LOST),
            "a consumer following the owner to the new evidence retains provenance: {:?}",
            report.findings
        );
    }

    #[test]
    fn hot_path_work_duplicated_fires_for_introduced_duplicate() {
        let before = br#"def render(batch):
    return combine(expensive_transform(preprocess(batch)))
"#;
        let after = br#"def render(batch):
    left = expensive_transform(preprocess(batch))
    right = expensive_transform(preprocess(batch))
    return combine(left, right)
"#;
        let report = compare(&[("render.py", before)], &[("render.py", after)]);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::HOT_PATH_WORK_DUPLICATED)
            .expect("introduced duplicate should fire");
        finding.validate().unwrap();
        assert!(
            finding
                .coverage_gaps
                .iter()
                .any(|gap| gap.reason.contains("unproved")),
            "cost must stay unproved: {:?}",
            finding.coverage_gaps
        );
    }

    #[test]
    fn preexisting_duplicate_does_not_fire_hot_path() {
        let source = br#"def render(batch):
    left = expensive_transform(preprocess(batch))
    right = expensive_transform(preprocess(batch))
    return combine(left, right)
"#;
        let report = compare(&[("render.py", source)], &[("render.py", source)]);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.rule != rule_names::HOT_PATH_WORK_DUPLICATED),
            "a duplicate present in both revisions was not introduced: {:?}",
            report.findings
        );
    }

    #[test]
    fn dynamic_access_is_a_capability_gap_not_a_clean_result() {
        let source = br#"def dispatch(handler, name):
    return getattr(handler, name)()
"#;
        let report = compare(&[("dispatch.py", source)], &[("dispatch.py", source)]);
        assert_eq!(report.coverage, deslop_parse::FactCoverage::Partial);
        assert!(
            report
                .coverage_reasons
                .iter()
                .any(|reason| reason.contains("dynamic access")),
            "dynamic access must surface as a coverage reason: {:?}",
            report.coverage_reasons
        );
    }

    use deslop_core::refactor_defect::{
        ProviderArtifact, RefactorHistoryBundle, RevisionFile, RevisionSnapshot,
    };

    const SCORER_AFTER_STALE: &[u8] = br#"class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
"#;

    fn scorer_bundle(artifacts: Vec<ProviderArtifact>) -> RefactorHistoryBundle {
        RefactorHistoryBundle::new(vec![
            RevisionSnapshot {
                revision: "rev-a".to_string(),
                parents: vec![],
                timestamp: None,
                files: vec![RevisionFile::new(
                    "scoring.py",
                    String::from_utf8(SCORER_BEFORE.to_vec()).unwrap(),
                )],
                provider_artifacts: vec![],
            },
            RevisionSnapshot {
                revision: "rev-b".to_string(),
                parents: vec!["rev-a".to_string()],
                timestamp: None,
                files: vec![RevisionFile::new(
                    "scoring.py",
                    String::from_utf8(SCORER_AFTER_STALE.to_vec()).unwrap(),
                )],
                provider_artifacts: artifacts,
            },
        ])
    }

    fn stale_edge_location(report: &RefactorRiskReport) -> (PathBuf, usize) {
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::OWNER_MOVED_CONSUMER_STALE)
            .expect("bundle should reproduce the stale consumer");
        let node = &finding.stale_edges[0].node;
        (node.path.clone(), node.span.start_line)
    }

    #[test]
    fn bundle_analysis_matches_directory_analysis_and_is_byte_stable() {
        let bundle = scorer_bundle(vec![]);
        let first = analyze_refactor_bundle(&bundle).unwrap();
        let second = analyze_refactor_bundle(&bundle).unwrap();
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap(),
            "identical history bundles must produce byte-stable reports"
        );
        assert_eq!(first.findings.len(), 1);
        assert_eq!(
            first.findings[0].rule,
            rule_names::OWNER_MOVED_CONSUMER_STALE
        );
        assert_eq!(first.revisions, vec!["rev-a", "rev-b"]);
    }

    #[test]
    fn agreeing_provider_artifact_becomes_supporting_evidence() {
        let probe = analyze_refactor_bundle(&scorer_bundle(vec![])).unwrap();
        let (path, start_line) = stale_edge_location(&probe);
        let payload = serde_json::json!({
            "schema": SEMANTIC_PROVIDER_FACTS_SCHEMA,
            "functions": [{
                "path": path,
                "start_line": start_line,
                "references": ["model.raw_score"],
            }],
        });
        let bundle = scorer_bundle(vec![ProviderArtifact {
            provider: deslop_core::refactor_defect::FactProvider::Lsp,
            revision: "rev-b".to_string(),
            capability: deslop_core::refactor_defect::CapabilityLevel::Partial,
            payload: payload.to_string(),
        }]);
        let report = analyze_refactor_bundle(&bundle).unwrap();
        let finding = &report.findings[0];
        assert!(
            finding.evidence.iter().any(|item| {
                item.provider == deslop_core::refactor_defect::FactProvider::Lsp
                    && item.detail.contains("provider attests")
            }),
            "agreeing provider facts join as supporting evidence: {:?}",
            finding.evidence
        );
        assert!(finding.counter_evidence.is_empty());
    }

    #[test]
    fn disagreeing_provider_artifact_is_visible_conflict_without_promotion() {
        let probe = analyze_refactor_bundle(&scorer_bundle(vec![])).unwrap();
        let (path, start_line) = stale_edge_location(&probe);
        let payload = serde_json::json!({
            "schema": SEMANTIC_PROVIDER_FACTS_SCHEMA,
            "functions": [{
                "path": path,
                "start_line": start_line,
                "references": ["posterior.committed_score"],
            }],
        });
        let bundle = scorer_bundle(vec![ProviderArtifact {
            provider: deslop_core::refactor_defect::FactProvider::Lsp,
            revision: "rev-b".to_string(),
            capability: deslop_core::refactor_defect::CapabilityLevel::Complete,
            payload: payload.to_string(),
        }]);
        let report = analyze_refactor_bundle(&bundle).unwrap();
        // The syntax finding survives (no suppression, no promotion) and the
        // disagreement is visible.
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.rule == rule_names::OWNER_MOVED_CONSUMER_STALE)
            .expect("syntax authority is unchanged by provider disagreement");
        assert!(
            finding
                .counter_evidence
                .iter()
                .any(|item| item.detail.contains("disagreement retained")),
            "{:?}",
            finding.counter_evidence
        );
        // The scan-path projection keeps the disagreement visible too.
        assert!(finding.to_finding().message.contains("counter-evidence"));
    }

    #[test]
    fn unintelligible_provider_artifact_is_a_coverage_reason() {
        let bundle = scorer_bundle(vec![ProviderArtifact {
            provider: deslop_core::refactor_defect::FactProvider::Lsp,
            revision: "rev-b".to_string(),
            capability: deslop_core::refactor_defect::CapabilityLevel::Unknown,
            payload: "not json".to_string(),
        }]);
        let report = analyze_refactor_bundle(&bundle).unwrap();
        assert_eq!(report.coverage, FactCoverage::Partial);
        assert!(
            report
                .coverage_reasons
                .iter()
                .any(|reason| reason.contains("payload not understood")),
            "{:?}",
            report.coverage_reasons
        );
    }

    #[test]
    fn window_requires_two_revisions() {
        let report = analyze_refactor_window(vec![(
            "only".to_string(),
            analysis(&[("scoring.py", SCORER_BEFORE)]),
        )]);
        assert!(report.is_err());
    }
}
