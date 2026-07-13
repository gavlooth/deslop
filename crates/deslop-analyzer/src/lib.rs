use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use anyhow::{Context, Result, bail};
use deslop_core::{
    AnalysisDiagnostic, AnalysisProvenance, AnalysisStatus, DetectedBy, Edit, FileReport, Finding,
    Lang, SafetyClass, Severity, Span, baseline_fingerprint, reports_permit_rewrites,
};
use deslop_external::{
    CljKondoAnalyzer, ExternalAnalyzer as ExternalAnalyzerTrait, ExternalFindings, JuliaAnalyzer,
};
use deslop_lang::{LangPack, Registry as LangRegistry, Rule};
use deslop_parse::{
    NodeId, ParsedFile, ProjectAnalysis, ProjectionId, SourceFile, SyntaxAdapterFacts,
    analysis_provenance_or_failed, parse_source,
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

mod agnostic;
mod boundary;
mod clojure;
mod julia;
mod packs;

pub use boundary::BoundaryConfig;
#[cfg(test)]
mod test_pack;
#[cfg(test)]
mod tests;
mod tokens;

static EXTERNAL_NOTICE_EMITTED: AtomicBool = AtomicBool::new(false);

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
    pub reports: Vec<FileReport>,
    pub input_contents: BTreeMap<PathBuf, String>,
    pub external_capabilities: Vec<ExternalCapability>,
}

const ANALYZER_PROJECTION_SCHEMA: &str = "deslop.analyzer.projection/1";
const ANALYZER_CAPABILITIES: &[u8] =
    b"rules=deslop.analyzer-owned/1\0boundary=disabled\0external=pinned-unavailable";

#[derive(Debug)]
pub struct AnalyzerProjection {
    pub id: ProjectionId,
    pub analysis: Arc<ProjectAnalysis>,
    pub config: AnalyzerConfigSnapshot,
    pub reports: Vec<FileReport>,
    pub input_contents: BTreeMap<PathBuf, String>,
    pub external_capabilities: Vec<ExternalCapability>,
}

/// One analyzer view over a file already parsed and owned by `ProjectAnalysis`.
///
/// `source` is a compatibility text view over pinned bytes; syntax authority remains
/// the analysis-owned arena and exact stored adapter facts.
pub struct AnalyzerFile<'analysis> {
    pub analysis: &'analysis ProjectAnalysis,
    pub file: &'analysis ParsedFile,
    source: SourceFile,
    adapter: &'static dyn LangPack,
    facts: Box<[SyntaxAdapterFacts]>,
    facts_by_node: HashMap<NodeId, usize>,
}

impl<'analysis> AnalyzerFile<'analysis> {
    pub fn new(analysis: &'analysis ProjectAnalysis, file: &'analysis ParsedFile) -> Result<Self> {
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
            source: SourceFile::new_with_lang(
                file.key().path.clone(),
                text.to_string(),
                file.grammar().lang(),
            ),
            adapter,
            facts,
            facts_by_node,
        })
    }

    pub fn source(&self) -> &SourceFile {
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
        self.analysis
            .node(node)
            .ok()?
            .children()
            .into_iter()
            .find(|child| {
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

pub trait AnalysisPack: Send + Sync {
    fn name(&self) -> &'static str;
    fn lang(&self) -> Lang;
    fn rules(&self) -> &'static [&'static dyn Rule<SourceFile, AnalyzerConfig, Finding>];
    fn external_analyzer(
        &self,
        config: &AnalyzerConfig,
    ) -> Option<Box<dyn ExternalAnalyzerTrait<SourceFile, Finding>>>;
}

pub struct AnalyzerRegistry {
    packs: Vec<&'static dyn AnalysisPack>,
}

impl AnalyzerRegistry {
    pub fn new() -> Self {
        Self { packs: Vec::new() }
    }

    pub fn with_default_packs() -> Self {
        let mut registry = Self::new();
        registry.register(&CLOJURE_PACK);
        registry.register(&JULIA_PACK);
        registry.register(&packs::python::PYTHON_PACK);
        registry.register(&packs::javascript::JAVASCRIPT_PACK);
        registry.register(&packs::javascript::TYPESCRIPT_PACK);
        registry.register(&packs::rust::RUST_PACK);
        registry
    }

    pub fn register(&mut self, pack: &'static dyn AnalysisPack) {
        self.packs.push(pack);
    }

    pub fn pack_for_lang(&self, lang: Lang) -> Option<&'static dyn AnalysisPack> {
        self.packs.iter().copied().find(|pack| pack.lang() == lang)
    }
}

impl Default for AnalyzerRegistry {
    fn default() -> Self {
        Self::with_default_packs()
    }
}

struct FunctionRule {
    name: &'static str,
    check_fn: fn(&SourceFile, &AnalyzerConfig) -> Vec<Finding>,
}

impl Rule<SourceFile, AnalyzerConfig, Finding> for FunctionRule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn check(&self, source: &SourceFile, config: &AnalyzerConfig) -> Vec<Finding> {
        (self.check_fn)(source, config)
    }
}

