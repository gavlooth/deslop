use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use deslop_analyzer::{
    AnalyzerConfig, AnalyzerLangConfig, BoundaryConfig, JuliaExternal, RuleSuppression,
    Suppression, scan_paths, scan_paths_with_config,
};
use deslop_core::{
    AnalysisStatus, FileAnalysis, FileReport, Severity, reports_analysis_status,
    reports_permit_rewrites, revision_guard,
};
use deslop_eval::{append_false_positive_feedback, render_eval_json, render_eval_text, run_eval};
use deslop_fix::{diff_paths, undo_paths, unified_file_diff};
use deslop_graph::{
    GraphConfig, graph_paths, render_dot as render_graph_dot, render_json as render_graph_json,
};
use deslop_metrics::{
    MetricsConfig, metrics_paths, render_json as render_metrics_json,
    render_text as render_metrics_text,
};
use deslop_protocol::{
    propose_work_orders, propose_work_orders_with_exclusions, recipe_work_orders,
};
use deslop_recipes::{TransformationCandidate, detect_rust_recipe_report};
use deslop_report::{render_agent, render_json, render_sarif, render_text};
use deslop_slim::{
    AnthropicClient, DEFAULT_MODEL, EgressDecision, EgressSummary, OpenAiClient, RecordedClient,
    SlimOptions, SlimProgress, SlimProgressOutcome, SlimReport, egress_consent_error,
    egress_prompt_message, egress_summary, env_egress_consent, provider_base_url,
    resolve_egress_consent, run_slim_with_progress,
};
use deslop_verify::{
    CoverageConfig, MutationConfig, RecipeApplyOptions, RecipeApplyStatus, VerifyOptions,
    apply_patches, apply_recipe_work_orders, characterization_work_orders_for_patches,
    load_characterization_tests, load_patches, load_recipe_work_orders, parse_coverage_mode,
    verify_characterization_tests, verify_patches,
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
    Metrics(MetricsArgs),
    Graph(GraphArgs),
    #[cfg(feature = "mcp")]
    Mcp,
    Fix(FixArgs),
    Propose(ProposeArgs),
    Eval(EvalArgs),
    Feedback(FeedbackArgs),
    Slop(SlopArgs),
    Characterize(CharacterizeArgs),
    VerifyCharacterization(VerifyCharacterizationArgs),
    Verify(PatchArgs),
    Apply(ApplyArgs),
    Baseline(BaselineArgs),
    Undo(PathArgs),
    Rules,
    Recipes(RecipesArgs),
}

#[derive(Debug, Args)]
struct RecipesArgs {
    #[command(subcommand)]
    command: RecipesCommand,
}

#[derive(Debug, Subcommand)]
enum RecipesCommand {
    Detect(RecipeDetectArgs),
    Apply(RecipeApplyArgs),
}

#[derive(Debug, Args)]
struct RecipeDetectArgs {
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    #[arg(long, default_value = "rust-remove-unreachable-literal-statement")]
    recipe: String,

    #[arg(long, value_enum, default_value_t = RecipeDetectFormat::Candidates)]
    format: RecipeDetectFormat,

    #[arg(long, default_value = ".")]
    root: PathBuf,

    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RecipeDetectFormat {
    Candidates,
    Workorders,
    Diff,
    Report,
}

#[derive(Debug, Args)]
struct RecipeApplyArgs {
    #[arg(long)]
    workorders: PathBuf,

    #[arg(long, default_value = ".")]
    root: PathBuf,

    #[arg(long)]
    build_cmd: String,

    #[arg(long)]
    test_cmd: String,

    #[arg(long)]
    no_backup: bool,

    /// Permit a controlled canary write. Automatic production application remains disabled.
    #[arg(long)]
    canary: bool,
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

    #[arg(long, alias = "since", num_args = 0..=1, default_missing_value = "HEAD")]
    changed: Option<String>,

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

    #[arg(long, alias = "consent")]
    yes: bool,

    #[arg(long)]
    check_cmd: Option<String>,

    #[arg(long)]
    no_backup: bool,

    #[arg(long)]
    quiet: bool,

    #[arg(long)]
    diff: bool,
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
struct GraphArgs {
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    #[arg(long, value_enum, default_value_t = GraphFormat::Json)]
    format: GraphFormat,

    #[arg(long)]
    no_calls: bool,
}

#[derive(Debug, Args)]
struct EvalArgs {
    #[arg(default_value = "tests/corpus")]
    corpus: PathBuf,

    #[arg(long, value_enum, default_value_t = MetricsFormat::Text)]
    format: MetricsFormat,
}

#[derive(Debug, Args)]
struct FeedbackArgs {
    fingerprint: String,

    #[arg(long)]
    false_positive: bool,

    #[arg(long, default_value = "tests/corpus")]
    corpus: PathBuf,

    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,
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
    mutation_jobs: Option<usize>,

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
    mutation_jobs: Option<usize>,

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

