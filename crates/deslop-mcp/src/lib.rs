use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use deslop_analyzer::{
    AnalyzerConfig, AnalyzerLangConfig, RuleSuppression, Suppression, SuppressionBuilder,
    scan_paths_with_config,
};
use deslop_graph::{GraphConfig, graph_paths};
use deslop_metrics::{MetricsConfig, metrics_paths};
use deslop_parse::SourceFile;
use deslop_protocol::{
    CharacterizationTest, Patch, WorkOrder, WorkOrderKind, work_orders_for_source,
    workorder_region_fingerprint,
};
use deslop_report::{render_agent, render_json};
use deslop_slim::build_prompt;
#[cfg(feature = "slim-llm")]
use deslop_slim::{
    AnthropicClient, EgressDecision, OpenAiClient, RecordedClient, SlimOptions,
    egress_consent_error, egress_summary, env_egress_consent, provider_base_url,
    resolve_egress_consent, resolve_model, run_slim,
};
use deslop_verify::{
    CoverageConfig, MutationConfig, VerifyOptions, apply_patches,
    characterization_work_orders_for_patches, parse_coverage_mode, verify_characterization_tests,
    verify_patches,
};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn run_stdio() -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    run(stdin.lock(), &mut stdout)
}

pub fn run<R: BufRead, W: Write>(reader: R, writer: &mut W) -> Result<()> {
    for line in reader.lines() {
        let line = line.context("failed to read MCP request")?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_json(&line)? {
            writeln!(writer, "{}", serde_json::to_string(&response)?)?;
            writer.flush()?;
        }
    }
    Ok(())
}

pub fn handle_json(request: &str) -> Result<Option<Value>> {
    let request: Value = serde_json::from_str(request).context("failed to parse JSON-RPC")?;
    handle_request(&request).map(Some)
}

pub fn handle_request(request: &Value) -> Result<Value> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let result = match method {
        "initialize" => initialize_result(),
        "tools/list" => tools_list_result(),
        "tools/call" => tools_call_result(request.get("params").unwrap_or(&Value::Null))?,
        other => {
            return Ok(error_response(
                id,
                -32601,
                &format!("method not found: {other}"),
            ));
        }
    };
    Ok(success_response(id, result))
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "deslop",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn tools_list_result() -> Value {
    json!({
        "tools": tool_definitions()
    })
}

fn tool_definitions() -> Vec<Value> {
    vec![
        scan_tool_spec(),
        propose_tool_spec(),
        fix_tool_spec(),
        verify_tool_spec(),
        characterize_tool_spec(),
        verify_characterization_tool_spec(),
        apply_tool_spec(),
        metrics_tool_spec(),
        graph_tool_spec(),
        rules_tool_spec(),
    ]
}

fn scan_tool_spec() -> Value {
    tool(
        "scan",
        "Scan paths and return deslop.findings/1 JSON.",
        object_schema(json!({
            "paths": paths_schema(),
            "format": { "type": "string", "enum": ["json"], "default": "json" },
            "config": config_schema("Optional deslop.toml path for [analyzer] scan settings."),
            "analyzer": analyzer_schema()
        })),
    )
}

fn propose_tool_spec() -> Value {
    tool(
        "propose",
        "Return deslop.workorder/1 JSONL-compatible work orders.",
        object_schema(json!({
            "paths": paths_schema(),
            "config": config_schema("Optional deslop.toml path for [analyzer] propose settings."),
            "analyzer": analyzer_schema()
        })),
    )
}

fn fix_tool_spec() -> Value {
    tool(
        "fix",
        "Return deslop-slim rewrite prompts by default (mode=prompts). With deslop-mcp built using --features slim-llm, mode=auto runs deslop-slim server-side and returns deslop.slim/1.",
        object_schema(fix_tool_properties()),
    )
}

fn fix_tool_properties() -> Value {
    json!({
        "mode": {
            "type": "string",
            "enum": ["prompts", "auto"],
            "default": "prompts",
            "description": "prompts returns deslop.fix/1 for agent-as-consumer. auto requires deslop-mcp --features slim-llm and runs deslop-slim server-side."
        },
        "paths": paths_schema(),
        "analyzer": analyzer_schema(),
        "provider": {
            "type": "string",
            "enum": ["anthropic", "openai"],
            "default": "anthropic",
            "description": "auto mode only; API keys are read from environment variables, never MCP arguments."
        },
        "model": string_schema("auto mode only; defaults via DESLOP_SLIM_MODEL or deslop-slim's built-in default."),
        "base_url": string_schema("auto mode only; OpenAI-compatible base URL."),
        "apply": { "type": "boolean", "default": false },
        "allow_unverified": { "type": "boolean", "default": false },
        "coverage": {
            "type": "string",
            "default": "disabled",
            "description": "auto mode only; disabled, auto, auto:<cmd>, lcov:<path>, cloverage:<path>, julia-cov:<path>, or coverage-py:<path>."
        },
        "check_cmd": { "type": "string" },
        "characterize": { "type": "boolean", "default": false },
        "mock": string_schema("auto mode only; path to a recorded response for deterministic no-network runs."),
        "consent": {
            "type": "boolean",
            "default": false,
            "description": "auto mode real providers only; explicit source-egress consent. Mock runs bypass consent."
        },
        "config": {
            "type": "string",
            "default": "deslop.toml",
            "description": "deslop.toml path used for prompt-mode [analyzer] settings and auto-mode [slim] egress_consent."
        }
    })
}

fn verify_tool_spec() -> Value {
    tool(
        "verify",
        "Verify deslop.patch/1 patches without writing files.",
        required_schema(&["patches"], patch_verification_properties()),
    )
}

fn characterize_tool_spec() -> Value {
    tool(
        "characterize",
        "Emit deslop.workorder/1 requests for weak-oracle regions.",
        required_schema(
            &["patches"],
            json!({
                "patches": patches_schema(),
                "check_cmd": { "type": "string" },
                "coverage": { "type": "boolean", "default": false },
                "mutation": { "type": "boolean", "default": false }
            }),
        ),
    )
}

fn verify_characterization_tool_spec() -> Value {
    tool(
        "verify_characterization",
        "Accept generated characterization tests only if they pass current code.",
        required_schema(
            &["tests", "check_cmd"],
            json!({
                "tests": characterization_tests_schema(),
                "check_cmd": { "type": "string" }
            }),
        ),
    )
}

fn apply_tool_spec() -> Value {
    let mut properties = patch_verification_properties();
    properties["allow_non_removable"] = json!({ "type": "boolean", "default": false });
    properties["no_backup"] = json!({ "type": "boolean", "default": false });
    tool(
        "apply",
        "Verify and atomically apply deslop.patch/1 patches.",
        required_schema(&["patches"], properties),
    )
}

fn patch_verification_properties() -> Value {
    json!({
        "patches": patches_schema(),
        "check_cmd": { "type": "string" },
        "coverage": coverage_schema(),
        "mutation": { "type": "boolean", "default": false },
        "characterization_tests": characterization_tests_schema()
    })
}

