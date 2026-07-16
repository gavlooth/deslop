use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use deslop_analyzer::{
    AnalyzerConfig, AnalyzerConfigSnapshot, ExternalCapability, ScanContext,
    scan_paths_with_context,
};
use deslop_core::{
    AnalysisProvenance, FileReport, Finding, Lang, RevisionGuard, SafetyClass, Severity, Span,
    baseline_fingerprint, revision_guard,
};
use deslop_parse::{ProjectAnalysis, SourceFile, SyntaxOwner};
use serde::{Deserialize, Serialize};

mod lifecycle;
mod planner;
mod recipe;
mod work_order;

pub use lifecycle::{
    ExpiredWorkOrder, WORK_ORDER_HANDLE_SCHEMA, WORK_ORDER_REPLAN_SCHEMA, WorkOrderHandle,
    WorkOrderReplanResult, replan_after_commit,
};
pub use planner::{
    AtomicWorkGroup, AtomicWorkGroupId, BlockedWorkGroup, ExplicitPrerequisite,
    MutuallyExclusiveRecipes, WORK_ORDER_PLAN_SCHEMA, WorkOrderBlockReason, WorkOrderEdge,
    WorkOrderEdgeKind, WorkOrderPlan, WorkOrderPlanId, WorkOrderPlannerConstraints,
    WorkOrderScheduleWave, plan_work_orders,
};
pub use recipe::{
    RECIPE_WORK_ORDER_SCHEMA, RecipePatchBudget, RecipeResource, RecipeResourceKind,
    RecipeVerificationContract, RecipeWorkOrder, RecipeWorkOrderId, recipe_work_orders,
};
pub use work_order::{
    SHARED_WORK_ORDER_SCHEMA, SharedWorkOrder, SharedWorkOrderId, WorkOrderAccess,
    WorkOrderEvidence, WorkOrderEvidenceKind, WorkOrderImpact, WorkOrderParameter,
    WorkOrderPatchBudget, WorkOrderProvenance, WorkOrderRecipe, WorkOrderResource,
    WorkOrderResourceKind, WorkOrderSubject, WorkOrderTarget, WorkOrderUnknown,
    WorkOrderVerification, shared_finding_work_orders, shared_transformation_work_orders,
};

