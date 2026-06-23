use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use deslop_analyzer::{AnalyzerConfig, JuliaExternal, scan_paths, scan_paths_with_config};
use deslop_core::{FileReport, Severity};
use deslop_eval::{render_eval_json, render_eval_text, run_eval};
use deslop_fix::{FixOptions, fix_paths, undo_paths};
use deslop_metrics::{
    MetricsConfig, metrics_paths, render_json as render_metrics_json,
    render_text as render_metrics_text,
};
use deslop_report::{render_agent, render_json, render_sarif, render_text};
use deslop_verify::{
    CoverageConfig, MutationConfig, VerifyOptions, apply_patches,
    characterization_work_orders_for_patches, load_characterization_tests, load_patches,
    verify_characterization_tests, verify_patches,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Deterministic code-bloat analyzer with agent-ready output"
)]
struct Cli {
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
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    #[arg(long)]
    dry_run: bool,

    #[arg(long)]
    no_backup: bool,

    #[arg(long)]
    yes: bool,
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

#[derive(Debug, Clone, Copy, ValueEnum)]
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
}

impl DeslopConfig {
    fn read_default() -> Result<Self> {
        let path = PathBuf::from("deslop.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
    }
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
    match cli.command {
        Command::Scan(args) => scan(args),
        Command::Metrics(args) => metrics(args),
        #[cfg(feature = "mcp")]
        Command::Mcp => deslop_mcp::run_stdio(),
        Command::Fix(args) => fix(args),
        Command::Propose(args) => propose(args),
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

fn scan(args: ScanArgs) -> Result<()> {
    let paths = paths_since(args.paths, args.since)?;
    let config = analyzer_config(args.rust_external, args.julia_external, args.julia_project)?;
    let mut reports = scan_paths_with_config(&paths, config)?;
    if let Some(path) = args.baseline {
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

    if let Some(threshold) = args.fail_on.map(Severity::from) {
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

fn fix(args: FixArgs) -> Result<()> {
    let outcomes = fix_paths(
        &args.paths,
        FixOptions {
            dry_run: args.dry_run,
            backup: !args.no_backup,
        },
    )?;
    for outcome in outcomes {
        let action = if args.dry_run {
            "would apply"
        } else if outcome.changed {
            "applied"
        } else {
            "unchanged"
        };
        println!(
            "{} {} safe edit(s) in {}",
            action,
            outcome.applied,
            outcome.path.display()
        );
    }
    Ok(())
}

fn propose(args: ProposeArgs) -> Result<()> {
    let config = analyzer_config(args.rust_external, args.julia_external, args.julia_project)?;
    let reports = scan_paths_with_config(&args.paths, config)?;
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
    let mut files = Vec::new();
    let mut rule_counts = BTreeMap::new();
    for report in reports {
        let text = fs::read_to_string(&report.path)
            .with_context(|| format!("failed to read {}", report.path.display()))?;
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
        files.push(FileSlopScore {
            path: report.path.to_path_buf(),
            score: density.min(100.0),
            findings: file_rules.values().sum(),
            nloc,
            rule_counts: file_rules,
        });
    }
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
    rust_external: bool,
    julia_external: Option<JuliaExternalArg>,
    julia_project: Option<PathBuf>,
) -> Result<AnalyzerConfig> {
    let config = DeslopConfig::read_default()?;
    Ok(analyzer_config_from_config(
        &config,
        rust_external,
        julia_external,
        julia_project,
    ))
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

    AnalyzerConfig {
        rust_external: rust_external || configured_clippy,
        julia_external: julia_external
            .map(JuliaExternal::from)
            .unwrap_or(configured_julia),
        julia_project: julia_project.or(configured_project),
        ..AnalyzerConfig::default()
    }
}

fn characterize(args: CharacterizeArgs) -> Result<()> {
    let patches = load_patches(&args.patches)?;
    let work_orders = characterization_work_orders_for_patches(
        &patches,
        &VerifyOptions {
            root: PathBuf::from("."),
            check_cmd: args.check_cmd,
            coverage: coverage_config(args.coverage),
            mutation: mutation_config(args.mutation),
            characterization_tests: Vec::new(),
            allow_non_removable: false,
        },
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
        &VerifyOptions {
            root: PathBuf::from("."),
            check_cmd: Some(args.check_cmd),
            coverage: CoverageConfig::Disabled,
            mutation: MutationConfig::Disabled,
            characterization_tests: Vec::new(),
            allow_non_removable: false,
        },
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
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
        &VerifyOptions {
            root: PathBuf::from("."),
            check_cmd: args.check_cmd,
            coverage: coverage_config(args.coverage),
            mutation: mutation_config(args.mutation),
            characterization_tests,
            allow_non_removable: false,
        },
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
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
        &VerifyOptions {
            root: PathBuf::from("."),
            check_cmd: args.check_cmd,
            coverage: coverage_config(args.coverage),
            mutation: mutation_config(args.mutation),
            characterization_tests,
            allow_non_removable: args.allow_non_removable,
        },
        !args.no_backup,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
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

fn mutation_config(enabled: bool) -> MutationConfig {
    if enabled {
        MutationConfig::Auto
    } else {
        MutationConfig::Disabled
    }
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
    fn read(path: &PathBuf) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
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
