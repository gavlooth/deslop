use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use deslop_analyzer::{AnalyzerConfig, JuliaExternal, scan_paths, scan_paths_with_config};
use deslop_core::{FileReport, Severity};
use deslop_eval::{render_eval_json, render_eval_text, run_eval};
use deslop_fix::undo_paths;
use deslop_metrics::{
    MetricsConfig, metrics_paths, render_json as render_metrics_json,
    render_text as render_metrics_text,
};
use deslop_report::{render_agent, render_json, render_sarif, render_text};
use deslop_slim::{
    AnthropicClient, DEFAULT_MODEL, OpenAiClient, RecordedClient, SlimOptions, run_slim,
};
use deslop_verify::{
    CoverageConfig, MutationConfig, VerifyOptions, apply_patches,
    characterization_work_orders_for_patches, load_characterization_tests, load_patches,
    parse_coverage_mode, verify_characterization_tests, verify_patches,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Deterministic code-bloat analyzer with agent-ready output"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        default_value = "deslop.toml"
    )]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Scan(ScanArgs),
    #[command(alias = "health")]
    Metrics(MetricsArgs),
    #[cfg(feature = "mcp")]
    Mcp,
    Fix(FixArgs),
    Propose(ProposeArgs),
    Eval(EvalArgs),
    Slop(SlopArgs),
    Characterize(CharacterizeArgs),
    VerifyCharacterization(VerifyCharacterizationArgs),
    Verify(PatchArgs),
    Apply(ApplyArgs),
    Baseline(BaselineArgs),
    Undo(PathArgs),
    Rules,
}

#[derive(Debug, Args)]
struct PathArgs {
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,
}

#[derive(Debug, Args)]
struct ScanArgs {
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,

    #[arg(long)]
    baseline: Option<PathBuf>,

    #[arg(long, value_enum)]
    fail_on: Option<SeverityArg>,

    #[arg(long)]
    since: Option<String>,

    #[arg(long)]
    rust_external: bool,

    #[arg(long, value_enum, num_args = 0..=1, default_missing_value = "staticlint")]
    julia_external: Option<JuliaExternalArg>,

    #[arg(long)]
    julia_project: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct FixArgs {
    #[arg(long, value_name = "PATH", num_args = 1..)]
    paths: Vec<PathBuf>,

    #[arg(long)]
    workorders: Option<PathBuf>,

    #[arg(long)]
    apply: bool,

    #[arg(long)]
    characterize: bool,

    #[arg(long, default_missing_value = "true", num_args = 0..=1)]
    allow_unverified: Option<bool>,

    #[arg(long, value_name = "MODE")]
    coverage: Option<String>,

    #[arg(long)]
    model: Option<String>,

    #[arg(long, value_enum)]
    provider: Option<SlimProvider>,

    #[arg(long)]
    base_url: Option<String>,

    #[arg(long)]
    mock: Option<PathBuf>,

    #[arg(long)]
    check_cmd: Option<String>,

    #[arg(long)]
    no_backup: bool,
}

#[derive(Debug, Args)]
struct ProposeArgs {
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    #[arg(short, long)]
    output: Option<PathBuf>,

    #[arg(long)]
    rust_external: bool,

    #[arg(long, value_enum, num_args = 0..=1, default_missing_value = "staticlint")]
    julia_external: Option<JuliaExternalArg>,

    #[arg(long)]
    julia_project: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct MetricsArgs {
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    #[arg(long, value_enum, default_value_t = MetricsFormat::Text)]
    format: MetricsFormat,

    #[arg(long)]
    hotspots_only: bool,

    #[arg(long, default_value_t = 2.0)]
    sigma: f64,
}

#[derive(Debug, Args)]
struct EvalArgs {
    #[arg(default_value = "tests/corpus")]
    corpus: PathBuf,

    #[arg(long, value_enum, default_value_t = MetricsFormat::Text)]
    format: MetricsFormat,
}

#[derive(Debug, Args)]
struct SlopArgs {
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    #[arg(long, value_enum, default_value_t = MetricsFormat::Text)]
    format: MetricsFormat,
}

#[derive(Debug, Args)]
struct PatchArgs {
    #[arg(long)]
    patches: PathBuf,

    #[arg(long)]
    check_cmd: Option<String>,

    #[arg(long)]
    coverage: bool,

    #[arg(long)]
    mutation: bool,