fn metrics_tool_spec() -> Value {
    tool(
        "metrics",
        "Return read-only deslop.metrics/3 JSON with Tree-sitter-derived per-region structural readability, labeled intrinsic confidence plus numeric score, explicit confidence_basis, nested repo_relative z-score/percentile, distribution statistics, ranked candidates, and complexity/entropy hotspots. Flat distributions cannot create relative candidates. Confidence is uncalibrated triage evidence, not proof that a rewrite is safe.",
        object_schema(json!({
            "paths": paths_schema(),
            "sigma": { "type": "number", "default": 2.0 }
        })),
    )
}

fn graph_tool_spec() -> Value {
    tool(
        "graph",
        "Return read-only deslop.graph/1 JSON for refactor planning. Contains edges are resolved syntax ownership. Calls/imports/inherits are syntactic planning hints or ambiguous evidence; a syntactic external-symbol target is unresolved, not proven external, and syntactic is not resolution proof. No writes, no network.",
        object_schema(json!({
            "paths": paths_schema(),
            "include_calls": { "type": "boolean", "default": true }
        })),
    )
}

fn rules_tool_spec() -> Value {
    tool(
        "rules",
        "Return the built-in rule catalog.",
        object_schema(json!({})),
    )
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
    })
}

fn object_schema(properties: Value) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false
    })
}

fn required_schema(required: &[&str], properties: Value) -> Value {
    let mut schema = object_schema(properties);
    schema["required"] = json!(required);
    schema
}

fn string_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "description": description
    })
}

fn config_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "default": "deslop.toml",
        "description": description
    })
}

fn analyzer_schema() -> Value {
    let lang_schema = || {
        json!({
            "type": "object",
            "properties": {
                "long_method_nloc": {
                    "type": "integer",
                    "description": "Per-language non-comment line threshold for long-method."
                }
            },
            "additionalProperties": false
        })
    };
    let rule_schema = || {
        json!({
            "type": "object",
            "properties": {
                "enabled": {
                    "type": "boolean",
                    "description": "Set false to disable the rule, same as listing it in disabled_rules."
                },
                "ignore_paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Path globs skipped for this rule only."
                }
            },
            "additionalProperties": false
        })
    };
    json!({
        "type": "object",
        "description": "Analyzer overrides. Global values apply first; per-language long_method_nloc overrides the global threshold for that language. Suppression (disabled_rules, ignore_paths, rules) filters findings after they are produced; unknown rule names are rejected.",
        "properties": {
            "min_duplication_tokens": { "type": "integer" },
            "long_method_nloc": {
                "type": "integer",
                "description": "Global non-comment line threshold for long-method."
            },
            "min_meaningful_tokens": { "type": "integer" },
            "disabled_rules": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Rule names to drop entirely. Must be known deslop rules."
            },
            "ignore_paths": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Path globs skipped for every rule (e.g. \"**/generated/**\")."
            },
            "rules": {
                "type": "object",
                "description": "Per-rule controls keyed by rule name.",
                "additionalProperties": rule_schema()
            },
            "rust": lang_schema(),
            "clojure": lang_schema(),
            "julia": lang_schema(),
            "python": lang_schema(),
            "javascript": lang_schema(),
            "typescript": lang_schema(),
            "generic": lang_schema()
        },
        "additionalProperties": false
    })
}

fn paths_schema() -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" },
        "default": ["."]
    })
}

fn patches_schema() -> Value {
    json!({
        "type": "array",
        "items": { "$ref": "#/$defs/deslop.patch/1" },
        "$defs": {
            "deslop.patch/1": {
                "type": "object",
                "required": ["schema", "workorder_id", "region_fingerprint", "replacement", "by"],
                "properties": {
                    "schema": { "const": "deslop.patch/1" },
                    "workorder_id": { "type": "string" },
                    "region_fingerprint": { "type": "string" },
                    "replacement": { "type": "string" },
                    "by": { "type": "string" }
                },
                "additionalProperties": false
            }
        }
    })
}

fn coverage_schema() -> Value {
    json!({
        "anyOf": [
            { "type": "boolean" },
            { "type": "string" }
        ],
        "default": false,
        "description": "Coverage gate. Boolean back-compat: true=auto, false=disabled. Mode string: disabled, off, none, auto, auto:<cmd>, lcov:<path>, cloverage:<path>, julia-cov:<path>, julia:<path>, coverage-py:<path>, coverage.py:<path>, or python:<path>."
    })
}

fn characterization_tests_schema() -> Value {
    json!({
        "type": "array",
        "items": { "$ref": "#/$defs/deslop.characterization-test/1" },
        "default": [],
        "$defs": {
            "deslop.characterization-test/1": {
                "type": "object",
                "required": ["schema", "workorder_id", "region_fingerprint", "test_path", "test_text", "by"],
                "properties": {
                    "schema": { "const": "deslop.characterization-test/1" },
                    "workorder_id": { "type": "string" },
                    "region_fingerprint": { "type": "string" },
                    "test_path": { "type": "string" },
                    "test_text": { "type": "string" },
                    "by": { "type": "string" }
                },
                "additionalProperties": false
            }
        }
    })
}

fn tools_call_result(params: &Value) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("tools/call missing tool name"))?;
    let args = params.get("arguments").unwrap_or(&Value::Null);
    let payload = match name {
        "scan" => scan_tool(args)?,
        "propose" => propose_tool(args)?,
        "fix" => fix_tool(args)?,
        "verify" => verify_tool(args)?,
        "characterize" => characterize_tool(args)?,
        "verify_characterization" => verify_characterization_tool(args)?,
        "apply" => apply_tool(args)?,
        "metrics" => metrics_tool(args)?,
        "graph" => graph_tool(args)?,
        "rules" => json!({ "rules": deslop_core::rules::render_table() }),
        other => bail!("unknown tool `{other}`"),
    };
    tool_result(payload)
}

fn tool_result(payload: Value) -> Result<Value> {
    Ok(json!({
        "content": [
            {
                "type": "text",
                "text": serde_json::to_string_pretty(&payload)?,
            }
        ],
        "structuredContent": payload,
        "isError": false,
    }))
}

fn scan_tool(args: &Value) -> Result<Value> {
    let reports = scan_reports(args)?;
    let text = render_json(&reports)?;
    Ok(serde_json::from_str(&text)?)
}

fn propose_tool(args: &Value) -> Result<Value> {
    let work_orders = proposed_work_orders(args)?;
    Ok(json!({
        "schema": "deslop.workorders/1",
        "workorders": work_orders,
    }))
}

fn fix_tool(args: &Value) -> Result<Value> {
    match fix_mode(args)? {
        "prompts" => fix_prompts_tool(args),
        "auto" => fix_auto_tool(args),
        other => bail!("unsupported fix mode `{other}`; use `prompts` or `auto`"),
    }
}

fn fix_mode(args: &Value) -> Result<&str> {
    match args.get("mode") {
        None => Ok("prompts"),
        Some(Value::String(mode)) => Ok(mode),
        Some(_) => bail!("fix mode must be a string"),
    }
}

