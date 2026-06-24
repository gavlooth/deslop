use std::io::{BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use deslop_analyzer::{AnalyzerConfig, scan_paths_with_config};
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
    AnthropicClient, OpenAiClient, RecordedClient, SlimOptions, resolve_model, run_slim,
};
use deslop_verify::{
    CoverageConfig, MutationConfig, VerifyOptions, apply_patches,
    characterization_work_orders_for_patches, parse_coverage_mode, verify_characterization_tests,
    verify_patches,
};
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
        "tools": [
            tool("scan", "Scan paths and return deslop.findings/1 JSON.", object_schema(json!({
                    "paths": paths_schema(),
                    "format": { "type": "string", "enum": ["json"], "default": "json" }
            }))),
            tool("propose", "Return deslop.workorder/1 JSONL-compatible work orders.", object_schema(json!({
                    "paths": paths_schema()
            }))),
            tool("fix", "Return deslop-slim rewrite prompts by default (mode=prompts). With deslop-mcp built using --features slim-llm, mode=auto runs deslop-slim server-side and returns deslop.slim/1.", object_schema(json!({
                    "mode": {
                        "type": "string",
                        "enum": ["prompts", "auto"],
                        "default": "prompts",
                        "description": "prompts returns deslop.fix/1 for agent-as-consumer. auto requires deslop-mcp --features slim-llm and runs deslop-slim server-side."
                    },
                    "paths": paths_schema(),
                    "provider": {
                        "type": "string",
                        "enum": ["anthropic", "openai"],
                        "default": "anthropic",
                        "description": "auto mode only; API keys are read from environment variables, never MCP arguments."
                    },
                    "model": {
                        "type": "string",
                        "description": "auto mode only; defaults via DESLOP_SLIM_MODEL or deslop-slim's built-in default."
                    },
                    "base_url": {
                        "type": "string",
                        "description": "auto mode only; OpenAI-compatible base URL."
                    },
                    "apply": { "type": "boolean", "default": false },
                    "allow_unverified": { "type": "boolean", "default": false },
                    "coverage": {
                        "type": "string",
                        "default": "disabled",
                        "description": "auto mode only; disabled, auto, auto:<cmd>, lcov:<path>, cloverage:<path>, julia-cov:<path>, or coverage-py:<path>."
                    },
                    "check_cmd": { "type": "string" },
                    "characterize": { "type": "boolean", "default": false },
                    "mock": {
                        "type": "string",
                        "description": "auto mode only; path to a recorded response for deterministic no-network runs."
                    }
            }))),
            tool("verify", "Verify deslop.patch/1 patches without writing files.", required_schema(&["patches"], json!({
                    "patches": patches_schema(),
                    "check_cmd": { "type": "string" },
                    "coverage": coverage_schema(),
                    "mutation": { "type": "boolean", "default": false },
                    "characterization_tests": characterization_tests_schema()
            }))),
            tool("characterize", "Emit deslop.workorder/1 requests for weak-oracle regions.", required_schema(&["patches"], json!({
                    "patches": patches_schema(),
                    "check_cmd": { "type": "string" },
                    "coverage": { "type": "boolean", "default": false },
                    "mutation": { "type": "boolean", "default": false }
            }))),
            tool("verify_characterization", "Accept generated characterization tests only if they pass current code.", required_schema(&["tests", "check_cmd"], json!({
                    "tests": characterization_tests_schema(),
                    "check_cmd": { "type": "string" }
            }))),
            tool("apply", "Verify and atomically apply deslop.patch/1 patches.", required_schema(&["patches"], json!({
                    "patches": patches_schema(),
                    "check_cmd": { "type": "string" },
                    "coverage": coverage_schema(),
                    "mutation": { "type": "boolean", "default": false },
                    "characterization_tests": characterization_tests_schema(),
                    "allow_non_removable": { "type": "boolean", "default": false },
                    "no_backup": { "type": "boolean", "default": false }
            }))),
            tool("metrics", "Return deslop.metrics/1 JSON with hotspots.", object_schema(json!({
                    "paths": paths_schema(),
                    "sigma": { "type": "number", "default": 2.0 }
            }))),
            tool("rules", "Return the built-in rule catalog.", object_schema(json!({}))),
        ]
    })
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
        "rules" => json!({ "rules": RULES }),
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
    };
    let report = if let Some(path) = optional_string(args, "mock") {
        let client = RecordedClient::from_path(path)?;
        run_slim(&client, options)?
    } else {
        match provider_arg(args)? {
            "anthropic" => {
                let client = AnthropicClient::from_env(model.clone())?;
                run_slim(&client, options)?
            }
            "openai" => {
                let client =
                    OpenAiClient::from_env(model.clone(), optional_string(args, "base_url"))?;
                run_slim(&client, options)?
            }
            other => bail!("unsupported fix provider `{other}`; use `anthropic` or `openai`"),
        }
    };
    Ok(serde_json::to_value(report)?)
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
    scan_paths_with_config(&paths, AnalyzerConfig::default())
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
single-use-binding      risky-suggest           suggest
incompleteness          llm-only                propose
magic-number            risky-suggest           suggest
long-method             llm-only                propose
slop-score              report                  deslop slop
narrating-comment       llm-only                propose
comment-block           llm-only                propose
duplicate-block         llm-only                propose
";

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
        let response = handle_request(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        }))
        .expect("response");
        let tools = response["result"]["tools"].as_array().expect("tools");
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
                "rules"
            ]
        );
        assert!(tools.iter().all(|tool| tool.get("inputSchema").is_some()));
        let verify = tools
            .iter()
            .find(|tool| tool["name"] == "verify")
            .expect("verify tool");
        let coverage = &verify["inputSchema"]["properties"]["coverage"];
        assert_eq!(coverage["default"], false);
        assert!(coverage["anyOf"].as_array().expect("anyOf").len() == 2);
        assert!(
            coverage["description"]
                .as_str()
                .expect("description")
                .contains("lcov:<path>")
        );
        let fix = tools
            .iter()
            .find(|tool| tool["name"] == "fix")
            .expect("fix tool");
        let mode = &fix["inputSchema"]["properties"]["mode"];
        assert_eq!(mode["default"], "prompts");
        assert_eq!(mode["enum"], json!(["prompts", "auto"]));
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
    fn propose_verify_roundtrip_accepts_clean_and_rejects_stale_patch() {
        let fixture = sample_fixture();

        let proposed = call_tool("propose", json!({ "paths": [fixture.path] })).expect("propose");
        let work_order: deslop_protocol::WorkOrder =
            serde_json::from_value(structured_content(&proposed)["workorders"][0].to_owned())
                .expect("workorder");

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

        let proposed =
            call_tool("propose", json!({ "paths": [fixture.path.clone()] })).expect("propose");
        let work_order: deslop_protocol::WorkOrder =
            serde_json::from_value(structured_content(&proposed)["workorders"][0].to_owned())
                .expect("workorder");
        let patch = patch_for_workorder(&work_order, "(empty? xs)\n");

        let absent =
            call_tool("verify", json!({ "patches": [patch.clone()] })).expect("verify absent");
        assert_eq!(first_tool_result(&absent)["verdict"], "coverage-unknown");
        assert!(
            first_tool_result(&absent)["reasons"]
                .as_array()
                .expect("reasons")
                .iter()
                .any(|reason| reason.as_str().unwrap().contains("coverage disabled"))
        );

        let disabled = call_tool(
            "verify",
            json!({
                "patches": [patch.clone()],
                "coverage": false
            }),
        )
        .expect("verify false");
        assert_eq!(first_tool_result(&disabled)["verdict"], "coverage-unknown");
        assert!(
            first_tool_result(&disabled)["reasons"]
                .as_array()
                .expect("reasons")
                .iter()
                .any(|reason| reason.as_str().unwrap().contains("coverage disabled"))
        );

        let enabled = call_tool(
            "verify",
            json!({
                "patches": [patch],
                "coverage": true
            }),
        )
        .expect("verify true");
        assert_eq!(first_tool_result(&enabled)["verdict"], "coverage-unknown");
        assert!(
            first_tool_result(&enabled)["reasons"]
                .as_array()
                .expect("reasons")
                .iter()
                .any(|reason| reason.as_str().unwrap().contains("coverage-unknown"))
        );
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
        let proposed = call_tool("propose", json!({ "paths": [fixture.path] })).expect("propose");
        let work_order: deslop_protocol::WorkOrder =
            serde_json::from_value(structured_content(&proposed)["workorders"][0].to_owned())
                .expect("workorder");
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

        let proposed =
            call_tool("propose", json!({ "paths": [fixture.path.clone()] })).expect("propose");
        let work_order: deslop_protocol::WorkOrder =
            serde_json::from_value(structured_content(&proposed)["workorders"][0].to_owned())
                .expect("workorder");
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
    fn fix_auto_mock_applies_verified_and_blocks_rejected_rewrite() {
        {
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
        let source = repo_relative_temp_path(&temp, "sample.rs");
        fs::write(&source, "fn f() -> i32 {\n    return 1;\n}\n").expect("rust fixture");
        let coverage = repo_relative_temp_path(&temp, "coverage.lcov");
        fs::write(
            &coverage,
            format!("TN:\nSF:{}\nDA:2,1\nend_of_record\n", source.display()),
        )
        .expect("coverage fixture");
        let proposed = call_tool("propose", json!({ "paths": [source.clone()] })).expect("propose");
        let work_order: WorkOrder =
            serde_json::from_value(structured_content(&proposed)["workorders"][0].to_owned())
                .expect("workorder");
        RustCoverageFixture {
            _guard: guard,
            _temp: temp,
            source,
            coverage,
            work_order,
        }
    }

    #[cfg(feature = "slim-llm")]
    fn rust_slim_fixture() -> RustSlimFixture {
        let guard = temp_test_lock();
        let temp = tempfile::tempdir_in(".").expect("tempdir");
        let source = repo_relative_temp_path(&temp, "sample.rs");
        fs::write(&source, RUST_SLIM_ORIGINAL).expect("rust fixture");
        let coverage = repo_relative_temp_path(&temp, "coverage.lcov");
        fs::write(
            &coverage,
            format!("TN:\nSF:{}\nDA:2,1\nend_of_record\n", source.display()),
        )
        .expect("coverage fixture");
        RustSlimFixture {
            _guard: guard,
            _temp: temp,
            source,
            coverage,
        }
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