static AGNOSTIC_RULE: FunctionRule = FunctionRule {
    name: "agnostic",
    check_fn: agnostic_findings,
};
static CLOJURE_RULE: FunctionRule = FunctionRule {
    name: "clojure",
    check_fn: clojure_findings_rule,
};
static JULIA_RULE: FunctionRule = FunctionRule {
    name: "julia",
    check_fn: julia_findings_rule,
};

static AGNOSTIC_RULES: [&'static dyn Rule<SourceFile, AnalyzerConfig, Finding>; 1] =
    [&AGNOSTIC_RULE];
static CLOJURE_RULES: [&'static dyn Rule<SourceFile, AnalyzerConfig, Finding>; 1] = [&CLOJURE_RULE];
static JULIA_RULES: [&'static dyn Rule<SourceFile, AnalyzerConfig, Finding>; 1] = [&JULIA_RULE];

struct AgnosticPack;
struct ClojurePack;
struct JuliaPack;

static AGNOSTIC_PACK: AgnosticPack = AgnosticPack;
static CLOJURE_PACK: ClojurePack = ClojurePack;
static JULIA_PACK: JuliaPack = JuliaPack;

macro_rules! analysis_pack {
    ($type:ty, $name:literal, $lang:expr, $rules:ident, $external:expr) => {
        impl AnalysisPack for $type {
            fn name(&self) -> &'static str {
                $name
            }

            fn lang(&self) -> Lang {
                $lang
            }

            fn rules(&self) -> &'static [&'static dyn Rule<SourceFile, AnalyzerConfig, Finding>] {
                &$rules
            }

            fn external_analyzer(
                &self,
                config: &AnalyzerConfig,
            ) -> Option<Box<dyn ExternalAnalyzerTrait<SourceFile, Finding>>> {
                $external(config)
            }
        }
    };
}

analysis_pack!(
    AgnosticPack,
    "agnostic",
    Lang::Generic,
    AGNOSTIC_RULES,
    |_| None
);
analysis_pack!(
    ClojurePack,
    "clojure",
    Lang::Clojure,
    CLOJURE_RULES,
    clojure_external_analyzer
);
analysis_pack!(
    JuliaPack,
    "julia",
    Lang::Julia,
    JULIA_RULES,
    julia_external_analyzer
);

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
    let mut config = config;
    config.suppression.match_root = None;
    if config.boundary.enabled {
        bail!(
            "owned source-only analysis cannot prove config-boundary coverage; disable boundary analysis or use a prepared analyzer input manifest"
        );
    }
    let config_snapshot = config.snapshot();
    let policy = serde_json::to_vec(&config_snapshot).context("serialize analyzer policy")?;
    let id = analysis.derive_projection_id(
        ANALYZER_PROJECTION_SCHEMA,
        &policy,
        ANALYZER_CAPABILITIES,
    )?;
    let mut reports = Vec::new();
    let mut input_contents = BTreeMap::new();
    let mut external_capabilities = Vec::new();
    for parsed in analysis.files() {
        let path = parsed.key().path.clone();
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
        let file = AnalyzerFile::new(&analysis, parsed)?;
        let mut findings = agnostic::findings_analysis(&file, &config);
        findings.extend(match file.adapter().name() {
            "clojure" => clojure::findings(file.source()),
            "julia" => julia::findings(file.source()),
            "python" => packs::python::python_findings(file.source()),
            "javascript" | "typescript" => packs::javascript::javascript_findings(file.source()),
            "rust" => packs::rust::rust_findings_analysis(&file),
            adapter => bail!(
                "stored language adapter {adapter:?} has no owned analyzer pack for {}",
                parsed.key().path.display()
            ),
        });
        record_unavailable_external(&file, &config, &mut external_capabilities);
        config.suppression.retain(&mut findings);
        apply_inline_suppression_analysis(&file, &mut findings);
        sort_findings(&mut findings);
        reports.push(FileReport {
            path,
            lang: parsed.grammar().lang(),
            analysis: provenance,
            findings,
        });
    }
    if reports_permit_rewrites(&reports) && reports.len() >= 2 {
        let files = analysis
            .files()
            .map(|parsed| AnalyzerFile::new(&analysis, parsed))
            .collect::<Result<Vec<_>>>()?;
        let mut cross_file = tokens::cross_file_duplicate_findings_analysis(&files, &config);
        config.suppression.retain(&mut cross_file);
        for file in &files {
            let mut file_findings = cross_file
                .iter()
                .filter(|finding| finding.path == file.file.key().path)
                .cloned()
                .collect::<Vec<_>>();
            apply_inline_suppression_analysis(file, &mut file_findings);
            if let Some(report) = reports
                .iter_mut()
                .find(|report| report.path == file.file.key().path)
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
    reports.sort_by(|left, right| left.path.cmp(&right.path));
    external_capabilities.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.analyzer.cmp(&right.analyzer))
    });
    Ok(AnalyzerProjection {
        id,
        analysis,
        config: config_snapshot,
        reports,
        input_contents,
        external_capabilities,
    })
}