fn fix_prompts_tool(args: &Value) -> Result<Value> {
    let prompts = proposed_work_orders(args)?
        .into_iter()
        .filter(|work_order| work_order.kind == WorkOrderKind::RewriteRegion)
        .map(fix_prompt_entry)
        .collect::<Vec<_>>();
    Ok(json!({
        "schema": "deslop.fix/1",
        "prompts": prompts,
        "next": "Rewrite each region. Build deslop.patch/1 patches { schema:\"deslop.patch/1\", workorder_id, region_fingerprint, replacement, by } and call the `apply` tool (default applies only Removable; pass coverage / allow_non_removable to widen)."
    }))
}

#[cfg(not(feature = "slim-llm"))]
fn fix_auto_tool(_args: &Value) -> Result<Value> {
    bail!("fix mode=auto requires deslop-mcp built with --features slim-llm")
}

#[cfg(feature = "slim-llm")]
fn fix_auto_tool(args: &Value) -> Result<Value> {
    let model = resolve_model(optional_string(args, "model"));
    let options = SlimOptions {
        root: PathBuf::from("."),
        paths: paths_arg(args)?,
        workorders: None,
        apply: bool_arg(args, "apply"),
        characterize: bool_arg(args, "characterize"),
        allow_unverified: bool_arg(args, "allow_unverified"),
        coverage: auto_coverage_config(args)?,
        model: model.to_owned(),
        check_cmd: optional_string(args, "check_cmd"),
        backup: true,
        analyzer: mcp_analyzer_config(args)?,
    };
    let report = if let Some(path) = optional_string(args, "mock") {
        let client = RecordedClient::from_path(path)?;
        run_slim(&client, options)?
    } else {
        let provider = provider_arg(args)?;
        let base_url = optional_string(args, "base_url");
        let destination = provider_base_url(provider, base_url.as_deref());
        require_mcp_egress_consent(args, provider, &destination, &options)?;
        match provider {
            "anthropic" => {
                let client = AnthropicClient::from_env(model.clone())?;
                run_slim(&client, options)?
            }
            "openai" => {
                let client = OpenAiClient::from_env(model.clone(), base_url)?;
                run_slim(&client, options)?
            }
            other => bail!("unsupported fix provider `{other}`; use `anthropic` or `openai`"),
        }
    };
    Ok(serde_json::to_value(report)?)
}

#[derive(Debug, Default, Deserialize)]
struct McpDeslopConfig {
    #[cfg(feature = "slim-llm")]
    #[serde(default)]
    slim: Option<McpSlimConfig>,
    #[serde(default)]
    analyzer: Option<McpAnalyzerConfig>,
}