macro_rules! protocol_struct {
    ($vis:vis struct $name:ident { $($field:ident: $type:ty),+ $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        $vis struct $name {
            $(pub $field: $type),+
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProposalScopeKind {
    File,
    Directory,
}

protocol_struct! {
pub struct ProposalScope {
    path: PathBuf,
    kind: ProposalScopeKind,
}
}

protocol_struct! {
pub struct ProposalSource {
    path: PathBuf,
    lang: Lang,
    revision_guard: RevisionGuard,
    analysis: Option<AnalysisProvenance>,
}
}

protocol_struct! {
pub struct ProposalContext {
    schema: String,
    analyzer_semantics: String,
    context_id: String,
    requested_scope: Vec<ProposalScope>,
    analyzer: AnalyzerConfigSnapshot,
    excluded_fingerprints: Vec<String>,
    sources: Vec<ProposalSource>,
    external_capabilities: Vec<ExternalCapability>,
    workorder_set_digest: String,
}
}

#[derive(Debug, Clone)]
pub struct ProposalBatch {
    pub analysis: Arc<ProjectAnalysis>,
    pub reports: Vec<FileReport>,
    pub context: ProposalContext,
    pub work_orders: Vec<WorkOrder>,
}

protocol_struct! {
pub struct Region {
    start_line: usize,
    end_line: usize,
    start_byte: usize,
    end_byte: usize,
    text: String,
}
}

protocol_struct! {
pub struct WorkOrderFinding {
    rule: String,
    severity: deslop_core::Severity,
    safety: SafetyClass,
    message: String,
    precondition: Option<String>,
}
}

protocol_struct! {
pub struct Contract {
    must_parse: bool,
    no_new_public_defs: bool,
    keep_error_handling: bool,
    max_growth_ratio: f32,
    check_cmd: Option<String>,
}
}

impl Default for Contract {
    fn default() -> Self {
        Self {
            must_parse: true,
            no_new_public_defs: true,
            keep_error_handling: true,
            max_growth_ratio: 1.0,
            check_cmd: None,
        }
    }
}

protocol_struct! {
pub struct WorkOrder {
    schema: String,
    kind: WorkOrderKind,
    id: String,
    path: PathBuf,
    region: Region,
    region_fingerprint: String,
    revision_guard: RevisionGuard,
    proposal_context: ProposalContext,
    findings: Vec<WorkOrderFinding>,
    instruction: String,
    contract: Contract,
}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkOrderKind {
    RewriteRegion,
    NeedsCharacterizationTest,
}

protocol_struct! {
pub struct Patch {
    schema: String,
    workorder_id: String,
    revision_guard: RevisionGuard,
    proposal_context: ProposalContext,
    replacement: String,
    by: String,
}
}

protocol_struct! {
pub struct CharacterizationTest {
    schema: String,
    workorder_id: String,
    revision_guard: RevisionGuard,
    proposal_context: ProposalContext,
    test_path: PathBuf,
    test_text: String,
    by: String,
}
}

fn work_order_drafts_for_report(
    source: &SourceFile,
    report: &FileReport,
    analysis: &ProjectAnalysis,
    logical_path: &Path,
) -> Vec<WorkOrderDraft> {
    if source.path != report.path
        || source.lang != report.lang
        || !report.analysis.permits_rewrites()
    {
        return Vec::new();
    }
    work_order_drafts_for_source(source, &report.findings, |finding| {
        owned_enclosing_region(analysis, logical_path, finding)
    })
}

fn work_order_drafts_for_source(
    source: &SourceFile,
    findings: &[Finding],
    mut enclosing_region: impl FnMut(&Finding) -> Option<(usize, usize)>,
) -> Vec<WorkOrderDraft> {
    let mut grouped: BTreeMap<RewriteRegionKey, Vec<&Finding>> = BTreeMap::new();
    for finding in findings
        .iter()
        .filter(|finding| finding.safety.permits_proposal())
    {
        let region = region_for_finding(source, finding, enclosing_region(finding));
        grouped
            .entry(RewriteRegionKey::new(&source.path, region))
            .or_default()
            .push(finding);
    }

    grouped
        .into_iter()
        .filter(|(key, _)| {
            !findings.iter().any(|finding| {
                finding.safety == SafetyClass::NeverAuto
                    && spans_overlap(finding.span, region_span(&key.region()))
            })
        })
        .map(|(key, mut findings)| {
            sort_grouped_findings(&mut findings);
            work_order_for_findings(key, findings)
        })
        .collect()
}

fn spans_overlap(left: Span, right: Span) -> bool {
    if left.start_byte == left.end_byte {
        return right.start_byte <= left.start_byte && left.start_byte < right.end_byte;
    }
    left.start_byte < right.end_byte && right.start_byte < left.end_byte
}

#[cfg(test)]
fn work_orders_for_source(source: &SourceFile, findings: &[Finding]) -> Vec<WorkOrder> {
    work_orders_from_test_drafts(work_order_drafts_for_source(source, findings, |finding| {
        let region =
            source.enclosing_region_for_span(finding.span.start_line, finding.span.end_line);
        Some((region.start_line, region.end_line))
    }))
}

#[cfg(test)]
fn work_orders_for_report(source: &SourceFile, report: &FileReport) -> Vec<WorkOrder> {
    if source.path != report.path
        || source.lang != report.lang
        || !report.analysis.permits_rewrites()
    {
        return Vec::new();
    }
    work_orders_for_source(source, &report.findings)
}

#[cfg(test)]
fn work_orders_from_test_drafts(drafts: Vec<WorkOrderDraft>) -> Vec<WorkOrder> {
    let mut context = ProposalContext {
        schema: "deslop.proposal-context/1".to_string(),
        analyzer_semantics: "deslop-analyzer/2".to_string(),
        context_id: String::new(),
        requested_scope: Vec::new(),
        analyzer: AnalyzerConfig::default().snapshot(),
        excluded_fingerprints: Vec::new(),
        sources: Vec::new(),
        external_capabilities: Vec::new(),
        workorder_set_digest: digest_json("deslop workorder set v1", &drafts).expect("digest"),
    };
    context.context_id = proposal_context_id(&context).expect("context id");
    drafts
        .into_iter()
        .map(|draft| draft.into_work_order(&context))
        .collect()
}

pub fn region_fingerprint(path: &Path, region: &Region) -> String {
    baseline_fingerprint(path, "region", region_span(region), &region.text)
}

pub fn region_revision_guard(path: &Path, region: &Region) -> RevisionGuard {
    revision_guard(path, region_span(region), &region.text)
}

pub fn workorder_revision_guard(work_order: &WorkOrder) -> &RevisionGuard {
    &work_order.revision_guard
}

pub fn workorder_id_for_context(
    path: &Path,
    region: &Region,
    proposal_context: &ProposalContext,
) -> String {
    let payload = format!(
        "{}\0{}",
        proposal_context.context_id,
        region_fingerprint(path, region)
    );
    format!(
        "wo3_{}",
        blake3::derive_key("deslop workorder identity v3", payload.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

pub fn validate_workorder_identity(work_order: &WorkOrder) -> Result<(), String> {
    if work_order.schema != "deslop.workorder/3" {
        return Err(format!(
            "unsupported workorder schema `{}`; regenerate as deslop.workorder/3",
            work_order.schema
        ));
    }
    validate_proposal_context(&work_order.proposal_context)?;
    if work_order.findings.is_empty() {
        return Err("workorder must contain at least one proposal-eligible finding".to_string());
    }
    if let Some(finding) = work_order
        .findings
        .iter()
        .find(|finding| !finding.safety.permits_proposal())
    {
        return Err(format!(
            "workorder finding `{}` has non-proposable safety class {:?}",
            finding.rule, finding.safety
        ));
    }
    validate_repo_path(&work_order.path, false)?;
    let fingerprint = region_fingerprint(&work_order.path, &work_order.region);
    if work_order.region_fingerprint != fingerprint {
        return Err(
            "workorder region_fingerprint does not match its normalized region identity"
                .to_string(),
        );
    }
    let guard = region_revision_guard(&work_order.path, &work_order.region);
    if work_order.revision_guard != guard {
        return Err("workorder revision_guard does not match its exact region bytes".to_string());
    }
    let id = workorder_id_for_context(
        &work_order.path,
        &work_order.region,
        &work_order.proposal_context,
    );
    if work_order.id != id {
        return Err(format!(
            "workorder id `{}` does not match expected `{id}`",
            work_order.id
        ));
    }
    Ok(())
}

pub fn validate_proposal_context(context: &ProposalContext) -> Result<(), String> {
    if context.schema != "deslop.proposal-context/1" {
        return Err(format!(
            "unsupported proposal context schema `{}`",
            context.schema
        ));
    }
    let expected = proposal_context_id(context).map_err(|error| error.to_string())?;
    if context.context_id != expected {
        return Err("proposal context_id does not match its canonical payload".to_string());
    }
    for scope in &context.requested_scope {
        validate_repo_path(&scope.path, true)?;
    }
    for source in &context.sources {
        validate_repo_path(&source.path, false)?;
    }
    for capability in &context.external_capabilities {
        validate_repo_path(&capability.path, false)?;
    }
    if let Some(project) = &context.analyzer.julia_project {
        validate_repo_path(project, true)?;
    }
    Ok(())
}

fn validate_repo_path(path: &Path, allow_empty: bool) -> Result<(), String> {
    if !allow_empty && path.as_os_str().is_empty() {
        return Err("proposal paths must not be empty".to_string());
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "proposal path `{}` must be normalized and root-relative",
            path.display()
        ));
    }
    Ok(())
}

pub fn propose_work_orders(
    root: &Path,
    requested_scope: &[PathBuf],
    config: AnalyzerConfig,
) -> Result<ProposalBatch> {
    propose_work_orders_with_exclusions(root, requested_scope, config, &[])
}

pub fn propose_work_orders_with_exclusions(
    root: &Path,
    requested_scope: &[PathBuf],
    mut config: AnalyzerConfig,
    excluded_fingerprints: &[String],
) -> Result<ProposalBatch> {
    let root = root
        .canonicalize()
        .with_context(|| format!("failed to resolve proposal root {}", root.display()))?;
    let scope = normalize_scope(&root, requested_scope)?;
    let scan_paths = scope
        .iter()
        .map(|entry| root.join(&entry.path))
        .collect::<Vec<_>>();
    if let Some(project) = &config.julia_project {
        let project = if project.is_absolute() {
            project.clone()
        } else {
            root.join(project)
        };
        config.julia_project =
            Some(project.canonicalize().with_context(|| {
                format!("failed to resolve Julia project {}", project.display())
            })?);
    }
    config.suppression = config.suppression.clone().with_match_root(root.clone());
    let mut analyzer = config.snapshot();
    normalize_analyzer_paths(&root, &mut analyzer)?;
    let scan = scan_paths_with_context(&scan_paths, config)?;
    proposal_batch_from_scan(&root, scope, analyzer, excluded_fingerprints.to_vec(), scan)
}

fn proposal_batch_from_scan(
    root: &Path,
    requested_scope: Vec<ProposalScope>,
    analyzer: AnalyzerConfigSnapshot,
    mut excluded_fingerprints: Vec<String>,
    mut scan: ScanContext,
) -> Result<ProposalBatch> {
    let analysis = Arc::clone(&scan.analysis);
    excluded_fingerprints.sort();
    excluded_fingerprints.dedup();
    if !excluded_fingerprints.is_empty() {
        let input_contents = &scan.input_contents;
        for report in &mut scan.reports {
            let relative = normalized_repo_path(root, &report.path)?;
            let source = input_contents
                .get(&report.path)
                .map(|text| SourceFile::new_with_lang(relative, text.clone(), report.lang));
            report.findings.retain(|finding| {
                if excluded_fingerprints
                    .binary_search(&finding.fingerprint)
                    .is_ok()
                {
                    return false;
                }
                let Some(source) = &source else {
                    return true;
                };
                let text = source.region_text(finding.span.start_line, finding.span.end_line);
                let normalized =
                    baseline_fingerprint(&source.path, &finding.rule, finding.span, &text);
                excluded_fingerprints.binary_search(&normalized).is_err()
            });
        }
    }
    let mut drafts = Vec::new();
    for report in &scan.reports {
        let text = scan.input_contents.get(&report.path).ok_or_else(|| {
            anyhow::anyhow!(
                "pinned proposal source unavailable for {}",
                report.path.display()
            )
        })?;
        let logical_path = scan
            .presentation
            .entries()
            .find_map(|(logical, display)| (display == report.path).then_some(logical))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "proposal presentation has no logical path for {}",
                    report.path.display()
                )
            })?;
        let source = SourceFile::new_with_lang(report.path.clone(), text.clone(), report.lang);
        drafts.extend(work_order_drafts_for_report(
            &source,
            report,
            &scan.analysis,
            logical_path,
        ));
    }
    for draft in &mut drafts {
        draft.normalize_path(root)?;
    }
    drafts.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.region.start_byte.cmp(&b.region.start_byte))
            .then(a.region_fingerprint.cmp(&b.region_fingerprint))
    });
    let workorder_set_digest = digest_json("deslop workorder set v1", &drafts)?;
    let sources = proposal_sources(root, &scan)?;
    let external_capabilities = normalize_external_capabilities(root, scan.external_capabilities)?;
    let mut context = ProposalContext {
        schema: "deslop.proposal-context/1".to_string(),
        analyzer_semantics: "deslop-analyzer/2".to_string(),
        context_id: String::new(),
        requested_scope,
        analyzer,
        excluded_fingerprints,
        sources,
        external_capabilities,
        workorder_set_digest,
    };
    context.context_id = proposal_context_id(&context)?;
    let work_orders = drafts
        .into_iter()
        .map(|draft| draft.into_work_order(&context))
        .collect();
    Ok(ProposalBatch {
        analysis,
        reports: scan.reports,
        context,
        work_orders,
    })
}

