use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use deslop_core::{
    AnalysisDiagnostic, AnalysisProvenance, DetectedBy, Edit, FileReport, Finding, Lang,
    SafetyClass, Severity, Span, baseline_fingerprint, reports_permit_rewrites,
};
use deslop_external::{CljKondoAnalyzer, ExternalAnalyzer as ExternalAnalyzerTrait, JuliaAnalyzer};
use deslop_lang::LangPack;
use deslop_parse::{
    DiscoveryPolicy, NodeId, ParsedFile, ProjectAnalysis, ProjectSnapshotPlanner,
    ProjectSnapshotRequest, ProjectionId, RepositorySpec, RootSpec, ScopeSpec,
    SnapshotPresentationMap, SourceFile, SyntaxAdapterFacts,
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

mod agnostic;
mod boundary;
mod clojure;
mod julia;
mod packs;

pub use boundary::BoundaryConfig;
#[cfg(test)]
mod tests;
mod tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JuliaExternal {
    Off,
    StaticLint,
    Jet,
}

#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    pub min_duplication_tokens: usize,
    pub long_method_nloc: usize,
    pub min_meaningful_tokens: usize,
    pub rust: AnalyzerLangConfig,
    pub clojure: AnalyzerLangConfig,
    pub julia: AnalyzerLangConfig,
    pub python: AnalyzerLangConfig,
    pub javascript: AnalyzerLangConfig,
    pub typescript: AnalyzerLangConfig,
    pub generic: AnalyzerLangConfig,
    pub rust_external: bool,
    pub julia_external: JuliaExternal,
    pub julia_project: Option<PathBuf>,
    /// Per-rule and per-path finding suppression. Empty by default (no-op).
    pub suppression: Suppression,
    /// Config-boundary analysis (config-key-unread/-unconsumed/-shadowed).
    pub boundary: BoundaryConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzerLangConfig {
    pub long_method_nloc: Option<usize>,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            min_duplication_tokens: 24,
            long_method_nloc: 40,
            min_meaningful_tokens: 8,
            rust: AnalyzerLangConfig::default(),
            clojure: AnalyzerLangConfig::default(),
            julia: AnalyzerLangConfig::default(),
            python: AnalyzerLangConfig::default(),
            javascript: AnalyzerLangConfig::default(),
            typescript: AnalyzerLangConfig::default(),
            generic: AnalyzerLangConfig::default(),
            rust_external: false,
            julia_external: JuliaExternal::Off,
            julia_project: None,
            suppression: Suppression::default(),
            boundary: BoundaryConfig::default(),
        }
    }
}

/// Returns whether `rule` is a rule name deslop knows how to emit.
///
/// Delegates to the canonical registry in [`deslop_core::rules`] so suppression validation,
/// `deslop rules`, and the MCP `rules` tool all share one source of truth.
pub fn is_known_rule(rule: &str) -> bool {
    deslop_core::rules::is_known(rule)
}

/// Compiled per-rule / per-path finding suppression.
///
/// Filtering happens after findings are produced, so it applies uniformly to every
/// analyzer pack and to external-analyzer findings without each rule needing to know
/// about it. An empty `Suppression` is a no-op and matches unconfigured behavior.
#[derive(Debug, Clone, Default)]
pub struct Suppression {
    inner: Arc<SuppressionInner>,
    match_root: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct SuppressionInner {
    /// Rules dropped entirely, regardless of path.
    disabled_rules: HashSet<String>,
    /// Paths skipped for every rule.
    global_ignore: Option<GlobSet>,
    /// Paths skipped for a specific rule only.
    per_rule_ignore: HashMap<String, GlobSet>,
    canonical: SuppressionConfig,
}

/// Canonical, serializable suppression semantics used by proposal reconstruction.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuppressionConfig {
    pub disabled_rules: Vec<String>,
    pub ignore_paths: Vec<String>,
    pub rules: BTreeMap<String, Vec<String>>,
}

/// Canonical effective analyzer settings. This stores resolved behavior, not sparse CLI/MCP input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzerConfigSnapshot {
    pub min_duplication_tokens: usize,
    pub long_method_nloc: usize,
    pub min_meaningful_tokens: usize,
    pub rust: AnalyzerLangConfig,
    pub clojure: AnalyzerLangConfig,
    pub julia: AnalyzerLangConfig,
    pub python: AnalyzerLangConfig,
    pub javascript: AnalyzerLangConfig,
    pub typescript: AnalyzerLangConfig,
    pub generic: AnalyzerLangConfig,
    pub rust_external: bool,
    pub julia_external: JuliaExternal,
    pub julia_project: Option<PathBuf>,
    pub suppression: SuppressionConfig,
    pub boundary: BoundaryConfig,
}

/// Proposal-time observation of one optional external analyzer on one source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalCapability {
    pub path: PathBuf,
    pub analyzer: String,
    pub available: bool,
    pub covered_rules: Vec<String>,
}

/// Full scan result needed to reproduce proposal membership without hidden defaults.
#[derive(Debug, Clone)]
pub struct ScanContext {
    pub analysis: Arc<ProjectAnalysis>,
    pub presentation: SnapshotPresentationMap,
    pub reports: Vec<FileReport>,
    pub input_contents: BTreeMap<PathBuf, String>,
    pub external_capabilities: Vec<ExternalCapability>,
}