#[cfg(feature = "slim-llm")]
#[derive(Debug, Default, Deserialize)]
struct McpSlimConfig {
    #[serde(default)]
    egress_consent: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct McpAnalyzerConfig {
    #[serde(default)]
    min_duplication_tokens: Option<usize>,
    #[serde(default)]
    long_method_nloc: Option<usize>,
    #[serde(default)]
    min_meaningful_tokens: Option<usize>,
    #[serde(default)]
    disabled_rules: Option<Vec<String>>,
    #[serde(default)]
    ignore_paths: Option<Vec<String>>,
    #[serde(default)]
    rules: Option<BTreeMap<String, McpRuleConfig>>,
    #[serde(default)]
    rust: Option<McpAnalyzerLangConfig>,
    #[serde(default)]
    clojure: Option<McpAnalyzerLangConfig>,
    #[serde(default)]
    julia: Option<McpAnalyzerLangConfig>,
    #[serde(default)]
    python: Option<McpAnalyzerLangConfig>,
    #[serde(default)]
    javascript: Option<McpAnalyzerLangConfig>,
    #[serde(default)]
    typescript: Option<McpAnalyzerLangConfig>,
    #[serde(default)]
    generic: Option<McpAnalyzerLangConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct McpRuleConfig {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    ignore_paths: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct McpAnalyzerLangConfig {
    #[serde(default)]
    long_method_nloc: Option<usize>,
}

#[cfg(feature = "slim-llm")]
fn require_mcp_egress_consent(
    args: &Value,
    provider: &str,
    base_url: &str,
    options: &SlimOptions,
) -> Result<()> {
    let explicit = bool_arg(args, "consent")
        || env_egress_consent(std::env::var("DESLOP_SLIM_CONSENT").ok())
        || mcp_config_egress_consent(args)?;
    match resolve_egress_consent(explicit, false) {
        EgressDecision::Granted => Ok(()),
        EgressDecision::Prompt => unreachable!("MCP fix auto is non-interactive"),
        EgressDecision::DeniedNonInteractive => {
            bail!(
                "{}",
                egress_consent_error(provider, base_url, egress_summary(options)?)
            )
        }
    }
}

#[cfg(feature = "slim-llm")]
fn mcp_config_egress_consent(args: &Value) -> Result<bool> {
    Ok(mcp_deslop_config(args)?
        .slim
        .and_then(|slim| slim.egress_consent)
        .unwrap_or(false))
}

fn mcp_deslop_config(args: &Value) -> Result<McpDeslopConfig> {
    let path = optional_string(args, "config").unwrap_or_else(|| "deslop.toml".to_string());
    let path = PathBuf::from(path);
    if !path.exists() {
        return Ok(McpDeslopConfig::default());
    }
    let text =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

#[cfg(feature = "slim-llm")]
fn provider_arg(args: &Value) -> Result<&str> {
    match args.get("provider") {
        None => Ok("anthropic"),
        Some(Value::String(provider)) => Ok(provider),
        Some(_) => bail!("provider must be a string"),
    }
}

#[cfg(feature = "slim-llm")]
fn auto_coverage_config(args: &Value) -> Result<CoverageConfig> {
    match args.get("coverage") {
        None => Ok(CoverageConfig::Disabled),
        Some(Value::String(mode)) => parse_coverage_mode(mode),
        Some(_) => bail!("fix auto coverage must be a coverage mode string"),
    }
}

fn fix_prompt_entry(work_order: WorkOrder) -> Value {
    let prompt = build_prompt(&work_order);
    json!({
        "workorder_id": work_order.id,
        "path": work_order.path,
        "region": {
            "start_line": work_order.region.start_line,
            "end_line": work_order.region.end_line,
        },
        "region_fingerprint": workorder_region_fingerprint(&work_order),
        "contract": work_order.contract,
        "findings": work_order.findings,
        "prompt": prompt.text,
    })
}

fn proposed_work_orders(args: &Value) -> Result<Vec<WorkOrder>> {
    let reports = scan_reports(args)?;
    let _jsonl = render_agent(&reports)?;
    let mut work_orders = Vec::new();
    for report in reports {
        let source = SourceFile::read(&report.path)?;
        work_orders.extend(work_orders_for_source(&source, &report.findings));
    }
    Ok(work_orders)
}

fn scan_reports(args: &Value) -> Result<Vec<deslop_core::FileReport>> {
    let paths = paths_arg(args)?;
    scan_paths_with_config(&paths, mcp_analyzer_config(args)?)
}

fn mcp_analyzer_config(args: &Value) -> Result<AnalyzerConfig> {
    let mut config = AnalyzerConfig::default();
    let mut suppression = Suppression::builder();
    if let Some(analyzer) = mcp_deslop_config(args)?.analyzer {
        apply_mcp_analyzer_config(&mut config, &analyzer);
        collect_mcp_suppression(&mut suppression, &analyzer);
    }
    if let Some(value) = args.get("analyzer") {
        let analyzer: McpAnalyzerConfig =
            serde_json::from_value(value.to_owned()).context("invalid analyzer config")?;
        apply_mcp_analyzer_config(&mut config, &analyzer);
        collect_mcp_suppression(&mut suppression, &analyzer);
    }
    config.suppression = suppression.build()?;
    Ok(config)
}

fn collect_mcp_suppression(builder: &mut SuppressionBuilder, analyzer: &McpAnalyzerConfig) {
    builder.add_section(
        analyzer.disabled_rules.as_deref().unwrap_or_default(),
        analyzer.ignore_paths.as_deref().unwrap_or_default(),
        analyzer.rules.iter().flatten().map(|(rule, rule_config)| {
            (
                rule.as_str(),
                RuleSuppression {
                    enabled: rule_config.enabled,
                    ignore_paths: rule_config.ignore_paths.as_deref().unwrap_or_default(),
                },
            )
        }),
    );
}

fn apply_mcp_analyzer_config(config: &mut AnalyzerConfig, analyzer: &McpAnalyzerConfig) {
    if let Some(value) = analyzer.min_duplication_tokens {
        config.min_duplication_tokens = value;
    }
    if let Some(value) = analyzer.long_method_nloc {
        config.long_method_nloc = value;
    }
    if let Some(value) = analyzer.min_meaningful_tokens {
        config.min_meaningful_tokens = value;
    }
    apply_mcp_lang_config(&mut config.rust, analyzer.rust.as_ref());
    apply_mcp_lang_config(&mut config.clojure, analyzer.clojure.as_ref());
    apply_mcp_lang_config(&mut config.julia, analyzer.julia.as_ref());
    apply_mcp_lang_config(&mut config.python, analyzer.python.as_ref());
    apply_mcp_lang_config(&mut config.javascript, analyzer.javascript.as_ref());
    apply_mcp_lang_config(&mut config.typescript, analyzer.typescript.as_ref());
    apply_mcp_lang_config(&mut config.generic, analyzer.generic.as_ref());
}

fn apply_mcp_lang_config(
    config: &mut AnalyzerLangConfig,
    analyzer: Option<&McpAnalyzerLangConfig>,
) {
    if let Some(value) = analyzer.and_then(|analyzer| analyzer.long_method_nloc) {
        config.long_method_nloc = Some(value);
    }
}

fn verify_tool(args: &Value) -> Result<Value> {
    let patches = patches_arg(args)?;
    let report = verify_patches(&patches, &verify_options(args, false)?)?;
    Ok(serde_json::to_value(report)?)
}

fn characterize_tool(args: &Value) -> Result<Value> {
    let patches = patches_arg(args)?;
    let work_orders = characterization_work_orders_for_patches(
        &patches,
        &VerifyOptions {
            characterization_tests: Vec::new(),
            ..verify_options(args, false)?
        },
    )?;
    Ok(json!({
        "schema": "deslop.workorders/1",
        "workorders": work_orders,
    }))
}

fn verify_characterization_tool(args: &Value) -> Result<Value> {
    let tests = characterization_tests_arg(args)?;
    let Some(check_cmd) = optional_string(args, "check_cmd") else {
        bail!("check_cmd is required");
    };
    let report = verify_characterization_tests(
        &tests,
        &VerifyOptions {
            root: PathBuf::from("."),
            check_cmd: Some(check_cmd),
            coverage: CoverageConfig::Disabled,
            mutation: MutationConfig::Disabled,
            characterization_tests: Vec::new(),
            allow_non_removable: false,
        },
    )?;
    Ok(serde_json::to_value(report)?)
}

fn apply_tool(args: &Value) -> Result<Value> {
    let patches = patches_arg(args)?;
    let report = apply_patches(
        &patches,
        &verify_options(args, bool_arg(args, "allow_non_removable"))?,
        !bool_arg(args, "no_backup"),
    )?;
    Ok(serde_json::to_value(report)?)
}

fn verify_options(args: &Value, allow_non_removable: bool) -> Result<VerifyOptions> {
    Ok(VerifyOptions {
        root: PathBuf::from("."),
        check_cmd: optional_string(args, "check_cmd"),
        coverage: coverage_config(args)?,
        mutation: mutation_config(args),
        characterization_tests: characterization_tests_arg(args)?,
        allow_non_removable,
    })
}

fn coverage_config(args: &Value) -> Result<CoverageConfig> {
    match args.get("coverage") {
        None => Ok(CoverageConfig::Disabled),
        Some(Value::Bool(true)) => Ok(CoverageConfig::Auto),
        Some(Value::Bool(false)) => Ok(CoverageConfig::Disabled),
        Some(Value::String(mode)) => parse_coverage_mode(mode),
        Some(_) => bail!("coverage must be a boolean or coverage mode string"),
    }
}

fn mutation_config(args: &Value) -> MutationConfig {
    if bool_arg(args, "mutation") {
        MutationConfig::Auto
    } else {
        MutationConfig::Disabled
    }
}

fn metrics_tool(args: &Value) -> Result<Value> {
    let paths = paths_arg(args)?;
    let report = metrics_paths(
        &paths,
        MetricsConfig {
            sigma: args.get("sigma").and_then(Value::as_f64).unwrap_or(2.0),
        },
    )?;
    Ok(serde_json::to_value(report)?)
}

fn graph_tool(args: &Value) -> Result<Value> {
    let paths = paths_arg(args)?;
    let graph = graph_paths(
        &paths,
        GraphConfig {
            include_calls: args
                .get("include_calls")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        },
    )?;
    Ok(serde_json::to_value(graph)?)
}

fn paths_arg(args: &Value) -> Result<Vec<PathBuf>> {
    if !args.is_object() && !args.is_null() {
        bail!("tool arguments must be an object");
    }
    let Some(paths) = args.get("paths") else {
        return Ok(vec![PathBuf::from(".")]);
    };
    let Some(paths) = paths.as_array() else {
        bail!("paths must be an array");
    };
    paths
        .iter()
        .map(|path| {
            path.as_str()
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("path entries must be strings"))
        })
        .collect::<Result<Vec<_>>>()
}

fn patches_arg(args: &Value) -> Result<Vec<Patch>> {
    let Some(patches) = args.get("patches").and_then(Value::as_array) else {
        bail!("patches must be an array");
    };
    patches
        .iter()
        .cloned()
        .map(serde_json::from_value)
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse deslop.patch/1 patch")
}

fn characterization_tests_arg(args: &Value) -> Result<Vec<CharacterizationTest>> {
    let Some(tests) = args
        .get("characterization_tests")
        .or_else(|| args.get("tests"))
    else {
        return Ok(Vec::new());
    };
    let Some(tests) = tests.as_array() else {
        bail!("characterization_tests must be an array");
    };
    tests
        .iter()
        .cloned()
        .map(serde_json::from_value)
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse deslop.characterization-test/1 test")
}

fn optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn bool_arg(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        }
    })
}