fn record_unavailable_external(
    file: &AnalyzerFile<'_>,
    config: &AnalyzerConfig,
    out: &mut Vec<ExternalCapability>,
) {
    let external = match file.adapter().name() {
        "clojure" => clojure_external_analyzer(config),
        "julia" => julia_external_analyzer(config),
        "rust" => packs::rust::RUST_PACK.external_analyzer(config),
        _ => None,
    };
    if let Some(external) = external {
        out.push(ExternalCapability {
            path: file.file.key().path.clone(),
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
    let lang_registry = LangRegistry::default();
    let analyzer_registry = AnalyzerRegistry::default();
    let paths = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };

    let mut supported_paths = Vec::new();
    for path in &paths {
        collect_supported_paths(
            &mut supported_paths,
            path,
            &lang_registry,
            &analyzer_registry,
        )?;
    }
    supported_paths = deduplicate_supported_paths(supported_paths);
    let scanned = scan_supported_paths_parallel(&supported_paths, &config)?;
    let mut reports = Vec::with_capacity(scanned.len());
    let mut input_contents = BTreeMap::new();
    let mut external_capabilities = Vec::new();
    for scanned in scanned {
        input_contents.insert(scanned.report.path.clone(), scanned.source_text);
        reports.push(scanned.report);
        external_capabilities.extend(scanned.external_capabilities);
    }
    if reports_permit_rewrites(&reports) {
        add_cross_file_duplication(&mut reports, &config)?;
        input_contents.extend(boundary::add_config_boundary(
            &mut reports,
            &supported_paths,
            &paths,
            &config,
        )?);
    }
    // Boundary findings are appended after the per-file pass, so suppression must run
    // over them here (the per-file pass already filtered its own findings).
    if !config.suppression.is_empty() {
        for report in &mut reports {
            config.suppression.retain(&mut report.findings);
        }
    }
    reports.sort_by(|a, b| a.path.cmp(&b.path));
    external_capabilities.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.analyzer.cmp(&b.analyzer))
            .then(a.available.cmp(&b.available))
    });
    Ok(ScanContext {
        reports,
        input_contents,
        external_capabilities,
    })
}