const ANALYZER_PROJECTION_SCHEMA: &str = "deslop.analyzer.projection/1";
const ANALYZER_CAPABILITIES: &[u8] =
    b"rules=deslop.analyzer-owned/1\0boundary=disabled\0external=pinned-unavailable";
const PREPARED_ANALYZER_CAPABILITIES: &[u8] =
    b"rules=deslop.analyzer-owned/1\0boundary=pinned-complete/1\0external=pinned-unavailable";

#[derive(Debug)]
pub struct AnalyzerProjection {
    pub id: ProjectionId,
    pub analysis: Arc<ProjectAnalysis>,
    pub presentation: SnapshotPresentationMap,
    pub config: AnalyzerConfigSnapshot,
    pub reports: Vec<FileReport>,
    pub input_contents: BTreeMap<PathBuf, String>,
    pub external_capabilities: Vec<ExternalCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum BoundaryCoverage {
    Complete,
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzerInputManifest {
    report_sources: Vec<PathBuf>,
    boundary_artifacts: Vec<PathBuf>,
    boundary_coverage: BoundaryCoverage,
    external_unavailable_reason: String,
}

#[derive(Debug, Clone)]
pub struct PreparedAnalyzerAnalysis {
    analysis: Arc<ProjectAnalysis>,
    inputs: AnalyzerInputManifest,
    presentation: SnapshotPresentationMap,
}

#[derive(Debug, Clone)]
pub(crate) struct AnalyzerText {
    pub(crate) path: PathBuf,
    pub(crate) lang: Lang,
    pub(crate) text: String,
    line_starts: Vec<usize>,
}

impl AnalyzerText {
    fn new(path: PathBuf, text: String, lang: Lang) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            text.bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        Self {
            path,
            lang,
            text,
            line_starts,
        }
    }

    pub(crate) fn lines(&self) -> Vec<&str> {
        self.text.lines().collect()
    }

    pub(crate) fn line_start_byte(&self, one_based_line: usize) -> usize {
        self.line_starts
            .get(one_based_line.saturating_sub(1))
            .copied()
            .unwrap_or(self.text.len())
    }

    pub(crate) fn line_end_byte(&self, one_based_line: usize) -> usize {
        self.line_starts
            .get(one_based_line)
            .copied()
            .map(|index| index.saturating_sub(1))
            .unwrap_or(self.text.len())
    }

    pub(crate) fn region_text(&self, start_line: usize, end_line: usize) -> String {
        let start = self.line_start_byte(start_line);
        let end = self
            .line_starts
            .get(end_line)
            .copied()
            .unwrap_or(self.text.len());
        self.text.get(start..end).unwrap_or("").to_string()
    }

    pub(crate) fn line_for_byte(&self, byte: usize) -> usize {
        match self.line_starts.binary_search(&byte) {
            Ok(index) => index + 1,
            Err(index) => index,
        }
        .max(1)
    }
}

pub(crate) trait TextSource {
    fn path(&self) -> &Path;
    fn text(&self) -> &str;
    fn line_start_byte(&self, one_based_line: usize) -> usize;
    fn line_end_byte(&self, one_based_line: usize) -> usize;
    fn region_text(&self, start_line: usize, end_line: usize) -> String;
    fn line_for_byte(&self, byte: usize) -> usize;
    fn lines(&self) -> Vec<&str> {
        self.text().lines().collect()
    }
}

impl TextSource for AnalyzerText {
    fn path(&self) -> &Path {
        &self.path
    }

    fn text(&self) -> &str {
        &self.text
    }

    fn line_start_byte(&self, one_based_line: usize) -> usize {
        self.line_start_byte(one_based_line)
    }

    fn line_end_byte(&self, one_based_line: usize) -> usize {
        self.line_end_byte(one_based_line)
    }

    fn region_text(&self, start_line: usize, end_line: usize) -> String {
        self.region_text(start_line, end_line)
    }

    fn line_for_byte(&self, byte: usize) -> usize {
        self.line_for_byte(byte)
    }
}

impl TextSource for SourceFile {
    fn path(&self) -> &Path {
        &self.path
    }

    fn text(&self) -> &str {
        &self.text
    }

    fn line_start_byte(&self, one_based_line: usize) -> usize {
        self.line_start_byte(one_based_line)
    }

    fn line_end_byte(&self, one_based_line: usize) -> usize {
        self.line_end_byte(one_based_line)
    }

    fn region_text(&self, start_line: usize, end_line: usize) -> String {
        self.region_text(start_line, end_line)
    }

    fn line_for_byte(&self, byte: usize) -> usize {
        self.line_for_byte(byte)
    }
}

impl PreparedAnalyzerAnalysis {
    fn new(
        analysis: Arc<ProjectAnalysis>,
        mut inputs: AnalyzerInputManifest,
        presentation: SnapshotPresentationMap,
    ) -> Result<Self> {
        inputs.report_sources.sort();
        inputs.report_sources.dedup();
        inputs.boundary_artifacts.sort();
        inputs.boundary_artifacts.dedup();
        for path in &inputs.report_sources {
            if analysis.file(path).is_none() {
                bail!(
                    "prepared analyzer report source {} is not parsed",
                    path.display()
                );
            }
        }
        for path in &inputs.boundary_artifacts {
            if analysis.snapshot().entry(path).is_none() {
                bail!(
                    "prepared boundary artifact {} is not pinned",
                    path.display()
                );
            }
        }
        Ok(Self {
            analysis,
            inputs,
            presentation,
        })
    }
}

/// One analyzer view over a file already parsed and owned by `ProjectAnalysis`.
///
/// `source` is a compatibility text view over pinned bytes; syntax authority remains
/// the analysis-owned arena and exact stored adapter facts.
pub struct AnalyzerFile<'analysis> {
    pub analysis: &'analysis ProjectAnalysis,
    pub file: &'analysis ParsedFile,
    source: AnalyzerText,
    adapter: &'static dyn LangPack,
    facts: Box<[SyntaxAdapterFacts]>,
    facts_by_node: HashMap<NodeId, usize>,
}