pub fn reconstruct_proposal(root: &Path, context: &ProposalContext) -> Result<ProposalBatch> {
    validate_proposal_context(context).map_err(anyhow::Error::msg)?;
    if context.analyzer_semantics != "deslop-analyzer/2" {
        bail!(
            "unsupported analyzer semantics `{}`",
            context.analyzer_semantics
        );
    }
    let root = root
        .canonicalize()
        .with_context(|| format!("failed to resolve verification root {}", root.display()))?;
    let mut analyzer_snapshot = context.analyzer.clone();
    restore_analyzer_paths(&root, &mut analyzer_snapshot)?;
    let mut config = analyzer_snapshot.to_config()?;
    config.suppression = config.suppression.clone().with_match_root(root.clone());
    let scope = context
        .requested_scope
        .iter()
        .map(|entry| {
            let path = root.join(&entry.path);
            let kind_matches = match entry.kind {
                ProposalScopeKind::File => path.is_file(),
                ProposalScopeKind::Directory => path.is_dir(),
            };
            if !kind_matches {
                bail!(
                    "proposal context no longer matches requested scope kind at {}",
                    entry.path.display()
                );
            }
            Ok(path)
        })
        .collect::<Result<Vec<_>>>()?;
    let scan = scan_paths_with_context(&scope, config)?;
    let rebuilt = proposal_batch_from_scan(
        &root,
        context.requested_scope.clone(),
        context.analyzer.clone(),
        context.excluded_fingerprints.clone(),
        scan,
    )?;
    if rebuilt.context.context_id != context.context_id {
        bail!("proposal context no longer matches current scope, sources, analysis, or capability");
    }
    Ok(rebuilt)
}

