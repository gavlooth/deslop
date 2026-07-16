//! MCP tool specifications: names, descriptions, input schemas, and behavior annotations.
//!
//! Descriptions follow one contract: state read/write behavior and network behavior
//! explicitly, name the output schema, and keep the rest to a sentence plus critical
//! constraints. Annotations carry the same facts as machine-readable hints.

use serde_json::{Value, json};

pub(crate) fn tool_definitions() -> Vec<Value> {
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

/// Machine-readable behavior hints attached to each tool (MCP tool annotations).
struct ToolBehavior {
    title: &'static str,
    read_only: bool,
    destructive: bool,
    idempotent: bool,
    open_world: bool,
}

impl ToolBehavior {
    fn read_only(title: &'static str) -> Self {
        Self {
            title,
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
        }
    }

    fn writes_files(title: &'static str) -> Self {
        Self {
            title,
            read_only: false,
            destructive: true,
            idempotent: false,
            open_world: false,
        }
    }

    fn annotations(&self) -> Value {
        json!({
            "title": self.title,
            "readOnlyHint": self.read_only,
            "destructiveHint": self.destructive,
            "idempotentHint": self.idempotent,
            "openWorldHint": self.open_world,
        })
    }
}

fn scan_tool_spec() -> Value {
    tool(
        "scan",
        "Read-only. Scan paths and return deslop.findings/2 JSON: per-file findings with rule, severity, safety class, span, analysis status, and any deterministic edit. Start here to see what deslop would change. No writes, no network. Unknown rule names in analyzer overrides are rejected.",
        ToolBehavior::read_only("Scan for findings"),
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
        "Read-only. Return shared deslop.work-order/1 transactions with exact revision guards, access sets, budgets, provenance, and self-contained proposal context for proposal-eligible findings. safe-auto uses deterministic fixes; never-auto remains report-only and never enters a work order or prompt. No writes, no network.",
        ToolBehavior::read_only("Propose rewrite work orders"),
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
        "mode=prompts (default): read-only, no network; returns deslop.fix/3 rewrite prompts plus exact proposal context for the calling agent to copy into patches. mode=auto: requires a --features slim-llm build; runs deslop-slim server-side, sending flagged source regions to the LLM provider (network egress, explicit consent required) and writing verified rewrites only when apply=true. Returns deslop.slim/4.",
        ToolBehavior {
            title: "Rewrite prompts or server-side fix",
            read_only: false,
            destructive: true,
            idempotent: false,
            open_world: true,
        },
        object_schema(fix_tool_properties()),
    )
}

fn fix_tool_properties() -> Value {
    json!({
        "mode": {
            "type": "string",
            "enum": ["prompts", "auto"],
            "default": "prompts",
            "description": "prompts returns deslop.fix/3 for agent-as-consumer. auto requires deslop-mcp --features slim-llm and runs deslop-slim server-side."
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
        "Read-only for the workspace. Verify deslop.patch/3 patches from their persisted proposal context in a temporary project copy and return deslop.verify/1 verdicts: removable, coverage-unknown, untested-risky, dead-candidate, or rejected. check_cmd (if set) runs in the temp copy, never in the workspace. Without coverage input, passing patches come back coverage-unknown, which `apply` will not write by default.",
        ToolBehavior::read_only("Verify patches (dry run)"),
        required_schema(&["patches"], patch_verification_properties()),
    )
}

fn characterize_tool_spec() -> Value {
    tool(
        "characterize",
        "Read-only. Verify patches, then return deslop.workorder/3 characterization-test requests for weak-oracle regions that passed verification. Use when coverage is unavailable and a behavior-pinning test is needed before apply.",
        ToolBehavior::read_only("Request characterization tests"),
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
        "Read-only for the workspace. Run generated characterization tests against the current, unmodified code in a temporary project copy via check_cmd; accepts only tests that compile and pass. Returns deslop.characterization/1. Fails without check_cmd.",
        ToolBehavior::read_only("Verify characterization tests"),
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
        "WRITES FILES. Verify deslop.patch/3 patches and atomically apply the ones that pass. Default writes only verifier-Removable patches; allow_non_removable widens to any non-rejected verdict. Backups (*.bak) are kept unless no_backup. No network. Returns deslop.apply/1 with verify results and written paths.",
        ToolBehavior::writes_files("Verify and apply patches"),
        required_schema(&["patches"], properties),
    )
}

fn patch_verification_properties() -> Value {
    json!({
        "patches": patches_schema(),
        "root": string_schema("Project root for work-order rediscovery and verification."),
        "scope": paths_schema(),
        "check_cmd": { "type": "string" },
        "coverage": coverage_schema(),
        "mutation": { "type": "boolean", "default": false },
        "characterization_tests": characterization_tests_schema()
    })
}

fn metrics_tool_spec() -> Value {
    tool(
        "metrics",
        "Read-only. Return deslop.metrics/5 with per-region structural measurements, experimental heuristic burden, scan-local burden outliers, and complexity/entropy hotspots. Burden and outliers are triage evidence only: they are not health, readability, refactor need, probability, confidence, or safety. No writes, no network.",
        ToolBehavior::read_only("Structural measurements and scan-local triage outliers"),
        object_schema(json!({
            "paths": paths_schema(),
            "sigma": { "type": "number", "default": 2.0 }
        })),
    )
}

fn graph_tool_spec() -> Value {
    tool(
        "graph",
        "Read-only. Return deslop.graph/2 JSON for refactor planning. Contains edges are resolved syntax ownership. Calls/imports/inherits are syntactic planning hints or ambiguous evidence; a syntactic external-symbol target is unresolved, not proven external, and syntactic is not resolution proof. No writes, no network.",
        ToolBehavior::read_only("Refactor dependency graph"),
        object_schema(json!({
            "paths": paths_schema(),
            "include_calls": { "type": "boolean", "default": true }
        })),
    )
}

fn rules_tool_spec() -> Value {
    tool(
        "rules",
        "Read-only. Return the built-in rule catalog: rule name, safety class, and default action. No writes, no network.",
        ToolBehavior::read_only("Rule catalog"),
        object_schema(json!({})),
    )
}

fn tool(name: &str, description: &str, behavior: ToolBehavior, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "annotations": behavior.annotations(),
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
        "items": { "$ref": "#/$defs/deslop.patch/3" },
        "$defs": {
            "deslop.patch/3": {
                "type": "object",
                "required": ["schema", "workorder_id", "revision_guard", "proposal_context", "replacement", "by"],
                "properties": {
                    "schema": { "const": "deslop.patch/3" },
                    "workorder_id": { "type": "string" },
                    "revision_guard": { "type": "string", "pattern": "^rg1_[0-9]+_[0-9a-f]{64}$" },
                    "proposal_context": { "$ref": "#/$defs/deslop.proposal-context/1" },
                    "replacement": { "type": "string" },
                    "by": { "type": "string" }
                },
                "additionalProperties": false
            },
            "deslop.proposal-context/1": proposal_context_schema()
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
        "items": { "$ref": "#/$defs/deslop.characterization-test/3" },
        "default": [],
        "$defs": {
            "deslop.characterization-test/3": {
                "type": "object",
                "required": ["schema", "workorder_id", "revision_guard", "proposal_context", "test_path", "test_text", "by"],
                "properties": {
                    "schema": { "const": "deslop.characterization-test/3" },
                    "workorder_id": { "type": "string" },
                    "revision_guard": { "type": "string", "pattern": "^rg1_[0-9]+_[0-9a-f]{64}$" },
                    "proposal_context": { "$ref": "#/$defs/deslop.proposal-context/1" },
                    "test_path": { "type": "string" },
                    "test_text": { "type": "string" },
                    "by": { "type": "string" }
                },
                "additionalProperties": false
            },
            "deslop.proposal-context/1": proposal_context_schema()
        }
    })
}

fn proposal_context_schema() -> Value {
    json!({
        "type": "object",
        "required": [
            "schema", "analyzer_semantics", "context_id", "requested_scope", "analyzer",
            "excluded_fingerprints", "sources", "external_capabilities", "workorder_set_digest"
        ],
        "properties": {
            "schema": { "const": "deslop.proposal-context/1" },
            "analyzer_semantics": { "const": "deslop-analyzer/2" },
            "context_id": { "type": "string", "pattern": "^pc1_[0-9a-f]{64}$" },
            "requested_scope": { "type": "array" },
            "analyzer": { "type": "object" },
            "excluded_fingerprints": { "type": "array", "items": { "type": "string" } },
            "sources": { "type": "array" },
            "external_capabilities": { "type": "array" },
            "workorder_set_digest": { "type": "string", "pattern": "^dg1_[0-9a-f]{64}$" }
        },
        "additionalProperties": false
    })
}
