use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use deslop_analyzer::scan_paths;
use deslop_parse::SourceFile;
use deslop_protocol::{
    Patch, WorkOrder, WorkOrderKind, work_orders_for_source, workorder_region_fingerprint,
};
use deslop_verify::{
    ApplyReport, CoverageConfig, MutationConfig, VerificationVerdict, VerifyOptions, VerifyReport,
    apply_patches, verify_patches,
};
use serde::{Deserialize, Serialize};

pub const DEFAULT_MODEL: &str = "claude-sonnet-4-6";

pub trait LlmClient {
    fn rewrite(&self, prompt: &SlimPrompt) -> Result<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlimPrompt {
    pub workorder_id: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SlimOptions {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    pub workorders: Option<PathBuf>,
    pub apply: bool,
    pub allow_unverified: bool,
    pub coverage: CoverageConfig,
    pub model: String,
    pub check_cmd: Option<String>,
    pub backup: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlimReport {
    pub schema: String,
    pub dry_run: bool,
    pub model: String,
    pub skipped: Vec<SkippedWorkOrder>,
    pub patches: Vec<Patch>,
    pub verified: VerifyReport,
    pub gating: SlimGatingReport,
    pub applied: Option<ApplyReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedWorkOrder {
    pub workorder_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlimGatingReport {
    pub applied: Vec<SlimPatchStatus>,
    pub held_unproven: Vec<SlimPatchStatus>,
    pub rejected: Vec<SlimPatchStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlimPatchStatus {
    pub workorder_id: String,
    pub path: Option<PathBuf>,
    pub verdict: VerificationVerdict,
    pub reasons: Vec<String>,
    pub suggestion: Option<String>,
}

#[cfg(feature = "anthropic")]
pub struct AnthropicClient {
    model: String,
    api_key: String,
}

#[cfg(feature = "anthropic")]
impl AnthropicClient {
    pub fn from_env(model: impl Into<String>) -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .context("ANTHROPIC_API_KEY is required for deslop-slim Anthropic requests")?;
        Ok(Self {
            model: model.into(),
            api_key,
        })
    }
}

#[cfg(feature = "anthropic")]
impl LlmClient for AnthropicClient {
    fn rewrite(&self, prompt: &SlimPrompt) -> Result<String> {
        let request = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": [
                {
                    "role": "user",
                    "content": prompt.text,
                },
            ],
        });
        let mut response = ureq::post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", self.api_key.as_str())
            .header("anthropic-version", "2023-06-01")
            .send_json(&request)
            .context("Anthropic Messages API request failed")?;
        let body = response
            .body_mut()
            .read_to_string()
            .context("failed to read Anthropic Messages API response")?;
        anthropic_text_response(&body)
    }
}

#[cfg(feature = "openai")]
pub struct OpenAiClient {
    model: String,
    api_key: String,
    base_url: String,
}

#[cfg(feature = "openai")]
impl OpenAiClient {
    pub fn from_env(model: impl Into<String>, base_url: Option<String>) -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .or_else(|_| std::env::var("DESLOP_SLIM_API_KEY"))
            .context("OPENAI_API_KEY or DESLOP_SLIM_API_KEY is required for deslop-slim OpenAI-compatible requests")?;
        Ok(Self {
            model: model.into(),
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
        })
    }

    pub fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

#[cfg(feature = "openai")]
impl LlmClient for OpenAiClient {
    fn rewrite(&self, prompt: &SlimPrompt) -> Result<String> {
        let request = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt.text,
                },
            ],
        });
        let mut response = ureq::post(&self.endpoint())
            .header("authorization", format!("Bearer {}", self.api_key))
            .send_json(&request)
            .context("OpenAI-compatible Chat Completions request failed")?;
        let body = response
            .body_mut()
            .read_to_string()
            .context("failed to read OpenAI-compatible Chat Completions response")?;
        openai_text_response(&body)
    }
}

#[derive(Debug, Clone)]
pub struct RecordedClient {
    response: String,
}

impl RecordedClient {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let response = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Ok(Self::new(response))
    }
}