impl<'analysis> AnalyzerFile<'analysis> {
    pub fn new(analysis: &'analysis ProjectAnalysis, file: &'analysis ParsedFile) -> Result<Self> {
        Self::new_with_path(analysis, file, file.key().path.clone())
    }

    fn new_with_path(
        analysis: &'analysis ProjectAnalysis,
        file: &'analysis ParsedFile,
        display_path: PathBuf,
    ) -> Result<Self> {
        let text = file.text().ok_or_else(|| {
            anyhow::anyhow!("syntax text unavailable for {}", file.key().path.display())
        })?;
        let adapter = analysis.language_adapter(&file.key().path).ok_or_else(|| {
            anyhow::anyhow!(
                "stored language adapter unavailable for {}",
                file.key().path.display()
            )
        })?;
        let facts = analysis.syntax_adapter_facts(&file.key().path)?;
        let facts_by_node = facts
            .iter()
            .enumerate()
            .map(|(index, fact)| (fact.node(), index))
            .collect();
        Ok(Self {
            analysis,
            file,
            source: AnalyzerText::new(display_path, text.to_string(), file.grammar().lang()),
            adapter,
            facts,
            facts_by_node,
        })
    }

    pub(crate) fn source(&self) -> &AnalyzerText {
        &self.source
    }

    pub fn adapter(&self) -> &'static dyn LangPack {
        self.adapter
    }

    pub fn node_ids(&self) -> deslop_parse::NodeIds {
        self.analysis
            .file_node_ids(&self.file.key().path)
            .expect("an analyzer file owns its node range")
    }

    pub fn fact(&self, node: NodeId) -> &SyntaxAdapterFacts {
        &self.facts[self.facts_by_node[&node]]
    }

    pub fn child_by_field(&self, node: NodeId, field: &str) -> Option<NodeId> {
        self.analysis.node(node).ok()?.children().find(|child| {
            self.analysis
                .node(*child)
                .is_ok_and(|view| view.field() == Some(field))
        })
    }
}

impl Suppression {
    /// Start building a `Suppression`. Rule names are validated and globs are compiled
    /// on [`SuppressionBuilder::build`].
    pub fn builder() -> SuppressionBuilder {
        SuppressionBuilder::default()
    }

    /// True when nothing is configured, so filtering can be skipped entirely.
    pub fn is_empty(&self) -> bool {
        self.inner.disabled_rules.is_empty()
            && self.inner.global_ignore.is_none()
            && self.inner.per_rule_ignore.is_empty()
    }

    pub fn config(&self) -> &SuppressionConfig {
        &self.inner.canonical
    }

    pub fn with_match_root(mut self, root: PathBuf) -> Self {
        self.match_root = Some(root);
        self
    }

    fn suppresses(&self, finding: &Finding) -> bool {
        if self.inner.disabled_rules.contains(&finding.rule) {
            return true;
        }
        let candidate = match_path(&finding.path, self.match_root.as_deref());
        if let Some(set) = &self.inner.global_ignore
            && set.is_match(&candidate)
        {
            return true;
        }
        if let Some(set) = self.inner.per_rule_ignore.get(&finding.rule)
            && set.is_match(&candidate)
        {
            return true;
        }
        false
    }

    /// Drop suppressed findings in place.
    pub fn retain(&self, findings: &mut Vec<Finding>) {
        if self.is_empty() {
            return;
        }
        findings.retain(|finding| !self.suppresses(finding));
    }
}

/// Normalize a finding path for glob matching by stripping a leading `./`, so that
/// `crates/**` matches whether the scan root was `.` or an explicit directory.
fn match_path(path: &Path, root: Option<&Path>) -> PathBuf {
    root.and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or_else(|| path.strip_prefix("./").unwrap_or(path))
        .to_path_buf()
}

/// Builder for [`Suppression`]. Accumulates raw inputs from one or more config sources,
/// then validates rule names and compiles globs once on [`build`](Self::build).
#[derive(Debug, Default)]
pub struct SuppressionBuilder {
    disabled_rules: HashSet<String>,
    global_globs: Vec<String>,
    per_rule_globs: BTreeMap<String, Vec<String>>,
}

/// One rule's controls from an `[analyzer.rules.<rule>]` table, in borrowed form, so the CLI
/// and MCP config structs can feed [`SuppressionBuilder::add_section`] without each restating
/// what the keys mean.
#[derive(Debug, Clone, Copy)]
pub struct RuleSuppression<'a> {
    /// `false` disables the rule (same as listing it in `disabled_rules`); `None`/`true` leave it on.
    pub enabled: Option<bool>,
    /// Path globs skipped for this rule only.
    pub ignore_paths: &'a [String],
}

