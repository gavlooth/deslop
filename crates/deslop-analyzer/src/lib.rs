use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use deslop_core::{
    DetectedBy, Edit, FileReport, Finding, Lang, SafetyClass, Severity, Span, fingerprint,
};
use deslop_external::{
    CljKondoAnalyzer, ExternalAnalyzer as ExternalAnalyzerTrait, ExternalFindings, JuliaAnalyzer,
};
use deslop_lang::{Registry as LangRegistry, Rule};
use deslop_parse::SourceFile;
use ignore::WalkBuilder;

mod agnostic;
mod clojure;
mod julia;
mod packs;
#[cfg(test)]
mod test_pack;
#[cfg(test)]
mod tests;
mod tokens;

static EXTERNAL_NOTICE_EMITTED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JuliaExternal {
    Off,
    StaticLint,
    Jet,
}

#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    pub min_duplication_tokens: usize,
    pub rust_external: bool,
    pub julia_external: JuliaExternal,
    pub julia_project: Option<PathBuf>,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            min_duplication_tokens: 24,
            rust_external: false,
            julia_external: JuliaExternal::Off,
            julia_project: None,
        }
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

pub fn scan_paths(paths: &[PathBuf]) -> Result<Vec<FileReport>> {
    scan_paths_with_config(paths, AnalyzerConfig::default())
}

pub fn scan_paths_with_config(
    paths: &[PathBuf],
    config: AnalyzerConfig,
) -> Result<Vec<FileReport>> {
    let lang_registry = LangRegistry::default();
    let analyzer_registry = AnalyzerRegistry::default();
    let paths = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };

    let mut reports = Vec::new();
    for path in paths {
        if path.is_file() {
            push_supported_report(
                &mut reports,
                &path,
                &lang_registry,
                &analyzer_registry,
                &config,
            )?;
            continue;
        }

        let walker = WalkBuilder::new(&path)
            .hidden(false)
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                !matches!(name.as_ref(), ".git" | ".jj" | "target" | "__pycache__")
            })
            .build();

        for entry in walker {
            let entry = entry.with_context(|| format!("failed to walk {}", path.display()))?;
            let file_type = entry.file_type();
            if !file_type.is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let path = entry.into_path();
            push_supported_report(
                &mut reports,
                &path,
                &lang_registry,
                &analyzer_registry,
                &config,
            )?;
        }
    }
    reports.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(reports)
}

fn push_supported_report(
    reports: &mut Vec<FileReport>,
    path: &Path,
    lang_registry: &LangRegistry,
    analyzer_registry: &AnalyzerRegistry,
    config: &AnalyzerConfig,
) -> Result<()> {
    if let Some(pack) = analysis_pack_for_path(path, lang_registry, analyzer_registry) {
        reports.push(scan_file_with_pack(path, pack, config.to_owned())?);
    }
    Ok(())
}

pub fn scan_file(path: &Path) -> Result<FileReport> {
    scan_file_with_config(path, AnalyzerConfig::default())
}

pub fn scan_file_with_config(path: &Path, config: AnalyzerConfig) -> Result<FileReport> {
    let lang_registry = LangRegistry::default();
    let analyzer_registry = AnalyzerRegistry::default();
    let Some(lang_pack) = lang_registry.supported_pack_for_path(path) else {
        return Ok(empty_report(path, Lang::Generic));
    };
    let Some(pack) = analyzer_registry.pack_for_lang(lang_pack.lang()) else {
        return Ok(empty_report(path, lang_pack.lang()));
    };
    scan_file_with_pack(path, pack, config)
}

fn scan_file_with_pack(
    path: &Path,
    pack: &'static dyn AnalysisPack,
    config: AnalyzerConfig,
) -> Result<FileReport> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let source = SourceFile::new_with_lang(path.to_path_buf(), text, pack.lang());
    let mut report = scan_source_with_pack(&source, pack, &config);
    if let Some(external) = pack.external_analyzer(&config) {
        match external.analyze(path, &source)? {
            ExternalFindings::Available(external_findings) => {
                let covered = external.covered_rules();
                report
                    .findings
                    .retain(|finding| !covered.contains(&finding.rule.as_str()));
                report.findings.extend(external_findings);
            }
            ExternalFindings::Unavailable { notice } => emit_external_notice_once(&notice),
        }
    }
    sort_findings(&mut report.findings);
    Ok(report)
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
        findings: Vec::new(),
    }
}

pub fn scan_source(source: &SourceFile) -> FileReport {
    scan_source_with_config(source, AnalyzerConfig::default())
}

pub fn scan_source_with_config(source: &SourceFile, config: AnalyzerConfig) -> FileReport {
    let registry = AnalyzerRegistry::default();
    let Some(pack) = registry.pack_for_lang(source.lang) else {
        let mut findings = run_rules(&AGNOSTIC_PACK, source, &config);
        sort_findings(&mut findings);
        return FileReport {
            path: source.path.to_path_buf(),
            lang: source.lang,
            findings,
        };
    };
    scan_source_with_pack(source, pack, &config)
}

fn scan_source_with_pack(
    source: &SourceFile,
    pack: &'static dyn AnalysisPack,
    config: &AnalyzerConfig,
) -> FileReport {
    let mut findings = Vec::new();
    findings.extend(run_rules(&AGNOSTIC_PACK, source, config));
    findings.extend(run_rules(pack, source, config));
    sort_findings(&mut findings);
    FileReport {
        path: source.path.to_path_buf(),
        lang: source.lang,
        findings,
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
    agnostic::findings(source, config.min_duplication_tokens)
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
        fingerprint: fingerprint(&source.path, rule, span, &text),
    }
}