pub fn runtime_scope_matches(
    root: &Path,
    paths: &[PathBuf],
    context: &ProposalContext,
) -> Result<bool> {
    let root = root
        .canonicalize()
        .with_context(|| format!("failed to resolve verification root {}", root.display()))?;
    let normalized = normalize_scope(&root, paths)?;
    Ok(normalized.len() == context.requested_scope.len()
        && normalized
            .iter()
            .zip(&context.requested_scope)
            .all(|(left, right)| left.path == right.path && left.kind == right.kind))
}

fn normalize_scope(root: &Path, paths: &[PathBuf]) -> Result<Vec<ProposalScope>> {
    let paths = if paths.is_empty() {
        vec![root.to_path_buf()]
    } else {
        paths
            .iter()
            .map(|path| {
                if path.is_absolute() {
                    path.clone()
                } else {
                    root.join(path)
                }
            })
            .collect()
    };
    let mut scope = Vec::new();
    for path in paths {
        let kind = if path.is_file() {
            ProposalScopeKind::File
        } else if path.is_dir() {
            ProposalScopeKind::Directory
        } else {
            bail!("proposal scope does not exist: {}", path.display());
        };
        let canonical = path
            .canonicalize()
            .with_context(|| format!("failed to resolve proposal scope {}", path.display()))?;
        let relative = canonical.strip_prefix(root).with_context(|| {
            format!(
                "proposal scope {} escapes root {}",
                canonical.display(),
                root.display()
            )
        })?;
        scope.push(ProposalScope {
            path: relative.to_path_buf(),
            kind,
        });
    }
    scope.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(scope_kind_order(a.kind).cmp(&scope_kind_order(b.kind)))
    });
    scope.dedup_by(|a, b| a.path == b.path && a.kind == b.kind);
    let directories = scope
        .iter()
        .filter(|entry| entry.kind == ProposalScopeKind::Directory)
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    scope.retain(|entry| {
        !directories
            .iter()
            .any(|directory| entry.path != *directory && entry.path.starts_with(directory))
    });
    Ok(scope)
}

fn scope_kind_order(kind: ProposalScopeKind) -> u8 {
    match kind {
        ProposalScopeKind::File => 0,
        ProposalScopeKind::Directory => 1,
    }
}

fn proposal_sources(root: &Path, scan: &ScanContext) -> Result<Vec<ProposalSource>> {
    let analyses = scan
        .reports
        .iter()
        .map(|report| {
            let path = normalized_repo_path(root, &report.path)?;
            Ok((path, (report.lang, report.analysis.clone())))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut sources = Vec::new();
    for (path, text) in &scan.input_contents {
        let relative = normalized_repo_path(root, path)?;
        let (lang, analysis) = analyses
            .get(&relative)
            .cloned()
            .map_or((Lang::Generic, None), |(lang, analysis)| {
                (lang, Some(analysis))
            });
        let lines = text.lines().count().max(1);
        sources.push(ProposalSource {
            path: relative.clone(),
            lang,
            revision_guard: revision_guard(&relative, Span::new(1, lines, 0, text.len()), text),
            analysis,
        });
    }
    sources.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(sources)
}

fn normalize_external_capabilities(
    root: &Path,
    capabilities: Vec<ExternalCapability>,
) -> Result<Vec<ExternalCapability>> {
    let mut normalized = capabilities
        .into_iter()
        .map(|mut capability| {
            capability.path = normalized_repo_path(root, &capability.path)?;
            capability.covered_rules.sort();
            capability.covered_rules.dedup();
            Ok(capability)
        })
        .collect::<Result<Vec<_>>>()?;
    normalized.sort_by(|a, b| a.path.cmp(&b.path).then(a.analyzer.cmp(&b.analyzer)));
    Ok(normalized)
}

fn normalized_repo_path(root: &Path, path: &Path) -> Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("failed to resolve proposal input {}", path.display()))?;
    Ok(canonical
        .strip_prefix(root)
        .with_context(|| {
            format!(
                "proposal input {} escapes root {}",
                canonical.display(),
                root.display()
            )
        })?
        .to_path_buf())
}

fn normalize_analyzer_paths(root: &Path, analyzer: &mut AnalyzerConfigSnapshot) -> Result<()> {
    if let Some(path) = &analyzer.julia_project {
        analyzer.julia_project = Some(normalized_repo_path(root, path)?);
    }
    canonicalize_analyzer(analyzer);
    Ok(())
}

fn restore_analyzer_paths(root: &Path, analyzer: &mut AnalyzerConfigSnapshot) -> Result<()> {
    if let Some(path) = &analyzer.julia_project {
        let restored = root.join(path);
        let canonical = restored
            .canonicalize()
            .with_context(|| format!("failed to resolve Julia project {}", restored.display()))?;
        if !canonical.starts_with(root) {
            bail!("Julia project escapes verification root");
        }
        analyzer.julia_project = Some(canonical);
    }
    Ok(())
}

fn canonicalize_analyzer(analyzer: &mut AnalyzerConfigSnapshot) {
    analyzer.boundary.extra_sinks.sort();
    analyzer.boundary.extra_sinks.dedup();
    analyzer.boundary.ignore_keys.sort();
    analyzer.boundary.ignore_keys.dedup();
    analyzer.boundary.skip_artifacts.sort();
    analyzer.boundary.skip_artifacts.dedup();
}

fn proposal_context_id(context: &ProposalContext) -> Result<String> {
    let mut payload = context.clone();
    payload.context_id.clear();
    digest_json("deslop proposal context v1", &payload)
        .map(|digest| digest.replacen("dg1_", "pc1_", 1))
}