pub fn patch_for_workorder(
    work_order: &deslop_protocol::WorkOrder,
    replacement: impl Into<String>,
) -> Patch {
    Patch {
        schema: "deslop.patch/1".to_string(),
        workorder_id: work_order.id.to_owned(),
        region_fingerprint: workorder_region_fingerprint(work_order),
        replacement: replacement.into(),
        by: "deslop-mcp-test".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::sync::{Mutex, MutexGuard};

    static TEMP_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct SampleFixture {
        _guard: MutexGuard<'static, ()>,
        _temp: tempfile::TempDir,
        path: PathBuf,
    }

    struct RustCoverageFixture {
        _guard: MutexGuard<'static, ()>,
        _temp: tempfile::TempDir,
        source: PathBuf,
        coverage: PathBuf,
        work_order: WorkOrder,
    }

    struct RustLongMethodFixture {
        _guard: MutexGuard<'static, ()>,
        temp: tempfile::TempDir,
        source: PathBuf,
    }

    #[cfg(feature = "slim-llm")]
    struct RustSlimFixture {
        _guard: MutexGuard<'static, ()>,
        _temp: tempfile::TempDir,
        source: PathBuf,
        coverage: PathBuf,
    }

    #[cfg(feature = "slim-llm")]
    const RUST_SLIM_ORIGINAL: &str =
        "fn unfinished() -> i32 {\n    todo!(\"TODO: implement\")\n}\n";

    #[test]
    fn tools_list_returns_expected_tool_set_with_schemas() {
        let response = tools_list_response();
        let tools = response["result"]["tools"].as_array().expect("tools");
        assert_expected_tool_names(tools);
        assert!(tools.iter().all(|tool| tool.get("inputSchema").is_some()));
        assert_scan_analyzer_schema(tool_by_name(tools, "scan"));
        assert_scan_analyzer_schema(tool_by_name(tools, "propose"));
        assert_verify_coverage_schema(tool_by_name(tools, "verify"));
        assert_fix_tool_schema(tool_by_name(tools, "fix"));
        assert!(
            tool_by_name(tools, "graph")["description"]
                .as_str()
                .expect("graph description")
                .contains("syntactic is not resolution proof")
        );
        assert!(
            tool_by_name(tools, "graph")["description"]
                .as_str()
                .expect("graph description")
                .contains("unresolved, not proven external")
        );
    }

    #[test]
    fn metrics_tool_exposes_readability_and_refactor_confidence() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("sample.js");
        std::fs::write(
            &source,
            "class Worker { run(value) { if (value) { return value; } return 0; } }\n",
        )
        .expect("fixture");
        let report = metrics_tool(&json!({ "paths": [source] })).expect("metrics");
        assert_eq!(report["schema"], "deslop.metrics/3");
        assert_eq!(report["readability_model"]["calibrated"], false);
        assert!(report["refactor_confidence_distribution"]["mean"].is_number());
        assert!(report["refactor_confidence_distribution"]["stddev"].is_number());
        let regions = report["functions"].as_array().expect("regions");
        assert!(regions.iter().any(|region| {
            region["kind"] == "class_declaration"
                && region["readability"]["refactor_confidence"].is_object()
                && region["readability"]["refactor_confidence_score"].is_number()
                && region["readability"]["confidence_basis"] == "tree_intrinsic_v1"
                && region["readability"]["repo_relative"]["zscore"].is_number()
                && region["readability"]["repo_relative"]["percentile"].is_number()
        }));
        assert!(regions.iter().any(|region| {
            region["kind"] == "method_definition"
                && region["readability"]["measurement_confidence"].is_number()
        }));
        let class = regions
            .iter()
            .find(|region| region["kind"] == "class_declaration")
            .expect("class region");
        let labeled = class["readability"]["refactor_confidence"]
            .as_object()
            .expect("labeled confidence");
        assert_eq!(labeled.len(), 1);
        assert_eq!(
            labeled.values().next(),
            Some(&class["readability"]["refactor_confidence_score"])
        );
        assert!(class["readability"].get("refactor_zscore").is_none());
        assert!(class["readability"].get("refactor_percentile").is_none());
        assert!(report["refactor_candidates"].is_array());
    }

    fn tools_list_response() -> Value {
        handle_request(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        }))
        .expect("response")
    }

    fn assert_expected_tool_names(tools: &[Value]) {
        let names = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "scan",
                "propose",
                "fix",
                "verify",
                "characterize",
                "verify_characterization",
                "apply",
                "metrics",
                "graph",
                "rules"
            ]
        );
    }

    fn tool_by_name<'a>(tools: &'a [Value], name: &str) -> &'a Value {
        tools
            .iter()
            .find(|tool| tool["name"] == name)
            .expect("tool")
    }

    fn assert_verify_coverage_schema(verify: &Value) {
        let coverage = &verify["inputSchema"]["properties"]["coverage"];
        assert_eq!(coverage["default"], false);
        assert!(coverage["anyOf"].as_array().expect("anyOf").len() == 2);
        assert!(
            coverage["description"]
                .as_str()
                .expect("description")
                .contains("lcov:<path>")
        );
    }

    fn assert_scan_analyzer_schema(tool: &Value) {
        let properties = &tool["inputSchema"]["properties"];
        assert_eq!(properties["config"]["default"], "deslop.toml");
        assert_eq!(
            properties["analyzer"]["properties"]["rust"]["properties"]["long_method_nloc"]["type"],
            "integer"
        );
        assert_eq!(
            properties["analyzer"]["properties"]["javascript"]["properties"]["long_method_nloc"]["type"],
            "integer"
        );
        assert_eq!(
            properties["analyzer"]["properties"]["typescript"]["properties"]["long_method_nloc"]["type"],
            "integer"
        );
        assert_eq!(
            properties["analyzer"]["description"],
            "Analyzer overrides. Global values apply first; per-language long_method_nloc overrides the global threshold for that language. Suppression (disabled_rules, ignore_paths, rules) filters findings after they are produced; unknown rule names are rejected."
        );
        assert_eq!(
            properties["analyzer"]["properties"]["rules"]["additionalProperties"]["properties"]["ignore_paths"]
                ["type"],
            "array"
        );
    }

    fn assert_fix_tool_schema(fix: &Value) {
        let mode = &fix["inputSchema"]["properties"]["mode"];
        assert_eq!(mode["default"], "prompts");
        assert_eq!(mode["enum"], json!(["prompts", "auto"]));
        assert_eq!(
            fix["inputSchema"]["properties"]["consent"]["default"],
            false
        );
        assert_eq!(
            fix["inputSchema"]["properties"]["config"]["default"],
            "deslop.toml"
        );
        assert_eq!(
            fix["inputSchema"]["properties"]["analyzer"]["properties"]["rust"]["properties"]["long_method_nloc"]
                ["type"],
            "integer"
        );
        assert!(
            fix["description"]
                .as_str()
                .expect("description")
                .contains("slim-llm")
        );
    }

    #[test]
    fn scan_tool_returns_findings_json_for_fixture() {
        let fixture = sample_fixture();
        let response = call_tool("scan", json!({ "paths": [fixture.path] })).expect("scan");
        let reports = structured_content(&response)["reports"]
            .as_array()
            .expect("reports");
        assert!(
            reports[0]["findings"]
                .as_array()
                .expect("findings")
                .iter()
                .any(|finding| finding["rule"] == "reimpl-empty?")
        );
    }

    #[test]
    fn graph_tool_returns_refactor_graph_json() {
        let _guard = temp_test_lock();
        let temp = tempfile::tempdir_in(".").expect("tempdir");
        let path = repo_relative_temp_path(&temp, "sample.rs");
        fs::write(&path, "fn helper() {}\nfn run() {\n    helper();\n}\n")
            .expect("rust graph fixture");

        let response = call_tool("graph", json!({ "paths": [path] })).expect("graph");
        let content = structured_content(&response);
        assert_eq!(content["schema"], "deslop.graph/1");
        assert!(
            content["agent_notes"]
                .as_array()
                .expect("agent notes")
                .len()
                >= 2
        );
        assert!(
            content["nodes"]
                .as_array()
                .expect("nodes")
                .iter()
                .any(|node| node["kind"] == "function" && node["name"] == "run")
        );
        assert!(
            content["edges"]
                .as_array()
                .expect("edges")
                .iter()
                .any(|edge| {
                    edge["kind"] == "calls"
                        && edge["confidence"] == "syntactic"
                        && edge["label"] == "helper"
                }),
            "{content:#}"
        );
    }

    #[test]
    fn graph_tool_preserves_unresolved_alias_placeholders() {
        let _guard = temp_test_lock();
        let temp = tempfile::tempdir_in(".").expect("tempdir");
        let origin = repo_relative_temp_path(&temp, "origin.rs");
        let unrelated = repo_relative_temp_path(&temp, "chosen.rs");
        let caller = repo_relative_temp_path(&temp, "caller.rs");
        fs::write(&origin, "pub fn helper() {}\n").expect("origin");
        fs::write(&unrelated, "pub fn chosen() {}\n").expect("unrelated");
        fs::write(
            &caller,
            "use crate::origin::helper as chosen;\nfn run() { chosen(); }\n",
        )
        .expect("caller");

        let response =
            call_tool("graph", json!({ "paths": [origin, unrelated, caller] })).expect("graph");
        let content = structured_content(&response);
        let call = content["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .find(|edge| edge["kind"] == "calls" && edge["label"] == "chosen")
            .expect("chosen call");
        let target = content["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .find(|node| node["id"] == call["to"])
            .expect("call target");

        assert_eq!(call["confidence"], "syntactic");
        assert_eq!(target["kind"], "external-symbol");
        assert!(
            content["agent_notes"]
                .as_array()
                .expect("agent notes")
                .iter()
                .any(|note| note
                    .as_str()
                    .is_some_and(|note| note.contains("unresolved placeholder")))
        );
    }

    #[test]
    fn scan_accepts_per_language_analyzer_override() {
        let fixture = rust_long_method_fixture(25);
        let response = call_tool(
            "scan",
            json!({
                "paths": [fixture.source],
                "analyzer": {
                    "long_method_nloc": 100,
                    "rust": { "long_method_nloc": 20 }
                }
            }),
        )
        .expect("scan");
        assert_scan_has_rule(&response, "long-method");
    }

    #[test]
    fn mcp_analyzer_config_accepts_javascript_and_typescript_thresholds() {
        let config = mcp_analyzer_config(&json!({
            "analyzer": {
                "javascript": { "long_method_nloc": 31 },
                "typescript": { "long_method_nloc": 37 }
            }
        }))
        .expect("analyzer config");

        assert_eq!(config.javascript.long_method_nloc, Some(31));
        assert_eq!(config.typescript.long_method_nloc, Some(37));
    }

    #[test]
    fn scan_inline_disabled_rule_suppresses_finding() {
        let fixture = sample_fixture();
        let baseline =
            call_tool("scan", json!({ "paths": [&fixture.path] })).expect("baseline scan");
        assert_scan_has_rule(&baseline, "reimpl-empty?");

        let response = call_tool(
            "scan",
            json!({
                "paths": [&fixture.path],
                "analyzer": { "disabled_rules": ["reimpl-empty?"] }
            }),
        )
        .expect("scan");
        let reports = structured_content(&response)["reports"]
            .as_array()
            .expect("reports");
        assert!(
            reports.iter().all(|report| report["findings"]
                .as_array()
                .expect("findings")
                .iter()
                .all(|finding| finding["rule"] != "reimpl-empty?")),
            "{response:#}"
        );
    }

    #[test]
    fn scan_inline_unknown_disabled_rule_is_rejected() {
        let fixture = sample_fixture();
        let err = call_tool(
            "scan",
            json!({
                "paths": [&fixture.path],
                "analyzer": { "disabled_rules": ["ignore-comments"] }
            }),
        )
        .expect_err("unknown rule must error");
        assert!(
            err.to_string().contains("unknown rule 'ignore-comments'"),
            "{err}"
        );
    }

    #[test]
    fn propose_reads_per_language_analyzer_config_file() {
        let fixture = rust_long_method_fixture(25);
        let config = repo_relative_temp_path(&fixture.temp, "deslop.toml");
        fs::write(
            &config,
            "[analyzer]\nlong_method_nloc = 100\n\n[analyzer.rust]\nlong_method_nloc = 20\n",
        )
        .expect("write config");
        let proposed = call_tool(
            "propose",
            json!({
                "paths": [fixture.source],
                "config": config
            }),
        )
        .expect("propose");
        assert!(
            structured_content(&proposed)["workorders"]
                .as_array()
                .expect("workorders")
                .iter()
                .any(|work_order| work_order["findings"]
                    .as_array()
                    .expect("findings")
                    .iter()
                    .any(|finding| finding["rule"] == "long-method")),
            "{proposed:#}"
        );
    }

    #[test]
    fn fix_prompts_accept_per_language_analyzer_override() {
        let fixture = rust_long_method_fixture(25);
        let response = call_tool(
            "fix",
            json!({
                "paths": [fixture.source],
                "analyzer": {
                    "long_method_nloc": 100,
                    "rust": { "long_method_nloc": 20 }
                }
            }),
        )
        .expect("fix prompts");
        assert!(
            structured_content(&response)["prompts"]
                .as_array()
                .expect("prompts")
                .iter()
                .any(|prompt| prompt["findings"]
                    .as_array()
                    .expect("findings")
                    .iter()
                    .any(|finding| finding["rule"] == "long-method")),
            "{response:#}"
        );
    }

    #[test]
    fn propose_verify_roundtrip_accepts_clean_and_rejects_stale_patch() {
        let fixture = sample_fixture();

        let work_order = propose_first_work_order(&fixture.path);
        let patch = patch_for_workorder(&work_order, "(empty? xs)\n");
        let verified = call_tool("verify", json!({ "patches": [patch] })).expect("verify");
        assert_eq!(first_tool_result(&verified)["passed"], true, "{verified:#}");
        assert_eq!(first_tool_result(&verified)["verdict"], "coverage-unknown");

        let mut stale = patch_for_workorder(&work_order, "(empty? xs)\n");
        stale.region_fingerprint = "stale".to_string();
        let rejected = call_tool("verify", json!({ "patches": [stale] })).expect("verify");
        assert_eq!(first_tool_result(&rejected)["passed"], false);
        assert_eq!(first_tool_result(&rejected)["verdict"], "rejected");
    }

    #[test]
    fn verify_coverage_boolean_back_compat_and_default() {
        let fixture = sample_fixture();

        let work_order = propose_first_work_order(&fixture.path);
        let patch = patch_for_workorder(&work_order, "(empty? xs)\n");

        let absent =
            call_tool("verify", json!({ "patches": [patch.clone()] })).expect("verify absent");
        assert_eq!(first_tool_result(&absent)["verdict"], "coverage-unknown");
        assert_first_tool_reason_contains(&absent, "coverage disabled");

        let disabled = call_tool(
            "verify",
            json!({
                "patches": [patch.clone()],
                "coverage": false
            }),
        )
        .expect("verify false");
        assert_eq!(first_tool_result(&disabled)["verdict"], "coverage-unknown");
        assert_first_tool_reason_contains(&disabled, "coverage disabled");

        let enabled = call_tool(
            "verify",
            json!({
                "patches": [patch],
                "coverage": true
            }),
        )
        .expect("verify true");
        assert_eq!(first_tool_result(&enabled)["verdict"], "coverage-unknown");
        assert_first_tool_reason_contains(&enabled, "coverage-unknown");
    }

    #[test]
    fn apply_accepts_lcov_coverage_mode_string_and_writes_removable_patch() {
        let fixture = rust_coverage_fixture();
        let patch = patch_for_workorder(&fixture.work_order, "fn f() -> i32 {\n    1\n}\n");
        let applied = call_tool(
            "apply",
            json!({
                "patches": [patch],
                "check_cmd": "true",
                "coverage": format!("lcov:{}", fixture.coverage.display()),
                "no_backup": true
            }),
        )
        .expect("apply");

        let content = structured_content(&applied);
        assert_eq!(content["schema"], "deslop.apply/1");
        assert_eq!(
            content["verified"]["results"][0]["verdict"], "removable",
            "{content:#}"
        );
        assert_eq!(content["verified"]["results"][0]["passed"], true);
        assert_eq!(
            fs::read_to_string(&fixture.source).expect("read source"),
            "fn f() -> i32 {\n    1\n}\n"
        );
        assert_eq!(content["written"].as_array().expect("written").len(), 1);
    }

    #[test]
    fn verify_rejects_bad_coverage_mode_string() {
        let fixture = sample_fixture();
        let work_order = propose_first_work_order(&fixture.path);
        let patch = patch_for_workorder(&work_order, "(empty? xs)\n");

        let error = call_tool(
            "verify",
            json!({
                "patches": [patch],
                "coverage": "bogus"
            }),
        )
        .expect_err("bad coverage mode");
        assert!(
            error
                .to_string()
                .contains("unsupported coverage mode `bogus`"),
            "{error:#}"
        );
    }

    #[test]
    fn fix_tool_returns_slim_prompts_for_agent_consumer() {
        let fixture = sample_fixture();

        let work_order = propose_first_work_order(&fixture.path);
        let fixed = call_tool("fix", json!({ "paths": [fixture.path] })).expect("fix");
        let content = structured_content(&fixed);
        let prompts = content["prompts"].as_array().expect("prompts");

        assert_eq!(content["schema"], "deslop.fix/1");
        assert!(!prompts.is_empty());
        assert_eq!(prompts[0]["workorder_id"], work_order.id);
        assert_eq!(
            prompts[0]["region_fingerprint"],
            workorder_region_fingerprint(&work_order)
        );
        assert!(
            prompts[0]["prompt"]
                .as_str()
                .unwrap()
                .contains("(= (count xs) 0)")
        );
        assert!(
            prompts[0]["prompt"]
                .as_str()
                .unwrap()
                .contains(&work_order.findings[0].message)
        );
        assert!(content["next"].as_str().unwrap().contains("apply"));
    }

    #[cfg(not(feature = "slim-llm"))]
    #[test]
    fn fix_auto_requires_slim_llm_feature_in_default_build() {
        let error = call_tool("fix", json!({ "mode": "auto" })).expect_err("feature error");
        assert!(
            error
                .to_string()
                .contains("fix mode=auto requires deslop-mcp built with --features slim-llm"),
            "{error:#}"
        );
    }

    #[cfg(feature = "slim-llm")]
    #[test]
    fn fix_auto_real_provider_requires_explicit_consent() {
        let fixture = rust_slim_fixture();
        let error = call_tool(
            "fix",
            json!({
                "mode": "auto",
                "paths": [fixture.source],
                "provider": "anthropic",
                "config": repo_relative_temp_path(&fixture._temp, "missing-deslop.toml")
            }),
        )
        .expect_err("consent error");
        let error = error.to_string();
        assert!(error.contains("without source-egress consent"), "{error}");
        assert!(error.contains("anthropic"), "{error}");
        assert!(error.contains("DESLOP_SLIM_CONSENT=1"), "{error}");
        assert!(!error.contains("ANTHROPIC_API_KEY"), "{error}");
    }

    #[cfg(feature = "slim-llm")]
    #[test]
    fn mcp_config_egress_consent_reads_slim_section() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = repo_relative_temp_path(&temp, "deslop.toml");
        fs::write(&config, "[slim]\negress_consent = true\n").expect("write config");
        assert!(mcp_config_egress_consent(&json!({ "config": config })).expect("config consent"));
        assert!(
            !mcp_config_egress_consent(&json!({
                "config": repo_relative_temp_path(&temp, "missing.toml")
            }))
            .expect("missing config")
        );
    }

    #[cfg(feature = "slim-llm")]
    #[test]
    fn fix_auto_mock_applies_verified_and_blocks_rejected_rewrite() {
        assert_fix_auto_mock_applies_verified_rewrite();
        assert_fix_auto_mock_blocks_rejected_rewrite();
    }

    #[test]
    fn initialize_list_scan_handshake_works() {
        let fixture = sample_fixture();
        let input = format!(
            "{}\n{}\n{}\n",
            json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"scan","arguments":{"paths":[fixture.path]}}}),
        );
        let mut output = Vec::new();
        run(std::io::Cursor::new(input), &mut output).expect("stdio");
        let lines = String::from_utf8(output).expect("utf8");
        let responses = lines
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("json"))
            .collect::<Vec<_>>();
        assert_eq!(responses.len(), 3);
        assert_eq!(responses[0]["result"]["serverInfo"]["name"], "deslop");
        assert_eq!(responses[1]["result"]["tools"][0]["name"], "scan");
        assert_eq!(
            structured_content(&responses[2])["schema"],
            "deslop.findings/1"
        );
    }

    fn structured_content(response: &Value) -> &Value {
        &response["result"]["structuredContent"]
    }

    fn first_tool_result(response: &Value) -> &Value {
        &structured_content(response)["results"][0]
    }

    fn assert_scan_has_rule(response: &Value, rule: &str) {
        let reports = structured_content(response)["reports"]
            .as_array()
            .expect("reports");
        assert!(
            reports.iter().any(|report| report["findings"]
                .as_array()
                .expect("findings")
                .iter()
                .any(|finding| finding["rule"] == rule)),
            "{response:#}"
        );
    }

    fn assert_first_tool_reason_contains(response: &Value, fragment: &str) {
        assert!(
            first_tool_result(response)["reasons"]
                .as_array()
                .expect("reasons")
                .iter()
                .any(|reason| reason.as_str().unwrap().contains(fragment))
        );
    }

    fn propose_first_work_order(path: &Path) -> WorkOrder {
        let proposed = call_tool("propose", json!({ "paths": [path] })).expect("propose");
        serde_json::from_value(structured_content(&proposed)["workorders"][0].to_owned())
            .expect("workorder")
    }

    fn sample_fixture() -> SampleFixture {
        let guard = temp_test_lock();
        let temp = tempfile::tempdir_in(".").expect("tempdir");
        let path = write_sample_fixture(&temp);
        SampleFixture {
            _guard: guard,
            _temp: temp,
            path,
        }
    }

    fn rust_coverage_fixture() -> RustCoverageFixture {
        let guard = temp_test_lock();
        let temp = tempfile::tempdir_in(".").expect("tempdir");
        let (source, coverage) =
            rust_source_with_lcov(&temp, "fn f() -> i32 {\n    return 1;\n}\n");
        let work_order = propose_first_work_order(&source);
        RustCoverageFixture {
            _guard: guard,
            _temp: temp,
            source,
            coverage,
            work_order,
        }
    }

    fn rust_long_method_fixture(nloc: usize) -> RustLongMethodFixture {
        let guard = temp_test_lock();
        let temp = tempfile::tempdir_in(".").expect("tempdir");
        let source = repo_relative_temp_path(&temp, "sample.rs");
        fs::write(&source, rust_long_method_text(nloc)).expect("rust fixture");
        RustLongMethodFixture {
            _guard: guard,
            temp,
            source,
        }
    }

    fn rust_long_method_text(nloc: usize) -> String {
        let mut text = String::from("fn longish() {\n");
        for idx in 0..nloc.saturating_sub(2) {
            text.push_str(&format!("    let _v{idx} = {idx};\n"));
        }
        text.push_str("}\n");
        text
    }

    #[cfg(feature = "slim-llm")]
    fn rust_slim_fixture() -> RustSlimFixture {
        let guard = temp_test_lock();
        let temp = tempfile::tempdir_in(".").expect("tempdir");
        let (source, coverage) = rust_source_with_lcov(&temp, RUST_SLIM_ORIGINAL);
        RustSlimFixture {
            _guard: guard,
            _temp: temp,
            source,
            coverage,
        }
    }

    fn rust_source_with_lcov(temp: &tempfile::TempDir, text: &str) -> (PathBuf, PathBuf) {
        let source = repo_relative_temp_path(temp, "sample.rs");
        fs::write(&source, text).expect("rust fixture");
        let coverage = repo_relative_temp_path(temp, "coverage.lcov");
        fs::write(
            &coverage,
            format!("TN:\nSF:{}\nDA:2,1\nend_of_record\n", source.display()),
        )
        .expect("coverage fixture");
        (source, coverage)
    }

    #[cfg(feature = "slim-llm")]
    fn assert_fix_auto_mock_applies_verified_rewrite() {
        let applied = rust_slim_fixture();
        let good_mock = repo_relative_temp_path(&applied._temp, "good-response.txt");
        fs::write(&good_mock, "fn unfinished() -> i32 {\n    1\n}\n").expect("good mock");
        let response = call_tool(
            "fix",
            json!({
                "mode": "auto",
                "paths": [applied.source.clone()],
                "mock": good_mock,
                "apply": true,
                "check_cmd": "true",
                "coverage": format!("lcov:{}", applied.coverage.display())
            }),
        )
        .expect("fix auto apply");
        let content = structured_content(&response);
        assert_eq!(content["schema"], "deslop.slim/1");
        assert_eq!(content["dry_run"], false);
        assert_eq!(content["verified"]["results"][0]["verdict"], "removable");
        assert_eq!(content["gating"]["applied"].as_array().unwrap().len(), 1);
        assert_eq!(
            fs::read_to_string(&applied.source).expect("read applied source"),
            "fn unfinished() -> i32 {\n    1\n}"
        );
    }

    #[cfg(feature = "slim-llm")]
    fn assert_fix_auto_mock_blocks_rejected_rewrite() {
        let rejected = rust_slim_fixture();
        let bad_mock = repo_relative_temp_path(&rejected._temp, "bad-response.txt");
        fs::write(&bad_mock, "pub fn added() {}\nfn unfinished() -> i32 {\n").expect("bad mock");
        let response = call_tool(
            "fix",
            json!({
                "mode": "auto",
                "paths": [rejected.source.clone()],
                "mock": bad_mock,
                "apply": true,
                "allow_unverified": true,
                "check_cmd": "true",
                "coverage": "disabled"
            }),
        )
        .expect("fix auto rejected");
        let content = structured_content(&response);
        assert_eq!(content["schema"], "deslop.slim/1");
        assert_eq!(content["verified"]["results"][0]["verdict"], "rejected");
        assert_eq!(content["gating"]["rejected"].as_array().unwrap().len(), 1);
        assert!(content["applied"]["written"].as_array().unwrap().is_empty());
        assert_eq!(
            fs::read_to_string(&rejected.source).expect("read rejected source"),
            RUST_SLIM_ORIGINAL
        );
    }

    fn call_tool(name: &str, arguments: Value) -> Result<Value> {
        handle_request(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            }
        }))
    }

    fn repo_relative_temp_path(temp: &tempfile::TempDir, file: &str) -> PathBuf {
        let cwd = std::env::current_dir().expect("cwd");
        temp.path()
            .strip_prefix(&cwd)
            .unwrap_or(temp.path())
            .join(file)
    }

    fn write_sample_fixture(temp: &tempfile::TempDir) -> PathBuf {
        let path = repo_relative_temp_path(temp, "sample.clj");
        std::fs::write(&path, "(= (count xs) 0)\n").expect("fixture");
        path
    }

    fn temp_test_lock() -> MutexGuard<'static, ()> {
        TEMP_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}