fn deduplicate_supported_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut unique: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    for path in paths {
        let path = normalized_display_path(&path);
        let identity = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        unique
            .entry(identity)
            .and_modify(|existing| {
                if path_precedes(&path, existing) {
                    *existing = path.to_path_buf();
                }
            })
            .or_insert(path);
    }
    unique.into_values().collect()
}

fn normalized_display_path(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect()
}

fn path_precedes(candidate: &Path, current: &Path) -> bool {
    match (candidate.is_absolute(), current.is_absolute()) {
        (false, true) => true,
        (true, false) => false,
        _ => candidate < current,
    }
}

fn collect_supported_paths(
    paths: &mut Vec<PathBuf>,
    path: &Path,
    lang_registry: &LangRegistry,
    analyzer_registry: &AnalyzerRegistry,
) -> Result<()> {
    if path.is_file() {
        if analysis_pack_for_path(path, lang_registry, analyzer_registry).is_some() {
            paths.push(path.to_path_buf());
        }
        return Ok(());
    }

    let walker = WalkBuilder::new(path)
        .hidden(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(name.as_ref(), ".git" | ".jj" | "target" | "__pycache__")
        })
        .build();

    for entry in walker {
        let entry = entry.with_context(|| format!("failed to walk {}", path.display()))?;
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let entry_path = entry.into_path();
        if analysis_pack_for_path(&entry_path, lang_registry, analyzer_registry).is_some() {
            paths.push(entry_path);
        }
    }
    Ok(())
}

fn scan_supported_paths_parallel(
    paths: &[PathBuf],
    config: &AnalyzerConfig,
) -> Result<Vec<ScannedFile>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let workers = thread::available_parallelism()
        .map_or(1, usize::from)
        .min(paths.len());
    let chunk_size = paths.len().div_ceil(workers);
    let mut reports = Vec::with_capacity(paths.len());
    thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in paths.chunks(chunk_size) {
            handles.push(scope.spawn(move || {
                chunk
                    .iter()
                    .map(|path| scan_file_with_context(path, config.to_owned()))
                    .collect::<Result<Vec<_>>>()
            }));
        }
        for handle in handles {
            let mut chunk_reports = handle
                .join()
                .map_err(|_| anyhow::anyhow!("parallel scan worker panicked"))??;
            reports.append(&mut chunk_reports);
        }
        Ok::<_, anyhow::Error>(())
    })?;
    reports.sort_by(|a, b| a.report.path.cmp(&b.report.path));
    Ok(reports)
}

struct ScannedFile {
    report: FileReport,
    source_text: String,
    external_capabilities: Vec<ExternalCapability>,
}