    #[arg(long)]
    characterization_tests: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ApplyArgs {
    #[arg(long)]
    patches: PathBuf,

    #[arg(long)]
    check_cmd: Option<String>,

    #[arg(long)]
    coverage: bool,

    #[arg(long)]
    mutation: bool,

    #[arg(long)]
    characterization_tests: Option<PathBuf>,

    #[arg(long)]
    allow_non_removable: bool,

    #[arg(long)]
    no_backup: bool,
}

#[derive(Debug, Args)]
struct CharacterizeArgs {
    #[arg(long)]
    patches: PathBuf,

    #[arg(short, long)]
    output: Option<PathBuf>,

    #[arg(long)]
    check_cmd: Option<String>,

    #[arg(long)]
    coverage: bool,

    #[arg(long)]
    mutation: bool,
}

#[derive(Debug, Args)]
struct VerifyCharacterizationArgs {
    #[arg(long)]
    tests: PathBuf,

    #[arg(long)]
    check_cmd: String,
}

#[derive(Debug, Args)]
struct BaselineArgs {
    #[command(subcommand)]
    command: BaselineCommand,
}

#[derive(Debug, Subcommand)]
enum BaselineCommand {
    Write {
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,

        #[arg(short, long, default_value = "deslop-baseline.json")]
        output: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
    Sarif,
    Agent,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum MetricsFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum SeverityArg {
    Info,
    Minor,
    Major,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum JuliaExternalArg {
    Staticlint,
    Jet,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum SlimProvider {
    Anthropic,
    Openai,
}

impl From<JuliaExternalArg> for JuliaExternal {
    fn from(value: JuliaExternalArg) -> Self {
        match value {
            JuliaExternalArg::Staticlint => JuliaExternal::StaticLint,
            JuliaExternalArg::Jet => JuliaExternal::Jet,
            JuliaExternalArg::Off => JuliaExternal::Off,
        }
    }
}

impl From<SeverityArg> for Severity {
    fn from(value: SeverityArg) -> Self {
        match value {
            SeverityArg::Info => Severity::Info,
            SeverityArg::Minor => Severity::Minor,
            SeverityArg::Major => Severity::Major,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct DeslopConfig {
    #[serde(default)]
    external: Option<ExternalConfig>,
    #[serde(default)]
    slim: Option<SlimConfig>,
    #[serde(default)]
    fix: Option<FixConfig>,
    #[serde(default)]
    scan: Option<ScanConfig>,
    #[serde(default)]
    analyzer: Option<AnalyzerConfigSection>,
}

impl DeslopConfig {
    fn read_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = read_to_string_ctx(path)?;
        toml::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
    }
}

fn read_to_string_ctx(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

#[derive(Debug, Default, Deserialize)]
struct ExternalConfig {
    #[serde(default)]
    julia_analyzer: Option<JuliaExternalConfig>,
    #[serde(default)]
    julia_project: Option<PathBuf>,
    #[serde(default)]
    clippy: Option<ClippyConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct SlimConfig {
    #[serde(default)]
    provider: Option<SlimProvider>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FixConfig {
    #[serde(default)]
    check_cmd: Option<String>,
    #[serde(default)]
    coverage: Option<String>,
    #[serde(default)]
    allow_unverified: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct ScanConfig {
    #[serde(default)]
    fail_on: Option<SeverityArg>,
    #[serde(default)]
    baseline: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
struct AnalyzerConfigSection {
    #[serde(default)]
    min_duplication_tokens: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum JuliaExternalConfig {
    Staticlint,
    Jet,
    Off,
}

impl From<JuliaExternalConfig> for JuliaExternal {
    fn from(value: JuliaExternalConfig) -> Self {
        match value {
            JuliaExternalConfig::Staticlint => JuliaExternal::StaticLint,
            JuliaExternalConfig::Jet => JuliaExternal::Jet,
            JuliaExternalConfig::Off => JuliaExternal::Off,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ClippyConfig {
    On,
    Off,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = DeslopConfig::read_from(&cli.config)?;
    match cli.command {
        Command::Scan(args) => scan(args, &config),
        Command::Metrics(args) => metrics(args),
        #[cfg(feature = "mcp")]
        Command::Mcp => deslop_mcp::run_stdio(),
        Command::Fix(args) => fix(args, &config),
        Command::Propose(args) => propose(args, &config),
        Command::Eval(args) => eval(args),
        Command::Slop(args) => slop(args),
        Command::Characterize(args) => characterize(args),
        Command::VerifyCharacterization(args) => verify_characterization(args),
        Command::Verify(args) => verify(args),
        Command::Apply(args) => apply(args),
        Command::Baseline(args) => baseline(args),
        Command::Undo(args) => undo(args),
        Command::Rules => rules(),
    }
}

fn metrics(args: MetricsArgs) -> Result<()> {
    let report = metrics_paths(&args.paths, MetricsConfig { sigma: args.sigma })?;
    let rendered = match args.format {
        MetricsFormat::Text => render_metrics_text(&report, args.hotspots_only),
        MetricsFormat::Json => render_metrics_json(&report)?,
    };
    print!("{rendered}");
    Ok(())
}

fn scan(args: ScanArgs, config: &DeslopConfig) -> Result<()> {
    let paths = paths_since(args.paths, args.since)?;
    let analyzer = analyzer_config(
        config,
        args.rust_external,
        args.julia_external,
        args.julia_project,
    );
    let mut reports = scan_paths_with_config(&paths, analyzer)?;
    if let Some(path) = resolve_scan_baseline(args.baseline, config) {
        let baseline = Baseline::read(&path)?;
        suppress_baseline(&mut reports, &baseline);
    }

    let rendered = match args.format {
        Format::Text => render_text(&reports),
        Format::Json => render_json(&reports)?,
        Format::Sarif => render_sarif(&reports)?,
        Format::Agent => render_agent(&reports)?,
    };
    print!("{rendered}");

    if let Some(threshold) = resolve_scan_fail_on(args.fail_on, config) {
        let should_fail = reports
            .iter()
            .flat_map(|report| &report.findings)
            .any(|finding| finding.severity.passes_threshold(threshold));
        if should_fail {
            std::process::exit(1);
        }
    }
    Ok(())
}

fn fix(args: FixArgs, config: &DeslopConfig) -> Result<()> {
    let model = resolve_slim_model(args.model, std::env::var("DESLOP_SLIM_MODEL").ok(), config);
    let paths = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths
    };
    let coverage = resolve_fix_coverage(args.coverage, config)?;
    let check_cmd = resolve_fix_check_cmd(args.check_cmd, config);
    let allow_unverified = resolve_fix_allow_unverified(args.allow_unverified, config);
    let provider = resolve_slim_provider(args.provider, config);
    let base_url = resolve_slim_base_url(args.base_url, config);
    let options = SlimOptions {
        root: PathBuf::from("."),
        paths,
        workorders: args.workorders,
        apply: args.apply,
        characterize: args.characterize,
        allow_unverified,
        coverage,
        model: model.to_owned(),
        check_cmd,
        backup: !args.no_backup,
    };
    let report = if let Some(path) = args.mock {
        let client = RecordedClient::from_path(path)?;
        run_slim(&client, options)?
    } else {
        match provider {
            SlimProvider::Anthropic => {
                let client = AnthropicClient::from_env(model.clone())?;
                run_slim(&client, options)?
            }
            SlimProvider::Openai => {
                let client = OpenAiClient::from_env(model.clone(), base_url)?;
                run_slim(&client, options)?
            }
        }
    };
    print_pretty_json(&report)?;
    Ok(())
}

fn propose(args: ProposeArgs, config: &DeslopConfig) -> Result<()> {
    let analyzer = analyzer_config(
        config,
        args.rust_external,
        args.julia_external,
        args.julia_project,
    );
    let reports = scan_paths_with_config(&args.paths, analyzer)?;
    let rendered = render_agent(&reports)?;
    if let Some(output) = args.output {
        fs::write(&output, rendered)
            .with_context(|| format!("failed to write {}", output.display()))?;
    } else {
        print!("{rendered}");
    }
    Ok(())
}

fn eval(args: EvalArgs) -> Result<()> {
    let report = run_eval(&args.corpus)?;
    let rendered = match args.format {
        MetricsFormat::Text => render_eval_text(&report),
        MetricsFormat::Json => render_eval_json(&report)?,
    };
    print!("{rendered}");
    if !rendered.ends_with('\n') {
        println!();
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct SlopReport {
    schema: &'static str,
    score: f64,
    files: Vec<FileSlopScore>,
    rule_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct FileSlopScore {
    path: PathBuf,
    score: f64,
    findings: usize,
    nloc: usize,
    rule_counts: BTreeMap<String, usize>,
}

fn slop(args: SlopArgs) -> Result<()> {
    let reports = scan_paths_with_config(&args.paths, AnalyzerConfig::default())?;
    let report = slop_report(&reports)?;
    match args.format {
        MetricsFormat::Text => print!("{}", render_slop_text(&report)),
        MetricsFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(())
}

fn slop_report(reports: &[FileReport]) -> Result<SlopReport> {
    let mut rule_counts = BTreeMap::new();
    let mut files = reports
        .iter()
        .map(|report| slop_score_for_file(report, &mut rule_counts))
        .collect::<Result<Vec<_>>>()?;
    files.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.path.cmp(&b.path))
    });
    let score = if files.is_empty() {
        0.0
    } else {
        files.iter().map(|file| file.score).sum::<f64>() / files.len() as f64
    };
    Ok(SlopReport {
        schema: "deslop.slop/1",
        score,
        files,
        rule_counts,
    })
}

fn slop_score_for_file(
    report: &FileReport,
    rule_counts: &mut BTreeMap<String, usize>,
) -> Result<FileSlopScore> {
    let text = read_to_string_ctx(&report.path)?;
    let nloc = text.lines().filter(|line| !line.trim().is_empty()).count();
    let mut file_rules = BTreeMap::new();
    let mut weighted = 0.0;
    for finding in &report.findings {
        if let Some(weight) = slop_weight(&finding.rule) {
            *file_rules.entry(finding.rule.to_owned()).or_default() += 1;
            *rule_counts.entry(finding.rule.to_owned()).or_default() += 1;
            weighted += weight;
        }
    }
    let density = if nloc == 0 {
        0.0
    } else {
        weighted * 100.0 / nloc as f64
    };
    Ok(FileSlopScore {
        path: report.path.to_path_buf(),
        score: density.min(100.0),
        findings: file_rules.values().sum(),
        nloc,
        rule_counts: file_rules,
    })
}

fn slop_weight(rule: &str) -> Option<f64> {
    match rule {
        "incompleteness" => Some(25.0),
        "long-method" => Some(15.0),
        "duplicate-block" | "near-duplicate" => Some(10.0),
        "comment-block" | "narrating-comment" => Some(5.0),
        "magic-number" | "needless-clone" | "needless-return" | "let-and-return" => Some(4.0),
        _ => None,
    }
}

fn render_slop_text(report: &SlopReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("Slop score: {:.1}/100\n", report.score));
    out.push_str("rule counts:\n");
    if report.rule_counts.is_empty() {
        out.push_str("  none\n");
    } else {
        for (rule, count) in &report.rule_counts {
            out.push_str(&format!("  {rule:<20} {count}\n"));
        }
    }
    out.push_str("\nfiles:\n");
    for file in report
        .files
        .iter()
        .filter(|file| file.findings > 0)
        .take(20)
    {
        out.push_str(&format!(
            "  {:>5.1} {:>3} finding(s) {}\n",
            file.score,
            file.findings,
            file.path.display()
        ));
    }
    out
}

fn analyzer_config(
    config: &DeslopConfig,
    rust_external: bool,
    julia_external: Option<JuliaExternalArg>,
    julia_project: Option<PathBuf>,
) -> AnalyzerConfig {
    analyzer_config_from_config(config, rust_external, julia_external, julia_project)
}

fn analyzer_config_from_config(
    config: &DeslopConfig,
    rust_external: bool,
    julia_external: Option<JuliaExternalArg>,
    julia_project: Option<PathBuf>,
) -> AnalyzerConfig {
    let external = config.external.as_ref();
    let configured_julia = external
        .and_then(|external| external.julia_analyzer)
        .map(JuliaExternal::from)
        .unwrap_or(JuliaExternal::Off);
    let configured_project = external.and_then(|external| external.julia_project.to_owned());
    let configured_clippy = external
        .and_then(|external| external.clippy)
        .is_some_and(|value| value == ClippyConfig::On);
    let default = AnalyzerConfig::default();
    let min_duplication_tokens = config
        .analyzer
        .as_ref()
        .and_then(|analyzer| analyzer.min_duplication_tokens)
        .unwrap_or(default.min_duplication_tokens);

    AnalyzerConfig {
        min_duplication_tokens,
        rust_external: rust_external || configured_clippy,
        julia_external: julia_external
            .map(JuliaExternal::from)
            .unwrap_or(configured_julia),
        julia_project: julia_project.or(configured_project),
    }
}

fn resolve_scan_baseline(cli: Option<PathBuf>, config: &DeslopConfig) -> Option<PathBuf> {
    cli.or_else(|| config.scan.as_ref().and_then(|scan| scan.baseline.clone()))
}

fn resolve_scan_fail_on(cli: Option<SeverityArg>, config: &DeslopConfig) -> Option<Severity> {
    cli.or_else(|| config.scan.as_ref().and_then(|scan| scan.fail_on))
        .map(Severity::from)
}

fn resolve_slim_provider(cli: Option<SlimProvider>, config: &DeslopConfig) -> SlimProvider {
    cli.or_else(|| config.slim.as_ref().and_then(|slim| slim.provider))
        .unwrap_or(SlimProvider::Anthropic)
}

fn resolve_slim_base_url(cli: Option<String>, config: &DeslopConfig) -> Option<String> {
    cli.or_else(|| config.slim.as_ref().and_then(|slim| slim.base_url.clone()))
}

fn resolve_slim_model(
    cli: Option<String>,
    env_model: Option<String>,
    config: &DeslopConfig,
) -> String {
    cli.or(env_model)
        .or_else(|| config.slim.as_ref().and_then(|slim| slim.model.clone()))
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

fn resolve_fix_check_cmd(cli: Option<String>, config: &DeslopConfig) -> Option<String> {
    cli.or_else(|| config.fix.as_ref().and_then(|fix| fix.check_cmd.clone()))
}

fn resolve_fix_allow_unverified(cli: Option<bool>, config: &DeslopConfig) -> bool {
    cli.or_else(|| config.fix.as_ref().and_then(|fix| fix.allow_unverified))
        .unwrap_or(false)
}

fn resolve_fix_coverage(cli: Option<String>, config: &DeslopConfig) -> Result<CoverageConfig> {
    let mode = cli
        .or_else(|| config.fix.as_ref().and_then(|fix| fix.coverage.clone()))
        .unwrap_or_else(|| "disabled".to_string());
    parse_coverage_config(&mode)
}

fn characterize(args: CharacterizeArgs) -> Result<()> {
    let patches = load_patches(&args.patches)?;
    let work_orders = characterization_work_orders_for_patches(
        &patches,
        &verify_options(
            args.check_cmd,
            args.coverage,
            args.mutation,
            Vec::new(),
            false,
        ),
    )?;
    let mut rendered = String::new();
    for work_order in work_orders {
        rendered.push_str(&serde_json::to_string(&work_order)?);
        rendered.push('\n');
    }
    if let Some(output) = args.output {
        fs::write(&output, rendered)
            .with_context(|| format!("failed to write {}", output.display()))?;
    } else {
        print!("{rendered}");
    }
    Ok(())
}

fn verify_characterization(args: VerifyCharacterizationArgs) -> Result<()> {
    let tests = load_characterization_tests(&args.tests)?;
    let report = verify_characterization_tests(
        &tests,
        &verify_options(Some(args.check_cmd), false, false, Vec::new(), false),
    )?;
    print_pretty_json(&report)?;
    if report.rejected_count() > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn verify(args: PatchArgs) -> Result<()> {
    let patches = load_patches(&args.patches)?;
    let characterization_tests =
        load_optional_characterization_tests(&args.characterization_tests)?;
    let report = verify_patches(
        &patches,
        &verify_options(
            args.check_cmd,
            args.coverage,
            args.mutation,
            characterization_tests,
            false,
        ),
    )?;
    print_pretty_json(&report)?;
    if report.failed_count() > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn apply(args: ApplyArgs) -> Result<()> {
    let patches = load_patches(&args.patches)?;
    let characterization_tests =
        load_optional_characterization_tests(&args.characterization_tests)?;
    let report = apply_patches(
        &patches,
        &verify_options(
            args.check_cmd,
            args.coverage,
            args.mutation,
            characterization_tests,
            args.allow_non_removable,
        ),
        !args.no_backup,
    )?;
    print_pretty_json(&report)?;
    if report.verified.failed_count() > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn load_optional_characterization_tests(
    path: &Option<PathBuf>,
) -> Result<Vec<deslop_protocol::CharacterizationTest>> {
    path.as_deref()
        .map(load_characterization_tests)
        .transpose()
        .map(Option::unwrap_or_default)
}

fn coverage_config(enabled: bool) -> CoverageConfig {
    if enabled {
        CoverageConfig::Auto
    } else {
        CoverageConfig::Disabled
    }
}

fn parse_coverage_config(value: &str) -> Result<CoverageConfig> {
    parse_coverage_mode(value)
}

fn mutation_config(enabled: bool) -> MutationConfig {
    if enabled {
        MutationConfig::Auto
    } else {
        MutationConfig::Disabled
    }
}

fn verify_options(
    check_cmd: Option<String>,
    coverage: bool,
    mutation: bool,
    characterization_tests: Vec<deslop_protocol::CharacterizationTest>,
    allow_non_removable: bool,
) -> VerifyOptions {
    VerifyOptions {
        root: PathBuf::from("."),
        check_cmd,
        coverage: coverage_config(coverage),
        mutation: mutation_config(mutation),
        characterization_tests,
        allow_non_removable,
    }
}

fn print_pretty_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn baseline(args: BaselineArgs) -> Result<()> {
    match args.command {
        BaselineCommand::Write { paths, output } => {
            let reports = scan_paths(&paths)?;
            let baseline = Baseline::from_reports(&reports);
            let rendered = serde_json::to_string_pretty(&baseline)?;
            fs::write(&output, rendered)
                .with_context(|| format!("failed to write {}", output.display()))?;
            println!(
                "wrote {} fingerprint(s) to {}",
                baseline.fingerprints.len(),
                output.display()
            );
        }
    }
    Ok(())
}

fn undo(args: PathArgs) -> Result<()> {
    let restored = undo_paths(&args.paths)?;
    for path in restored {
        println!("restored {}", path.display());
    }
    Ok(())
}

fn rules() -> Result<()> {
    io::stdout().write_all(RULES.as_bytes())?;
    Ok(())
}

fn paths_since(paths: Vec<PathBuf>, since: Option<String>) -> Result<Vec<PathBuf>> {
    let Some(since) = since else {
        return Ok(paths);
    };
    let output = std::process::Command::new("git")
        .args(["diff", "--name-only", &since, "--"])
        .args(paths.iter())
        .output()
        .context("failed to run git diff for --since")?;
    if !output.status.success() {
        anyhow::bail!(
            "--since requires git-compatible history; git diff failed with status {}",
            output.status
        );
    }
    let changed = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    Ok(changed)
}

fn suppress_baseline(reports: &mut [FileReport], baseline: &Baseline) {
    for report in reports {
        report
            .findings
            .retain(|finding| !baseline.fingerprints.contains(&finding.fingerprint));
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Baseline {
    schema: String,
    fingerprints: BTreeSet<String>,
}

impl Baseline {
    fn read(path: &Path) -> Result<Self> {
        let text = read_to_string_ctx(path)?;
        serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
    }

    fn from_reports(reports: &[FileReport]) -> Self {
        let fingerprints = reports
            .iter()
            .flat_map(|report| &report.findings)
            .map(|finding| finding.fingerprint.to_owned())
            .collect();
        Self {
            schema: "deslop.baseline/1".to_string(),
            fingerprints,
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn parses_external_config() {
        let config: DeslopConfig = toml::from_str(
            r#"
            [external]
            clippy = "on"
            julia_analyzer = "staticlint"
            julia_project = "julia-env"
            "#,
        )
        .expect("parse config");
        let analyzer = analyzer_config_from_config(&config, false, None, None);
        assert!(analyzer.rust_external);
        assert_eq!(analyzer.julia_external, JuliaExternal::StaticLint);
        assert_eq!(analyzer.julia_project, Some(PathBuf::from("julia-env")));
    }

    #[test]
    fn parses_all_config_sections() {
        let config: DeslopConfig = toml::from_str(
            r#"
            [slim]
            provider = "openai"
            model = "configured-model"
            base_url = "http://localhost:11434/v1"

            [fix]
            check_cmd = "cargo test -p configured"
            coverage = "lcov:coverage.lcov"
            allow_unverified = true

            [scan]
            fail_on = "major"
            baseline = "deslop-baseline.json"

            [analyzer]
            min_duplication_tokens = 42

            [external]
            clippy = "on"
            julia_analyzer = "jet"
            julia_project = "julia-env"
            "#,
        )
        .expect("parse config");

        assert_eq!(resolve_slim_provider(None, &config), SlimProvider::Openai);
        assert_eq!(resolve_slim_model(None, None, &config), "configured-model");
        assert_eq!(
            resolve_slim_base_url(None, &config).as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert_eq!(
            resolve_fix_check_cmd(None, &config).as_deref(),
            Some("cargo test -p configured")
        );
        assert!(resolve_fix_allow_unverified(None, &config));
        assert!(matches!(
            resolve_fix_coverage(None, &config).expect("parse coverage"),
            CoverageConfig::LcovFile(path) if path == PathBuf::from("coverage.lcov")
        ));
        assert_eq!(
            resolve_scan_baseline(None, &config),
            Some(PathBuf::from("deslop-baseline.json"))
        );
        assert_eq!(resolve_scan_fail_on(None, &config), Some(Severity::Major));

        let analyzer = analyzer_config_from_config(&config, false, None, None);
        assert_eq!(analyzer.min_duplication_tokens, 42);
        assert!(analyzer.rust_external);
        assert_eq!(analyzer.julia_external, JuliaExternal::Jet);
        assert_eq!(analyzer.julia_project, Some(PathBuf::from("julia-env")));
    }

    #[test]
    fn cli_julia_external_overrides_config() {
        let config: DeslopConfig = toml::from_str(
            r#"
            [external]
            julia_analyzer = "staticlint"
            julia_project = "configured"
            "#,
        )
        .expect("parse config");
        let analyzer = analyzer_config_from_config(
            &config,
            false,
            Some(JuliaExternalArg::Off),
            Some(PathBuf::from("cli-project")),
        );
        assert_eq!(analyzer.julia_external, JuliaExternal::Off);
        assert_eq!(analyzer.julia_project, Some(PathBuf::from("cli-project")));
    }

    #[test]
    fn slim_model_precedence_is_cli_env_config_default() {
        let config: DeslopConfig = toml::from_str(
            r#"
            [slim]
            model = "config-model"
            "#,
        )
        .expect("parse config");

        assert_eq!(
            resolve_slim_model(
                Some("cli-model".to_string()),
                Some("env-model".to_string()),
                &config
            ),
            "cli-model"
        );
        assert_eq!(
            resolve_slim_model(None, Some("env-model".to_string()), &config),
            "env-model"
        );
        assert_eq!(resolve_slim_model(None, None, &config), "config-model");
        assert_eq!(
            resolve_slim_model(None, None, &DeslopConfig::default()),
            DEFAULT_MODEL
        );
    }

    #[test]
    fn scan_precedence_is_cli_config_default() {
        let config: DeslopConfig = toml::from_str(
            r#"
            [scan]
            fail_on = "minor"
            baseline = "configured-baseline.json"
            "#,
        )
        .expect("parse config");

        assert_eq!(
            resolve_scan_fail_on(Some(SeverityArg::Major), &config),
            Some(Severity::Major)
        );
        assert_eq!(resolve_scan_fail_on(None, &config), Some(Severity::Minor));
        assert_eq!(resolve_scan_fail_on(None, &DeslopConfig::default()), None);
        assert_eq!(
            resolve_scan_baseline(Some(PathBuf::from("cli-baseline.json")), &config),
            Some(PathBuf::from("cli-baseline.json"))
        );
        assert_eq!(
            resolve_scan_baseline(None, &config),
            Some(PathBuf::from("configured-baseline.json"))
        );
    }

    #[test]
    fn fix_config_coverage_uses_shared_mode_parser() {
        let config: DeslopConfig = toml::from_str(
            r#"
            [fix]
            coverage = "coverage-py:coverage.json"
            allow_unverified = true
            check_cmd = "cargo test"
            "#,
        )
        .expect("parse config");

        assert!(matches!(
            resolve_fix_coverage(None, &config).expect("parse coverage"),
            CoverageConfig::CoveragePyFile(path) if path == PathBuf::from("coverage.json")
        ));
        assert!(matches!(
            resolve_fix_coverage(Some("disabled".to_string()), &config).expect("parse coverage"),
            CoverageConfig::Disabled
        ));
        assert!(!resolve_fix_allow_unverified(Some(false), &config));
        assert_eq!(
            resolve_fix_check_cmd(Some("cargo check".to_string()), &config).as_deref(),
            Some("cargo check")
        );
    }

    #[test]
    fn fix_help_lists_slim_flags() {
        let mut command = Cli::command();
        let fix = command
            .find_subcommand_mut("fix")
            .expect("fix subcommand exists");
        let mut help = Vec::new();
        fix.write_long_help(&mut help).expect("write help");
        let help = String::from_utf8(help).expect("utf8 help");

        for flag in [
            "--paths",
            "--workorders",
            "--apply",
            "--characterize",
            "--allow-unverified",
            "--coverage",
            "--model",
            "--provider",
            "--base-url",
            "--mock",
            "--check-cmd",
        ] {
            assert!(help.contains(flag), "{flag} missing from help:\n{help}");
        }
    }

    #[test]
    fn parses_slim_coverage_modes() {
        assert!(matches!(
            parse_coverage_config("disabled").expect("parse"),
            CoverageConfig::Disabled
        ));
        assert!(matches!(
            parse_coverage_config("auto").expect("parse"),
            CoverageConfig::Auto
        ));
        assert!(matches!(
            parse_coverage_config("auto:cargo").expect("parse"),
            CoverageConfig::AutoWithCommand(command) if command == "cargo"
        ));
        assert!(matches!(
            parse_coverage_config("lcov:coverage.lcov").expect("parse"),
            CoverageConfig::LcovFile(path) if path == PathBuf::from("coverage.lcov")
        ));
        assert!(matches!(
            parse_coverage_config("cloverage:coverage.json").expect("parse"),
            CoverageConfig::CloverageFile(path) if path == PathBuf::from("coverage.json")
        ));
        assert!(matches!(
            parse_coverage_config("julia-cov:coverage.cov").expect("parse"),
            CoverageConfig::JuliaCovFile(path) if path == PathBuf::from("coverage.cov")
        ));
        assert!(matches!(
            parse_coverage_config("coverage-py:coverage.json").expect("parse"),
            CoverageConfig::CoveragePyFile(path) if path == PathBuf::from("coverage.json")
        ));
        assert!(parse_coverage_config("unknown").is_err());
    }

    #[test]
    fn parses_openai_provider_selection_without_network() {
        let cli = Cli::try_parse_from([
            "deslop",
            "fix",
            "--provider",
            "openai",
            "--base-url",
            "http://localhost:11434/v1",
        ])
        .expect("parse cli");

        let Command::Fix(args) = cli.command else {
            panic!("expected fix command");
        };
        assert_eq!(args.provider, Some(SlimProvider::Openai));
        assert_eq!(args.base_url.as_deref(), Some("http://localhost:11434/v1"));
    }

    #[test]
    fn parses_allow_unverified_bool_forms() {
        let cli = Cli::try_parse_from(["deslop", "fix", "--allow-unverified"]).expect("parse cli");
        let Command::Fix(args) = cli.command else {
            panic!("expected fix command");
        };
        assert_eq!(args.allow_unverified, Some(true));

        let cli =
            Cli::try_parse_from(["deslop", "fix", "--allow-unverified=false"]).expect("parse cli");
        let Command::Fix(args) = cli.command else {
            panic!("expected fix command");
        };
        assert_eq!(args.allow_unverified, Some(false));
    }
}

const RULES: &str = "\
rule                    safety                  default
consecutive-blank-lines safe-auto               fix
reimpl-not=             safe-auto               fix
reimpl-some?            safe-auto               fix
reimpl-boolean          safe-auto               fix
redundant-do            safe-auto               fix
reimpl-empty?           safe-with-precondition  suggest (finite/countable collection)
reimpl-seq              safe-with-precondition  suggest (finite/countable collection)
reimpl-vec              safe-with-precondition  suggest (finite collection)
reimpl-isempty          safe-with-precondition  suggest (standard collection semantics)
reimpl-eachindex        safe-with-precondition  suggest (1-based positional indexing)
reimpl-isnothing        risky-suggest           suggest
unused-arg             analyzer-confirmed      fix only with StaticLint confirmation
unused-binding         analyzer-confirmed      fix only with external analyzer confirmation
single-use-binding      risky-suggest           suggest
incompleteness          llm-only                propose
magic-number            risky-suggest           suggest
long-method             llm-only                propose
slop-score              report                  deslop slop
narrating-comment       llm-only                propose
comment-block           llm-only                propose
duplicate-block         llm-only                propose
";