impl SuppressionBuilder {
    /// Disable a rule entirely.
    pub fn disable_rule(&mut self, rule: impl Into<String>) -> &mut Self {
        self.disabled_rules.insert(rule.into());
        self
    }

    /// Skip a path glob for every rule.
    pub fn ignore_path(&mut self, glob: impl Into<String>) -> &mut Self {
        self.global_globs.push(glob.into());
        self
    }

    /// Skip a path glob for a single rule.
    pub fn ignore_path_for_rule(
        &mut self,
        rule: impl Into<String>,
        glob: impl Into<String>,
    ) -> &mut Self {
        self.per_rule_globs
            .entry(rule.into())
            .or_default()
            .push(glob.into());
        self
    }

    /// Merge one analyzer config section's suppression keys into the builder.
    ///
    /// This is the single place that defines what each key means: `disabled_rules` and an
    /// explicit `enabled = false` disable a rule, while `ignore_paths` (global and per-rule)
    /// skip paths. Both the CLI (`deslop.toml`) and MCP (inline + config file) feed it, so the
    /// collection logic is not duplicated per surface.
    pub fn add_section<'a, R>(
        &mut self,
        disabled_rules: &'a [String],
        ignore_paths: &'a [String],
        rules: R,
    ) -> &mut Self
    where
        R: IntoIterator<Item = (&'a str, RuleSuppression<'a>)>,
    {
        for rule in disabled_rules {
            self.disable_rule(rule);
        }
        for glob in ignore_paths {
            self.ignore_path(glob);
        }
        for (rule, rule_config) in rules {
            if matches!(rule_config.enabled, Some(false)) {
                self.disable_rule(rule);
            }
            for glob in rule_config.ignore_paths {
                self.ignore_path_for_rule(rule, glob);
            }
        }
        self
    }

    /// Validate rule names and compile globs into a [`Suppression`].
    ///
    /// Returns an error for any unknown rule name or invalid glob so misconfiguration is
    /// surfaced instead of silently doing nothing.
    pub fn build(self) -> Result<Suppression> {
        for rule in self.disabled_rules.iter().chain(self.per_rule_globs.keys()) {
            if !is_known_rule(rule) {
                bail!(
                    "unknown rule '{rule}' in analyzer suppression; valid rules are: {}",
                    deslop_core::rules::names_csv()
                );
            }
        }
        let global_ignore = compile_globs(&self.global_globs)?;
        let mut per_rule_ignore = HashMap::new();
        for (rule, globs) in &self.per_rule_globs {
            if let Some(set) = compile_globs(globs)? {
                per_rule_ignore.insert(rule.clone(), set);
            }
        }
        Ok(Suppression {
            inner: Arc::new(SuppressionInner {
                canonical: SuppressionConfig {
                    disabled_rules: sorted_strings(self.disabled_rules.iter()),
                    ignore_paths: sorted_strings(self.global_globs.iter()),
                    rules: self
                        .per_rule_globs
                        .iter()
                        .map(|(rule, globs)| (rule.clone(), sorted_strings(globs.iter())))
                        .collect(),
                },
                disabled_rules: self.disabled_rules,
                global_ignore,
                per_rule_ignore,
            }),
            match_root: None,
        })
    }
}

fn sorted_strings<'a>(values: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut values = values.into_iter().cloned().collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn compile_globs(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob =
            Glob::new(pattern).with_context(|| format!("invalid ignore_paths glob '{pattern}'"))?;
        builder.add(glob);
    }
    Ok(Some(
        builder
            .build()
            .context("failed to compile ignore_paths globs")?,
    ))
}

impl AnalyzerConfig {
    pub fn long_method_nloc_for(&self, lang: Lang) -> usize {
        let override_value = match lang {
            Lang::Clojure => self.clojure.long_method_nloc,
            Lang::Julia => self.julia.long_method_nloc,
            Lang::Python => self.python.long_method_nloc,
            Lang::JavaScript => self.javascript.long_method_nloc,
            Lang::TypeScript => self.typescript.long_method_nloc,
            Lang::Rust => self.rust.long_method_nloc,
            Lang::Generic => self.generic.long_method_nloc,
        };
        override_value.unwrap_or(self.long_method_nloc)
    }

    pub fn snapshot(&self) -> AnalyzerConfigSnapshot {
        AnalyzerConfigSnapshot {
            min_duplication_tokens: self.min_duplication_tokens,
            long_method_nloc: self.long_method_nloc,
            min_meaningful_tokens: self.min_meaningful_tokens,
            rust: self.rust.clone(),
            clojure: self.clojure.clone(),
            julia: self.julia.clone(),
            python: self.python.clone(),
            javascript: self.javascript.clone(),
            typescript: self.typescript.clone(),
            generic: self.generic.clone(),
            rust_external: self.rust_external,
            julia_external: self.julia_external,
            julia_project: self.julia_project.clone(),
            suppression: self.suppression.config().clone(),
            boundary: self.boundary.clone(),
        }
    }
}