impl LlmClient for RecordedClient {
    fn rewrite(&self, _prompt: &SlimPrompt) -> Result<String> {
        Ok(self.response.to_owned())
    }
}

pub fn resolve_model(explicit: Option<String>) -> String {
    explicit
        .or_else(|| std::env::var("DESLOP_SLIM_MODEL").ok())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

pub fn run_slim(client: &impl LlmClient, options: SlimOptions) -> Result<SlimReport> {
    let work_orders = load_or_propose_work_orders(&options)?;
    let mut skipped = Vec::new();
    let mut patches = Vec::new();
    for work_order in work_orders {
        if work_order.kind == WorkOrderKind::NeedsCharacterizationTest {
            skipped.push(SkippedWorkOrder {
                workorder_id: work_order.id,
                reason: "needs-characterization-test work orders are not rewrite candidates"
                    .to_string(),
            });
            continue;
        }
        let prompt = build_prompt(&work_order);
        let replacement = strip_code_fences(&client.rewrite(&prompt)?);
        patches.push(Patch {
            schema: "deslop.patch/1".to_string(),
            workorder_id: work_order.id.to_owned(),
            region_fingerprint: workorder_region_fingerprint(&work_order),
            replacement,
            by: format!("deslop-slim/{}", options.model),
        });
    }

    let verify_options = verify_options(&options);
    let verified = verify_patches(&patches, &verify_options)?;
    let gating = gating_report(&verified, options.apply, options.allow_unverified);
    let applied = if options.apply {
        Some(apply_patches(&patches, &verify_options, options.backup)?)
    } else {
        None
    };

    Ok(SlimReport {
        schema: "deslop.slim/1".to_string(),
        dry_run: !options.apply,
        model: options.model,
        skipped,
        patches,
        verified,
        gating,
        applied,
    })
}

pub fn build_prompt(work_order: &WorkOrder) -> SlimPrompt {
    let findings = work_order
        .findings
        .iter()
        .map(|finding| {
            format!(
                "- rule: {}\n  severity: {:?}\n  message: {}\n  precondition: {}",
                finding.rule,
                finding.severity,
                finding.message,
                finding.precondition.as_deref().unwrap_or("none")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let contract = format!(
        "- must parse: {}\n- no new public definitions: {}\n- keep error handling/assertions: {}\n- max growth ratio: {}\n- check command: {}",
        work_order.contract.must_parse,
        work_order.contract.no_new_public_defs,
        work_order.contract.keep_error_handling,
        work_order.contract.max_growth_ratio,
        work_order.contract.check_cmd.as_deref().unwrap_or("none")
    );
    SlimPrompt {
        workorder_id: work_order.id.to_owned(),
        text: format!(
            "You are deslop-slim. Rewrite exactly the target region to remove the flagged bloat while preserving behavior.\n\nReturn only the replacement text for the region. Do not return markdown fences, JSON, explanations, or surrounding file text.\n\nInstruction:\n{}\n\nPath: {}\nLines: {}-{}\n\nFindings:\n{}\n\nContract:\n{}\n\nTarget region:\n<<<DESLOP_REGION\n{}>>>",
            work_order.instruction,
            work_order.path.display(),
            work_order.region.start_line,
            work_order.region.end_line,
            findings,
            contract,
            work_order.region.text
        ),
    }
}

pub fn strip_code_fences(text: &str) -> String {
    let trimmed = text.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }
    let mut lines = trimmed.lines();
    let _opening = lines.next();
    let mut body = lines.collect::<Vec<_>>();
    if body.last().is_some_and(|line| line.trim() == "```") {
        body.pop();
    }
    body.join("\n").trim().to_string()
}

fn verify_options(options: &SlimOptions) -> VerifyOptions {
    VerifyOptions {
        root: options.root.to_owned(),
        check_cmd: options.check_cmd.to_owned(),
        coverage: options.coverage.clone(),
        mutation: MutationConfig::Disabled,
        characterization_tests: Vec::new(),
        allow_non_removable: options.allow_unverified,
    }
}

fn gating_report(
    verified: &VerifyReport,
    applying: bool,
    allow_unverified: bool,
) -> SlimGatingReport {
    let mut report = SlimGatingReport::default();
    for result in &verified.results {
        let status = SlimPatchStatus {
            workorder_id: result.workorder_id.to_owned(),
            path: result.path.to_owned(),
            verdict: result.verdict,
            reasons: result.reasons.clone(),
            suggestion: gating_suggestion(result.verdict),
        };
        if !result.passed || result.verdict == VerificationVerdict::Rejected {
            report.rejected.push(status);
        } else if applying && (result.verdict == VerificationVerdict::Removable || allow_unverified)
        {
            report.applied.push(status);
        } else if result.verdict != VerificationVerdict::Removable {
            report.held_unproven.push(status);
        }
    }
    report
}

fn gating_suggestion(verdict: VerificationVerdict) -> Option<String> {
    match verdict {
        VerificationVerdict::CoverageUnknown
        | VerificationVerdict::UntestedRisky
        | VerificationVerdict::DeadCandidate => Some(
            "pass --coverage, add characterization tests, or use --allow-unverified".to_string(),
        ),
        VerificationVerdict::Rejected => Some("fix the rewrite and rerun deslop fix".to_string()),
        VerificationVerdict::Removable => None,
    }
}

fn load_or_propose_work_orders(options: &SlimOptions) -> Result<Vec<WorkOrder>> {
    if let Some(path) = &options.workorders {
        return load_workorders_jsonl(path);
    }
    propose_work_orders(&options.paths)
}

pub fn propose_work_orders(paths: &[PathBuf]) -> Result<Vec<WorkOrder>> {
    let reports = scan_paths(paths)?;
    let mut work_orders = Vec::new();
    for report in reports {
        let source = SourceFile::read(&report.path)?;
        work_orders.extend(work_orders_for_source(&source, &report.findings));
    }
    Ok(work_orders)
}

pub fn load_workorders_jsonl(path: &Path) -> Result<Vec<WorkOrder>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    parse_workorders_jsonl(&text)
}

pub fn parse_workorders_jsonl(text: &str) -> Result<Vec<WorkOrder>> {
    let mut records = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let work_order: WorkOrder = serde_json::from_str(line)
            .with_context(|| format!("failed to parse workorder JSONL line {}", idx + 1))?;
        if work_order.schema != "deslop.workorder/1" {
            bail!(
                "line {} has unsupported schema `{}`",
                idx + 1,
                work_order.schema
            );
        }
        records.push(work_order);
    }
    Ok(records)
}

#[cfg(feature = "anthropic")]
fn anthropic_text_response(body: &str) -> Result<String> {
    let value: serde_json::Value =
        serde_json::from_str(body).context("failed to parse Anthropic response JSON")?;
    let content = value
        .get("content")
        .and_then(serde_json::Value::as_array)
        .context("Anthropic response did not include content array")?;
    let text = content
        .iter()
        .filter(|block| block.get("type").and_then(serde_json::Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>()
        .join("");
    if text.is_empty() {
        bail!("Anthropic response did not include a text block");
    }
    Ok(strip_code_fences(&text))
}

#[cfg(feature = "openai")]
fn openai_text_response(body: &str) -> Result<String> {
    let value: serde_json::Value =
        serde_json::from_str(body).context("failed to parse OpenAI-compatible response JSON")?;
    let content = value
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(serde_json::Value::as_str)
        .context("OpenAI-compatible response did not include choices[0].message.content")?;
    Ok(strip_code_fences(content))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use deslop_protocol::{Contract, Region, WorkOrderFinding};
    use deslop_verify::VerificationVerdict;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn prompt_contains_region_finding_and_contract() {
        let work_order = WorkOrder {
            schema: "deslop.workorder/1".to_string(),
            kind: WorkOrderKind::RewriteRegion,
            id: "wo_prompt".to_string(),
            path: PathBuf::from("sample.rs"),
            region: Region {
                start_line: 1,
                end_line: 3,
                text: "fn sample() {\n    return;\n}\n".to_string(),
            },
            findings: vec![WorkOrderFinding {
                rule: "needless-return".to_string(),
                severity: deslop_core::Severity::Minor,
                safety: deslop_core::SafetyClass::LlmOnly,
                message: "unneeded return".to_string(),
                precondition: Some("preserve early returns".to_string()),
            }],
            instruction: "Rewrite without changing behavior.".to_string(),
            contract: Contract::default(),
        };

        let prompt = build_prompt(&work_order);

        assert!(prompt.text.contains("fn sample()"));
        assert!(prompt.text.contains("unneeded return"));
        assert!(prompt.text.contains("must parse: true"));
        assert!(prompt.text.contains("no new public definitions: true"));
    }

    #[test]
    fn recorded_client_e2e_applies_verified_rewrite() -> Result<()> {
        let temp = TempDir::new()?;
        let source = temp.path().join("sample.rs");
        fs::write(
            &source,
            "fn identity(value: i32) -> i32 {\n    return value;\n}\n",
        )?;
        let coverage = lcov_fixture(temp.path(), "coverage.lcov", &source, 2, 1);
        let client = RecordedClient::new("fn identity(value: i32) -> i32 {\n    value\n}\n");
        let report = run_slim(
            &client,
            SlimOptions {
                root: temp.path().to_path_buf(),
                paths: vec![temp.path().to_path_buf()],
                workorders: None,
                apply: true,
                allow_unverified: false,
                coverage: CoverageConfig::LcovFile(coverage),
                model: "recorded".to_string(),
                check_cmd: Some("true".to_string()),
                backup: false,
            },
        )?;

        assert_eq!(report.schema, "deslop.slim/1");
        assert_eq!(report.patches.len(), 1);
        assert_eq!(report.patches[0].schema, "deslop.patch/1");
        assert_eq!(report.patches[0].by, "deslop-slim/recorded");
        assert!(report.verified.results[0].passed);
        assert_eq!(
            report.verified.results[0].verdict,
            VerificationVerdict::Removable
        );
        assert_eq!(report.gating.applied.len(), 1);
        assert!(report.gating.held_unproven.is_empty());
        assert!(report.gating.rejected.is_empty());
        assert_eq!(
            report.applied.as_ref().expect("apply report").written,
            vec![source.clone()]
        );
        assert_eq!(
            fs::read_to_string(&source)?,
            "fn identity(value: i32) -> i32 {\n    value\n}"
        );
        Ok(())
    }

    #[test]
    fn default_apply_holds_unproven_coverage_unknown_rewrite() -> Result<()> {
        let temp = TempDir::new()?;
        let source = temp.path().join("sample.rs");
        let original = "fn identity(value: i32) -> i32 {\n    return value;\n}\n";
        fs::write(&source, original)?;
        let client = RecordedClient::new("fn identity(value: i32) -> i32 {\n    value\n}\n");
        let report = run_slim(
            &client,
            SlimOptions {
                root: temp.path().to_path_buf(),
                paths: vec![temp.path().to_path_buf()],
                workorders: None,
                apply: true,
                allow_unverified: false,
                coverage: CoverageConfig::Disabled,
                model: "recorded".to_string(),
                check_cmd: Some("true".to_string()),
                backup: false,
            },
        )?;

        assert!(report.verified.results[0].passed);
        assert_eq!(
            report.verified.results[0].verdict,
            VerificationVerdict::CoverageUnknown
        );
        assert!(report.gating.applied.is_empty());
        assert_eq!(report.gating.held_unproven.len(), 1);
        assert!(report.gating.rejected.is_empty());
        assert!(
            report
                .applied
                .as_ref()
                .expect("apply report")
                .written
                .is_empty()
        );
        assert_eq!(fs::read_to_string(&source)?, original);
        Ok(())
    }

    #[test]
    fn allow_unverified_applies_coverage_unknown_rewrite() -> Result<()> {
        let temp = TempDir::new()?;
        let source = temp.path().join("sample.rs");
        fs::write(
            &source,
            "fn identity(value: i32) -> i32 {\n    return value;\n}\n",
        )?;
        let client = RecordedClient::new("fn identity(value: i32) -> i32 {\n    value\n}\n");
        let report = run_slim(
            &client,
            SlimOptions {
                root: temp.path().to_path_buf(),
                paths: vec![temp.path().to_path_buf()],
                workorders: None,
                apply: true,
                allow_unverified: true,
                coverage: CoverageConfig::Disabled,
                model: "recorded".to_string(),
                check_cmd: Some("true".to_string()),
                backup: false,
            },
        )?;

        assert!(report.verified.results[0].passed);
        assert_eq!(
            report.verified.results[0].verdict,
            VerificationVerdict::CoverageUnknown
        );
        assert_eq!(report.gating.applied.len(), 1);
        assert!(report.gating.held_unproven.is_empty());
        assert!(report.gating.rejected.is_empty());
        assert_eq!(
            report.applied.as_ref().expect("apply report").written,
            vec![source.clone()]
        );
        assert_eq!(
            fs::read_to_string(&source)?,
            "fn identity(value: i32) -> i32 {\n    value\n}"
        );
        Ok(())
    }

    #[test]
    fn verify_rejects_bad_rewrite_and_apply_leaves_file_unchanged() -> Result<()> {
        assert_bad_rewrite_blocked(false)?;
        assert_bad_rewrite_blocked(true)
    }

    fn assert_bad_rewrite_blocked(allow_unverified: bool) -> Result<()> {
        let temp = TempDir::new()?;
        let source = temp.path().join("sample.rs");
        let original = "fn identity(value: i32) -> i32 {\n    return value;\n}\n";
        fs::write(&source, original)?;
        let client = RecordedClient::new("pub fn added() {}\nfn identity(value: i32) -> i32 {\n");
        let report = run_slim(
            &client,
            SlimOptions {
                root: temp.path().to_path_buf(),
                paths: vec![temp.path().to_path_buf()],
                workorders: None,
                apply: true,
                allow_unverified,
                coverage: CoverageConfig::Disabled,
                model: "recorded".to_string(),
                check_cmd: Some("true".to_string()),
                backup: false,
            },
        )?;

        assert!(!report.verified.results[0].passed);
        assert_eq!(
            report.verified.results[0].verdict,
            VerificationVerdict::Rejected
        );
        assert!(report.gating.applied.is_empty());
        assert!(report.gating.held_unproven.is_empty());
        assert_eq!(report.gating.rejected.len(), 1);
        assert!(
            report
                .applied
                .as_ref()
                .expect("apply report")
                .written
                .is_empty()
        );
        assert_eq!(fs::read_to_string(&source)?, original);
        Ok(())
    }

    #[cfg(feature = "anthropic")]
    #[test]
    fn anthropic_response_extracts_text_block_and_strips_fences() -> Result<()> {
        let body = r#"{"content":[{"type":"text","text":"```rust\nfn f() {}\n```"}]}"#;

        assert_eq!(anthropic_text_response(body)?, "fn f() {}");
        Ok(())
    }

    #[cfg(feature = "openai")]
    #[test]
    fn openai_response_extracts_message_content_and_strips_fences() -> Result<()> {
        let body = r#"{"choices":[{"message":{"content":"```rust\nfn f() {}\n```"}}]}"#;

        assert_eq!(openai_text_response(body)?, "fn f() {}");
        Ok(())
    }

    #[cfg(feature = "openai")]
    #[test]
    fn openai_endpoint_joins_base_url_without_double_slash() {
        let client = OpenAiClient {
            model: "model".to_string(),
            api_key: "key".to_string(),
            base_url: "http://localhost:11434/v1/".to_string(),
        };

        assert_eq!(
            client.endpoint(),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    fn lcov_fixture(root: &Path, name: &str, source: &Path, line: usize, count: usize) -> PathBuf {
        let path = root.join(name);
        fs::write(
            &path,
            format!(
                "TN:\nSF:{}\nDA:{line},{count}\nend_of_record\n",
                source.display()
            ),
        )
        .expect("write lcov fixture");
        path
    }
}