fn digest_json<T: Serialize>(domain: &str, value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value)?;
    let digest = blake3::derive_key(domain, &bytes);
    Ok(format!(
        "dg1_{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn region_span(region: &Region) -> Span {
    Span::new(
        region.start_line,
        region.end_line,
        region.start_byte,
        region.end_byte,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RewriteRegionKey {
    path: PathBuf,
    start_line: usize,
    end_line: usize,
    start_byte: usize,
    end_byte: usize,
    text: String,
}

impl RewriteRegionKey {
    fn new(path: &Path, region: Region) -> Self {
        Self {
            path: path.to_path_buf(),
            start_line: region.start_line,
            end_line: region.end_line,
            start_byte: region.start_byte,
            end_byte: region.end_byte,
            text: region.text,
        }
    }

    fn region(&self) -> Region {
        Region {
            start_line: self.start_line,
            end_line: self.end_line,
            start_byte: self.start_byte,
            end_byte: self.end_byte,
            text: self.text.to_owned(),
        }
    }
}

fn owned_enclosing_region(
    analysis: &ProjectAnalysis,
    logical_path: &Path,
    finding: &Finding,
) -> Option<(usize, usize)> {
    let start = finding.span.start_byte;
    let end = finding.span.end_byte.max(start.saturating_add(1));
    let owner = analysis
        .smallest_containing_named_syntax(logical_path, start..end)
        .ok()?;
    let SyntaxOwner::Node(node) = owner else {
        return None;
    };
    analysis
        .syntax_adapter_facts(logical_path)
        .ok()?
        .iter()
        .find(|fact| fact.node() == node.id())?
        .enclosing_region()
        .map(|region| (region.start_line, region.end_line))
}

fn region_for_finding(
    source: &SourceFile,
    finding: &Finding,
    enclosing_region: Option<(usize, usize)>,
) -> Region {
    let (start_line, end_line) =
        enclosing_region.unwrap_or((finding.span.start_line, finding.span.end_line));
    let start_byte = source.line_start_byte(start_line);
    let end_byte = source.line_start_byte(end_line + 1).min(source.text.len());
    Region {
        start_line,
        end_line,
        start_byte,
        end_byte,
        text: source
            .text
            .get(start_byte..end_byte)
            .unwrap_or("")
            .to_string(),
    }
}

fn sort_grouped_findings(findings: &mut [&Finding]) {
    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.span.start_line.cmp(&b.span.start_line))
            .then(a.rule.cmp(&b.rule))
            .then(a.span.end_line.cmp(&b.span.end_line))
            .then(a.span.start_byte.cmp(&b.span.start_byte))
            .then(a.span.end_byte.cmp(&b.span.end_byte))
            .then(a.fingerprint.cmp(&b.fingerprint))
            .then(a.severity.cmp(&b.severity))
            .then(safety_order(a.safety).cmp(&safety_order(b.safety)))
            .then(a.message.cmp(&b.message))
            .then(a.precondition.cmp(&b.precondition))
    });
}

fn safety_order(safety: SafetyClass) -> u8 {
    match safety {
        SafetyClass::SafeAuto => 0,
        SafetyClass::AnalyzerConfirmed => 1,
        SafetyClass::SafeWithPrecondition => 2,
        SafetyClass::RiskySuggest => 3,
        SafetyClass::LlmOnly => 4,
        SafetyClass::NeverAuto => 5,
    }
}

#[derive(Debug, Clone, Serialize)]
struct WorkOrderDraft {
    kind: WorkOrderKind,
    path: PathBuf,
    region: Region,
    region_fingerprint: String,
    revision_guard: RevisionGuard,
    findings: Vec<WorkOrderFinding>,
    instruction: String,
    contract: Contract,
}

impl WorkOrderDraft {
    fn normalize_path(&mut self, root: &Path) -> Result<()> {
        self.path = normalized_repo_path(root, &self.path)?;
        self.region_fingerprint = region_fingerprint(&self.path, &self.region);
        self.revision_guard = region_revision_guard(&self.path, &self.region);
        Ok(())
    }

    fn into_work_order(self, context: &ProposalContext) -> WorkOrder {
        let id = workorder_id_for_context(&self.path, &self.region, context);
        WorkOrder {
            schema: "deslop.workorder/3".to_string(),
            kind: self.kind,
            id,
            path: self.path,
            region: self.region,
            region_fingerprint: self.region_fingerprint,
            revision_guard: self.revision_guard,
            proposal_context: context.clone(),
            findings: self.findings,
            instruction: self.instruction,
            contract: self.contract,
        }
    }
}

fn work_order_for_findings(key: RewriteRegionKey, findings: Vec<&Finding>) -> WorkOrderDraft {
    let region = key.region();
    let region_fingerprint = region_fingerprint(&key.path, &region);
    let revision_guard = region_revision_guard(&key.path, &region);
    WorkOrderDraft {
        kind: WorkOrderKind::RewriteRegion,
        path: key.path,
        region,
        region_fingerprint,
        revision_guard,
        findings: findings.into_iter().map(work_order_finding).collect(),
        instruction: "Rewrite the region to address every listed finding that can be resolved without changing behavior or the public API. The safety contract wins if findings conflict. Preserve language and indentation.".to_string(),
        contract: Contract::default(),
    }
}

fn work_order_finding(finding: &Finding) -> WorkOrderFinding {
    WorkOrderFinding {
        rule: finding.rule.to_owned(),
        severity: finding.severity,
        safety: finding.safety,
        message: finding.message.to_owned(),
        precondition: finding.precondition.to_owned(),
    }
}

