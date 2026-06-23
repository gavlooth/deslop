use std::io::{BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use deslop_analyzer::{AnalyzerConfig, scan_paths_with_config};
use deslop_metrics::{MetricsConfig, metrics_paths};
use deslop_parse::SourceFile;
use deslop_protocol::{
    CharacterizationTest, Patch, work_orders_for_source, workorder_region_fingerprint,
};
use deslop_report::{render_agent, render_json};
use deslop_verify::{
    CoverageConfig, MutationConfig, VerifyOptions, apply_patches,
    characterization_work_orders_for_patches, verify_characterization_tests, verify_patches,
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
            tool("verify", "Verify deslop.patch/1 patches without writing files.", required_schema(&["patches"], json!({
                    "patches": patches_schema(),
                    "check_cmd": { "type": "string" },
                    "coverage": { "type": "boolean", "default": false },
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
                    "coverage": { "type": "boolean", "default": false },
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
    let reports = scan_reports(args)?;
    let _jsonl = render_agent(&reports)?;
    let mut work_orders = Vec::new();
    for report in reports {
        let source = SourceFile::read(&report.path)?;
        work_orders.extend(work_orders_for_source(&source, &report.findings));
    }
    Ok(json!({
        "schema": "deslop.workorders/1",
        "workorders": work_orders,
    }))
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
        coverage: coverage_config(args),
        mutation: mutation_config(args),
        characterization_tests: characterization_tests_arg(args)?,
        allow_non_removable,
    })
}

fn coverage_config(args: &Value) -> CoverageConfig {
    if bool_arg(args, "coverage") {
        CoverageConfig::Auto
    } else {
        CoverageConfig::Disabled
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
    use std::sync::{Mutex, MutexGuard};

    static TEMP_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct SampleFixture {
        _guard: MutexGuard<'static, ()>,
        _temp: tempfile::TempDir,
        path: PathBuf,
    }

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
                "verify",
                "characterize",
                "verify_characterization",
                "apply",
                "metrics",
                "rules"
            ]
        );
        assert!(tools.iter().all(|tool| tool.get("inputSchema").is_some()));
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