impl AnalyzerConfigSnapshot {
    pub fn to_config(&self) -> Result<AnalyzerConfig> {
        let mut suppression = Suppression::builder();
        suppression.add_section(
            &self.suppression.disabled_rules,
            &self.suppression.ignore_paths,
            self.suppression.rules.iter().map(|(rule, globs)| {
                (
                    rule.as_str(),
                    RuleSuppression {
                        enabled: None,
                        ignore_paths: globs,
                    },
                )
            }),
        );
        Ok(AnalyzerConfig {
            min_duplication_tokens: self.min_duplication_tokens,
            long_method_nloc: self.long_method_nloc,
            min_meaningful_tokens: self.min_meaningful_tokens,
            rust: self.rust.clone(),
            clojure: self.clojure.clone(),
            julia: self.julia.clone(),
            python: self.python.clone(),
            javascript: self.javascript.clone(),
            typescript: self.typescript.clone(),
            generic: self.generic.clone(),
            rust_external: self.rust_external,
            julia_external: self.julia_external,
            julia_project: self.julia_project.clone(),
            suppression: suppression.build()?,
            boundary: self.boundary.clone(),
        })
    }
}

fn julia_external_analyzer(
    config: &AnalyzerConfig,
) -> Option<Box<dyn ExternalAnalyzerTrait<SourceFile, Finding>>> {
    match config.julia_external {
        JuliaExternal::Off => None,
        JuliaExternal::StaticLint => Some(Box::new(JuliaAnalyzer::staticlint(
            config.julia_project.to_owned(),
        ))),
        JuliaExternal::Jet => Some(Box::new(JuliaAnalyzer::jet(
            config.julia_project.to_owned(),
        ))),
    }
}

fn clojure_external_analyzer(
    _config: &AnalyzerConfig,
) -> Option<Box<dyn ExternalAnalyzerTrait<SourceFile, Finding>>> {
    Some(Box::new(CljKondoAnalyzer))
}

/// Run file-local analyzer rules over one immutable, already-owned analysis.
///
/// Project boundary claims require a prepared input manifest and are intentionally rejected by
/// this source-only entry point. Optional external analyzers are recorded unavailable rather than
/// consulting live paths.
pub fn scan_analysis(
    analysis: Arc<ProjectAnalysis>,
    config: AnalyzerConfig,
) -> Result<AnalyzerProjection> {
    if config.boundary.enabled {
        bail!(
            "owned source-only analysis cannot prove config-boundary coverage; disable boundary analysis or use a prepared analyzer input manifest"
        );
    }
    scan_owned_analysis(analysis, None, None, config)
}

/// Run source-only analyzer rules with an already-pinned presentation map.
///
/// This is the owned entry point for in-memory clients such as the LSP. Project-boundary analysis
/// remains unavailable without a prepared input manifest.
pub fn scan_analysis_with_presentation(
    analysis: Arc<ProjectAnalysis>,
    presentation: &SnapshotPresentationMap,
    config: AnalyzerConfig,
) -> Result<AnalyzerProjection> {
    if config.boundary.enabled {
        bail!(
            "owned source-only analysis cannot prove config-boundary coverage; disable boundary analysis or use a prepared analyzer input manifest"
        );
    }
    scan_owned_analysis(analysis, Some(presentation), None, config)
}

/// Run analyzer rules over a project analysis whose project-level inputs were pinned by the
/// snapshot planner. Enabled boundary analysis requires complete discovery coverage.
pub fn scan_prepared_analysis(
    prepared: PreparedAnalyzerAnalysis,
    config: AnalyzerConfig,
) -> Result<AnalyzerProjection> {
    if config.boundary.enabled {
        match &prepared.inputs.boundary_coverage {
            BoundaryCoverage::Complete => {}
            BoundaryCoverage::Unavailable { reason } => {
                bail!("prepared analyzer cannot prove config-boundary coverage: {reason}")
            }
        }
    }
    scan_owned_analysis(
        prepared.analysis,
        Some(&prepared.presentation),
        Some(&prepared.inputs),
        config,
    )
}

