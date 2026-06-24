use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use deslop_analyzer::{AnalyzerConfig, scan_paths_with_config};
use deslop_parse::SourceFile;
use deslop_protocol::{
    CharacterizationTest, Patch, WorkOrder, WorkOrderKind, work_orders_for_source,
    workorder_region_fingerprint,
};
use deslop_verify::{
    ApplyReport, CoverageConfig, MutationConfig, VerificationVerdict, VerifyOptions, VerifyReport,
    apply_patches, characterization_work_orders_for_patches, verify_characterization_tests,
    verify_patches,
};
use serde::{Deserialize, Serialize};

pub const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1/messages";
pub const OPENAI_DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

pub trait LlmClient {
    fn rewrite(&self, prompt: &SlimPrompt) -> Result<String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlimPromptKind {
    Rewrite,
    Characterization,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlimPrompt {
    pub kind: SlimPromptKind,
    pub workorder_id: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SlimOptions {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    pub workorders: Option<PathBuf>,
    pub apply: bool,
    pub characterize: bool,
    pub allow_unverified: bool,
    pub coverage: CoverageConfig,
    pub model: String,
    pub check_cmd: Option<String>,
    pub backup: bool,
    /// Analyzer config (thresholds + suppression) used when auto mode scans paths itself.
    /// Ignored when `workorders` provides a precomputed work-order file.
    pub analyzer: AnalyzerConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlimProgress {
    Started {
        work_orders: usize,
    },
    Rewriting {
        index: usize,
        total: usize,
        workorder_id: String,
        path: PathBuf,
        start_line: usize,
        end_line: usize,
    },
    Characterizing {
        workorder_id: String,
    },
    Verified {
        workorder_id: String,
        verdict: VerificationVerdict,
    },
    Outcome {
        workorder_id: String,
        outcome: SlimProgressOutcome,
    },
    Finished {
        applied: usize,
        held: usize,
        rejected: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlimProgressOutcome {
    Applied,
    Held,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressDecision {
    Granted,
    Prompt,
    DeniedNonInteractive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressSummary {
    pub file_count: usize,
    pub region_count: usize,
}

pub fn resolve_egress_consent(explicit: bool, is_interactive: bool) -> EgressDecision {
    if explicit {
        EgressDecision::Granted
    } else if is_interactive {
        EgressDecision::Prompt
    } else {
        EgressDecision::DeniedNonInteractive
    }
}

pub fn env_egress_consent(value: Option<String>) -> bool {
    value.as_deref().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "y"
        )
    })
}

pub fn provider_base_url(provider: &str, base_url: Option<&str>) -> String {
    match provider {
        "anthropic" => ANTHROPIC_BASE_URL.to_string(),
        "openai" => base_url.unwrap_or(OPENAI_DEFAULT_BASE_URL).to_string(),
        _ => base_url.unwrap_or("unknown").to_string(),
    }
}

pub fn egress_prompt_message(provider: &str, base_url: &str, summary: EgressSummary) -> String {
    format!(
        "deslop will send code regions from {} file(s), {} region(s), to {} ({}). Continue? [y/N]",
        summary.file_count, summary.region_count, provider, base_url
    )
}

pub fn egress_consent_error(provider: &str, base_url: &str, summary: EgressSummary) -> String {
    format!(
        "{} Refusing to call the real LLM provider without source-egress consent. Pass --yes/--consent, set DESLOP_SLIM_CONSENT=1, or set [slim] egress_consent = true in deslop.toml.",
        egress_prompt_message(provider, base_url, summary)
    )
}

pub fn egress_summary(options: &SlimOptions) -> Result<EgressSummary> {
    let work_orders = load_or_propose_work_orders(options)?;
    let rewrite_orders = work_orders
        .into_iter()
        .filter(|work_order| work_order.kind != WorkOrderKind::NeedsCharacterizationTest)
        .collect::<Vec<_>>();
    let files = rewrite_orders
        .iter()
        .map(|work_order| work_order.path.to_path_buf())
        .collect::<BTreeSet<_>>();
    Ok(EgressSummary {
        file_count: files.len(),
        region_count: rewrite_orders.len(),
    })
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
    pub characterization: Option<SlimCharacterizationReport>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlimCharacterizationReport {
    pub attempts: Vec<SlimCharacterizationAttempt>,
    pub accepted: Vec<SlimCharacterizationAttempt>,
    pub rejected: Vec<SlimCharacterizationAttempt>,
    pub upgrades: Vec<SlimVerdictUpgrade>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlimCharacterizationAttempt {
    pub workorder_id: String,
    pub test_path: PathBuf,
    pub accepted: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlimVerdictUpgrade {
    pub workorder_id: String,
    pub before: VerificationVerdict,
    pub after: VerificationVerdict,
    pub applied_after_characterization: bool,
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
    let mut progress = |_| {};
    run_slim_with_progress(client, options, &mut progress)
}

pub fn run_slim_with_progress(
    client: &impl LlmClient,
    options: SlimOptions,
    progress: &mut dyn FnMut(SlimProgress),
) -> Result<SlimReport> {
    let work_orders = load_or_propose_work_orders(&options)?;
    emit_started_progress(&work_orders, progress);

    let rewrite = rewrite_work_orders(client, &options, work_orders, progress)?;
    let verification = verify_rewrites(client, &options, &rewrite.patches, progress)?;
    let gating = gating_report(
        &verification.verified,
        options.apply,
        options.allow_unverified,
    );
    let applied = apply_verified_patches(&options, &rewrite, &verification)?;
    emit_outcome_progress(
        &verification.verified,
        options.apply,
        options.allow_unverified,
        progress,
    );
    Ok(finish_slim_report(
        options,
        rewrite,
        verification,
        gating,
        applied,
    ))
}

struct SlimRewriteRun {
    skipped: Vec<SkippedWorkOrder>,
    patches: Vec<Patch>,
}

struct SlimVerificationRun {
    initial_verified: VerifyReport,
    final_verify_options: VerifyOptions,
    verified: VerifyReport,
    characterization: Option<CharacterizationRun>,
}

fn rewrite_candidate_count(work_orders: &[WorkOrder]) -> usize {
    work_orders
        .iter()
        .filter(|work_order| work_order.kind != WorkOrderKind::NeedsCharacterizationTest)
        .count()
}

fn emit_started_progress(work_orders: &[WorkOrder], progress: &mut dyn FnMut(SlimProgress)) {
    progress(SlimProgress::Started {
        work_orders: rewrite_candidate_count(work_orders),
    });
}

fn rewrite_work_orders(
    client: &impl LlmClient,
    options: &SlimOptions,
    work_orders: Vec<WorkOrder>,
    progress: &mut dyn FnMut(SlimProgress),
) -> Result<SlimRewriteRun> {
    let total_rewrites = rewrite_candidate_count(&work_orders);
    let mut skipped = Vec::new();
    let mut patches = Vec::new();
    let mut rewrite_index = 0;
    for work_order in work_orders {
        if work_order.kind == WorkOrderKind::NeedsCharacterizationTest {
            skipped.push(SkippedWorkOrder {
                workorder_id: work_order.id,
                reason: "needs-characterization-test work orders are not rewrite candidates"
                    .to_string(),
            });
            continue;
        }

        rewrite_index += 1;
        emit_rewrite_progress(rewrite_index, total_rewrites, &work_order, progress);
        patches.push(rewrite_patch(client, options, &work_order)?);
    }
    Ok(SlimRewriteRun { skipped, patches })
}

fn emit_rewrite_progress(
    index: usize,
    total: usize,
    work_order: &WorkOrder,
    progress: &mut dyn FnMut(SlimProgress),
) {
    progress(SlimProgress::Rewriting {
        index,
        total,
        workorder_id: work_order.id.to_owned(),
        path: work_order.path.to_owned(),
        start_line: work_order.region.start_line,
        end_line: work_order.region.end_line,
    });
}

fn rewrite_patch(
    client: &impl LlmClient,
    options: &SlimOptions,
    work_order: &WorkOrder,
) -> Result<Patch> {
    let prompt = build_prompt(work_order);
    let replacement = strip_code_fences(&client.rewrite(&prompt)?);
    Ok(Patch {
        schema: "deslop.patch/1".to_string(),
        workorder_id: work_order.id.to_owned(),
        region_fingerprint: workorder_region_fingerprint(work_order),
        replacement,
        by: format!("deslop-slim/{}", options.model),
    })
}

fn verify_rewrites(
    client: &impl LlmClient,
    options: &SlimOptions,
    patches: &[Patch],
    progress: &mut dyn FnMut(SlimProgress),
) -> Result<SlimVerificationRun> {
    let verify_options = verify_options(options);
    let initial_verified = verify_patches(patches, &verify_options)?;
    let characterization =
        maybe_characterize_rewrites(client, options, patches, &verify_options, progress)?;
    let accepted_tests = characterization
        .as_ref()
        .map(|report| report.accepted_tests.clone())
        .unwrap_or_default();
    let final_verify_options = verify_options_with_characterization(options, accepted_tests);
    let verified = if options.characterize {
        verify_patches(patches, &final_verify_options)?
    } else {
        initial_verified.clone()
    };
    emit_verified_progress(&verified, progress);
    Ok(SlimVerificationRun {
        initial_verified,
        final_verify_options,
        verified,
        characterization,
    })
}

fn maybe_characterize_rewrites(
    client: &impl LlmClient,
    options: &SlimOptions,
    patches: &[Patch],
    verify_options: &VerifyOptions,
    progress: &mut dyn FnMut(SlimProgress),
) -> Result<Option<CharacterizationRun>> {
    if !options.characterize {
        return Ok(None);
    }
    Ok(Some(run_characterization_pass(
        client,
        options,
        patches,
        verify_options,
        progress,
    )?))
}

fn emit_verified_progress(verified: &VerifyReport, progress: &mut dyn FnMut(SlimProgress)) {
    for result in &verified.results {
        progress(SlimProgress::Verified {
            workorder_id: result.workorder_id.to_owned(),
            verdict: result.verdict,
        });
    }
}

fn emit_outcome_progress(
    verified: &VerifyReport,
    applying: bool,
    allow_unverified: bool,
    progress: &mut dyn FnMut(SlimProgress),
) {
    let progress_outcomes = progress_outcomes(verified, applying, allow_unverified);
    for (workorder_id, outcome) in &progress_outcomes {
        progress(SlimProgress::Outcome {
            workorder_id: workorder_id.to_owned(),
            outcome: *outcome,
        });
    }
    let (applied, held, rejected) = progress_outcome_counts(&progress_outcomes);
    progress(SlimProgress::Finished {
        applied,
        held,
        rejected,
    });
}

fn apply_verified_patches(
    options: &SlimOptions,
    rewrite: &SlimRewriteRun,
    verification: &SlimVerificationRun,
) -> Result<Option<ApplyReport>> {
    if !options.apply {
        return Ok(None);
    }
    apply_patches(
        &rewrite.patches,
        &verification.final_verify_options,
        options.backup,
    )
    .map(Some)
}

fn public_characterization_report(
    characterization: Option<CharacterizationRun>,
    initial_verified: &VerifyReport,
    verified: &VerifyReport,
    applying: bool,
) -> Option<SlimCharacterizationReport> {
    characterization.map(|report| report.into_public_report(initial_verified, verified, applying))
}

fn finish_slim_report(
    options: SlimOptions,
    rewrite: SlimRewriteRun,
    verification: SlimVerificationRun,
    gating: SlimGatingReport,
    applied: Option<ApplyReport>,
) -> SlimReport {
    let characterization = public_characterization_report(
        verification.characterization,
        &verification.initial_verified,
        &verification.verified,
        options.apply,
    );
    SlimReport {
        schema: "deslop.slim/1".to_string(),
        dry_run: !options.apply,
        model: options.model,
        skipped: rewrite.skipped,
        patches: rewrite.patches,
        verified: verification.verified,
        gating,
        characterization,
        applied,
    }
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
        kind: SlimPromptKind::Rewrite,
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

pub fn build_characterization_prompt(work_order: &WorkOrder) -> SlimPrompt {
    SlimPrompt {
        kind: SlimPromptKind::Characterization,
        workorder_id: work_order.id.to_owned(),
        text: format!(
            "You are deslop-slim. Write a characterization test that pins the CURRENT observable behavior of the target region before any rewrite is applied.\n\nReturn only the test source text. Do not return markdown fences, JSON, explanations, or production code changes.\n\nInstruction:\n{}\n\nPath: {}\nLines: {}-{}\n\nCurrent target region:\n<<<DESLOP_REGION\n{}>>>\n\nThe test must pass against the current unmodified code and should fail if the rewrite changes observable behavior.",
            work_order.instruction,
            work_order.path.display(),
            work_order.region.start_line,
            work_order.region.end_line,
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
    verify_options_with_characterization(options, Vec::new())
}

fn verify_options_with_characterization(
    options: &SlimOptions,
    characterization_tests: Vec<CharacterizationTest>,
) -> VerifyOptions {
    VerifyOptions {
        root: options.root.to_owned(),
        check_cmd: options.check_cmd.to_owned(),
        coverage: options.coverage.clone(),
        mutation: MutationConfig::Disabled,
        characterization_tests,
        allow_non_removable: options.allow_unverified,
    }
}

struct CharacterizationRun {
    accepted_tests: Vec<CharacterizationTest>,
    attempts: Vec<SlimCharacterizationAttempt>,
}

impl CharacterizationRun {
    fn into_public_report(
        self,
        before: &VerifyReport,
        after: &VerifyReport,
        applying: bool,
    ) -> SlimCharacterizationReport {
        let accepted = self
            .attempts
            .iter()
            .filter(|attempt| attempt.accepted)
            .cloned()
            .collect();
        let rejected = self
            .attempts
            .iter()
            .filter(|attempt| !attempt.accepted)
            .cloned()
            .collect();
        SlimCharacterizationReport {
            attempts: self.attempts,
            accepted,
            rejected,
            upgrades: verdict_upgrades(before, after, applying),
        }
    }
}

fn run_characterization_pass(
    client: &impl LlmClient,
    options: &SlimOptions,
    patches: &[Patch],
    verify_options: &VerifyOptions,
    progress: &mut dyn FnMut(SlimProgress),
) -> Result<CharacterizationRun> {
    let work_orders = characterization_work_orders_for_patches(patches, verify_options)?;
    let tests = work_orders
        .iter()
        .map(|work_order| {
            progress(SlimProgress::Characterizing {
                workorder_id: work_order.id.to_owned(),
            });
            characterization_test_for_work_order(client, options, work_order)
        })
        .collect::<Result<Vec<_>>>()?;
    let report = verify_characterization_tests(&tests, verify_options)?;
    let mut accepted_tests = Vec::new();
    let mut attempts = Vec::new();
    for (test, result) in tests.into_iter().zip(report.results) {
        if result.accepted {
            accepted_tests.push(test.clone());
        }
        attempts.push(SlimCharacterizationAttempt {
            workorder_id: result.workorder_id,
            test_path: test.test_path,
            accepted: result.accepted,
            reasons: result.reasons,
        });
    }
    Ok(CharacterizationRun {
        accepted_tests,
        attempts,
    })
}

fn characterization_test_for_work_order(
    client: &impl LlmClient,
    options: &SlimOptions,
    work_order: &WorkOrder,
) -> Result<CharacterizationTest> {
    let prompt = build_characterization_prompt(work_order);
    Ok(CharacterizationTest {
        schema: "deslop.characterization-test/1".to_string(),
        workorder_id: work_order.id.to_owned(),
        region_fingerprint: workorder_region_fingerprint(work_order),
        test_path: characterization_test_path(work_order),
        test_text: strip_code_fences(&client.rewrite(&prompt)?),
        by: format!("deslop-slim/{}", options.model),
    })
}

fn characterization_test_path(work_order: &WorkOrder) -> PathBuf {
    let extension = work_order
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("txt");
    PathBuf::from("deslop_characterization").join(format!(
        "{}.{}",
        safe_filename_component(&work_order.id),
        extension
    ))
}

fn safe_filename_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn verdict_upgrades(
    before: &VerifyReport,
    after: &VerifyReport,
    applying: bool,
) -> Vec<SlimVerdictUpgrade> {
    before
        .results
        .iter()
        .filter_map(|initial| {
            let final_result = after
                .results
                .iter()
                .find(|result| result.workorder_id == initial.workorder_id)?;
            (initial.verdict != final_result.verdict).then(|| SlimVerdictUpgrade {
                workorder_id: initial.workorder_id.to_owned(),
                before: initial.verdict,
                after: final_result.verdict,
                applied_after_characterization: applying
                    && final_result.verdict == VerificationVerdict::Removable,
            })
        })
        .collect()
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

fn progress_outcomes(
    verified: &VerifyReport,
    applying: bool,
    allow_unverified: bool,
) -> Vec<(String, SlimProgressOutcome)> {
    verified
        .results
        .iter()
        .map(|result| {
            let outcome = if !result.passed || result.verdict == VerificationVerdict::Rejected {
                SlimProgressOutcome::Rejected
            } else if applying
                && (result.verdict == VerificationVerdict::Removable || allow_unverified)
            {
                SlimProgressOutcome::Applied
            } else {
                SlimProgressOutcome::Held
            };
            (result.workorder_id.to_owned(), outcome)
        })
        .collect()
}

fn progress_outcome_counts(outcomes: &[(String, SlimProgressOutcome)]) -> (usize, usize, usize) {
    outcomes.iter().fold(
        (0, 0, 0),
        |(applied, held, rejected), (_, outcome)| match outcome {
            SlimProgressOutcome::Applied => (applied + 1, held, rejected),
            SlimProgressOutcome::Held => (applied, held + 1, rejected),
            SlimProgressOutcome::Rejected => (applied, held, rejected + 1),
        },
    )
}

fn load_or_propose_work_orders(options: &SlimOptions) -> Result<Vec<WorkOrder>> {
    if let Some(path) = &options.workorders {
        return load_workorders_jsonl(path);
    }
    propose_work_orders_with_config(&options.paths, options.analyzer.clone())
}

/// Propose work orders using the default analyzer config (no suppression, default thresholds).
pub fn propose_work_orders(paths: &[PathBuf]) -> Result<Vec<WorkOrder>> {
    propose_work_orders_with_config(paths, AnalyzerConfig::default())
}

/// Propose work orders honoring an explicit analyzer config, including suppression. Auto-mode
/// `fix` uses this so disabled rules and ignored paths never reach the rewrite pipeline.
pub fn propose_work_orders_with_config(
    paths: &[PathBuf],
    config: AnalyzerConfig,
) -> Result<Vec<WorkOrder>> {
    let reports = scan_paths_with_config(paths, config)?;
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

    const ORIGINAL_IDENTITY: &str = "fn identity(value: i32) -> i32 {\n    return value;\n}\n";
    const REWRITTEN_IDENTITY: &str = "fn identity(value: i32) -> i32 {\n    value\n}";

    struct SlimTestFixture {
        temp: TempDir,
        source: PathBuf,
    }

    impl SlimTestFixture {
        fn identity() -> Result<Self> {
            Self::with_source(ORIGINAL_IDENTITY)
        }

        fn with_source(text: &str) -> Result<Self> {
            let temp = TempDir::new()?;
            let source = temp.path().join("sample.rs");
            fs::write(&source, text)?;
            Ok(Self { temp, source })
        }

        fn root(&self) -> PathBuf {
            self.temp.path().to_path_buf()
        }

        fn lcov(&self) -> PathBuf {
            lcov_fixture(self.temp.path(), "coverage.lcov", &self.source, 2, 1)
        }

        fn recorded_options(
            &self,
            apply: bool,
            allow_unverified: bool,
            coverage: CoverageConfig,
        ) -> SlimOptions {
            self.options(
                apply,
                false,
                allow_unverified,
                coverage,
                "recorded",
                Some("true"),
            )
        }

        fn scripted_options(&self, check_cmd: String) -> SlimOptions {
            self.options(
                true,
                true,
                false,
                CoverageConfig::Disabled,
                "scripted",
                Some(&check_cmd),
            )
        }

        fn options(
            &self,
            apply: bool,
            characterize: bool,
            allow_unverified: bool,
            coverage: CoverageConfig,
            model: &str,
            check_cmd: Option<&str>,
        ) -> SlimOptions {
            SlimOptions {
                root: self.root(),
                paths: vec![self.root()],
                workorders: None,
                apply,
                characterize,
                allow_unverified,
                coverage,
                model: model.to_string(),
                check_cmd: check_cmd.map(str::to_string),
                backup: false,
                analyzer: AnalyzerConfig::default(),
            }
        }
    }

    fn recorded_client() -> RecordedClient {
        RecordedClient::new(REWRITTEN_IDENTITY)
    }

    #[test]
    fn auto_mode_suppression_drops_work_orders_for_disabled_rule() {
        // The magic number becomes a (non-safe-auto) rewrite work order by default.
        let fixture =
            SlimTestFixture::with_source("fn answer() -> i32 {\n    let x = 4242;\n    x\n}\n")
                .expect("fixture");
        let paths = vec![fixture.source.clone()];

        let baseline =
            propose_work_orders_with_config(&paths, AnalyzerConfig::default()).expect("baseline");
        assert!(
            baseline
                .iter()
                .any(|wo| wo.findings.iter().any(|f| f.rule == "magic-number")),
            "baseline should propose a magic-number rewrite"
        );

        let mut builder = deslop_analyzer::Suppression::builder();
        builder.disable_rule("magic-number");
        let analyzer = AnalyzerConfig {
            suppression: builder.build().expect("build suppression"),
            ..AnalyzerConfig::default()
        };
        let suppressed =
            propose_work_orders_with_config(&paths, analyzer).expect("suppressed proposal");
        assert!(
            suppressed
                .iter()
                .all(|wo| wo.findings.iter().all(|f| f.rule != "magic-number")),
            "disabled rule must not reach the rewrite pipeline"
        );
    }

    fn assert_single_result(report: &SlimReport, passed: bool, verdict: VerificationVerdict) {
        assert_eq!(report.verified.results[0].passed, passed);
        assert_eq!(report.verified.results[0].verdict, verdict);
    }

    fn assert_gating_counts(
        report: &SlimReport,
        applied: usize,
        held_unproven: usize,
        rejected: usize,
    ) {
        assert_eq!(report.gating.applied.len(), applied);
        assert_eq!(report.gating.held_unproven.len(), held_unproven);
        assert_eq!(report.gating.rejected.len(), rejected);
    }

    fn assert_written_paths(report: &SlimReport, paths: &[PathBuf]) {
        assert_eq!(
            report.applied.as_ref().expect("apply report").written,
            paths
        );
    }

    fn assert_no_written_paths(report: &SlimReport) {
        assert!(
            report
                .applied
                .as_ref()
                .expect("apply report")
                .written
                .is_empty()
        );
    }

    fn assert_fixture_source(fixture: &SlimTestFixture, expected: &str) -> Result<()> {
        assert_eq!(fs::read_to_string(&fixture.source)?, expected);
        Ok(())
    }

    #[test]
    fn egress_consent_decision_truth_table() {
        assert_eq!(resolve_egress_consent(true, false), EgressDecision::Granted);
        assert_eq!(resolve_egress_consent(true, true), EgressDecision::Granted);
        assert_eq!(resolve_egress_consent(false, true), EgressDecision::Prompt);
        assert_eq!(
            resolve_egress_consent(false, false),
            EgressDecision::DeniedNonInteractive
        );
    }

    #[test]
    fn egress_env_and_messages_are_deterministic() {
        assert!(env_egress_consent(Some("1".to_string())));
        assert!(env_egress_consent(Some("true".to_string())));
        assert!(!env_egress_consent(Some("0".to_string())));
        assert!(!env_egress_consent(None));

        assert_eq!(
            provider_base_url("anthropic", Some("ignored")),
            ANTHROPIC_BASE_URL
        );
        assert_eq!(provider_base_url("openai", None), OPENAI_DEFAULT_BASE_URL);
        assert_eq!(
            provider_base_url("openai", Some("http://localhost:11434/v1")),
            "http://localhost:11434/v1"
        );
        let message = egress_prompt_message(
            "openai",
            "http://localhost:11434/v1",
            EgressSummary {
                file_count: 2,
                region_count: 3,
            },
        );
        assert!(message.contains("2 file(s), 3 region(s)"));
        assert!(message.contains("openai (http://localhost:11434/v1)"));
        assert!(!message.contains("API_KEY"));
    }

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

        assert_eq!(prompt.kind, SlimPromptKind::Rewrite);
        assert!(prompt.text.contains("fn sample()"));
        assert!(prompt.text.contains("unneeded return"));
        assert!(prompt.text.contains("must parse: true"));
        assert!(prompt.text.contains("no new public definitions: true"));

        let characterization = build_characterization_prompt(&work_order);
        assert_eq!(characterization.kind, SlimPromptKind::Characterization);
        assert!(
            characterization
                .text
                .contains("pins the CURRENT observable behavior")
        );
        assert!(characterization.text.contains("fn sample()"));
    }

    #[test]
    fn recorded_client_e2e_applies_verified_rewrite() -> Result<()> {
        let fixture = SlimTestFixture::identity()?;
        let client = recorded_client();
        let report = run_slim(
            &client,
            fixture.recorded_options(true, false, CoverageConfig::LcovFile(fixture.lcov())),
        )?;

        assert_eq!(report.schema, "deslop.slim/1");
        assert_eq!(report.patches.len(), 1);
        assert_eq!(report.patches[0].schema, "deslop.patch/1");
        assert_eq!(report.patches[0].by, "deslop-slim/recorded");
        assert_single_result(&report, true, VerificationVerdict::Removable);
        assert_gating_counts(&report, 1, 0, 0);
        assert_written_paths(&report, &[fixture.source.clone()]);
        assert_fixture_source(&fixture, REWRITTEN_IDENTITY)?;
        Ok(())
    }

    #[test]
    fn progress_sink_records_mock_run_sequence() -> Result<()> {
        let fixture = SlimTestFixture::identity()?;
        let client = recorded_client();
        let mut events = Vec::new();
        let report = run_slim_with_progress(
            &client,
            fixture.recorded_options(true, false, CoverageConfig::LcovFile(fixture.lcov())),
            &mut |event| events.push(event),
        )?;

        assert_eq!(report.gating.applied.len(), 1);
        assert_mock_progress_sequence(&events);
        Ok(())
    }

    fn assert_mock_progress_sequence(events: &[SlimProgress]) {
        assert_eq!(events.len(), 5);
        assert!(matches!(
            events[0],
            SlimProgress::Started { work_orders: 1 }
        ));
        assert!(matches!(
            events[1],
            SlimProgress::Rewriting {
                index: 1,
                total: 1,
                start_line: 1,
                end_line: 3,
                ..
            }
        ));
        assert!(matches!(
            events[2],
            SlimProgress::Verified {
                verdict: VerificationVerdict::Removable,
                ..
            }
        ));
        assert!(matches!(
            events[3],
            SlimProgress::Outcome {
                outcome: SlimProgressOutcome::Applied,
                ..
            }
        ));
        assert!(matches!(
            events[4],
            SlimProgress::Finished {
                applied: 1,
                held: 0,
                rejected: 0
            }
        ));
    }

    #[test]
    fn progress_sink_does_not_change_final_report() -> Result<()> {
        let fixture = SlimTestFixture::identity()?;
        let client = recorded_client();
        let options = fixture.recorded_options(false, false, CoverageConfig::Disabled);
        let mut events = Vec::new();
        let with_progress =
            run_slim_with_progress(&client, options.clone(), &mut |event| events.push(event))?;
        let quiet = run_slim(&client, options)?;

        assert!(!events.is_empty());
        assert_eq!(
            serde_json::to_value(&with_progress)?,
            serde_json::to_value(&quiet)?
        );
        Ok(())
    }

    #[test]
    fn default_apply_holds_unproven_coverage_unknown_rewrite() -> Result<()> {
        let fixture = SlimTestFixture::identity()?;
        let client = recorded_client();
        let report = run_slim(
            &client,
            fixture.recorded_options(true, false, CoverageConfig::Disabled),
        )?;

        assert_single_result(&report, true, VerificationVerdict::CoverageUnknown);
        assert_gating_counts(&report, 0, 1, 0);
        assert_no_written_paths(&report);
        assert_fixture_source(&fixture, ORIGINAL_IDENTITY)?;
        Ok(())
    }

    #[test]
    fn allow_unverified_applies_coverage_unknown_rewrite() -> Result<()> {
        let fixture = SlimTestFixture::identity()?;
        let client = recorded_client();
        let report = run_slim(
            &client,
            fixture.recorded_options(true, true, CoverageConfig::Disabled),
        )?;

        assert_single_result(&report, true, VerificationVerdict::CoverageUnknown);
        assert_gating_counts(&report, 1, 0, 0);
        assert_written_paths(&report, &[fixture.source.clone()]);
        assert_fixture_source(&fixture, REWRITTEN_IDENTITY)?;
        Ok(())
    }

    #[test]
    fn verify_rejects_bad_rewrite_and_apply_leaves_file_unchanged() -> Result<()> {
        assert_bad_rewrite_blocked(false)?;
        assert_bad_rewrite_blocked(true)
    }

    fn assert_bad_rewrite_blocked(allow_unverified: bool) -> Result<()> {
        let fixture = SlimTestFixture::identity()?;
        let client = RecordedClient::new("pub fn added() {}\nfn identity(value: i32) -> i32 {\n");
        let report = run_slim(
            &client,
            fixture.recorded_options(true, allow_unverified, CoverageConfig::Disabled),
        )?;

        assert_single_result(&report, false, VerificationVerdict::Rejected);
        assert_gating_counts(&report, 0, 0, 1);
        assert_no_written_paths(&report);
        assert_fixture_source(&fixture, ORIGINAL_IDENTITY)?;
        Ok(())
    }

    #[test]
    fn characterize_accepts_test_upgrades_and_applies_rewrite() -> Result<()> {
        let fixture = SlimTestFixture::identity()?;
        let check_cmd = characterization_check_cmd(fixture.temp.path(), "PIN")?;
        let client = ScriptedClient {
            rewrite: REWRITTEN_IDENTITY.to_string(),
            characterization: "PIN current behavior".to_string(),
        };
        let report = run_slim(&client, fixture.scripted_options(check_cmd))?;

        let characterization = report.characterization.as_ref().expect("characterization");
        assert_eq!(characterization.attempts.len(), 1);
        assert_eq!(characterization.accepted.len(), 1);
        assert!(characterization.rejected.is_empty());
        assert_eq!(characterization.upgrades.len(), 1);
        assert_eq!(
            characterization.upgrades[0].before,
            VerificationVerdict::CoverageUnknown
        );
        assert_eq!(
            characterization.upgrades[0].after,
            VerificationVerdict::Removable
        );
        assert!(characterization.upgrades[0].applied_after_characterization);
        assert_single_result(&report, true, VerificationVerdict::Removable);
        assert_gating_counts(&report, 1, 0, 0);
        assert_written_paths(&report, &[fixture.source.clone()]);
        assert_fixture_source(&fixture, REWRITTEN_IDENTITY)?;
        Ok(())
    }

    #[test]
    fn characterize_rejects_failing_test_and_holds_rewrite() -> Result<()> {
        let fixture = SlimTestFixture::identity()?;
        let check_cmd = characterization_check_cmd(fixture.temp.path(), "PIN")?;
        let client = ScriptedClient {
            rewrite: REWRITTEN_IDENTITY.to_string(),
            characterization: "WRONG current behavior".to_string(),
        };
        let report = run_slim(&client, fixture.scripted_options(check_cmd))?;

        let characterization = report.characterization.as_ref().expect("characterization");
        assert_eq!(characterization.attempts.len(), 1);
        assert!(characterization.accepted.is_empty());
        assert_eq!(characterization.rejected.len(), 1);
        assert!(characterization.upgrades.is_empty());
        assert_single_result(&report, true, VerificationVerdict::CoverageUnknown);
        assert_gating_counts(&report, 0, 1, 0);
        assert_no_written_paths(&report);
        assert_fixture_source(&fixture, ORIGINAL_IDENTITY)?;
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

    struct ScriptedClient {
        rewrite: String,
        characterization: String,
    }

    impl LlmClient for ScriptedClient {
        fn rewrite(&self, prompt: &SlimPrompt) -> Result<String> {
            Ok(match prompt.kind {
                SlimPromptKind::Rewrite => self.rewrite.to_owned(),
                SlimPromptKind::Characterization => self.characterization.to_owned(),
            })
        }
    }

    fn characterization_check_cmd(root: &Path, expected: &str) -> Result<String> {
        let work_order = propose_work_orders(&[root.to_path_buf()])?
            .into_iter()
            .next()
            .context("expected work order")?;
        let test_path = characterization_test_path(&work_order);
        Ok(format!(
            "if [ -f {path} ]; then grep -q {expected} {path}; else true; fi",
            path = test_path.display()
        ))
    }
}