    #[arg(long)]
    mutation_jobs: Option<usize>,
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
    Update {
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
enum GraphFormat {
    Json,
    Dot,
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

impl SlimProvider {
    fn as_str(self) -> &'static str {
        match self {
            SlimProvider::Anthropic => "anthropic",
            SlimProvider::Openai => "openai",
        }
    }
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
    #[serde(default)]
    egress_consent: Option<bool>,
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
#[serde(deny_unknown_fields)]
struct AnalyzerConfigSection {
    #[serde(default)]
    min_duplication_tokens: Option<usize>,
    #[serde(default)]
    long_method_nloc: Option<usize>,
    #[serde(default)]
    min_meaningful_tokens: Option<usize>,
    /// Rules to disable entirely. Validated against known rule names.
    #[serde(default)]
    disabled_rules: Option<Vec<String>>,
    /// Path globs skipped for every rule.
    #[serde(default)]
    ignore_paths: Option<Vec<String>>,
    /// Per-rule controls, keyed by rule name.
    #[serde(default)]
    rules: Option<BTreeMap<String, RuleConfigSection>>,
    #[serde(default)]
    rust: Option<AnalyzerLangConfigSection>,
    #[serde(default)]
    clojure: Option<AnalyzerLangConfigSection>,
    #[serde(default)]
    julia: Option<AnalyzerLangConfigSection>,
    #[serde(default)]
    python: Option<AnalyzerLangConfigSection>,
    #[serde(default)]
    javascript: Option<AnalyzerLangConfigSection>,
    #[serde(default)]
    typescript: Option<AnalyzerLangConfigSection>,
    #[serde(default)]
    generic: Option<AnalyzerLangConfigSection>,
    /// `[analyzer.boundary]` — config-boundary (dishonest-wiring) analysis controls.
    #[serde(default)]
    boundary: Option<BoundaryConfigSection>,
}

/// `[analyzer.boundary]` keys. Every field maps 1:1 onto [`BoundaryConfig`]; unknown keys
/// are rejected by `deny_unknown_fields` so misspellings fail loudly instead of silently
/// configuring nothing (the exact pathology this analyzer exists to catch).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundaryConfigSection {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    min_key_length: Option<usize>,
    /// Extra echo-sink callee fragments, merged with the built-ins.
    #[serde(default)]
    extra_sinks: Option<Vec<String>>,
    /// Key names exempt from all boundary rules.
    #[serde(default)]
    ignore_keys: Option<Vec<String>>,
    /// Replaces (not extends) the built-in tool-config skip list when set.
    #[serde(default)]
    skip_artifacts: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzerLangConfigSection {
    #[serde(default)]
    long_method_nloc: Option<usize>,
}

/// `[analyzer.rules.<rule>]` table: scope a single rule.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleConfigSection {
    /// Set false to disable the rule (same as listing it in `disabled_rules`).
    #[serde(default)]
    enabled: Option<bool>,
    /// Path globs skipped for this rule only.
    #[serde(default)]
    ignore_paths: Option<Vec<String>>,
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
        Command::Graph(args) => graph(args),
        #[cfg(feature = "mcp")]
        Command::Mcp => deslop_mcp::run_stdio(),
        Command::Fix(args) => fix(args, &config),
        Command::Propose(args) => propose(args, &config),
        Command::Eval(args) => eval(args),
        Command::Feedback(args) => feedback(args, &config),
        Command::Slop(args) => slop(args),
        Command::Characterize(args) => characterize(args),
        Command::VerifyCharacterization(args) => verify_characterization(args),
        Command::Verify(args) => verify(args),
        Command::Apply(args) => apply(args),
        Command::Baseline(args) => baseline(args),
        Command::Undo(args) => undo(args),
        Command::Rules => rules(),
        Command::Recipes(args) => recipes(args),
    }
}

fn recipes(args: RecipesArgs) -> Result<()> {
    match args.command {
        RecipesCommand::Detect(args) => detect_recipes(args),
        RecipesCommand::Apply(args) => apply_recipes(args),
    }
}

fn apply_recipes(args: RecipeApplyArgs) -> Result<()> {
    let orders = load_recipe_work_orders(&args.workorders)?;
    let report = apply_recipe_work_orders(
        &orders,
        &RecipeApplyOptions {
            root: args.root,
            build_command: args.build_cmd,
            test_command: args.test_cmd,
            backup: !args.no_backup,
            explicit_canary: args.canary,
        },
    )?;
    print_pretty_json(&report)?;
    if report.status != RecipeApplyStatus::Applied {
        std::process::exit(2);
    }
    Ok(())
}

fn detect_recipes(args: RecipeDetectArgs) -> Result<()> {
    if !matches!(
        args.recipe.as_str(),
        "rust-remove-unreachable-literal-statement"
            | "rust-factor-equivalent-branch-fragments"
            | "rust-merge-adjacent-conditions"
            | "rust-split-independent-branch-actions"
            | "rust-invert-guard-clause"
            | "rust-remove-literal-dead-arm"
            | "rust-convert-exhaustive-chain-to-match"
            | "rust-extract-sese-branch-method"
            | "rust-split-dependence-cohesive-callable"
            | "rust-inline-exact-single-use-helper"
    ) {
        bail!("unknown production recipe `{}`", args.recipe);
    }
    let root = args
        .root
        .canonicalize()
        .with_context(|| format!("failed to resolve recipe root {}", args.root.display()))?;
    let mut report = detect_rust_recipe_report(&root, &args.paths)?;
    report
        .candidates
        .retain(|candidate| candidate.recipe().name() == args.recipe);
    let candidates = report.candidates.clone();
    let work_orders = recipe_work_orders(candidates.clone())?;
    let rendered = match args.format {
        RecipeDetectFormat::Candidates => serde_json::to_string_pretty(&candidates)?,
        RecipeDetectFormat::Workorders => serde_json::to_string_pretty(&work_orders)?,
        RecipeDetectFormat::Diff => render_recipe_diff(&root, &candidates)?,
        RecipeDetectFormat::Report => serde_json::to_string_pretty(&report)?,
    };
    if let Some(output) = args.output {
        fs::write(&output, format!("{rendered}\n"))
            .with_context(|| format!("failed to write {}", output.display()))?;
    } else {
        println!("{rendered}");
    }
    for abstention in &report.abstentions {
        eprintln!(
            "recipe abstained for {} at {}: {}",
            abstention.path.display(),
            abstention.stage,
            abstention.reason
        );
    }
    Ok(())
}

fn render_recipe_diff(root: &Path, candidates: &[TransformationCandidate]) -> Result<String> {
    let mut edits_by_path = BTreeMap::<PathBuf, Vec<_>>::new();
    for candidate in candidates {
        for edit in candidate.edits() {
            if edit.target.file().path != candidate.target().node.file().path {
                bail!(
                    "candidate `{}` contains a foreign-source edit",
                    candidate.id()
                );
            }
            edits_by_path
                .entry(edit.target.file().path.clone())
                .or_default()
                .push(edit);
        }
    }

    let mut rendered = String::new();
    for (logical, mut edits) in edits_by_path {
        let physical = root.join(&logical);
        let original = fs::read_to_string(&physical)
            .with_context(|| format!("failed to read {}", physical.display()))?;
        edits.sort_by_key(|edit| (edit.span.start_byte, edit.span.end_byte));
        if edits
            .windows(2)
            .any(|pair| pair[0].span.end_byte > pair[1].span.start_byte)
        {
            bail!(
                "recipe candidates contain overlapping edits for {}",
                logical.display()
            );
        }
        for edit in &edits {
            let before = original
                .get(edit.span.start_byte..edit.span.end_byte)
                .with_context(|| format!("candidate edit is outside {}", logical.display()))?;
            if before != edit.before
                || revision_guard(&logical, edit.span, before) != edit.revision_guard
            {
                bail!(
                    "candidate edit for {} is stale before preview",
                    logical.display()
                );
            }
        }
        let mut preview = original.clone();
        for edit in edits.into_iter().rev() {
            preview.replace_range(edit.span.start_byte..edit.span.end_byte, &edit.after);
        }
        rendered.push_str(&unified_file_diff(&logical, &original, &preview));
    }
    Ok(rendered)
}

fn metrics(args: MetricsArgs) -> Result<()> {
    let report = metrics_paths(&args.paths, MetricsConfig { sigma: args.sigma })?;
    let rendered = match args.format {
        MetricsFormat::Text => render_metrics_text(&report, args.hotspots_only),
        MetricsFormat::Json => render_metrics_json(&report)?,
    };
    print!("{rendered}");
    if report.status != AnalysisStatus::Complete {
        std::process::exit(2);
    }
    Ok(())
}

fn graph(args: GraphArgs) -> Result<()> {
    let report = graph_paths(
        &args.paths,
        GraphConfig {
            include_calls: !args.no_calls,
        },
    )?;
    let rendered = match args.format {
        GraphFormat::Json => render_graph_json(&report)?,
        GraphFormat::Dot => render_graph_dot(&report),
    };
    print!("{rendered}");
    if report.status != AnalysisStatus::Complete {
        std::process::exit(2);
    }
    Ok(())
}

fn scan(args: ScanArgs, config: &DeslopConfig) -> Result<()> {
    let paths = paths_since(args.paths, args.changed)?;
    let analyzer = analyzer_config(
        config,
        args.rust_external,
        args.julia_external,
        args.julia_project,
    )?;
    let baseline = resolve_scan_baseline(args.baseline, config)
        .map(|path| Baseline::read(&path))
        .transpose()?;
    let (mut reports, agent_work_orders) = if matches!(args.format, Format::Agent) {
        let excluded = baseline
            .as_ref()
            .map(|baseline| baseline.fingerprints.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let batch = propose_work_orders_with_exclusions(
            &proposal_root_for_paths(&paths)?,
            &paths,
            analyzer,
            &excluded,
        )?;
        (batch.reports, Some(batch.work_orders))
    } else {
        (scan_paths_with_config(&paths, analyzer)?, None)
    };
    if let Some(baseline) = baseline {
        suppress_baseline(&mut reports, &baseline);
    }

    let complete = reports_permit_rewrites(&reports);
    if !complete && matches!(args.format, Format::Agent) {
        print_analysis_diagnostics(&reports);
        std::process::exit(2);
    }

    let rendered = match args.format {
        Format::Text => render_text(&reports),
        Format::Json => render_json(&reports)?,
        Format::Sarif => render_sarif(&reports)?,
        Format::Agent => render_agent(agent_work_orders.as_deref().unwrap_or_default())?,
    };
    print!("{rendered}");

    if !complete {
        std::process::exit(2);
    }

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
    if args.diff {
        print!("{}", diff_paths(&args.paths)?);
        return Ok(());
    }
    let request = resolve_fix_request(args, config)?;
    let mut progress = slim_progress_sink(!request.quiet && io::stderr().is_terminal());
    let report = run_fix_request(request, &mut progress)?;
    print_pretty_json(&report)?;
    Ok(())
}

struct FixRequest {
    options: SlimOptions,
    provider: SlimProvider,
    base_url: Option<String>,
    mock: Option<PathBuf>,
    explicit_consent: bool,
    quiet: bool,
}

fn resolve_fix_request(args: FixArgs, config: &DeslopConfig) -> Result<FixRequest> {
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
    let explicit_consent =
        resolve_slim_egress_consent(args.yes, std::env::var("DESLOP_SLIM_CONSENT").ok(), config);
    let analyzer = analyzer_config(config, false, None, None)?;
    Ok(FixRequest {
        options: SlimOptions {
            root: PathBuf::from("."),
            paths,
            workorders: args.workorders,
            apply: args.apply,
            characterize: args.characterize,
            allow_unverified,
            coverage,
            model,
            check_cmd,
            backup: !args.no_backup,
            analyzer,
        },
        provider,
        base_url,
        mock: args.mock,
        explicit_consent,
        quiet: args.quiet,
    })
}

fn run_fix_request(
    request: FixRequest,
    progress: &mut dyn FnMut(SlimProgress),
) -> Result<SlimReport> {
    if let Some(path) = request.mock {
        let client = RecordedClient::from_path(path)?;
        return run_slim_with_progress(&client, request.options, progress);
    }
    run_real_provider_fix(request, progress)
}

fn run_real_provider_fix(
    request: FixRequest,
    progress: &mut dyn FnMut(SlimProgress),
) -> Result<SlimReport> {
    let summary = egress_summary(&request.options)?;
    if summary.region_count == 0 {
        return run_slim_with_progress(&RecordedClient::new(""), request.options, progress);
    }
    let provider_name = request.provider.as_str();
    let destination = provider_base_url(provider_name, request.base_url.as_deref());
    require_cli_egress_consent(
        provider_name,
        &destination,
        summary,
        request.explicit_consent,
    )?;
    match request.provider {
        SlimProvider::Anthropic => {
            let client = AnthropicClient::from_env(request.options.model.clone())?;
            run_slim_with_progress(&client, request.options, progress)
        }
        SlimProvider::Openai => {
            let client = OpenAiClient::from_env(request.options.model.clone(), request.base_url)?;
            run_slim_with_progress(&client, request.options, progress)
        }
    }
}

fn require_cli_egress_consent(
    provider: &str,
    base_url: &str,
    summary: EgressSummary,
    explicit_consent: bool,
) -> Result<()> {
    match resolve_egress_consent(explicit_consent, io::stdin().is_terminal()) {
        EgressDecision::Granted => Ok(()),
        EgressDecision::DeniedNonInteractive => {
            bail!("{}", egress_consent_error(provider, base_url, summary))
        }
        EgressDecision::Prompt => {
            let message = egress_prompt_message(provider, base_url, summary);
            eprint!("{message} ");
            io::stderr().flush()?;
            let mut answer = String::new();
            io::stdin()
                .read_line(&mut answer)
                .context("failed to read source-egress consent response")?;
            if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                Ok(())
            } else {
                bail!("source-egress consent declined; no LLM request was sent")
            }
        }
    }
}

fn slim_progress_sink(enabled: bool) -> impl FnMut(SlimProgress) {
    move |event| {
        if enabled {
            let mut stderr = io::stderr();
            let _ = write_slim_progress(&event, &mut stderr);
        }
    }
}

fn write_slim_progress(event: &SlimProgress, writer: &mut impl Write) -> Result<()> {
    writeln!(writer, "{}", slim_progress_line(event))?;
    Ok(())
}

fn slim_progress_line(event: &SlimProgress) -> String {
    match event {
        SlimProgress::Started { work_orders } => started_progress_line(*work_orders),
        SlimProgress::Rewriting {
            index,
            total,
            path,
            start_line,
            end_line,
            ..
        } => rewrite_progress_line(*index, *total, path, *start_line, *end_line),
        SlimProgress::Characterizing { workorder_id } => characterizing_progress_line(workorder_id),
        SlimProgress::Verified {
            workorder_id,
            verdict,
        } => verified_progress_line(workorder_id, verdict),
        SlimProgress::Outcome {
            workorder_id,
            outcome,
        } => outcome_progress_line(workorder_id, *outcome),
        SlimProgress::Finished {
            applied,
            held,
            rejected,
        } => finished_progress_line(*applied, *held, *rejected),
    }
}

fn started_progress_line(work_orders: usize) -> String {
    format!("deslop fix: {work_orders} rewrite region(s)")
}

fn rewrite_progress_line(
    index: usize,
    total: usize,
    path: &Path,
    start_line: usize,
    end_line: usize,
) -> String {
    format!(
        "[{index}/{total}] rewriting {}:{start_line}-{end_line}",
        path.display()
    )
}

fn characterizing_progress_line(workorder_id: &str) -> String {
    format!("characterizing {workorder_id}")
}

fn verified_progress_line(
    workorder_id: &str,
    verdict: &deslop_verify::VerificationVerdict,
) -> String {
    format!("verified {workorder_id}: {verdict:?}")
}

fn outcome_progress_line(workorder_id: &str, outcome: SlimProgressOutcome) -> String {
    let outcome = match outcome {
        SlimProgressOutcome::Applied => "applied",
        SlimProgressOutcome::Held => "held",
        SlimProgressOutcome::Rejected => "rejected",
    };
    format!("outcome {workorder_id}: {outcome}")
}

fn finished_progress_line(applied: usize, held: usize, rejected: usize) -> String {
    format!("finished: applied={applied} held={held} rejected={rejected}")
}

fn propose(args: ProposeArgs, config: &DeslopConfig) -> Result<()> {
    let analyzer = analyzer_config(
        config,
        args.rust_external,
        args.julia_external,
        args.julia_project,
    )?;
    let batch = propose_work_orders(
        &proposal_root_for_paths(&args.paths)?,
        &args.paths,
        analyzer,
    )?;
    if !reports_permit_rewrites(&batch.reports) {
        print_analysis_diagnostics(&batch.reports);
        std::process::exit(2);
    }
    let rendered = render_agent(&batch.work_orders)?;
    if let Some(output) = args.output {
        fs::write(&output, rendered)
            .with_context(|| format!("failed to write {}", output.display()))?;
    } else {
        print!("{rendered}");
    }
    Ok(())
}

fn print_analysis_diagnostics(reports: &[FileReport]) {
    for report in reports {
        for diagnostic in &report.analysis.diagnostics {
            let location = diagnostic.span.map_or_else(
                || report.path.display().to_string(),
                |span| format!("{}:{}", report.path.display(), span.start_line),
            );
            eprintln!("{location} [{}] {}", diagnostic.code, diagnostic.message);
        }
    }
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

fn feedback(args: FeedbackArgs, config: &DeslopConfig) -> Result<()> {
    if !args.false_positive {
        bail!("feedback currently requires --false-positive");
    }
    let analyzer = analyzer_config(config, false, None, None)?;
    let reports = scan_paths_with_config(&args.paths, analyzer)?;
    for report in &reports {
        if let Some(finding) = report
            .findings
            .iter()
            .find(|finding| finding.fingerprint == args.fingerprint)
        {
            let case_path = append_false_positive_feedback(&args.corpus, report, finding)?;
            println!(
                "appended false-positive corpus case {} for {}",
                case_path.display(),
                finding.rule
            );
            return Ok(());
        }
    }
    bail!(
        "no finding with fingerprint {} in scanned paths",
        args.fingerprint
    )
}

#[derive(Debug, Serialize)]
struct SlopReport {
    schema: &'static str,
    status: AnalysisStatus,
    score: Option<f64>,
    blocked_files: Vec<FileAnalysis>,
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
    if report.status != AnalysisStatus::Complete {
        std::process::exit(2);
    }
    Ok(())
}

fn slop_report(reports: &[FileReport]) -> Result<SlopReport> {
    let status = reports_analysis_status(reports);
    let blocked_files = reports
        .iter()
        .filter(|report| !report.analysis.permits_rewrites())
        .map(|report| FileAnalysis {
            path: report.path.clone(),
            lang: report.lang,
            analysis: report.analysis.clone(),
        })
        .collect::<Vec<_>>();
    let mut rule_counts = BTreeMap::new();
    let mut files = reports
        .iter()
        .filter(|report| report.analysis.permits_rewrites())
        .map(|report| slop_score_for_file(report, &mut rule_counts))
        .collect::<Result<Vec<_>>>()?;
    files.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.path.cmp(&b.path))
    });
    let score = if status != AnalysisStatus::Complete || files.is_empty() {
        None
    } else {
        Some(files.iter().map(|file| file.score).sum::<f64>() / files.len() as f64)
    };
    Ok(SlopReport {
        schema: "deslop.slop/2",
        status,
        score,
        blocked_files,
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
    match report.score {
        Some(score) => out.push_str(&format!("Slop score: {score:.1}/100\n")),
        None => out.push_str("Slop score: unavailable (analysis incomplete)\n"),
    }
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
) -> Result<AnalyzerConfig> {
    analyzer_config_from_config(config, rust_external, julia_external, julia_project)
}

fn analyzer_config_from_config(
    config: &DeslopConfig,
    rust_external: bool,
    julia_external: Option<JuliaExternalArg>,
    julia_project: Option<PathBuf>,
) -> Result<AnalyzerConfig> {
    let external = config.external.as_ref();
    let configured_julia = external
        .and_then(|external| external.julia_analyzer)
        .map(JuliaExternal::from)
        .unwrap_or(JuliaExternal::Off);
    let configured_project = external.and_then(|external| external.julia_project.to_owned());
    let configured_clippy = external
        .and_then(|external| external.clippy)
        .is_some_and(|value| value == ClippyConfig::On);
    let thresholds = analyzer_thresholds(config);

    Ok(AnalyzerConfig {
        min_duplication_tokens: thresholds.min_duplication_tokens,
        long_method_nloc: thresholds.long_method_nloc,
        min_meaningful_tokens: thresholds.min_meaningful_tokens,
        rust: thresholds.rust,
        clojure: thresholds.clojure,
        julia: thresholds.julia,
        python: thresholds.python,
        javascript: thresholds.javascript,
        typescript: thresholds.typescript,
        generic: thresholds.generic,
        rust_external: rust_external || configured_clippy,
        julia_external: julia_external
            .map(JuliaExternal::from)
            .unwrap_or(configured_julia),
        julia_project: julia_project.or(configured_project),
        suppression: build_suppression(config.analyzer.as_ref())?,
        boundary: build_boundary_config(config.analyzer.as_ref()),
    })
}

/// Merge `[analyzer.boundary]` over [`BoundaryConfig`] defaults.
fn build_boundary_config(section: Option<&AnalyzerConfigSection>) -> BoundaryConfig {
    let default = BoundaryConfig::default();
    let Some(section) = section.and_then(|analyzer| analyzer.boundary.as_ref()) else {
        return default;
    };
    let mut merged = default;
    if let Some(enabled) = section.enabled {
        merged.enabled = enabled;
    }
    if let Some(min_key_length) = section.min_key_length {
        merged.min_key_length = min_key_length;
    }
    if let Some(extra_sinks) = &section.extra_sinks {
        merged.extra_sinks.extend(extra_sinks.iter().cloned());
    }
    if let Some(ignore_keys) = &section.ignore_keys {
        merged.ignore_keys.extend(ignore_keys.iter().cloned());
    }
    if let Some(skip_artifacts) = &section.skip_artifacts {
        merged.skip_artifacts = skip_artifacts.clone();
    }
    merged
}

/// Compile `[analyzer]` suppression keys into a [`Suppression`]. Unknown rule names and
/// invalid globs are reported as errors rather than silently ignored.
fn build_suppression(section: Option<&AnalyzerConfigSection>) -> Result<Suppression> {
    let Some(section) = section else {
        return Ok(Suppression::default());
    };
    let mut builder = Suppression::builder();
    builder.add_section(
        section.disabled_rules.as_deref().unwrap_or_default(),
        section.ignore_paths.as_deref().unwrap_or_default(),
        section.rules.iter().flatten().map(|(rule, rule_config)| {
            (
                rule.as_str(),
                RuleSuppression {
                    enabled: rule_config.enabled,
                    ignore_paths: rule_config.ignore_paths.as_deref().unwrap_or_default(),
                },
            )
        }),
    );
    builder.build()
}

fn analyzer_thresholds(config: &DeslopConfig) -> AnalyzerConfig {
    let default = AnalyzerConfig::default();
    let configured = config.analyzer.as_ref();
    AnalyzerConfig {
        min_duplication_tokens: configured
            .and_then(|analyzer| analyzer.min_duplication_tokens)
            .unwrap_or(default.min_duplication_tokens),
        long_method_nloc: configured
            .and_then(|analyzer| analyzer.long_method_nloc)
            .unwrap_or(default.long_method_nloc),
        min_meaningful_tokens: configured
            .and_then(|analyzer| analyzer.min_meaningful_tokens)
            .unwrap_or(default.min_meaningful_tokens),
        rust: lang_threshold(configured.and_then(|analyzer| analyzer.rust.as_ref())),
        clojure: lang_threshold(configured.and_then(|analyzer| analyzer.clojure.as_ref())),
        julia: lang_threshold(configured.and_then(|analyzer| analyzer.julia.as_ref())),
        python: lang_threshold(configured.and_then(|analyzer| analyzer.python.as_ref())),
        javascript: lang_threshold(configured.and_then(|analyzer| analyzer.javascript.as_ref())),
        typescript: lang_threshold(configured.and_then(|analyzer| analyzer.typescript.as_ref())),
        generic: lang_threshold(configured.and_then(|analyzer| analyzer.generic.as_ref())),
        ..default
    }
}

fn lang_threshold(configured: Option<&AnalyzerLangConfigSection>) -> AnalyzerLangConfig {
    AnalyzerLangConfig {
        long_method_nloc: configured.and_then(|lang| lang.long_method_nloc),
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

fn resolve_slim_egress_consent(
    cli_yes: bool,
    env_consent: Option<String>,
    config: &DeslopConfig,
) -> bool {
    cli_yes
        || env_egress_consent(env_consent)
        || config
            .slim
            .as_ref()
            .and_then(|slim| slim.egress_consent)
            .unwrap_or(false)
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
            args.mutation_jobs,
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
        &verify_options(Some(args.check_cmd), false, false, None, Vec::new(), false),
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
            args.mutation_jobs,
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
            args.mutation_jobs,
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

fn mutation_config(enabled: bool, jobs: Option<usize>) -> MutationConfig {
    if enabled {
        match jobs {
            Some(jobs) => MutationConfig::AutoWithOptions {
                timeout: std::time::Duration::from_secs(10),
                jobs,
            },
            None => MutationConfig::Auto,
        }
    } else {
        MutationConfig::Disabled
    }
}

fn verify_options(
    check_cmd: Option<String>,
    coverage: bool,
    mutation: bool,
    mutation_jobs: Option<usize>,
    characterization_tests: Vec<deslop_protocol::CharacterizationTest>,
    allow_non_removable: bool,
) -> VerifyOptions {
    VerifyOptions {
        root: PathBuf::from("."),
        scope: None,
        check_cmd,
        coverage: coverage_config(coverage),
        mutation: mutation_config(mutation, mutation_jobs),
        characterization_tests,
        allow_non_removable,
    }
}

fn print_pretty_json<T: Serialize>(value: &T) -> Result<()> {
    let mut stdout = io::stdout();
    write_pretty_json(value, &mut stdout)
}

fn write_pretty_json<T: Serialize>(value: &T, writer: &mut impl Write) -> Result<()> {
    writeln!(writer, "{}", serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn baseline(args: BaselineArgs) -> Result<()> {
    match args.command {
        BaselineCommand::Write { paths, output } => {
            write_baseline(&paths, &output, "wrote")?;
        }
        BaselineCommand::Update { paths, output } => {
            write_baseline(&paths, &output, "updated")?;
        }
    }
    Ok(())
}

fn write_baseline(paths: &[PathBuf], output: &Path, verb: &str) -> Result<()> {
    let reports = scan_paths(paths)?;
    if !reports_permit_rewrites(&reports) {
        bail!("analysis is incomplete; refusing to write or update a baseline");
    }
    let baseline = Baseline::from_reports(&reports);
    let rendered = serde_json::to_string_pretty(&baseline)?;
    fs::write(output, rendered).with_context(|| format!("failed to write {}", output.display()))?;
    println!(
        "{verb} {} fingerprint(s) to {}",
        baseline.fingerprints.len(),
        output.display()
    );
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
    io::stdout().write_all(deslop_core::rules::render_table().as_bytes())?;
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

fn proposal_root_for_paths(paths: &[PathBuf]) -> Result<PathBuf> {
    let cwd = std::env::current_dir()?.canonicalize()?;
    if paths.is_empty() {
        return Ok(cwd);
    }
    let canonical = paths
        .iter()
        .map(|path| {
            path.canonicalize()
                .with_context(|| format!("failed to resolve proposal path {}", path.display()))
        })
        .collect::<Result<Vec<_>>>()?;
    if canonical.iter().all(|path| path.starts_with(&cwd)) {
        return Ok(cwd);
    }
    let first = &canonical[0];
    let mut root = if first.is_file() {
        first.parent().unwrap_or(Path::new("/")).to_path_buf()
    } else {
        first.clone()
    };
    for path in &canonical[1..] {
        while !path.starts_with(&root) {
            root = root
                .parent()
                .context("proposal paths do not share a filesystem root")?
                .to_path_buf();
        }
    }
    Ok(root)
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
    use deslop_core::Lang;

    use super::*;

    #[test]
    fn parses_graph_command() {
        let cli = Cli::parse_from(["deslop", "graph", "src", "--format", "dot", "--no-calls"]);
        let Command::Graph(args) = cli.command else {
            panic!("expected graph command");
        };
        assert_eq!(args.paths, vec![PathBuf::from("src")]);
        assert!(matches!(args.format, GraphFormat::Dot));
        assert!(args.no_calls);
    }

    #[test]
    fn baseline_identity_survives_outer_whitespace_without_gaining_write_authority() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.clj");
        fs::write(&path, "(= (count xs) 0)\n").expect("original");
        let original = scan_paths(std::slice::from_ref(&path)).expect("original scan");
        let baseline = Baseline::from_reports(&original);
        assert!(!baseline.fingerprints.is_empty());

        fs::write(&path, " (= (count xs) 0)\n").expect("boundary whitespace");
        let mut changed = scan_paths(std::slice::from_ref(&path)).expect("changed scan");
        assert!(!changed[0].findings.is_empty());
        assert!(
            changed[0]
                .findings
                .iter()
                .all(|finding| baseline.fingerprints.contains(&finding.fingerprint))
        );

        suppress_baseline(&mut changed, &baseline);
        assert!(changed[0].findings.is_empty());
    }

    #[test]
    fn health_is_not_a_metrics_command_alias() {
        let error = Cli::try_parse_from(["deslop", "health"]).expect_err("health alias removed");
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }

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
        let analyzer =
            analyzer_config_from_config(&config, false, None, None).expect("build analyzer config");
        assert!(analyzer.rust_external);
        assert_eq!(analyzer.julia_external, JuliaExternal::StaticLint);
        assert_eq!(analyzer.julia_project, Some(PathBuf::from("julia-env")));
    }

    #[test]
    fn parses_all_config_sections() {
        let config = full_config_fixture();

        assert_slim_config(&config);
        assert_fix_config(&config);
        assert_scan_config(&config);
        assert_analyzer_config(&config);
    }

    fn full_config_fixture() -> DeslopConfig {
        toml::from_str(
            r#"
        [slim]
        provider = "openai"
        model = "configured-model"
        base_url = "http://localhost:11434/v1"
        egress_consent = true

        [fix]
        check_cmd = "cargo test -p configured"
        coverage = "lcov:coverage.lcov"
        allow_unverified = true

        [scan]
        fail_on = "major"
        baseline = "deslop-baseline.json"

        [analyzer]
        min_duplication_tokens = 42
        long_method_nloc = 30
        min_meaningful_tokens = 5

        [analyzer.rust]
        long_method_nloc = 55

        [analyzer.clojure]
        long_method_nloc = 35

        [analyzer.python]
        long_method_nloc = 34

        [analyzer.javascript]
        long_method_nloc = 36

        [analyzer.typescript]
        long_method_nloc = 37

        [external]
        clippy = "on"
        julia_analyzer = "jet"
        julia_project = "julia-env"
        "#,
        )
        .expect("parse config")
    }

    fn assert_slim_config(config: &DeslopConfig) {
        assert_eq!(resolve_slim_provider(None, config), SlimProvider::Openai);
        assert_eq!(resolve_slim_model(None, None, config), "configured-model");
        assert_eq!(
            resolve_slim_base_url(None, config).as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert!(resolve_slim_egress_consent(false, None, config));
    }

    fn assert_fix_config(config: &DeslopConfig) {
        assert_eq!(
            resolve_fix_check_cmd(None, config).as_deref(),
            Some("cargo test -p configured")
        );
        assert!(resolve_fix_allow_unverified(None, config));
        assert!(matches!(
            resolve_fix_coverage(None, config).expect("parse coverage"),
            CoverageConfig::LcovFile(path) if path == Path::new("coverage.lcov")
        ));
    }

    fn assert_scan_config(config: &DeslopConfig) {
        assert_eq!(
            resolve_scan_baseline(None, config),
            Some(PathBuf::from("deslop-baseline.json"))
        );
        assert_eq!(resolve_scan_fail_on(None, config), Some(Severity::Major));
    }

    fn assert_analyzer_config(config: &DeslopConfig) {
        let analyzer =
            analyzer_config_from_config(config, false, None, None).expect("build analyzer config");
        assert_eq!(analyzer.min_duplication_tokens, 42);
        assert_eq!(analyzer.long_method_nloc, 30);
        assert_eq!(analyzer.min_meaningful_tokens, 5);
        assert_eq!(analyzer.long_method_nloc_for(Lang::Rust), 55);
        assert_eq!(analyzer.long_method_nloc_for(Lang::Clojure), 35);
        assert_eq!(analyzer.long_method_nloc_for(Lang::Julia), 30);
        assert_eq!(analyzer.long_method_nloc_for(Lang::Python), 34);
        assert_eq!(analyzer.long_method_nloc_for(Lang::JavaScript), 36);
        assert_eq!(analyzer.long_method_nloc_for(Lang::TypeScript), 37);
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
        )
        .expect("build analyzer config");
        assert_eq!(analyzer.julia_external, JuliaExternal::Off);
        assert_eq!(analyzer.julia_project, Some(PathBuf::from("cli-project")));
    }

    #[test]
    fn analyzer_suppression_config_parses_and_validates() {
        let config: DeslopConfig = toml::from_str(
            r#"
            [analyzer]
            disabled_rules = ["magic-number"]
            ignore_paths = ["**/generated/**"]

            [analyzer.rules.long-method]
            enabled = false

            [analyzer.rules.duplicate-block]
            ignore_paths = ["tests/**"]
            "#,
        )
        .expect("parse config");
        let analyzer =
            analyzer_config_from_config(&config, false, None, None).expect("build analyzer config");
        assert!(!analyzer.suppression.is_empty());
    }

    #[test]
    fn analyzer_suppression_rejects_unknown_rule() {
        let config: DeslopConfig =
            toml::from_str("[analyzer]\ndisabled_rules = [\"ignore-comments\"]\n")
                .expect("parse config");
        let err = analyzer_config_from_config(&config, false, None, None)
            .expect_err("unknown rule must error");
        assert!(err.to_string().contains("unknown rule 'ignore-comments'"));
    }

    #[test]
    fn unknown_analyzer_keys_are_rejected_not_silently_ignored() {
        // The exact keys from the bug report used to be silently ignored; now they error.
        let err = toml::from_str::<DeslopConfig>("[analyzer]\nignore_comments = true\n")
            .expect_err("unknown analyzer key must error");
        assert!(err.to_string().contains("ignore_comments"));
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
    fn slim_egress_consent_sources_grant_independently() {
        let config: DeslopConfig = toml::from_str(
            r#"
            [slim]
            egress_consent = true
            "#,
        )
        .expect("parse config");
        assert!(resolve_slim_egress_consent(
            true,
            None,
            &DeslopConfig::default()
        ));
        assert!(resolve_slim_egress_consent(
            false,
            Some("1".to_string()),
            &DeslopConfig::default()
        ));
        assert!(resolve_slim_egress_consent(false, None, &config));
        assert!(!resolve_slim_egress_consent(
            false,
            Some("0".to_string()),
            &DeslopConfig::default()
        ));
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
            CoverageConfig::CoveragePyFile(path) if path == Path::new("coverage.json")
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
            "--yes",
            "--check-cmd",
            "--diff",
            "--quiet",
        ] {
            assert!(help.contains(flag), "{flag} missing from help:\n{help}");
        }
    }

    #[test]
    fn slim_progress_never_changes_stdout_report_rendering() {
        let report = serde_json::json!({
            "schema": "deslop.slim/4",
            "dry_run": true,
            "verified": { "results": [] }
        });
        let mut stdout_with_progress = Vec::new();
        let mut stdout_quiet = Vec::new();
        let mut stderr = Vec::new();

        write_slim_progress(&SlimProgress::Started { work_orders: 2 }, &mut stderr)
            .expect("write progress");
        write_pretty_json(&report, &mut stdout_with_progress).expect("stdout with progress");
        write_pretty_json(&report, &mut stdout_quiet).expect("stdout quiet");

        assert_eq!(stdout_with_progress, stdout_quiet);
        assert!(
            String::from_utf8(stderr)
                .unwrap()
                .contains("rewrite region")
        );
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
            CoverageConfig::LcovFile(path) if path == Path::new("coverage.lcov")
        ));
        assert!(matches!(
            parse_coverage_config("cloverage:coverage.json").expect("parse"),
            CoverageConfig::CloverageFile(path) if path == Path::new("coverage.json")
        ));
        assert!(matches!(
            parse_coverage_config("julia-cov:coverage.cov").expect("parse"),
            CoverageConfig::JuliaCovFile(path) if path == Path::new("coverage.cov")
        ));
        assert!(matches!(
            parse_coverage_config("coverage-py:coverage.json").expect("parse"),
            CoverageConfig::CoveragePyFile(path) if path == Path::new("coverage.json")
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
    fn parses_mutation_jobs_override() {
        let cli = Cli::try_parse_from([
            "deslop",
            "verify",
            "--patches",
            "patches.jsonl",
            "--mutation",
            "--mutation-jobs",
            "2",
        ])
        .expect("parse cli");
        let Command::Verify(args) = cli.command else {
            panic!("expected verify command");
        };
        assert_eq!(args.mutation_jobs, Some(2));
        assert!(matches!(
            mutation_config(args.mutation, args.mutation_jobs),
            MutationConfig::AutoWithOptions { jobs: 2, .. }
        ));
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

    #[test]
    fn parses_feedback_false_positive_command() {
        let cli = Cli::try_parse_from([
            "deslop",
            "feedback",
            "abc123",
            "--false-positive",
            "--corpus",
            "tests/corpus",
            "src",
        ])
        .expect("parse cli");
        let Command::Feedback(args) = cli.command else {
            panic!("expected feedback command");
        };
        assert_eq!(args.fingerprint, "abc123");
        assert!(args.false_positive);
        assert_eq!(args.corpus, PathBuf::from("tests/corpus"));
        assert_eq!(args.paths, vec![PathBuf::from("src")]);
    }
}