fn scan_owned_analysis(
    analysis: Arc<ProjectAnalysis>,
    presentation: Option<&SnapshotPresentationMap>,
    inputs: Option<&AnalyzerInputManifest>,
    config: AnalyzerConfig,
) -> Result<AnalyzerProjection> {
    let mut config = config;
    config.suppression.match_root = None;
    let config_snapshot = config.snapshot();
    let mut policy = serde_json::to_vec(&config_snapshot).context("serialize analyzer policy")?;
    let capabilities = if let Some(inputs) = inputs {
        policy.extend_from_slice(
            &serde_json::to_vec(inputs).context("serialize prepared analyzer input manifest")?,
        );
        PREPARED_ANALYZER_CAPABILITIES
    } else {
        ANALYZER_CAPABILITIES
    };
    if let Some(presentation) = presentation {
        let presentation_entries = presentation
            .entries()
            .map(|(logical, display)| (logical.to_path_buf(), display.to_path_buf()))
            .collect::<Vec<_>>();
        policy.extend_from_slice(
            &serde_json::to_vec(&presentation_entries)
                .context("serialize analyzer presentation paths")?,
        );
    }
    let id = analysis.derive_projection_id(ANALYZER_PROJECTION_SCHEMA, &policy, capabilities)?;
    let mut reports = Vec::new();
    let mut input_contents = BTreeMap::new();
    let mut external_capabilities = Vec::new();
    let analyzer_files = analysis
        .files()
        .filter(|parsed| {
            parsed.provenance().permits_rewrites()
                && inputs.is_none_or(|manifest| {
                    manifest
                        .report_sources
                        .binary_search(&parsed.key().path)
                        .is_ok()
                })
        })
        .map(|parsed| {
            AnalyzerFile::new_with_path(
                &analysis,
                parsed,
                display_path(presentation, &parsed.key().path),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    for parsed in analysis.files() {
        if inputs.is_some_and(|manifest| {
            manifest
                .report_sources
                .binary_search(&parsed.key().path)
                .is_err()
        }) {
            continue;
        }
        let logical_path = parsed.key().path.clone();
        let path = display_path(presentation, &logical_path);
        if let Some(text) = parsed.text() {
            input_contents.insert(path.clone(), text.to_string());
        }
        let provenance = parsed.provenance().clone();
        if !provenance.permits_rewrites() {
            reports.push(FileReport {
                path,
                lang: parsed.grammar().lang(),
                analysis: provenance,
                findings: Vec::new(),
            });
            continue;
        }
        let file = analyzer_files
            .iter()
            .find(|file| file.file.key().path == logical_path)
            .expect("complete prepared source has one analyzer view");
        let mut findings = agnostic::findings_analysis(file, &config);
        findings.extend(match file.adapter().name() {
            "clojure" => clojure::findings(file.source()),
            "julia" => julia::findings(file.source()),
            "python" => packs::python::python_findings(file.source()),
            "javascript" | "typescript" => packs::javascript::javascript_findings(file.source()),
            "rust" => packs::rust::rust_findings_analysis(file),
            adapter => bail!(
                "stored language adapter {adapter:?} has no owned analyzer pack for {}",
                logical_path.display()
            ),
        });
        record_unavailable_external(file, &config, &mut external_capabilities);
        config.suppression.retain(&mut findings);
        apply_inline_suppression_analysis(file, &mut findings);
        sort_findings(&mut findings);
        reports.push(FileReport {
            path,
            lang: parsed.grammar().lang(),
            analysis: provenance,
            findings,
        });
    }
    if reports_permit_rewrites(&reports) && reports.len() >= 2 {
        let mut cross_file =
            tokens::cross_file_duplicate_findings_analysis(&analyzer_files, &config);
        config.suppression.retain(&mut cross_file);
        for file in &analyzer_files {
            let mut file_findings = cross_file
                .iter()
                .filter(|finding| finding.path == file.source().path)
                .cloned()
                .collect::<Vec<_>>();
            apply_inline_suppression_analysis(file, &mut file_findings);
            if let Some(report) = reports
                .iter_mut()
                .find(|report| report.path == file.source().path)
            {
                for finding in file_findings {
                    if !report.findings.iter().any(|existing| {
                        existing.rule == finding.rule
                            && existing.span == finding.span
                            && existing.fingerprint == finding.fingerprint
                    }) {
                        report.findings.push(finding);
                    }
                }
                sort_findings(&mut report.findings);
            }
        }
    }
    if config.boundary.enabled {
        let manifest = inputs.expect("enabled prepared boundary has an input manifest");
        let mut artifacts = Vec::with_capacity(manifest.boundary_artifacts.len());
        for logical in &manifest.boundary_artifacts {
            let entry = analysis
                .snapshot()
                .entry(logical)
                .expect("prepared boundary artifact was validated");
            let path = display_path(presentation, logical);
            let skipped = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    config
                        .boundary
                        .skip_artifacts
                        .iter()
                        .any(|skipped| skipped == name)
                });
            if skipped {
                continue;
            }
            let text = match std::str::from_utf8(entry.bytes()) {
                Ok(text) => text,
                Err(error) => {
                    reports.push(FileReport {
                        path,
                        lang: Lang::Generic,
                        analysis: AnalysisProvenance::failed(vec![AnalysisDiagnostic {
                            code: "invalid-utf8-analysis-input".to_string(),
                            message: format!(
                                "prepared boundary artifact is not valid UTF-8: {error}"
                            ),
                            span: None,
                        }]),
                        findings: Vec::new(),
                    });
                    continue;
                }
            };
            input_contents.insert(path.clone(), text.to_string());
            artifacts.push(AnalyzerText::new(path, text.to_string(), Lang::Generic));
        }
        if reports_permit_rewrites(&reports) {
            boundary::add_config_boundary_analysis(
                &mut reports,
                &analyzer_files,
                &artifacts,
                &config,
            )?;
        }
        if !config.suppression.is_empty() {
            for report in &mut reports {
                config.suppression.retain(&mut report.findings);
            }
        }
    }
    reports.sort_by(|left, right| left.path.cmp(&right.path));
    external_capabilities.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.analyzer.cmp(&right.analyzer))
    });
    let projection_presentation =
        presentation
            .cloned()
            .unwrap_or(SnapshotPresentationMap::from_entries(
                analyzer_files
                    .iter()
                    .map(|file| (file.file.key().path.clone(), file.file.key().path.clone())),
            )?);
    Ok(AnalyzerProjection {
        id,
        analysis,
        presentation: projection_presentation,
        config: config_snapshot,
        reports,
        input_contents,
        external_capabilities,
    })
}

fn display_path(presentation: Option<&SnapshotPresentationMap>, logical: &Path) -> PathBuf {
    presentation
        .map(|paths| paths.display_path(logical).to_path_buf())
        .unwrap_or_else(|| logical.to_path_buf())
}