pub fn characterization_work_order_for(work_order: &WorkOrder) -> WorkOrder {
    WorkOrder {
        schema: "deslop.workorder/3".to_string(),
        kind: WorkOrderKind::NeedsCharacterizationTest,
        id: work_order.id.to_owned(),
        path: work_order.path.to_path_buf(),
        region: work_order.region.clone(),
        region_fingerprint: work_order.region_fingerprint.to_owned(),
        revision_guard: work_order.revision_guard.clone(),
        proposal_context: work_order.proposal_context.clone(),
        findings: vec![WorkOrderFinding {
            rule: "needs-characterization-test".to_string(),
            severity: Severity::Major,
            safety: SafetyClass::LlmOnly,
            message: "region has a weak test oracle; generate a characterization test before removal".to_string(),
            precondition: None,
        }],
        instruction: "Write a test that pins the current observable behavior of this exact region. Do not change production behavior. Return deslop.characterization-test/3 JSONL with test_path and test_text; copy proposal_context exactly; the test must compile and pass against the current unmodified code.".to_string(),
        contract: Contract {
            must_parse: true,
            no_new_public_defs: false,
            keep_error_handling: true,
            max_growth_ratio: 1.0,
            check_cmd: work_order.contract.check_cmd.to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_core::{DetectedBy, SafetyClass, Severity, Span};
    use std::fs;

    fn finding(source: &SourceFile, line: usize, rule: &str, safety: SafetyClass) -> Finding {
        Finding {
            path: source.path.to_path_buf(),
            span: Span::new(
                line,
                line,
                source.line_start_byte(line),
                source.line_end_byte(line),
            ),
            rule: rule.to_string(),
            severity: Severity::Minor,
            safety,
            detected_by: DetectedBy::Idiom,
            message: format!("{rule} message"),
            suggestion: format!("{rule} suggestion"),
            precondition: None,
            edit: None,
            fingerprint: format!("finding-{line}-{rule}"),
        }
    }

    #[test]
    fn proposal_production_uses_retained_analysis_and_pinned_sources() {
        let source = include_str!("lib.rs");
        let start = source.find("pub fn propose_work_orders(").unwrap();
        let end = source[start..]
            .find("fn normalize_external_capabilities(")
            .unwrap()
            + start;
        let production = &source[start..end];
        for forbidden in [
            "parse_source",
            "analysis_provenance_or_failed",
            "SourceFile::read",
            "read_to_string",
            "enclosing_region_for_span",
            "pack_for_path",
            "pack_for_lang",
        ] {
            assert!(
                !production.contains(forbidden),
                "proposal production reintroduced {forbidden}"
            );
        }

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("sample.rs");
        fs::write(&path, "fn sample() { todo!(); }\n").unwrap();
        deslop_parse::reset_parse_source_invocations();

        let first = propose_work_orders(
            root.path(),
            std::slice::from_ref(&path),
            AnalyzerConfig::default(),
        )
        .unwrap();
        let second = propose_work_orders(
            root.path(),
            std::slice::from_ref(&path),
            AnalyzerConfig::default(),
        )
        .unwrap();

        assert_eq!(
            serde_json::to_value(&first.work_orders).unwrap(),
            serde_json::to_value(&second.work_orders).unwrap()
        );
        assert_eq!(first.work_orders.len(), 1);
        for batch in [&first, &second] {
            assert!(batch.analysis.instrumentation().parse.invariant_holds());
            assert_eq!(batch.analysis.parse_counts().len(), 1);
            assert!(batch.analysis.parse_counts().values().all(|count| {
                (
                    count.requested,
                    count.owners,
                    count.parser_invocations,
                    count.reused,
                ) == (1, 1, 1, 0)
            }));
        }
        assert_eq!(deslop_parse::parse_source_invocations(), 0);
    }

    #[test]
    fn workorder_schema_matches_spec_surface() {
        let source = SourceFile::new(PathBuf::from("sample.clj"), "(= (count xs) 0)\n".into());
        let finding = Finding {
            path: source.path.to_path_buf(),
            span: Span::new(1, 1, 0, source.text.len()),
            rule: "reimpl-empty?".to_string(),
            severity: Severity::Minor,
            safety: SafetyClass::SafeWithPrecondition,
            detected_by: DetectedBy::Idiom,
            message: "message".to_string(),
            suggestion: "suggestion".to_string(),
            precondition: Some("finite".to_string()),
            edit: None,
            fingerprint: "finding".to_string(),
        };
        let work_order = work_orders_for_source(&source, &[finding]).remove(0);
        let value = serde_json::to_value(&work_order).expect("json");
        assert_eq!(value["schema"], "deslop.workorder/3");
        assert_eq!(value["kind"], "rewrite-region");
        assert!(
            value["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("wo3_"))
        );
        assert!(value.get("path").is_some());
        assert_eq!(value["region"]["start_byte"], 0);
        assert_eq!(value["region"]["end_byte"], source.text.len());
        assert!(value.get("findings").is_some());
        assert!(value.get("instruction").is_some());
        assert!(value.get("contract").is_some());
        assert!(value["region_fingerprint"].is_string());
        assert!(
            value["revision_guard"]
                .as_str()
                .is_some_and(|guard| guard.starts_with("rg1_"))
        );
        validate_workorder_identity(&work_order).expect("valid generated identity");
    }

    #[test]
    fn region_identity_survives_outer_whitespace_but_context_bound_workorder_expires() {
        let original = SourceFile::new(PathBuf::from("sample.rs"), "value();\n".into());
        let changed = SourceFile::new(PathBuf::from("sample.rs"), " value();\n".into());
        let original_order = work_orders_for_source(
            &original,
            &[finding(&original, 1, "long-method", SafetyClass::LlmOnly)],
        )
        .remove(0);
        let changed_order = work_orders_for_source(
            &changed,
            &[finding(&changed, 1, "long-method", SafetyClass::LlmOnly)],
        )
        .remove(0);

        assert_eq!(
            original_order.region_fingerprint,
            changed_order.region_fingerprint
        );
        assert_ne!(original_order.id, changed_order.id);
        assert_ne!(original_order.revision_guard, changed_order.revision_guard);
    }

    #[test]
    fn partial_unknown_and_mismatched_reports_cannot_create_workorders() {
        let source = SourceFile::new(
            PathBuf::from("malformed.ts"),
            include_str!("../../../tests/fixtures/typescript/malformed.ts").to_string(),
        );
        let injected = finding(&source, 1, "narrating-comment", SafetyClass::LlmOnly);
        let partial = FileReport {
            path: source.path.clone(),
            lang: source.lang,
            analysis: deslop_parse::analysis_provenance_or_failed(&source),
            findings: vec![injected.clone()],
        };
        let unknown = FileReport {
            analysis: deslop_core::AnalysisProvenance::default(),
            ..partial.clone()
        };
        let mismatched = FileReport {
            path: PathBuf::from("other.ts"),
            analysis: deslop_core::AnalysisProvenance::complete(),
            ..partial.clone()
        };

        assert!(work_orders_for_report(&source, &partial).is_empty());
        assert!(work_orders_for_report(&source, &unknown).is_empty());
        assert!(work_orders_for_report(&source, &mismatched).is_empty());
    }

    #[test]
    fn groups_all_proposable_findings_in_the_same_enclosing_region() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn noisy() {\n    todo!();\n    println!(\"narration\");\n}\n".into(),
        );
        let findings = vec![
            finding(&source, 3, "narrating-comment", SafetyClass::LlmOnly),
            finding(&source, 2, "placeholder", SafetyClass::RiskySuggest),
            finding(&source, 2, "safe-format", SafetyClass::SafeAuto),
        ];

        let work_orders = work_orders_for_source(&source, &findings);

        assert_eq!(work_orders.len(), 1);
        assert_eq!(work_orders[0].region.start_line, 1);
        assert_eq!(work_orders[0].region.end_line, 4);
        assert_eq!(work_orders[0].findings.len(), 2);
        assert_eq!(work_orders[0].findings[0].rule, "placeholder");
        assert_eq!(work_orders[0].findings[1].rule, "narrating-comment");
    }

    #[test]
    fn never_auto_findings_quarantine_their_rewrite_region() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn noisy() {\n    todo!();\n}\n".into(),
        );
        let report_only = finding(&source, 2, "config-key-unread", SafetyClass::NeverAuto);
        let proposable = finding(&source, 2, "placeholder", SafetyClass::LlmOnly);

        assert!(work_orders_for_source(&source, std::slice::from_ref(&report_only)).is_empty());

        assert!(work_orders_for_source(&source, &[report_only, proposable]).is_empty());
    }

    #[test]
    fn never_auto_findings_do_not_quarantine_disjoint_rewrite_regions() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn report_only() {\n    todo!();\n}\n\nfn rewrite() {\n    todo!();\n}\n".into(),
        );
        let findings = [
            finding(&source, 2, "missing-reference", SafetyClass::NeverAuto),
            finding(&source, 6, "placeholder", SafetyClass::LlmOnly),
        ];

        let work_orders = work_orders_for_source(&source, &findings);
        assert_eq!(work_orders.len(), 1);
        assert_eq!(work_orders[0].region.start_line, 5);
        assert_eq!(work_orders[0].findings[0].rule, "placeholder");
    }

    #[test]
    fn nested_never_auto_evidence_quarantines_an_outer_rewrite_region() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn outer() {\n    todo!();\n    fn inner() {\n        todo!();\n    }\n}\n".into(),
        );
        let findings = [
            finding(&source, 2, "outer-placeholder", SafetyClass::LlmOnly),
            finding(&source, 4, "missing-reference", SafetyClass::NeverAuto),
        ];

        assert!(work_orders_for_source(&source, &findings).is_empty());
    }

    #[test]
    fn identity_validation_rejects_non_proposable_findings() {
        let source = SourceFile::new(PathBuf::from("sample.rs"), "fn noisy() {}\n".into());
        let mut work_order = work_orders_for_source(
            &source,
            &[finding(&source, 1, "long-method", SafetyClass::LlmOnly)],
        )
        .remove(0);
        work_order.findings[0].safety = SafetyClass::NeverAuto;

        let error = validate_workorder_identity(&work_order).expect_err("report-only workorder");
        assert!(error.contains("non-proposable safety class NeverAuto"));
    }

    #[test]
    fn zero_width_never_auto_evidence_quarantines_its_region() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn noisy() {\n    todo!();\n}\n".into(),
        );
        let mut report_only = finding(&source, 2, "missing-reference", SafetyClass::NeverAuto);
        report_only.span.end_byte = report_only.span.start_byte;
        let proposable = finding(&source, 2, "placeholder", SafetyClass::LlmOnly);

        assert!(work_orders_for_source(&source, &[report_only, proposable]).is_empty());
    }

    #[test]
    fn typed_tsx_finding_targets_the_enclosing_component() {
        let source = SourceFile::new(
            PathBuf::from("component.tsx"),
            include_str!("../../../tests/fixtures/typescript/component.tsx").to_string(),
        );
        let work_orders = work_orders_for_source(
            &source,
            &[finding(
                &source,
                14,
                "typed-component-cleanup",
                SafetyClass::LlmOnly,
            )],
        );

        assert_eq!(source.lang, deslop_core::Lang::TypeScript);
        assert_eq!(work_orders.len(), 1);
        assert_eq!(work_orders[0].region.start_line, 11);
        assert_eq!(work_orders[0].region.end_line, 21);
        assert!(work_orders[0].region.text.contains("function View"));
    }

    #[test]
    fn python_findings_target_decorated_and_nested_callable_regions() {
        let source = SourceFile::new(
            PathBuf::from("behavioral.py"),
            include_str!("../../../tests/fixtures/python/behavioral.py").to_string(),
        );
        let findings = vec![
            finding(&source, 14, "async-cleanup", SafetyClass::LlmOnly),
            finding(&source, 16, "nested-cleanup", SafetyClass::LlmOnly),
        ];
        let work_orders = work_orders_for_source(&source, &findings);

        assert_eq!(source.lang, deslop_core::Lang::Python);
        assert_eq!(work_orders.len(), 2);
        assert_eq!(work_orders[0].region.start_line, 13);
        assert_eq!(work_orders[0].region.end_line, 18);
        assert!(work_orders[0].region.text.starts_with("    @traced"));
        assert_eq!(work_orders[1].region.start_line, 15);
        assert_eq!(work_orders[1].region.end_line, 16);
        assert!(work_orders[1].region.text.contains("def normalize"));
    }

    #[test]
    fn emits_distinct_unique_orders_for_distinct_regions() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn first() {\n    todo!();\n}\n\nfn second() {\n    todo!();\n}\n".into(),
        );
        let findings = vec![
            finding(&source, 6, "placeholder", SafetyClass::LlmOnly),
            finding(&source, 2, "placeholder", SafetyClass::LlmOnly),
        ];

        let work_orders = work_orders_for_source(&source, &findings);

        assert_eq!(work_orders.len(), 2);
        assert_eq!(work_orders[0].region.start_line, 1);
        assert_eq!(work_orders[1].region.start_line, 5);
        assert_ne!(work_orders[0].id, work_orders[1].id);
    }

    #[test]
    fn grouping_is_invariant_to_finding_input_order() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn noisy() {\n    todo!();\n    println!(\"narration\");\n}\n".into(),
        );
        let mut left = finding(&source, 2, "placeholder", SafetyClass::RiskySuggest);
        left.fingerprint = "shared-fingerprint".to_string();
        left.message = "first message".to_string();
        let mut right = finding(&source, 2, "placeholder", SafetyClass::LlmOnly);
        right.fingerprint = "shared-fingerprint".to_string();
        right.message = "second message".to_string();

        let forward = work_orders_for_source(&source, &[left.clone(), right.clone()]);
        let reversed = work_orders_for_source(&source, &[right, left]);

        assert_eq!(
            serde_json::to_value(forward).expect("forward JSON"),
            serde_json::to_value(reversed).expect("reversed JSON")
        );
    }

    #[test]
    fn source_path_is_the_authoritative_group_and_identity_path() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn noisy() {\n    todo!();\n    println!(\"narration\");\n}\n".into(),
        );
        let direct = finding(&source, 2, "placeholder", SafetyClass::RiskySuggest);
        let mut equivalent = finding(&source, 3, "narration", SafetyClass::LlmOnly);
        equivalent.path = PathBuf::from("./sample.rs");

        let work_orders = work_orders_for_source(&source, &[direct, equivalent]);

        assert_eq!(work_orders.len(), 1);
        assert_eq!(work_orders[0].path, source.path);
        assert_eq!(work_orders[0].findings.len(), 2);
    }

    #[test]
    fn overlapping_nested_callable_regions_remain_distinct_targets() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn outer() {\n    todo!();\n    fn inner() {\n        todo!();\n    }\n}\n".into(),
        );
        let findings = vec![
            finding(&source, 2, "outer-placeholder", SafetyClass::LlmOnly),
            finding(&source, 4, "inner-placeholder", SafetyClass::LlmOnly),
        ];

        let work_orders = work_orders_for_source(&source, &findings);

        assert_eq!(work_orders.len(), 2);
        assert_eq!(
            work_orders
                .iter()
                .map(|work_order| (work_order.region.start_line, work_order.region.end_line))
                .collect::<Vec<_>>(),
            vec![(1, 6), (3, 5)]
        );
        assert_ne!(work_orders[0].id, work_orders[1].id);
    }

    #[test]
    fn non_default_analyzer_context_reconstructs_without_default_rescan() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("short.rs");
        fs::write(
            &source,
            "fn short() {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    println!(\"{}\", a + b + c);\n}\n",
        )
        .expect("source");
        let config = AnalyzerConfig {
            long_method_nloc: 4,
            ..AnalyzerConfig::default()
        };

        let batch = propose_work_orders(temp.path(), &[source], config).expect("proposal");
        assert_eq!(batch.context.analyzer.long_method_nloc, 4);
        assert!(batch.work_orders.iter().any(|work_order| {
            work_order
                .findings
                .iter()
                .any(|finding| finding.rule == "long-method")
        }));

        let rebuilt = reconstruct_proposal(temp.path(), &batch.context).expect("reconstruct");
        assert_eq!(rebuilt.context.context_id, batch.context.context_id);
        assert_eq!(
            rebuilt.context.workorder_set_digest,
            batch.context.workorder_set_digest
        );

        let finding = batch
            .reports
            .iter()
            .flat_map(|report| &report.findings)
            .find(|finding| finding.rule == "long-method")
            .expect("long-method finding");
        let relative_source = SourceFile::new_with_lang(
            PathBuf::from("short.rs"),
            fs::read_to_string(temp.path().join("short.rs")).expect("source text"),
            Lang::Rust,
        );
        let normalized_fingerprint = baseline_fingerprint(
            &relative_source.path,
            &finding.rule,
            finding.span,
            &relative_source.region_text(finding.span.start_line, finding.span.end_line),
        );
        let filtered = propose_work_orders_with_exclusions(
            temp.path(),
            &[temp.path().join("short.rs")],
            AnalyzerConfig {
                long_method_nloc: 4,
                ..AnalyzerConfig::default()
            },
            std::slice::from_ref(&normalized_fingerprint),
        )
        .expect("baseline-filtered proposal");
        assert_eq!(
            filtered.context.excluded_fingerprints,
            [normalized_fingerprint]
        );
        assert!(!filtered.work_orders.iter().any(|work_order| {
            work_order
                .findings
                .iter()
                .any(|finding| finding.rule == "long-method")
        }));
    }

    #[test]
    fn peer_revision_and_context_tampering_fail_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("target.rs"),
            "fn target() -> i32 { return 1; }\n",
        )
        .expect("target");
        let peer = temp.path().join("peer.rs");
        fs::write(&peer, "fn peer() -> i32 { 2 }\n").expect("peer");
        let batch = propose_work_orders(
            temp.path(),
            &[temp.path().to_path_buf()],
            AnalyzerConfig::default(),
        )
        .expect("proposal");

        let mut tampered = batch.context.clone();
        tampered.analyzer.long_method_nloc += 1;
        assert!(validate_proposal_context(&tampered).is_err());

        let mut escaping = batch.context.clone();
        escaping.requested_scope[0].path = PathBuf::from("../outside");
        escaping.context_id = proposal_context_id(&escaping).expect("recomputed digest");
        assert!(
            validate_proposal_context(&escaping)
                .expect_err("root escape must fail")
                .contains("root-relative")
        );

        let mut legacy_semantics = batch.context.clone();
        legacy_semantics.analyzer_semantics = "deslop-analyzer/1".to_string();
        legacy_semantics.context_id =
            proposal_context_id(&legacy_semantics).expect("recomputed context id");
        let error = reconstruct_proposal(temp.path(), &legacy_semantics)
            .expect_err("legacy proposal semantics must expire");
        assert!(error.to_string().contains("unsupported analyzer semantics"));

        fs::write(peer, "fn peer() -> i32 { 3 }\n").expect("mutate peer");
        let error = reconstruct_proposal(temp.path(), &batch.context)
            .expect_err("peer revision must expire context");
        assert!(
            error
                .to_string()
                .contains("proposal context no longer matches")
        );
    }
}