fn add_cross_file_duplication(reports: &mut [FileReport], config: &AnalyzerConfig) -> Result<()> {
    if reports.len() < 2 || config.min_duplication_tokens == 0 {
        return Ok(());
    }
    let sources = reports
        .iter()
        .filter(|report| report.analysis.permits_rewrites())
        .map(|report| SourceFile::read(&report.path))
        .collect::<Result<Vec<_>>>()?;
    if sources.len() < 2 {
        return Ok(());
    }
    let mut findings = tokens::cross_file_duplicate_findings(&sources, config);
    config.suppression.retain(&mut findings);
    for source in &sources {
        let mut source_findings = findings
            .iter()
            .filter(|finding| finding.path == source.path)
            .cloned()
            .collect::<Vec<_>>();
        apply_inline_suppression(source, &mut source_findings);
        if let Some(report) = reports.iter_mut().find(|report| report.path == source.path) {
            for finding in source_findings {
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
    Ok(())
}

pub fn scan_file(path: &Path) -> Result<FileReport> {
    scan_file_with_config(path, AnalyzerConfig::default())
}

pub fn scan_file_with_config(path: &Path, config: AnalyzerConfig) -> Result<FileReport> {
    Ok(scan_file_with_context(path, config)?.report)
}

fn scan_file_with_context(path: &Path, config: AnalyzerConfig) -> Result<ScannedFile> {
    let lang_registry = LangRegistry::default();
    let analyzer_registry = AnalyzerRegistry::default();
    let Some(lang_pack) = lang_registry.supported_pack_for_path(path) else {
        return Ok(ScannedFile {
            report: empty_report(path, Lang::Generic),
            source_text: String::new(),
            external_capabilities: Vec::new(),
        });
    };
    let Some(pack) = analyzer_registry.pack_for_lang(lang_pack.lang()) else {
        return Ok(ScannedFile {
            report: empty_report(path, lang_pack.lang()),
            source_text: String::new(),
            external_capabilities: Vec::new(),
        });
    };
    scan_file_with_pack_context(path, pack, config)
}

#[cfg(test)]
fn scan_file_with_pack(
    path: &Path,
    pack: &'static dyn AnalysisPack,
    config: AnalyzerConfig,
) -> Result<FileReport> {
    Ok(scan_file_with_pack_context(path, pack, config)?.report)
}

fn scan_file_with_pack_context(
    path: &Path,
    pack: &'static dyn AnalysisPack,
    config: AnalyzerConfig,
) -> Result<ScannedFile> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let source = SourceFile::new_with_lang(path.to_path_buf(), text, pack.lang());
    let source_text = source.text.clone();
    let mut report = scan_source_with_pack(&source, pack, &config);
    if !report.analysis.permits_rewrites() {
        return Ok(ScannedFile {
            report,
            source_text,
            external_capabilities: Vec::new(),
        });
    }
    let mut external_capabilities = Vec::new();
    if let Some(external) = pack.external_analyzer(&config) {
        let analyzer = external.name().to_string();
        let covered_rules = external
            .covered_rules()
            .iter()
            .map(|rule| (*rule).to_string())
            .collect::<Vec<_>>();
        match external.analyze(path, &source)? {
            ExternalFindings::Available(external_findings) => {
                external_capabilities.push(ExternalCapability {
                    path: path.to_path_buf(),
                    analyzer,
                    available: true,
                    covered_rules,
                });
                let covered = external.covered_rules();
                report
                    .findings
                    .retain(|finding| !covered.contains(&finding.rule.as_str()));
                report.findings.extend(external_findings);
            }
            ExternalFindings::Unavailable { notice } => {
                external_capabilities.push(ExternalCapability {
                    path: path.to_path_buf(),
                    analyzer,
                    available: false,
                    covered_rules,
                });
                emit_external_notice_once(&notice);
            }
        }
    }
    config.suppression.retain(&mut report.findings);
    sort_findings(&mut report.findings);
    Ok(ScannedFile {
        report,
        source_text,
        external_capabilities,
    })
}

#[cfg(test)]
fn scan_file_with_registries(
    path: &Path,
    lang_registry: &LangRegistry,
    analyzer_registry: &AnalyzerRegistry,
    config: AnalyzerConfig,
) -> Result<FileReport> {
    let Some(lang_pack) = lang_registry.supported_pack_for_path(path) else {
        return Ok(empty_report(path, Lang::Generic));
    };
    let Some(pack) = analyzer_registry.pack_for_lang(lang_pack.lang()) else {
        return Ok(empty_report(path, lang_pack.lang()));
    };
    scan_file_with_pack(path, pack, config)
}

fn analysis_pack_for_path(
    path: &Path,
    lang_registry: &LangRegistry,
    analyzer_registry: &AnalyzerRegistry,
) -> Option<&'static dyn AnalysisPack> {
    let lang_pack = lang_registry.supported_pack_for_path(path)?;
    analyzer_registry.pack_for_lang(lang_pack.lang())
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
    let registry = AnalyzerRegistry::default();
    let Some(pack) = registry.pack_for_lang(source.lang) else {
        return FileReport {
            path: source.path.to_path_buf(),
            lang: source.lang,
            analysis: source_analysis_provenance(source),
            findings: Vec::new(),
        };
    };
    scan_source_with_pack(source, pack, &config)
}

fn scan_source_with_pack(
    source: &SourceFile,
    pack: &'static dyn AnalysisPack,
    config: &AnalyzerConfig,
) -> FileReport {
    let analysis = source_analysis_provenance(source);
    if analysis.status == AnalysisStatus::Unsupported {
        let mut findings = Vec::new();
        findings.extend(run_rules(&AGNOSTIC_PACK, source, config));
        findings.extend(run_rules(pack, source, config));
        for finding in &mut findings {
            finding.safety = SafetyClass::NeverAuto;
            finding.edit = None;
            finding.precondition = Some(
                "report-only text evidence; install a parser adapter before proposing rewrites"
                    .to_string(),
            );
        }
        config.suppression.retain(&mut findings);
        sort_findings(&mut findings);
        return FileReport {
            path: source.path.to_path_buf(),
            lang: source.lang,
            analysis,
            findings,
        };
    }
    if !analysis.permits_rewrites() {
        return FileReport {
            path: source.path.to_path_buf(),
            lang: source.lang,
            analysis,
            findings: Vec::new(),
        };
    }
    let mut findings = Vec::new();
    findings.extend(run_rules(&AGNOSTIC_PACK, source, config));
    findings.extend(run_rules(pack, source, config));
    config.suppression.retain(&mut findings);
    apply_inline_suppression(source, &mut findings);
    sort_findings(&mut findings);
    FileReport {
        path: source.path.to_path_buf(),
        lang: source.lang,
        analysis,
        findings,
    }
}

fn source_analysis_provenance(source: &SourceFile) -> AnalysisProvenance {
    analysis_provenance_or_failed(source)
}

fn apply_inline_suppression(source: &SourceFile, findings: &mut Vec<Finding>) {
    let directives = inline_suppression_lines(source);
    findings.retain(|finding| !is_inline_suppressed(finding, &directives));
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

fn inline_suppression_lines(source: &SourceFile) -> InlineSuppressions {
    let mut directives = InlineSuppressions::default();
    let Some(tree) = parse_source(source).ok().flatten() else {
        return directives;
    };
    if tree.root_node().has_error() {
        return directives;
    }
    collect_comment_directives(tree.root_node(), source, &mut directives);
    directives
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

fn collect_comment_directives(
    node: tree_sitter::Node<'_>,
    source: &SourceFile,
    directives: &mut InlineSuppressions,
) {
    if node.kind().contains("comment") {
        if let Some(comment_text) = source.text.get(node.start_byte()..node.end_byte()) {
            let line = source.line_for_byte(node.start_byte());
            for (line_offset, line_text) in comment_text.lines().enumerate() {
                if let Some((next_line, rule_names)) = inline_ignore_rules_for_line(line_text) {
                    if next_line {
                        directives.add_next_line(line + line_offset, &rule_names);
                    } else {
                        directives.add_same_line(line + line_offset, &rule_names);
                    }
                }
            }
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_comment_directives(child, source, directives);
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

fn run_rules(
    pack: &dyn AnalysisPack,
    source: &SourceFile,
    config: &AnalyzerConfig,
) -> Vec<Finding> {
    pack.rules()
        .iter()
        .flat_map(|rule| rule.check(source, config))
        .collect()
}

fn agnostic_findings(source: &SourceFile, config: &AnalyzerConfig) -> Vec<Finding> {
    agnostic::findings(source, config)
}

fn clojure_findings_rule(source: &SourceFile, _config: &AnalyzerConfig) -> Vec<Finding> {
    clojure::findings(source)
}

fn julia_findings_rule(source: &SourceFile, _config: &AnalyzerConfig) -> Vec<Finding> {
    julia::findings(source)
}

fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.span.start_line.cmp(&b.span.start_line))
            .then(a.rule.cmp(&b.rule))
    });
}

fn emit_external_notice_once(notice: &str) {
    if !EXTERNAL_NOTICE_EMITTED.swap(true, Ordering::Relaxed) {
        eprintln!("{notice}");
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn finding(
    source: &SourceFile,
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
        path: source.path.to_path_buf(),
        span,
        rule: rule.to_string(),
        severity,
        safety,
        detected_by,
        message: message.to_string(),
        suggestion: suggestion.to_string(),
        precondition: precondition.map(str::to_string),
        edit,
        fingerprint: baseline_fingerprint(&source.path, rule, span, &text),
    }
}