fn record_unavailable_external(
    file: &AnalyzerFile<'_>,
    config: &AnalyzerConfig,
    out: &mut Vec<ExternalCapability>,
) {
    let external = match file.adapter().name() {
        "clojure" => clojure_external_analyzer(config),
        "julia" => julia_external_analyzer(config),
        "rust" => packs::rust::external_analyzer(config),
        _ => None,
    };
    if let Some(external) = external {
        out.push(ExternalCapability {
            path: file.source().path.clone(),
            analyzer: external.name().to_string(),
            available: false,
            covered_rules: external
                .covered_rules()
                .iter()
                .map(|rule| (*rule).to_string())
                .collect(),
        });
    }
}

pub fn scan_paths(paths: &[PathBuf]) -> Result<Vec<FileReport>> {
    scan_paths_with_config(paths, AnalyzerConfig::default())
}

pub fn scan_paths_with_config(
    paths: &[PathBuf],
    config: AnalyzerConfig,
) -> Result<Vec<FileReport>> {
    Ok(scan_paths_with_context(paths, config)?.reports)
}

pub fn scan_paths_with_context(paths: &[PathBuf], config: AnalyzerConfig) -> Result<ScanContext> {
    let paths = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    let invocation_base = std::env::current_dir().context("resolve analyzer invocation base")?;
    let mut planner = ProjectSnapshotPlanner::resolve(ProjectSnapshotRequest {
        invocation_base,
        root: RootSpec::Auto,
        repository: RepositorySpec::Auto,
        scope: ScopeSpec::Requested(paths.clone()),
        discovery: DiscoveryPolicy::LegacyRespectIgnore,
    })?;
    let mut boundary_artifacts = Vec::new();
    let boundary_coverage = if config.boundary.enabled {
        for path in boundary::discover_config_artifacts(&paths)? {
            boundary_artifacts.push(planner.add_disk_analysis_input(path)?);
        }
        BoundaryCoverage::Complete
    } else {
        BoundaryCoverage::Unavailable {
            reason: "boundary analysis disabled by policy".to_string(),
        }
    };
    let built = planner.build()?;
    let analysis = ProjectAnalysis::build(built.snapshot)?;
    let report_sources = analysis
        .files()
        .map(|file| file.key().path.clone())
        .collect();
    let prepared = PreparedAnalyzerAnalysis::new(
        analysis,
        AnalyzerInputManifest {
            report_sources,
            boundary_artifacts,
            boundary_coverage,
            external_unavailable_reason:
                "no revision-isolated external execution plan was prepared".to_string(),
        },
        built.presentation,
    )?;
    let projection = scan_prepared_analysis(prepared, config)?;
    Ok(ScanContext {
        analysis: projection.analysis,
        presentation: projection.presentation,
        reports: projection.reports,
        input_contents: projection.input_contents,
        external_capabilities: projection.external_capabilities,
    })
}

pub fn scan_file(path: &Path) -> Result<FileReport> {
    scan_file_with_config(path, AnalyzerConfig::default())
}

pub fn scan_file_with_config(path: &Path, config: AnalyzerConfig) -> Result<FileReport> {
    Ok(scan_paths_with_config(&[path.to_path_buf()], config)?
        .into_iter()
        .next()
        .unwrap_or_else(|| empty_report(path, Lang::Generic)))
}

fn empty_report(path: &Path, lang: Lang) -> FileReport {
    FileReport {
        path: path.to_path_buf(),
        lang,
        analysis: AnalysisProvenance::unsupported(vec![AnalysisDiagnostic {
            code: "unsupported-language".to_string(),
            message: "no analyzer and parser adapter is available; analysis is partial".to_string(),
            span: None,
        }]),
        findings: Vec::new(),
    }
}

pub fn scan_source(source: &SourceFile) -> FileReport {
    scan_source_with_config(source, AnalyzerConfig::default())
}

pub fn scan_source_with_config(source: &SourceFile, config: AnalyzerConfig) -> FileReport {
    let result = (|| -> Result<FileReport> {
        let built = ProjectSnapshotPlanner::build_single_source_overlay(
            std::env::current_dir().context("resolve source scan invocation base")?,
            &source.path,
            source.text.as_bytes().to_vec(),
        )?;
        let analysis = ProjectAnalysis::build(built.snapshot)?;
        let report_sources = analysis
            .files()
            .map(|file| file.key().path.clone())
            .collect();
        let prepared = PreparedAnalyzerAnalysis::new(
            analysis,
            AnalyzerInputManifest {
                report_sources,
                boundary_artifacts: Vec::new(),
                boundary_coverage: BoundaryCoverage::Unavailable {
                    reason: "single-source compatibility scan has no project boundary".to_string(),
                },
                external_unavailable_reason:
                    "no revision-isolated external execution plan was prepared".to_string(),
            },
            built.presentation,
        )?;
        let mut config = config;
        config.boundary.enabled = false;
        Ok(scan_prepared_analysis(prepared, config)?
            .reports
            .into_iter()
            .next()
            .unwrap_or_else(|| empty_report(&source.path, source.lang)))
    })();
    result.unwrap_or_else(|error| FileReport {
        path: source.path.to_path_buf(),
        lang: source.lang,
        analysis: AnalysisProvenance::failed(vec![AnalysisDiagnostic {
            code: "source-snapshot-failed".to_string(),
            message: error.to_string(),
            span: None,
        }]),
        findings: Vec::new(),
    })
}

fn apply_inline_suppression_analysis(file: &AnalyzerFile<'_>, findings: &mut Vec<Finding>) {
    let directives = inline_suppression_lines_analysis(file);
    findings.retain(|finding| !is_inline_suppressed(finding, &directives));
}

fn is_inline_suppressed(finding: &Finding, directives: &InlineSuppressions) -> bool {
    (finding.span.start_line..=finding.span.end_line)
        .any(|line| directives.is_suppressed_on_line(line, &finding.rule))
}

#[derive(Debug, Default)]
struct InlineSuppressions {
    suppress_all_same_line: Vec<usize>,
    suppress_all_next_line: Vec<usize>,
    suppress_same_line: std::collections::BTreeMap<usize, std::collections::BTreeSet<String>>,
    suppress_next_line: std::collections::BTreeMap<usize, std::collections::BTreeSet<String>>,
}

impl InlineSuppressions {
    fn add_same_line(&mut self, line: usize, rules: &[String]) {
        if rules.iter().any(|rule| rule == "*") {
            self.suppress_all_same_line.push(line);
            return;
        }
        for rule in rules {
            self.suppress_same_line
                .entry(line)
                .or_default()
                .insert(rule.to_string());
        }
    }

    fn add_next_line(&mut self, line: usize, rules: &[String]) {
        if rules.iter().any(|rule| rule == "*") {
            self.suppress_all_next_line.push(line + 1);
            return;
        }
        for rule in rules {
            self.suppress_next_line
                .entry(line + 1)
                .or_default()
                .insert(rule.to_string());
        }
    }

    fn is_suppressed_on_line(&self, line: usize, rule: &str) -> bool {
        (self.suppress_all_same_line.binary_search(&line).is_ok())
            || (self.suppress_all_next_line.binary_search(&line).is_ok())
            || self
                .suppress_same_line
                .get(&line)
                .is_some_and(|rules| rules.contains(rule))
            || self
                .suppress_next_line
                .get(&line)
                .is_some_and(|rules| rules.contains(rule))
    }
}

fn inline_suppression_lines_analysis(file: &AnalyzerFile<'_>) -> InlineSuppressions {
    let mut directives = InlineSuppressions::default();
    let Some(root) = file.node_ids().next() else {
        return directives;
    };
    let root_view = file
        .analysis
        .node(root)
        .expect("AnalyzerFile root belongs to its analysis");
    if root_view.has_error() {
        return directives;
    }
    collect_comment_directives_analysis(file, root, &mut directives);
    directives
}

fn collect_comment_directives_analysis(
    file: &AnalyzerFile<'_>,
    node: NodeId,
    directives: &mut InlineSuppressions,
) {
    let view = file
        .analysis
        .node(node)
        .expect("AnalyzerFile NodeId belongs to its analysis");
    if view.raw_kind().contains("comment") {
        let line = file.source().line_for_byte(view.span().start_byte());
        for (line_offset, line_text) in view.text().lines().enumerate() {
            if let Some((next_line, rule_names)) = inline_ignore_rules_for_line(line_text) {
                if next_line {
                    directives.add_next_line(line + line_offset, &rule_names);
                } else {
                    directives.add_same_line(line + line_offset, &rule_names);
                }
            }
        }
        return;
    }
    for child in view.children() {
        collect_comment_directives_analysis(file, child, directives);
    }
}

fn inline_ignore_rules_for_line(text: &str) -> Option<(bool, Vec<String>)> {
    let marker = text.find("deslop:ignore")?;
    let rest = &text[marker + "deslop:ignore".len()..];
    let trimmed = rest.trim_start();
    let (next_line, rules_text) = if let Some(stripped) = trimmed.strip_prefix("-next-line") {
        (true, stripped.trim_start())
    } else {
        (false, trimmed)
    };
    let rules_text = rules_text.split("--").next().unwrap_or("").trim();
    if rules_text.is_empty() {
        return None;
    }
    let rules = rules_text
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|rule: &&str| !rule.is_empty())
        .map(|rule| rule.to_string())
        .collect::<Vec<_>>();
    if rules.is_empty() {
        None
    } else {
        Some((next_line, rules))
    }
}

fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.span.start_line.cmp(&b.span.start_line))
            .then(a.rule.cmp(&b.rule))
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn finding(
    source: &impl TextSource,
    start_line: usize,
    end_line: usize,
    rule: &str,
    severity: Severity,
    safety: SafetyClass,
    detected_by: DetectedBy,
    message: &str,
    suggestion: &str,
    precondition: Option<&str>,
    edit: Option<Edit>,
) -> Finding {
    let start_byte = source.line_start_byte(start_line);
    let end_byte = source.line_end_byte(end_line);
    let span = Span::new(start_line, end_line, start_byte, end_byte);
    let text = source.region_text(start_line, end_line);
    Finding {
        path: source.path().to_path_buf(),
        span,
        rule: rule.to_string(),
        severity,
        safety,
        detected_by,
        message: message.to_string(),
        suggestion: suggestion.to_string(),
        precondition: precondition.map(str::to_string),
        edit,
        fingerprint: baseline_fingerprint(source.path(), rule, span, &text),
    }
}
