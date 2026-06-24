use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use deslop_analyzer::scan_paths;
use deslop_core::Lang;
use deslop_parse::{SourceFile, parses_without_errors};
use deslop_protocol::{
    CharacterizationTest, Patch, WorkOrder, characterization_work_order_for,
    work_orders_for_source, workorder_region_fingerprint,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

#[derive(Debug, Clone)]
pub struct VerifyOptions {
    pub root: PathBuf,
    pub check_cmd: Option<String>,
    pub coverage: CoverageConfig,
    pub mutation: MutationConfig,
    pub characterization_tests: Vec<CharacterizationTest>,
    pub allow_non_removable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyReport {
    pub schema: String,
    pub results: Vec<PatchVerification>,
}

impl VerifyReport {
    pub fn passed_count(&self) -> usize {
        self.results.iter().filter(|result| result.passed).count()
    }

    pub fn failed_count(&self) -> usize {
        self.results.len() - self.passed_count()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchVerification {
    pub workorder_id: String,
    pub path: Option<PathBuf>,
    pub passed: bool,
    pub verdict: VerificationVerdict,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterizationReport {
    pub schema: String,
    pub results: Vec<CharacterizationVerification>,
}

impl CharacterizationReport {
    pub fn accepted_count(&self) -> usize {
        self.results.iter().filter(|result| result.accepted).count()
    }

    pub fn rejected_count(&self) -> usize {
        self.results.len() - self.accepted_count()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterizationVerification {
    pub workorder_id: String,
    pub path: Option<PathBuf>,
    pub accepted: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationVerdict {
    Removable,
    DeadCandidate,
    UntestedRisky,
    CoverageUnknown,
    Rejected,
}

impl VerificationVerdict {
    fn is_writable_by_default(self) -> bool {
        self == Self::Removable
    }

    fn is_non_rejected(self) -> bool {
        self != Self::Rejected
    }

    fn needs_characterization_test(self) -> bool {
        matches!(
            self,
            Self::CoverageUnknown | Self::UntestedRisky | Self::DeadCandidate
        )
    }
}

#[derive(Debug, Clone, Default)]
pub enum CoverageConfig {
    #[default]
    Disabled,
    Auto,
    AutoWithCommand(String),
    LcovFile(PathBuf),
    CloverageFile(PathBuf),
    JuliaCovFile(PathBuf),
    CoveragePyFile(PathBuf),
}

pub fn parse_coverage_mode(value: &str) -> Result<CoverageConfig> {
    let value = value.trim();
    match value {
        "disabled" | "off" | "none" => Ok(CoverageConfig::Disabled),
        "auto" => Ok(CoverageConfig::Auto),
        _ => parse_coverage_mode_with_value(value),
    }
}

fn parse_coverage_mode_with_value(value: &str) -> Result<CoverageConfig> {
    let Some((kind, payload)) = value.split_once(':') else {
        bail!(
            "unsupported coverage mode `{value}`; use disabled, auto, auto:<cmd>, lcov:<path>, cloverage:<path>, julia-cov:<path>, or coverage-py:<path>"
        );
    };
    if payload.is_empty() {
        bail!("coverage mode `{kind}` requires a value");
    }
    match kind {
        "auto" => Ok(CoverageConfig::AutoWithCommand(payload.to_string())),
        "lcov" => Ok(CoverageConfig::LcovFile(PathBuf::from(payload))),
        "cloverage" => Ok(CoverageConfig::CloverageFile(PathBuf::from(payload))),
        "julia-cov" | "julia" => Ok(CoverageConfig::JuliaCovFile(PathBuf::from(payload))),
        "coverage-py" | "coverage.py" | "python" => {
            Ok(CoverageConfig::CoveragePyFile(PathBuf::from(payload)))
        }
        _ => bail!(
            "unsupported coverage mode `{kind}`; use disabled, auto, auto:<cmd>, lcov:<path>, cloverage:<path>, julia-cov:<path>, or coverage-py:<path>"
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageStatus {
    Covered,
    Uncovered,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct CoverageAssessment {
    pub status: CoverageStatus,
    pub reason: Option<String>,
}

pub struct CoverageRequest<'a> {
    pub root: &'a Path,
    pub source: &'a SourceFile,
    pub work_order: &'a WorkOrder,
}

pub trait CoverageProvider {
    fn name(&self) -> &'static str;
    fn supports(&self, source: &SourceFile) -> bool;
    fn assess(&mut self, request: CoverageRequest<'_>) -> Result<CoverageAssessment>;
}

#[derive(Debug, Clone, Default)]
pub enum MutationConfig {
    #[default]
    Disabled,
    Auto,
    AutoWithCommand(String),
    OutcomesFile(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationStatus {
    Survived,
    NoSurvivor,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct MutationAssessment {
    pub status: MutationStatus,
    pub reason: Option<String>,
}

pub struct MutationRequest<'a> {
    pub root: &'a Path,
    pub source: &'a SourceFile,
    pub work_order: &'a WorkOrder,
}

pub trait MutationProbe {
    fn name(&self) -> &'static str;
    fn supports(&self, source: &SourceFile) -> bool;
    fn assess(&mut self, request: MutationRequest<'_>) -> Result<MutationAssessment>;
}

#[derive(Debug, Clone)]
struct PreparedPatch {
    path: PathBuf,
    replacement: String,
    range: std::ops::Range<usize>,
    verdict: VerificationVerdict,
    reasons: Vec<String>,
}

struct PatchSignals {
    coverage: CoverageAssessment,
    mutation: MutationAssessment,
    characterized: bool,
}

struct VerificationRun {
    work_orders: BTreeMap<String, WorkOrder>,
    coverage: CoverageRegistry,
    mutation: MutationRegistry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyReport {
    pub schema: String,
    pub verified: VerifyReport,
    pub written: Vec<PathBuf>,
}

fn read_to_string_ctx(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

pub fn load_patches(path: &Path) -> Result<Vec<Patch>> {
    let text = read_to_string_ctx(path)?;
    parse_patches_jsonl(&text)
}

pub fn load_characterization_tests(path: &Path) -> Result<Vec<CharacterizationTest>> {
    let text = read_to_string_ctx(path)?;
    parse_characterization_tests_jsonl(&text)
}

pub fn parse_patches_jsonl(text: &str) -> Result<Vec<Patch>> {
    parse_jsonl_records(text, "patch", "deslop.patch/1", |patch: &Patch| {
        patch.schema.as_str()
    })
}

pub fn parse_characterization_tests_jsonl(text: &str) -> Result<Vec<CharacterizationTest>> {
    parse_jsonl_records(
        text,
        "characterization test",
        "deslop.characterization-test/1",
        |test: &CharacterizationTest| test.schema.as_str(),
    )
}

fn parse_jsonl_records<T>(
    text: &str,
    label: &str,
    expected_schema: &str,
    schema: impl Fn(&T) -> &str,
) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let mut records = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let record: T = serde_json::from_str(line)
            .with_context(|| format!("failed to parse {label} JSONL line {}", idx + 1))?;
        let actual_schema = schema(&record);
        if actual_schema != expected_schema {
            bail!(
                "line {} has unsupported schema `{}`",
                idx + 1,
                actual_schema
            );
        }
        records.push(record);
    }
    Ok(records)
}

pub fn characterization_work_orders_for_patches(
    patches: &[Patch],
    options: &VerifyOptions,
) -> Result<Vec<WorkOrder>> {
    let report = verify_patches(patches, options)?;
    let work_orders = current_work_orders(&options.root)?;
    let mut out = Vec::new();
    for result in report
        .results
        .iter()
        .filter(|result| result.passed && result.verdict.needs_characterization_test())
    {
        if let Some(work_order) = work_orders.get(&result.workorder_id) {
            out.push(characterization_work_order_for(work_order));
        }
    }
    Ok(out)
}

pub fn verify_characterization_tests(
    tests: &[CharacterizationTest],
    options: &VerifyOptions,
) -> Result<CharacterizationReport> {
    let work_orders = current_work_orders(&options.root)?;
    let mut results = Vec::new();
    for test in tests {
        results.push(verify_one_characterization_test(
            test,
            &work_orders,
            options,
        )?);
    }
    Ok(CharacterizationReport {
        schema: "deslop.characterization/1".to_string(),
        results,
    })
}

pub fn verify_patches(patches: &[Patch], options: &VerifyOptions) -> Result<VerifyReport> {
    let mut run = verification_run(options)?;
    let mut results = Vec::new();
    for patch in patches {
        let result = verify_one_patch(
            patch,
            &run.work_orders,
            options,
            &mut run.coverage,
            &mut run.mutation,
        )?;
        results.push(result);
    }
    Ok(VerifyReport {
        schema: "deslop.verify/1".to_string(),
        results,
    })
}

pub fn apply_patches(
    patches: &[Patch],
    options: &VerifyOptions,
    backup: bool,
) -> Result<ApplyReport> {
    let mut run = verification_run(options)?;
    let mut results = Vec::new();
    let mut prepared = Vec::new();
    for patch in patches {
        match prepare_patch(
            patch,
            &run.work_orders,
            options,
            &mut run.coverage,
            &mut run.mutation,
        )? {
            PreparedOutcome::Pass(prepared_patch) => {
                results.push(passed_patch_verification(patch, &prepared_patch));
                if prepared_patch.verdict.is_writable_by_default()
                    || (options.allow_non_removable && prepared_patch.verdict.is_non_rejected())
                {
                    prepared.push(prepared_patch);
                }
            }
            PreparedOutcome::Reject(result) => results.push(result),
        }
    }

    let written = write_prepared_patches(&options.root, &prepared, backup)?;
    Ok(ApplyReport {
        schema: "deslop.apply/1".to_string(),
        verified: VerifyReport {
            schema: "deslop.verify/1".to_string(),
            results,
        },
        written,
    })
}

fn verification_run(options: &VerifyOptions) -> Result<VerificationRun> {
    Ok(VerificationRun {
        work_orders: current_work_orders(&options.root)?,
        coverage: CoverageRegistry::new(&options.coverage),
        mutation: MutationRegistry::new(&options.mutation),
    })
}

fn passed_patch_verification(patch: &Patch, prepared_patch: &PreparedPatch) -> PatchVerification {
    PatchVerification {
        workorder_id: patch.workorder_id.to_owned(),
        path: Some(prepared_patch.path.to_path_buf()),
        passed: true,
        verdict: prepared_patch.verdict,
        reasons: prepared_patch.reasons.clone(),
    }
}

fn verify_one_patch(
    patch: &Patch,
    work_orders: &BTreeMap<String, WorkOrder>,
    options: &VerifyOptions,
    coverage: &mut CoverageRegistry,
    mutation: &mut MutationRegistry,
) -> Result<PatchVerification> {
    Ok(
        match prepare_patch(patch, work_orders, options, coverage, mutation)? {
            PreparedOutcome::Pass(prepared) => PatchVerification {
                workorder_id: patch.workorder_id.to_owned(),
                path: Some(prepared.path),
                passed: true,
                verdict: prepared.verdict,
                reasons: prepared.reasons,
            },
            PreparedOutcome::Reject(result) => result,
        },
    )
}

enum PreparedOutcome {
    Pass(PreparedPatch),
    Reject(PatchVerification),
}

fn prepare_patch(
    patch: &Patch,
    work_orders: &BTreeMap<String, WorkOrder>,
    options: &VerifyOptions,
    coverage: &mut CoverageRegistry,
    mutation: &mut MutationRegistry,
) -> Result<PreparedOutcome> {
    let Some(work_order) = work_orders.get(&patch.workorder_id) else {
        return Ok(PreparedOutcome::Reject(reject_unknown_workorder(patch)));
    };

    if let Some(rejection) = reject_stale_fingerprint(patch, work_order) {
        return Ok(PreparedOutcome::Reject(rejection));
    }

    let source = SourceFile::read(path_in_root(&options.root, &work_order.path))?;
    if let Some(rejection) = reject_stale_region_bytes(patch, work_order, &source) {
        return Ok(PreparedOutcome::Reject(rejection));
    }

    let range = region_byte_range(&source, work_order)?;
    let candidate = replace_region(&source.text, range.start..range.end, &patch.replacement)?;
    let mut reasons = guard_rejections(work_order, patch, source.lang, &candidate)?;
    let signals = assess_patch_signals(
        options,
        &source,
        work_order,
        &candidate,
        &mut reasons,
        coverage,
        mutation,
    )?;

    Ok(prepared_outcome(
        patch,
        work_order,
        range,
        reasons,
        signals.coverage,
        signals.mutation,
        signals.characterized,
    ))
}

fn assess_patch_signals(
    options: &VerifyOptions,
    source: &SourceFile,
    work_order: &WorkOrder,
    candidate: &str,
    reasons: &mut Vec<String>,
    coverage: &mut CoverageRegistry,
    mutation: &mut MutationRegistry,
) -> Result<PatchSignals> {
    if reasons.is_empty()
        && let Some(command) = selected_check_cmd(options, work_order)
    {
        run_check_cmd_on_temp_copy(options, work_order, candidate, command, reasons)?;
    }

    let characterized = if reasons.is_empty() {
        run_characterization_gate(options, work_order, candidate, reasons)?
    } else {
        false
    };

    let coverage = assess_coverage_if_clean(options, source, work_order, reasons, coverage)?;
    let mutation = assess_mutation_if_clean(options, source, work_order, reasons, mutation)?;

    Ok(PatchSignals {
        coverage,
        mutation,
        characterized,
    })
}

fn assess_coverage_if_clean(
    options: &VerifyOptions,
    source: &SourceFile,
    work_order: &WorkOrder,
    reasons: &[String],
    coverage: &mut CoverageRegistry,
) -> Result<CoverageAssessment> {
    if reasons.is_empty() {
        coverage.assess(CoverageRequest {
            root: &options.root,
            source,
            work_order,
        })
    } else {
        Ok(unknown_coverage_assessment())
    }
}

fn assess_mutation_if_clean(
    options: &VerifyOptions,
    source: &SourceFile,
    work_order: &WorkOrder,
    reasons: &[String],
    mutation: &mut MutationRegistry,
) -> Result<MutationAssessment> {
    if reasons.is_empty() {
        mutation.assess(MutationRequest {
            root: &options.root,
            source,
            work_order,
        })
    } else {
        Ok(unknown_mutation_assessment())
    }
}

fn unknown_coverage_assessment() -> CoverageAssessment {
    CoverageAssessment {
        status: CoverageStatus::Unknown,
        reason: None,
    }
}

fn unknown_mutation_assessment() -> MutationAssessment {
    MutationAssessment {
        status: MutationStatus::Unknown,
        reason: None,
    }
}

fn reject_unknown_workorder(patch: &Patch) -> PatchVerification {
    reject(patch, None, "stale region_fingerprint or unknown workorder")
}

fn reject_stale_fingerprint(patch: &Patch, work_order: &WorkOrder) -> Option<PatchVerification> {
    let expected_fingerprint = workorder_region_fingerprint(work_order);
    (patch.region_fingerprint != expected_fingerprint).then(|| {
        reject(
            patch,
            Some(work_order.path.to_path_buf()),
            "stale region_fingerprint",
        )
    })
}

fn reject_stale_region_bytes(
    patch: &Patch,
    work_order: &WorkOrder,
    source: &SourceFile,
) -> Option<PatchVerification> {
    let current_region =
        source.region_text(work_order.region.start_line, work_order.region.end_line);
    (current_region != work_order.region.text).then(|| {
        reject(
            patch,
            Some(work_order.path.to_path_buf()),
            "stale region bytes",
        )
    })
}

fn guard_rejections(
    work_order: &WorkOrder,
    patch: &Patch,
    lang: Lang,
    candidate: &str,
) -> Result<Vec<String>> {
    let mut reasons = Vec::new();
    reject_growth_overflow(work_order, patch, &mut reasons);
    reject_defensive_code_deletion(work_order, patch, &mut reasons);
    reject_new_public_defs(work_order, patch, &mut reasons);
    reject_parse_errors(work_order, lang, candidate, &mut reasons)?;
    Ok(reasons)
}

fn reject_growth_overflow(work_order: &WorkOrder, patch: &Patch, reasons: &mut Vec<String>) {
    if work_order.contract.max_growth_ratio < 0.0 {
        return;
    }
    let allowed = (work_order.region.text.len() as f32 * work_order.contract.max_growth_ratio)
        .ceil() as usize;
    if patch.replacement.len() > allowed {
        reasons.push(format!(
            "max_growth_ratio exceeded: replacement {} bytes > allowed {} bytes",
            patch.replacement.len(),
            allowed
        ));
    }
}

fn reject_defensive_code_deletion(
    work_order: &WorkOrder,
    patch: &Patch,
    reasons: &mut Vec<String>,
) {
    if work_order.contract.keep_error_handling
        && deletes_defensive_code(&work_order.region.text, &patch.replacement)
    {
        reasons.push(
            "defensive-code guard rejected deletion of error handling/assertions".to_string(),
        );
    }
}

fn reject_new_public_defs(work_order: &WorkOrder, patch: &Patch, reasons: &mut Vec<String>) {
    if work_order.contract.no_new_public_defs
        && count_public_defs(&patch.replacement) > count_public_defs(&work_order.region.text)
    {
        reasons.push("no_new_public_defs guard rejected added public definition".to_string());
    }
}

fn reject_parse_errors(
    work_order: &WorkOrder,
    lang: Lang,
    candidate: &str,
    reasons: &mut Vec<String>,
) -> Result<()> {
    if work_order.contract.must_parse && !parse_check_passes(lang, candidate)? {
        reasons.push(
            "must_parse guard rejected tree-sitter ERROR nodes or unbalanced delimiters"
                .to_string(),
        );
    }
    Ok(())
}

fn selected_check_cmd<'a>(
    options: &'a VerifyOptions,
    work_order: &'a WorkOrder,
) -> Option<&'a str> {
    options
        .check_cmd
        .as_deref()
        .or(work_order.contract.check_cmd.as_deref())
}

fn verify_one_characterization_test(
    test: &CharacterizationTest,
    work_orders: &BTreeMap<String, WorkOrder>,
    options: &VerifyOptions,
) -> Result<CharacterizationVerification> {
    let Some(work_order) = work_orders.get(&test.workorder_id) else {
        return Ok(reject_characterization(test, None, "unknown workorder"));
    };
    if test.region_fingerprint != workorder_region_fingerprint(work_order) {
        return Ok(reject_characterization(
            test,
            Some(work_order.path.to_path_buf()),
            "stale region_fingerprint",
        ));
    }
    let Some(command) = selected_check_cmd(options, work_order) else {
        return Ok(reject_characterization(
            test,
            Some(work_order.path.to_path_buf()),
            "characterization test verification requires --check-cmd",
        ));
    };
    let mut reasons = Vec::new();
    run_characterization_test_on_current_code(options, test, command, &mut reasons)?;
    Ok(CharacterizationVerification {
        workorder_id: test.workorder_id.to_owned(),
        path: Some(work_order.path.to_path_buf()),
        accepted: reasons.is_empty(),
        reasons: if reasons.is_empty() {
            vec!["characterization test compiles and passes on current code".to_string()]
        } else {
            reasons
        },
    })
}

fn reject_characterization(
    test: &CharacterizationTest,
    path: Option<PathBuf>,
    reason: &str,
) -> CharacterizationVerification {
    CharacterizationVerification {
        workorder_id: test.workorder_id.to_owned(),
        path,
        accepted: false,
        reasons: vec![reason.to_string()],
    }
}

fn run_characterization_gate(
    options: &VerifyOptions,
    work_order: &WorkOrder,
    candidate: &str,
    reasons: &mut Vec<String>,
) -> Result<bool> {
    let tests = matching_characterization_tests(options, work_order);
    if tests.is_empty() {
        return Ok(false);
    }
    let Some(command) = selected_check_cmd(options, work_order) else {
        reasons.push("characterization gate requires --check-cmd".to_string());
        return Ok(false);
    };
    let mut all_passed = true;
    for test in tests {
        if test.region_fingerprint != workorder_region_fingerprint(work_order) {
            reasons.push("characterization gate rejected stale region_fingerprint".to_string());
            all_passed = false;
            continue;
        }
        run_characterization_test_on_current_code(options, test, command, reasons)?;
        if !reasons.is_empty() {
            all_passed = false;
            continue;
        }
        run_characterization_test_on_patched_code(
            options, work_order, candidate, test, command, reasons,
        )?;
        if !reasons.is_empty() {
            all_passed = false;
        }
    }
    Ok(all_passed && reasons.is_empty())
}

fn matching_characterization_tests<'a>(
    options: &'a VerifyOptions,
    work_order: &WorkOrder,
) -> Vec<&'a CharacterizationTest> {
    options
        .characterization_tests
        .iter()
        .filter(|test| test.workorder_id == work_order.id)
        .collect()
}

fn prepared_outcome(
    patch: &Patch,
    work_order: &WorkOrder,
    range: std::ops::Range<usize>,
    reasons: Vec<String>,
    coverage: CoverageAssessment,
    mutation: MutationAssessment,
    characterized: bool,
) -> PreparedOutcome {
    if reasons.is_empty() {
        let verdict =
            verdict_for_passing_patch(patch, coverage.status, mutation.status, characterized);
        let mut reasons = Vec::new();
        if let Some(reason) = coverage.reason {
            reasons.push(reason);
        }
        if let Some(reason) = mutation.reason {
            reasons.push(reason);
        }
        if characterized {
            reasons.push("characterization test passed on current and patched code".to_string());
        }
        PreparedOutcome::Pass(PreparedPatch {
            path: work_order.path.to_path_buf(),
            replacement: patch.replacement.to_owned(),
            range,
            verdict,
            reasons,
        })
    } else {
        PreparedOutcome::Reject(PatchVerification {
            workorder_id: patch.workorder_id.to_owned(),
            path: Some(work_order.path.to_path_buf()),
            passed: false,
            verdict: VerificationVerdict::Rejected,
            reasons,
        })
    }
}

fn verdict_for_passing_patch(
    patch: &Patch,
    coverage: CoverageStatus,
    mutation: MutationStatus,
    characterized: bool,
) -> VerificationVerdict {
    if characterized {
        return VerificationVerdict::Removable;
    }
    if mutation == MutationStatus::Survived {
        return if patch.replacement.trim().is_empty() {
            VerificationVerdict::DeadCandidate
        } else {
            VerificationVerdict::UntestedRisky
        };
    }
    match coverage {
        CoverageStatus::Covered => VerificationVerdict::Removable,
        CoverageStatus::Uncovered if patch.replacement.trim().is_empty() => {
            VerificationVerdict::DeadCandidate
        }
        CoverageStatus::Uncovered => VerificationVerdict::UntestedRisky,
        CoverageStatus::Unknown => VerificationVerdict::CoverageUnknown,
    }
}

struct MutationRegistry {
    probes: Vec<Box<dyn MutationProbe>>,
    disabled: bool,
}

impl MutationRegistry {
    fn new(config: &MutationConfig) -> Self {
        Self {
            probes: vec![
                Box::new(RustCargoMutantsProbe::new(config)),
                Box::new(PythonMutationProbe::new(config)),
            ],
            disabled: matches!(config, MutationConfig::Disabled),
        }
    }

    fn assess(&mut self, request: MutationRequest<'_>) -> Result<MutationAssessment> {
        if self.disabled {
            return Ok(MutationAssessment {
                status: MutationStatus::Unknown,
                reason: Some("mutation disabled".to_string()),
            });
        }
        let Some(probe) = self
            .probes
            .iter_mut()
            .find(|probe| probe.supports(request.source))
        else {
            return Ok(MutationAssessment {
                status: MutationStatus::Unknown,
                reason: Some("no mutation probe for language".to_string()),
            });
        };
        probe.assess(request)
    }
}

#[derive(Debug, Clone)]
enum MutationProbeMode {
    Disabled,
    Auto { command: String },
    OutcomesFile(PathBuf),
}

struct RustCargoMutantsProbe {
    mode: MutationProbeMode,
    outcomes: Option<Result<MutantOutcomes, String>>,
}

impl RustCargoMutantsProbe {
    fn new(config: &MutationConfig) -> Self {
        let mode = match config {
            MutationConfig::Disabled => MutationProbeMode::Disabled,
            MutationConfig::Auto => MutationProbeMode::Auto {
                command: "cargo".to_string(),
            },
            MutationConfig::AutoWithCommand(command) => MutationProbeMode::Auto {
                command: command.to_owned(),
            },
            MutationConfig::OutcomesFile(path) => {
                MutationProbeMode::OutcomesFile(path.to_path_buf())
            }
        };
        Self {
            mode,
            outcomes: None,
        }
    }

    fn outcomes(&mut self, root: &Path) -> Result<&MutantOutcomes, MutationAssessment> {
        if self.outcomes.is_none() {
            self.outcomes = Some(match self.load_outcomes(root) {
                Ok(outcomes) => Ok(outcomes),
                Err(reason) => Err(reason),
            });
        }
        match self.outcomes.as_ref().expect("mutation initialized") {
            Ok(outcomes) => Ok(outcomes),
            Err(reason) => Err(MutationAssessment {
                status: MutationStatus::Unknown,
                reason: Some(reason.to_owned()),
            }),
        }
    }

    fn load_outcomes(&self, root: &Path) -> std::result::Result<MutantOutcomes, String> {
        match &self.mode {
            MutationProbeMode::Disabled => Err("mutation disabled".to_string()),
            MutationProbeMode::OutcomesFile(path) => {
                let text = read_report_text(path, "cargo-mutants outcomes")?;
                MutantOutcomes::parse(&text).map_err(|err| err.to_string())
            }
            MutationProbeMode::Auto { command } => {
                if !cargo_mutants_available(command, root) {
                    return Err("mutation-unknown: cargo-mutants not available".to_string());
                }
                let temp = TempDir::new()
                    .map_err(|err| format!("failed to create mutation tempdir: {err}"))?;
                let output = Command::new(command)
                    .args(["mutants", "--json", "--output"])
                    .arg(temp.path())
                    .current_dir(root)
                    .output()
                    .map_err(|err| format!("failed to run cargo-mutants: {err}"))?;
                let outcomes_path = temp.path().join("outcomes.json");
                if !outcomes_path.exists() {
                    return Err(command_failure_reason(
                        "cargo-mutants",
                        output.status,
                        &output.stderr,
                    )
                    .replace("coverage unknown", "mutation unknown"));
                }
                let text = read_report_text(&outcomes_path, "cargo-mutants outcomes")?;
                MutantOutcomes::parse(&text).map_err(|err| err.to_string())
            }
        }
    }
}

impl MutationProbe for RustCargoMutantsProbe {
    fn name(&self) -> &'static str {
        "cargo-mutants"
    }

    fn supports(&self, source: &SourceFile) -> bool {
        source.lang == Lang::Rust
    }

    fn assess(&mut self, request: MutationRequest<'_>) -> Result<MutationAssessment> {
        let outcomes = match self.outcomes(request.root) {
            Ok(outcomes) => outcomes,
            Err(assessment) => return Ok(assessment),
        };
        let relative = relative_to_root(request.root, &request.source.path)?;
        if outcomes.has_surviving_mutant(
            &request.source.path,
            &relative,
            request.work_order.region.start_line,
            request.work_order.region.end_line,
        ) {
            Ok(MutationAssessment {
                status: MutationStatus::Survived,
                reason: Some(format!(
                    "mutation probe {} found surviving mutant in region",
                    self.name()
                )),
            })
        } else {
            Ok(MutationAssessment {
                status: MutationStatus::NoSurvivor,
                reason: Some(format!(
                    "mutation probe {} found no surviving mutant in region",
                    self.name()
                )),
            })
        }
    }
}

struct PythonMutationProbe {
    mode: MutationProbeMode,
    outcomes: Option<Result<MutantOutcomes, String>>,
}

impl PythonMutationProbe {
    fn new(config: &MutationConfig) -> Self {
        let mode = match config {
            MutationConfig::Disabled => MutationProbeMode::Disabled,
            MutationConfig::Auto => MutationProbeMode::Auto {
                command: "cosmic-ray".to_string(),
            },
            MutationConfig::AutoWithCommand(command) => MutationProbeMode::Auto {
                command: command.to_owned(),
            },
            MutationConfig::OutcomesFile(path) => {
                MutationProbeMode::OutcomesFile(path.to_path_buf())
            }
        };
        Self {
            mode,
            outcomes: None,
        }
    }

    fn outcomes(&mut self, root: &Path) -> Result<&MutantOutcomes, MutationAssessment> {
        if self.outcomes.is_none() {
            self.outcomes = Some(match self.load_outcomes(root) {
                Ok(outcomes) => Ok(outcomes),
                Err(reason) => Err(reason),
            });
        }
        match self.outcomes.as_ref().expect("mutation initialized") {
            Ok(outcomes) => Ok(outcomes),
            Err(reason) => Err(MutationAssessment {
                status: MutationStatus::Unknown,
                reason: Some(reason.to_owned()),
            }),
        }
    }

    fn load_outcomes(&self, root: &Path) -> std::result::Result<MutantOutcomes, String> {
        match &self.mode {
            MutationProbeMode::Disabled => Err("mutation disabled".to_string()),
            MutationProbeMode::OutcomesFile(path) => {
                let text = read_report_text(path, "cosmic-ray outcomes")?;
                MutantOutcomes::parse(&text).map_err(|err| err.to_string())
            }
            MutationProbeMode::Auto { command } => run_cosmic_ray(command, root),
        }
    }
}

impl MutationProbe for PythonMutationProbe {
    fn name(&self) -> &'static str {
        "cosmic-ray"
    }

    fn supports(&self, source: &SourceFile) -> bool {
        source.lang == Lang::Python
    }

    fn assess(&mut self, request: MutationRequest<'_>) -> Result<MutationAssessment> {
        let outcomes = match self.outcomes(request.root) {
            Ok(outcomes) => outcomes,
            Err(assessment) => return Ok(assessment),
        };
        let relative = relative_to_root(request.root, &request.source.path)?;
        if outcomes.has_surviving_mutant(
            &request.source.path,
            &relative,
            request.work_order.region.start_line,
            request.work_order.region.end_line,
        ) {
            Ok(MutationAssessment {
                status: MutationStatus::Survived,
                reason: Some(format!(
                    "mutation probe {} found surviving mutant in region",
                    self.name()
                )),
            })
        } else {
            Ok(MutationAssessment {
                status: MutationStatus::NoSurvivor,
                reason: Some(format!(
                    "mutation probe {} found no surviving mutant in region",
                    self.name()
                )),
            })
        }
    }
}

fn cargo_mutants_available(command: &str, root: &Path) -> bool {
    Command::new(command)
        .args(["mutants", "--version"])
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn cosmic_ray_available(command: &str, root: &Path) -> bool {
    Command::new(command)
        .arg("--version")
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn run_cosmic_ray(command: &str, root: &Path) -> std::result::Result<MutantOutcomes, String> {
    if !cosmic_ray_available(command, root) {
        return Err("mutation-unknown: cosmic-ray not available".to_string());
    }
    let config = cosmic_ray_config(root)
        .ok_or_else(|| "mutation-unknown: cosmic-ray config not found".to_string())?;
    let temp = TempDir::new().map_err(|err| format!("failed to create mutation tempdir: {err}"))?;
    let session = temp.path().join("cosmic-ray.sqlite");
    let init = Command::new(command)
        .arg("init")
        .arg(&config)
        .arg(&session)
        .current_dir(root)
        .output()
        .map_err(|err| format!("failed to run cosmic-ray init: {err}"))?;
    if !init.status.success() {
        return Err(
            command_failure_reason("cosmic-ray init", init.status, &init.stderr)
                .replace("coverage unknown", "mutation unknown"),
        );
    }
    let exec = Command::new(command)
        .arg("exec")
        .arg(&config)
        .arg(&session)
        .current_dir(root)
        .output()
        .map_err(|err| format!("failed to run cosmic-ray exec: {err}"))?;
    if !exec.status.success() {
        return Err(
            command_failure_reason("cosmic-ray exec", exec.status, &exec.stderr)
                .replace("coverage unknown", "mutation unknown"),
        );
    }
    let text = dump_sqlite_to_json(&session)?;
    MutantOutcomes::parse(&text).map_err(|err| err.to_string())
}

fn cosmic_ray_config(root: &Path) -> Option<PathBuf> {
    [
        "cosmic-ray.toml",
        "cosmic_ray.toml",
        "cosmic-ray.ini",
        "cosmic_ray.ini",
    ]
    .iter()
    .map(|name| root.join(name))
    .find(|path| path.exists())
}

fn dump_sqlite_to_json(path: &Path) -> std::result::Result<String, String> {
    const SCRIPT: &str = r#"
import json
import sqlite3
import sys

def quote_ident(name):
    return '"' + name.replace('"', '""') + '"'

def decode(value):
    if isinstance(value, bytes):
        return value.decode("utf-8", "replace")
    if isinstance(value, str):
        text = value.strip()
        if text.startswith("{") or text.startswith("["):
            try:
                return json.loads(text)
            except Exception:
                return value
    return value

db = sqlite3.connect(sys.argv[1])
rows = []
for (table,) in db.execute("select name from sqlite_master where type='table'"):
    cols = [row[1] for row in db.execute("pragma table_info(%s)" % quote_ident(table))]
    for row in db.execute("select * from %s" % quote_ident(table)):
        item = {"__table": table}
        item.update({col: decode(value) for col, value in zip(cols, row)})
        rows.append(item)
print(json.dumps({"cosmic_ray_sqlite": rows}, default=str))
"#;
    let output = Command::new("python3")
        .arg("-c")
        .arg(SCRIPT)
        .arg(path)
        .output()
        .map_err(|err| format!("failed to inspect cosmic-ray sqlite with python3: {err}"))?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|err| format!("cosmic-ray sqlite dump was not utf8: {err}"))
    } else {
        Err(command_failure_reason(
            "python3 cosmic-ray sqlite dump",
            output.status,
            &output.stderr,
        )
        .replace("coverage unknown", "mutation unknown"))
    }
}

#[derive(Debug, Clone)]
struct MutantOutcomes {
    outcomes: Vec<MutantOutcome>,
}

#[derive(Debug, Clone)]
struct MutantOutcome {
    path: PathBuf,
    start_line: usize,
    end_line: usize,
    kind: MutantOutcomeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MutantOutcomeKind {
    Missed,
    Caught,
    Other,
}

impl MutantOutcomes {
    fn parse(text: &str) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let mut entries = Vec::new();
        collect_outcome_entries(&value, &mut entries);
        let outcomes = entries.into_iter().filter_map(mutant_outcome).collect();
        Ok(Self { outcomes })
    }

    fn has_surviving_mutant(
        &self,
        absolute_path: &Path,
        relative_path: &Path,
        start_line: usize,
        end_line: usize,
    ) -> bool {
        self.outcomes.iter().any(|outcome| {
            outcome.kind == MutantOutcomeKind::Missed
                && mutation_path_matches(&outcome.path, absolute_path, relative_path)
                && line_ranges_overlap(outcome.start_line, outcome.end_line, start_line, end_line)
        })
    }
}

fn collect_outcome_entries<'a>(
    value: &'a serde_json::Value,
    entries: &mut Vec<&'a serde_json::Value>,
) {
    match value {
        serde_json::Value::Object(map) => {
            if map.contains_key("outcome")
                || map.contains_key("status")
                || map.contains_key("result")
                || map.contains_key("test_outcome")
                || map.contains_key("test-outcome")
            {
                entries.push(value);
                return;
            }
            visit_json_children(value, |child| collect_outcome_entries(child, entries));
        }
        serde_json::Value::Array(_) => {
            visit_json_children(value, |child| collect_outcome_entries(child, entries));
        }
        _ => {}
    }
}

fn visit_json_children<'a>(
    value: &'a serde_json::Value,
    mut visit: impl FnMut(&'a serde_json::Value),
) {
    match value {
        serde_json::Value::Object(map) => {
            for child in map.values() {
                visit(child);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                visit(child);
            }
        }
        _ => {}
    }
}

fn mutant_outcome(value: &serde_json::Value) -> Option<MutantOutcome> {
    let kind = outcome_kind(outcome_text(value)?)?;
    let path = PathBuf::from(find_string_by_keys(
        value,
        &[
            "source_file",
            "source_path",
            "module_path",
            "module-path",
            "filename",
            "file",
            "path",
        ],
    )?);
    let start_line = find_usize_by_keys(
        value,
        &[
            "start_line",
            "line_start",
            "line_number",
            "line-number",
            "line",
        ],
    )?;
    let end_line = find_usize_by_keys(value, &["end_line", "line_end", "end_line_number"])
        .unwrap_or(start_line);
    Some(MutantOutcome {
        path,
        start_line,
        end_line,
        kind,
    })
}

fn outcome_text(value: &serde_json::Value) -> Option<&str> {
    find_string_by_keys(
        value,
        &[
            "outcome",
            "status",
            "result",
            "test_outcome",
            "test-outcome",
        ],
    )
}

fn outcome_kind(text: &str) -> Option<MutantOutcomeKind> {
    let text = text.to_ascii_lowercase();
    if matches!(text.as_str(), "missed" | "survived" | "surviving") {
        Some(MutantOutcomeKind::Missed)
    } else if matches!(text.as_str(), "caught" | "killed" | "detected") {
        Some(MutantOutcomeKind::Caught)
    } else {
        Some(MutantOutcomeKind::Other)
    }
}

fn find_string_by_keys<'a>(value: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(text) = map.get(*key).and_then(serde_json::Value::as_str) {
                    return Some(text);
                }
            }
            for child in map.values() {
                if let Some(text) = find_string_by_keys(child, keys) {
                    return Some(text);
                }
            }
            None
        }
        serde_json::Value::Array(values) => values
            .iter()
            .find_map(|child| find_string_by_keys(child, keys)),
        _ => None,
    }
}

fn find_usize_by_keys(value: &serde_json::Value, keys: &[&str]) -> Option<usize> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(number) = map.get(*key).and_then(serde_json::Value::as_u64) {
                    return usize::try_from(number).ok();
                }
            }
            for child in map.values() {
                if let Some(number) = find_usize_by_keys(child, keys) {
                    return Some(number);
                }
            }
            None
        }
        serde_json::Value::Array(values) => values
            .iter()
            .find_map(|child| find_usize_by_keys(child, keys)),
        _ => None,
    }
}

fn mutation_path_matches(path: &Path, absolute_path: &Path, relative_path: &Path) -> bool {
    path == absolute_path
        || path == relative_path
        || path
            .file_name()
            .is_some_and(|name| Some(name) == relative_path.file_name())
}

fn line_ranges_overlap(
    left_start: usize,
    left_end: usize,
    right_start: usize,
    right_end: usize,
) -> bool {
    left_start <= right_end && right_start <= left_end
}

struct CoverageRegistry {
    providers: Vec<Box<dyn CoverageProvider>>,
    disabled: bool,
}

impl CoverageRegistry {
    fn new(config: &CoverageConfig) -> Self {
        Self {
            providers: vec![
                Box::new(RustCargoLlvmCovProvider::new(config)),
                Box::new(ClojureCloverageProvider::new(config)),
                Box::new(JuliaCoverageProvider::new(config)),
                Box::new(PythonCoveragePyProvider::new(config)),
            ],
            disabled: matches!(config, CoverageConfig::Disabled),
        }
    }

    fn assess(&mut self, request: CoverageRequest<'_>) -> Result<CoverageAssessment> {
        if self.disabled {
            return Ok(CoverageAssessment {
                status: CoverageStatus::Unknown,
                reason: Some("coverage disabled".to_string()),
            });
        }
        let Some(provider) = self
            .providers
            .iter_mut()
            .find(|provider| provider.supports(request.source))
        else {
            return Ok(CoverageAssessment {
                status: CoverageStatus::Unknown,
                reason: Some("no coverage provider for language".to_string()),
            });
        };
        provider.assess(request)
    }
}

#[derive(Debug, Clone)]
enum CoverageProviderMode {
    Disabled,
    Auto { command: String },
    LcovFile(PathBuf),
    CloverageFile(PathBuf),
    JuliaCovFile(PathBuf),
    CoveragePyFile(PathBuf),
}

struct RustCargoLlvmCovProvider {
    mode: CoverageProviderMode,
    lcov: Option<Result<LcovCoverage, String>>,
}

impl RustCargoLlvmCovProvider {
    fn new(config: &CoverageConfig) -> Self {
        let mode = match config {
            CoverageConfig::Disabled => CoverageProviderMode::Disabled,
            CoverageConfig::Auto => CoverageProviderMode::Auto {
                command: "cargo".to_string(),
            },
            CoverageConfig::AutoWithCommand(command) => CoverageProviderMode::Auto {
                command: command.to_owned(),
            },
            CoverageConfig::LcovFile(path) => CoverageProviderMode::LcovFile(path.to_path_buf()),
            CoverageConfig::CloverageFile(path) => {
                CoverageProviderMode::CloverageFile(path.to_path_buf())
            }
            CoverageConfig::JuliaCovFile(path) => {
                CoverageProviderMode::JuliaCovFile(path.to_path_buf())
            }
            CoverageConfig::CoveragePyFile(path) => {
                CoverageProviderMode::CoveragePyFile(path.to_path_buf())
            }
        };
        Self { mode, lcov: None }
    }

    fn lcov(&mut self, root: &Path) -> Result<&LcovCoverage, CoverageAssessment> {
        if self.lcov.is_none() {
            self.lcov = Some(match self.load_lcov(root) {
                Ok(coverage) => Ok(coverage),
                Err(reason) => Err(reason),
            });
        }
        match self.lcov.as_ref().expect("coverage initialized") {
            Ok(coverage) => Ok(coverage),
            Err(reason) => Err(CoverageAssessment {
                status: CoverageStatus::Unknown,
                reason: Some(reason.to_owned()),
            }),
        }
    }

    fn load_lcov(&self, root: &Path) -> std::result::Result<LcovCoverage, String> {
        match &self.mode {
            CoverageProviderMode::Disabled
            | CoverageProviderMode::CloverageFile(_)
            | CoverageProviderMode::JuliaCovFile(_)
            | CoverageProviderMode::CoveragePyFile(_) => Err("coverage disabled".to_string()),
            CoverageProviderMode::LcovFile(path) => {
                let text = read_report_text(path, "coverage LCOV")?;
                LcovCoverage::parse(&text).map_err(|err| err.to_string())
            }
            CoverageProviderMode::Auto { command } => {
                if !cargo_llvm_cov_available(command, root) {
                    return Err("coverage-unknown: cargo-llvm-cov not available".to_string());
                }
                let text = run_output_file_command(
                    command,
                    root,
                    "cargo-llvm-cov",
                    "coverage",
                    "coverage.lcov",
                    "coverage LCOV",
                    |cmd, output_path| {
                        cmd.args(["llvm-cov", "--workspace", "--lcov", "--output-path"])
                            .arg(output_path);
                    },
                )?;
                LcovCoverage::parse(&text).map_err(|err| err.to_string())
            }
        }
    }
}

impl CoverageProvider for RustCargoLlvmCovProvider {
    fn name(&self) -> &'static str {
        "cargo-llvm-cov"
    }

    fn supports(&self, source: &SourceFile) -> bool {
        source.lang == Lang::Rust
    }

    fn assess(&mut self, request: CoverageRequest<'_>) -> Result<CoverageAssessment> {
        let coverage = match self.lcov(request.root) {
            Ok(coverage) => coverage,
            Err(assessment) => return Ok(assessment),
        };
        let relative = relative_to_root(request.root, &request.source.path)?;
        Ok(
            match coverage.region_status(
                &request.source.path,
                &relative,
                request.work_order.region.start_line,
                request.work_order.region.end_line,
            ) {
                CoverageStatus::Covered => CoverageAssessment {
                    status: CoverageStatus::Covered,
                    reason: Some(format!(
                        "coverage provider {} exercised region",
                        self.name()
                    )),
                },
                CoverageStatus::Uncovered => CoverageAssessment {
                    status: CoverageStatus::Uncovered,
                    reason: Some(format!(
                        "coverage provider {} did not exercise region",
                        self.name()
                    )),
                },
                CoverageStatus::Unknown => CoverageAssessment {
                    status: CoverageStatus::Unknown,
                    reason: Some(format!(
                        "coverage provider {} had no executable lines for region",
                        self.name()
                    )),
                },
            },
        )
    }
}

struct ClojureCloverageProvider {
    mode: CoverageProviderMode,
    coverage: Option<Result<LineCoverage, String>>,
}

impl ClojureCloverageProvider {
    fn new(config: &CoverageConfig) -> Self {
        let mode = match config {
            CoverageConfig::Disabled => CoverageProviderMode::Disabled,
            CoverageConfig::Auto => CoverageProviderMode::Auto {
                command: "lein".to_string(),
            },
            CoverageConfig::AutoWithCommand(command) => CoverageProviderMode::Auto {
                command: command.to_owned(),
            },
            CoverageConfig::CloverageFile(path) => {
                CoverageProviderMode::CloverageFile(path.to_path_buf())
            }
            CoverageConfig::LcovFile(path) => CoverageProviderMode::LcovFile(path.to_path_buf()),
            CoverageConfig::JuliaCovFile(path) => {
                CoverageProviderMode::JuliaCovFile(path.to_path_buf())
            }
            CoverageConfig::CoveragePyFile(path) => {
                CoverageProviderMode::CoveragePyFile(path.to_path_buf())
            }
        };
        Self {
            mode,
            coverage: None,
        }
    }

    fn coverage(&mut self, root: &Path) -> Result<&LineCoverage, CoverageAssessment> {
        if self.coverage.is_none() {
            self.coverage = Some(match self.load_coverage(root) {
                Ok(coverage) => Ok(coverage),
                Err(reason) => Err(reason),
            });
        }
        match self.coverage.as_ref().expect("coverage initialized") {
            Ok(coverage) => Ok(coverage),
            Err(reason) => Err(CoverageAssessment {
                status: CoverageStatus::Unknown,
                reason: Some(reason.to_owned()),
            }),
        }
    }

    fn load_coverage(&self, root: &Path) -> std::result::Result<LineCoverage, String> {
        match &self.mode {
            CoverageProviderMode::CloverageFile(path) => {
                let text = read_report_text(path, "cloverage report")?;
                LineCoverage::parse_cloverage(&text).map_err(|err| err.to_string())
            }
            CoverageProviderMode::Auto { command } => {
                if !cloverage_available(command, root) {
                    return Err("coverage-unknown: cloverage not available".to_string());
                }
                let temp = TempDir::new()
                    .map_err(|err| format!("failed to create cloverage tempdir: {err}"))?;
                run_coverage_tool(cloverage_command(command, temp.path()), root, "cloverage")?;
                let report = find_named_file(temp.path(), "coverage.json").ok_or_else(|| {
                    "coverage-unknown: cloverage produced no coverage.json".to_string()
                })?;
                let text = read_report_text(&report, "cloverage report")?;
                LineCoverage::parse_cloverage(&text).map_err(|err| err.to_string())
            }
            _ => Err("coverage disabled".to_string()),
        }
    }
}

impl CoverageProvider for ClojureCloverageProvider {
    fn name(&self) -> &'static str {
        "cloverage"
    }

    fn supports(&self, source: &SourceFile) -> bool {
        source.lang == Lang::Clojure
    }

    fn assess(&mut self, request: CoverageRequest<'_>) -> Result<CoverageAssessment> {
        let name = self.name();
        let coverage = match self.coverage(request.root) {
            Ok(coverage) => coverage,
            Err(assessment) => return Ok(assessment),
        };
        assess_line_coverage(name, coverage, request)
    }
}

struct JuliaCoverageProvider {
    mode: CoverageProviderMode,
    coverage: Option<Result<LineCoverage, String>>,
}

impl JuliaCoverageProvider {
    fn new(config: &CoverageConfig) -> Self {
        let mode = match config {
            CoverageConfig::Disabled => CoverageProviderMode::Disabled,
            CoverageConfig::Auto => CoverageProviderMode::Auto {
                command: "julia".to_string(),
            },
            CoverageConfig::AutoWithCommand(command) => CoverageProviderMode::Auto {
                command: command.to_owned(),
            },
            CoverageConfig::JuliaCovFile(path) => {
                CoverageProviderMode::JuliaCovFile(path.to_path_buf())
            }
            CoverageConfig::LcovFile(path) => CoverageProviderMode::LcovFile(path.to_path_buf()),
            CoverageConfig::CloverageFile(path) => {
                CoverageProviderMode::CloverageFile(path.to_path_buf())
            }
            CoverageConfig::CoveragePyFile(path) => {
                CoverageProviderMode::CoveragePyFile(path.to_path_buf())
            }
        };
        Self {
            mode,
            coverage: None,
        }
    }

    fn coverage(&mut self, root: &Path) -> Result<&LineCoverage, CoverageAssessment> {
        if self.coverage.is_none() {
            self.coverage = Some(match self.load_coverage(root) {
                Ok(coverage) => Ok(coverage),
                Err(reason) => Err(reason),
            });
        }
        match self.coverage.as_ref().expect("coverage initialized") {
            Ok(coverage) => Ok(coverage),
            Err(reason) => Err(CoverageAssessment {
                status: CoverageStatus::Unknown,
                reason: Some(reason.to_owned()),
            }),
        }
    }

    fn load_coverage(&self, root: &Path) -> std::result::Result<LineCoverage, String> {
        match &self.mode {
            CoverageProviderMode::JuliaCovFile(path) => {
                let text = read_report_text(path, "Coverage.jl .cov")?;
                Ok(LineCoverage::from_julia_cov(path, &text))
            }
            CoverageProviderMode::LcovFile(path) => {
                let text = read_report_text(path, "Coverage.jl LCOV")?;
                LineCoverage::parse_lcov(&text).map_err(|err| err.to_string())
            }
            CoverageProviderMode::Auto { command } => {
                if !julia_available(command, root) {
                    return Err("coverage-unknown: julia not available".to_string());
                }
                run_julia_coverage(command, root)
            }
            _ => Err("coverage disabled".to_string()),
        }
    }
}

impl CoverageProvider for JuliaCoverageProvider {
    fn name(&self) -> &'static str {
        "Coverage.jl"
    }

    fn supports(&self, source: &SourceFile) -> bool {
        source.lang == Lang::Julia
    }

    fn assess(&mut self, request: CoverageRequest<'_>) -> Result<CoverageAssessment> {
        let name = self.name();
        let coverage = match self.coverage(request.root) {
            Ok(coverage) => coverage,
            Err(assessment) => return Ok(assessment),
        };
        assess_line_coverage(name, coverage, request)
    }
}

struct PythonCoveragePyProvider {
    mode: CoverageProviderMode,
    coverage: Option<Result<LineCoverage, String>>,
}

impl PythonCoveragePyProvider {
    fn new(config: &CoverageConfig) -> Self {
        let mode = match config {
            CoverageConfig::Disabled => CoverageProviderMode::Disabled,
            CoverageConfig::Auto => CoverageProviderMode::Auto {
                command: "coverage".to_string(),
            },
            CoverageConfig::AutoWithCommand(command) => CoverageProviderMode::Auto {
                command: command.to_owned(),
            },
            CoverageConfig::CoveragePyFile(path) => {
                CoverageProviderMode::CoveragePyFile(path.to_path_buf())
            }
            CoverageConfig::LcovFile(path) => CoverageProviderMode::LcovFile(path.to_path_buf()),
            CoverageConfig::CloverageFile(path) => {
                CoverageProviderMode::CloverageFile(path.to_path_buf())
            }
            CoverageConfig::JuliaCovFile(path) => {
                CoverageProviderMode::JuliaCovFile(path.to_path_buf())
            }
        };
        Self {
            mode,
            coverage: None,
        }
    }

    fn coverage(&mut self, root: &Path) -> Result<&LineCoverage, CoverageAssessment> {
        if self.coverage.is_none() {
            self.coverage = Some(match self.load_coverage(root) {
                Ok(coverage) => Ok(coverage),
                Err(reason) => Err(reason),
            });
        }
        match self.coverage.as_ref().expect("coverage initialized") {
            Ok(coverage) => Ok(coverage),
            Err(reason) => Err(CoverageAssessment {
                status: CoverageStatus::Unknown,
                reason: Some(reason.to_owned()),
            }),
        }
    }

    fn load_coverage(&self, root: &Path) -> std::result::Result<LineCoverage, String> {
        match &self.mode {
            CoverageProviderMode::CoveragePyFile(path) => {
                let text = read_report_text(path, "coverage.py report")?;
                LineCoverage::parse_coverage_py(&text).map_err(|err| err.to_string())
            }
            CoverageProviderMode::Auto { command } => {
                if !coverage_py_available(command, root) {
                    return Err("coverage-unknown: coverage.py not available".to_string());
                }
                run_coverage_py(command, root)
            }
            _ => Err("coverage disabled".to_string()),
        }
    }
}

impl CoverageProvider for PythonCoveragePyProvider {
    fn name(&self) -> &'static str {
        "coverage.py"
    }

    fn supports(&self, source: &SourceFile) -> bool {
        source.lang == Lang::Python
    }

    fn assess(&mut self, request: CoverageRequest<'_>) -> Result<CoverageAssessment> {
        let name = self.name();
        let coverage = match self.coverage(request.root) {
            Ok(coverage) => coverage,
            Err(assessment) => return Ok(assessment),
        };
        assess_line_coverage(name, coverage, request)
    }
}

fn assess_line_coverage(
    provider: &str,
    coverage: &LineCoverage,
    request: CoverageRequest<'_>,
) -> Result<CoverageAssessment> {
    let relative = relative_to_root(request.root, &request.source.path)?;
    Ok(
        match coverage.region_status(
            &request.source.path,
            &relative,
            request.work_order.region.start_line,
            request.work_order.region.end_line,
        ) {
            CoverageStatus::Covered => CoverageAssessment {
                status: CoverageStatus::Covered,
                reason: Some(format!("coverage provider {provider} exercised region")),
            },
            CoverageStatus::Uncovered => CoverageAssessment {
                status: CoverageStatus::Uncovered,
                reason: Some(format!(
                    "coverage provider {provider} did not exercise region"
                )),
            },
            CoverageStatus::Unknown => CoverageAssessment {
                status: CoverageStatus::Unknown,
                reason: Some(format!(
                    "coverage provider {provider} had no executable lines for region"
                )),
            },
        },
    )
}

fn cargo_llvm_cov_available(command: &str, root: &Path) -> bool {
    Command::new(command)
        .args(["llvm-cov", "--version"])
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn cloverage_available(command: &str, root: &Path) -> bool {
    Command::new(command)
        .args(["cloverage", "--help"])
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn julia_available(command: &str, root: &Path) -> bool {
    Command::new(command)
        .arg("--version")
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn coverage_py_available(command: &str, root: &Path) -> bool {
    Command::new(command)
        .arg("--version")
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CoverageToolCommand {
    program: String,
    args: Vec<String>,
}

impl CoverageToolCommand {
    fn new(program: &str, args: impl IntoIterator<Item = String>) -> Self {
        Self {
            program: program.to_string(),
            args: args.into_iter().collect(),
        }
    }
}

fn cloverage_command(command: &str, output_dir: &Path) -> CoverageToolCommand {
    CoverageToolCommand::new(
        command,
        [
            "cloverage".to_string(),
            "--json".to_string(),
            "--output".to_string(),
            output_dir.display().to_string(),
        ],
    )
}

fn julia_coverage_command(command: &str) -> CoverageToolCommand {
    CoverageToolCommand::new(
        command,
        [
            "--startup-file=no".to_string(),
            "--code-coverage=user".to_string(),
            "-e".to_string(),
            "using Pkg; Pkg.test()".to_string(),
        ],
    )
}

fn coverage_py_run_command(command: &str) -> CoverageToolCommand {
    CoverageToolCommand::new(
        command,
        [
            "run".to_string(),
            "-m".to_string(),
            "unittest".to_string(),
            "discover".to_string(),
        ],
    )
}

fn coverage_py_json_command(command: &str, output_path: &Path) -> CoverageToolCommand {
    CoverageToolCommand::new(
        command,
        [
            "json".to_string(),
            "-o".to_string(),
            output_path.display().to_string(),
        ],
    )
}

fn run_coverage_tool(
    tool: CoverageToolCommand,
    root: &Path,
    tool_name: &str,
) -> std::result::Result<(), String> {
    run_coverage_tool_with_env(tool, root, tool_name, &[])
}

fn run_coverage_tool_with_env(
    tool: CoverageToolCommand,
    root: &Path,
    tool_name: &str,
    envs: &[(&str, &Path)],
) -> std::result::Result<(), String> {
    let mut command = Command::new(&tool.program);
    command.args(&tool.args).current_dir(root);
    for (name, value) in envs {
        command.env(name, value);
    }
    let output = command
        .output()
        .map_err(|err| format!("failed to run {tool_name}: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_failure_reason(
            tool_name,
            output.status,
            &output.stderr,
        ))
    }
}

fn run_julia_coverage(command: &str, root: &Path) -> std::result::Result<LineCoverage, String> {
    let temp =
        TempDir::new().map_err(|err| format!("failed to create julia coverage tempdir: {err}"))?;
    copy_project_for_check(root, temp.path())
        .map_err(|err| format!("failed to copy project for julia coverage: {err}"))?;
    run_coverage_tool(
        julia_coverage_command(command),
        temp.path(),
        "julia coverage",
    )?;
    let cov_files = find_files_with_extension(temp.path(), "cov");
    if cov_files.is_empty() {
        return Err("coverage-unknown: julia produced no .cov files".to_string());
    }
    LineCoverage::from_julia_cov_files(root, temp.path(), &cov_files)
}

fn run_coverage_py(command: &str, root: &Path) -> std::result::Result<LineCoverage, String> {
    let temp =
        TempDir::new().map_err(|err| format!("failed to create coverage.py tempdir: {err}"))?;
    let data_file = temp.path().join(".coverage");
    let report = temp.path().join("coverage.json");
    run_coverage_tool_with_env(
        coverage_py_run_command(command),
        root,
        "coverage.py run",
        &[("COVERAGE_FILE", &data_file)],
    )?;
    run_coverage_tool_with_env(
        coverage_py_json_command(command, &report),
        root,
        "coverage.py json",
        &[("COVERAGE_FILE", &data_file)],
    )?;
    let text = read_report_text(&report, "coverage.py report")?;
    LineCoverage::parse_coverage_py(&text).map_err(|err| err.to_string())
}

fn run_output_file_command(
    command: &str,
    root: &Path,
    tool_name: &str,
    temp_label: &str,
    output_name: &str,
    read_label: &str,
    configure: impl FnOnce(&mut Command, &Path),
) -> std::result::Result<String, String> {
    let temp =
        TempDir::new().map_err(|err| format!("failed to create {temp_label} tempdir: {err}"))?;
    let output_path = temp.path().join(output_name);
    let mut cmd = Command::new(command);
    configure(&mut cmd, &output_path);
    let output = cmd
        .current_dir(root)
        .output()
        .map_err(|err| format!("failed to run {tool_name}: {err}"))?;
    if !output.status.success() {
        return Err(command_failure_reason(
            tool_name,
            output.status,
            &output.stderr,
        ));
    }
    read_report_text(&output_path, read_label)
}

fn read_report_text(path: &Path, label: &str) -> std::result::Result<String, String> {
    fs::read_to_string(path)
        .map_err(|err| format!("failed to read {label} {}: {err}", path.display()))
}

fn find_named_file(root: &Path, name: &str) -> Option<PathBuf> {
    for entry in ignore::WalkBuilder::new(root).hidden(false).build() {
        let Ok(entry) = entry else {
            continue;
        };
        if entry.file_type().is_some_and(|kind| kind.is_file()) && entry.file_name() == name {
            return Some(entry.path().to_path_buf());
        }
    }
    None
}

fn find_files_with_extension(root: &Path, extension: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in ignore::WalkBuilder::new(root).hidden(false).build() {
        let Ok(entry) = entry else {
            continue;
        };
        if entry.file_type().is_some_and(|kind| kind.is_file())
            && entry.path().extension().and_then(|value| value.to_str()) == Some(extension)
        {
            paths.push(entry.path().to_path_buf());
        }
    }
    paths.sort();
    paths
}

fn command_failure_reason(name: &str, status: std::process::ExitStatus, stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    if stderr.trim().is_empty() {
        format!("{name} failed with status {status}; coverage unknown")
    } else {
        format!(
            "{name} failed with status {status}: {}; coverage unknown",
            stderr.trim()
        )
    }
}

#[derive(Debug, Clone)]
struct LineCoverage {
    files: Vec<LineCoverageFile>,
}

#[derive(Debug, Clone)]
struct LineCoverageFile {
    path: PathBuf,
    lines: BTreeMap<usize, usize>,
}

impl LineCoverage {
    fn parse_lcov(text: &str) -> Result<Self> {
        let lcov = LcovCoverage::parse(text)?;
        Ok(Self {
            files: lcov
                .files
                .into_iter()
                .map(|file| LineCoverageFile {
                    path: file.path,
                    lines: file.lines,
                })
                .collect(),
        })
    }

    fn parse_cloverage(text: &str) -> Result<Self> {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
            return Ok(Self {
                files: line_files_from_json(&value),
            });
        }
        Ok(Self {
            files: line_files_from_edn_like(text),
        })
    }

    fn from_julia_cov(path: &Path, text: &str) -> Self {
        Self::from_julia_cov_text(path_without_cov_suffix(path), text)
    }

    fn from_julia_cov_files(
        root: &Path,
        temp_root: &Path,
        paths: &[PathBuf],
    ) -> std::result::Result<Self, String> {
        let mut files = Vec::new();
        for path in paths {
            let text = read_report_text(path, "Coverage.jl .cov")?;
            let temp_source = path_without_cov_suffix(path);
            let source = match temp_source.strip_prefix(temp_root) {
                Ok(relative) => root.join(relative),
                Err(_) => temp_source,
            };
            files.extend(Self::from_julia_cov_text(source, &text).files);
        }
        Ok(Self { files })
    }

    fn from_julia_cov_text(path: PathBuf, text: &str) -> Self {
        let mut lines = BTreeMap::new();
        for (idx, line) in text.lines().enumerate() {
            if let Some(count) = julia_cov_line_count(line) {
                lines.insert(idx + 1, count);
            }
        }
        Self {
            files: vec![LineCoverageFile { path, lines }],
        }
    }

    fn parse_coverage_py(text: &str) -> Result<Self> {
        if text.trim_start().starts_with('<') {
            Ok(Self {
                files: line_files_from_coverage_xml(text),
            })
        } else {
            let value: serde_json::Value = serde_json::from_str(text)?;
            Ok(Self {
                files: line_files_from_coverage_py_json(&value),
            })
        }
    }

    fn region_status(
        &self,
        absolute_path: &Path,
        relative_path: &Path,
        start_line: usize,
        end_line: usize,
    ) -> CoverageStatus {
        let Some(file) = self.file_for(absolute_path, relative_path) else {
            return CoverageStatus::Unknown;
        };
        coverage_status_for_lines(&file.lines, start_line, end_line)
    }

    fn file_for(&self, absolute_path: &Path, relative_path: &Path) -> Option<&LineCoverageFile> {
        self.files
            .iter()
            .find(|file| coverage_path_matches(&file.path, absolute_path, relative_path))
    }
}

fn line_files_from_json(value: &serde_json::Value) -> Vec<LineCoverageFile> {
    let mut files = Vec::new();
    collect_line_files_from_json(value, &mut files);
    files
}

fn coverage_status_for_lines(
    lines: &BTreeMap<usize, usize>,
    start_line: usize,
    end_line: usize,
) -> CoverageStatus {
    let mut saw_executable_line = false;
    for line in start_line..=end_line {
        if let Some(count) = lines.get(&line) {
            saw_executable_line = true;
            if *count == 0 {
                return CoverageStatus::Uncovered;
            }
        }
    }
    if saw_executable_line {
        CoverageStatus::Covered
    } else {
        CoverageStatus::Unknown
    }
}

fn collect_line_files_from_json(value: &serde_json::Value, files: &mut Vec<LineCoverageFile>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(path) = string_field(map, &["filename", "file", "path", "name"]) {
                let lines = json_line_counts(value);
                if !lines.is_empty() {
                    files.push(LineCoverageFile {
                        path: PathBuf::from(path),
                        lines,
                    });
                    return;
                }
            }
            visit_json_children(value, |child| collect_line_files_from_json(child, files));
        }
        serde_json::Value::Array(_) => {
            visit_json_children(value, |child| collect_line_files_from_json(child, files));
        }
        _ => {}
    }
}

fn json_line_counts(value: &serde_json::Value) -> BTreeMap<usize, usize> {
    let mut lines = BTreeMap::new();
    collect_json_line_counts(value, &mut lines);
    lines
}

fn collect_json_line_counts(value: &serde_json::Value, lines: &mut BTreeMap<usize, usize>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(line) = usize_field(map, &["line", "line_no", "number"]) {
                let count = usize_field(map, &["hits", "count", "covered", "coverage"])
                    .or_else(|| bool_field(map, &["covered"]).map(usize::from))
                    .unwrap_or(0);
                lines.insert(line, count);
                return;
            }
            visit_json_children(value, |child| collect_json_line_counts(child, lines));
        }
        serde_json::Value::Array(_) => {
            visit_json_children(value, |child| collect_json_line_counts(child, lines));
        }
        _ => {}
    }
}

fn line_files_from_coverage_py_json(value: &serde_json::Value) -> Vec<LineCoverageFile> {
    let mut files = Vec::new();
    if let Some(file_map) = value.get("files").and_then(serde_json::Value::as_object) {
        for (path, file) in file_map {
            let mut lines = BTreeMap::new();
            for line in json_usize_array(file.get("executed_lines")) {
                lines.insert(line, 1);
            }
            for line in json_usize_array(file.get("missing_lines")) {
                lines.insert(line, 0);
            }
            files.push(LineCoverageFile {
                path: PathBuf::from(path),
                lines,
            });
        }
    }
    files
}

fn line_files_from_coverage_xml(text: &str) -> Vec<LineCoverageFile> {
    let mut files = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_lines = BTreeMap::new();
    for line in text.lines() {
        if line.contains("<class ") {
            if let Some(path) = current_path.take() {
                files.push(LineCoverageFile {
                    path,
                    lines: std::mem::take(&mut current_lines),
                });
            }
            current_path = xml_attr(line, "filename").map(PathBuf::from);
            continue;
        }
        if line.contains("<line ")
            && let Some(number) = xml_attr(line, "number").and_then(|value| value.parse().ok())
        {
            let hits = xml_attr(line, "hits")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            current_lines.insert(number, hits);
        }
    }
    if let Some(path) = current_path {
        files.push(LineCoverageFile {
            path,
            lines: current_lines,
        });
    }
    files
}

fn line_files_from_edn_like(text: &str) -> Vec<LineCoverageFile> {
    let mut grouped: BTreeMap<PathBuf, BTreeMap<usize, usize>> = BTreeMap::new();
    for line in text.lines() {
        let Some(path) = edn_string_after(line, ":filename")
            .or_else(|| edn_string_after(line, ":file"))
            .or_else(|| edn_string_after(line, ":path"))
        else {
            continue;
        };
        let Some(line_no) = edn_usize_after(line, ":line") else {
            continue;
        };
        let count = edn_usize_after(line, ":hits")
            .or_else(|| edn_usize_after(line, ":count"))
            .unwrap_or(0);
        grouped
            .entry(PathBuf::from(path))
            .or_default()
            .insert(line_no, count);
    }
    grouped
        .into_iter()
        .map(|(path, lines)| LineCoverageFile { path, lines })
        .collect()
}

fn julia_cov_line_count(line: &str) -> Option<usize> {
    let token = line.split_whitespace().next()?;
    match token {
        "-" => None,
        "#####" => Some(0),
        value => value.trim_end_matches(':').parse().ok(),
    }
}

fn path_without_cov_suffix(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(stripped) = text.strip_suffix(".cov") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

fn string_field<'a>(
    map: &'a serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| map.get(*key).and_then(serde_json::Value::as_str))
}

fn usize_field(map: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<usize> {
    keys.iter().find_map(|key| {
        map.get(*key)
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
    })
}

fn bool_field(map: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| map.get(*key).and_then(serde_json::Value::as_bool))
}

fn json_usize_array(value: Option<&serde_json::Value>) -> Vec<usize> {
    value
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_u64())
        .filter_map(|value| usize::try_from(value).ok())
        .collect()
}

fn xml_attr<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=\"");
    let start = line.find(&prefix)? + prefix.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn edn_string_after(line: &str, key: &str) -> Option<String> {
    let idx = line.find(key)? + key.len();
    let rest = line[idx..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn edn_usize_after(line: &str, key: &str) -> Option<usize> {
    let idx = line.find(key)? + key.len();
    let rest = line[idx..].trim_start();
    let token = rest
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | '}'))
        .find(|token| !token.is_empty())?;
    token.parse().ok()
}

fn coverage_path_matches(path: &Path, absolute_path: &Path, relative_path: &Path) -> bool {
    path == absolute_path
        || path == relative_path
        || path
            .file_name()
            .is_some_and(|name| Some(name) == relative_path.file_name())
}

#[derive(Debug, Clone)]
struct LcovCoverage {
    files: Vec<LcovFile>,
}

#[derive(Debug, Clone)]
struct LcovFile {
    path: PathBuf,
    lines: BTreeMap<usize, usize>,
}

impl LcovCoverage {
    fn parse(text: &str) -> Result<Self> {
        let mut files = Vec::new();
        let mut current_path: Option<PathBuf> = None;
        let mut current_lines = BTreeMap::new();
        for line in text.lines() {
            if let Some(path) = line.strip_prefix("SF:") {
                if let Some(path) = current_path.replace(PathBuf::from(path)) {
                    push_lcov_file(&mut files, path, &mut current_lines);
                }
                continue;
            }
            if let Some(data) = line.strip_prefix("DA:") {
                let Some((line_no, count)) = data.split_once(',') else {
                    bail!("invalid LCOV DA entry `{line}`");
                };
                current_lines.insert(line_no.parse()?, count.parse()?);
                continue;
            }
            if line == "end_of_record"
                && let Some(path) = current_path.take()
            {
                push_lcov_file(&mut files, path, &mut current_lines);
            }
        }
        if let Some(path) = current_path {
            files.push(LcovFile {
                path,
                lines: current_lines,
            });
        }
        Ok(Self { files })
    }

    fn region_status(
        &self,
        absolute_path: &Path,
        relative_path: &Path,
        start_line: usize,
        end_line: usize,
    ) -> CoverageStatus {
        let Some(file) = self.file_for(absolute_path, relative_path) else {
            return CoverageStatus::Unknown;
        };
        coverage_status_for_lines(&file.lines, start_line, end_line)
    }

    fn file_for(&self, absolute_path: &Path, relative_path: &Path) -> Option<&LcovFile> {
        self.files.iter().find(|file| {
            file.path == absolute_path
                || file.path == relative_path
                || file
                    .path
                    .file_name()
                    .is_some_and(|name| Some(name) == relative_path.file_name())
        })
    }
}

fn push_lcov_file(
    files: &mut Vec<LcovFile>,
    path: PathBuf,
    current_lines: &mut BTreeMap<usize, usize>,
) {
    files.push(LcovFile {
        path,
        lines: std::mem::take(current_lines),
    });
}

fn current_work_orders(root: &Path) -> Result<BTreeMap<String, WorkOrder>> {
    let reports = scan_paths(&[root.to_path_buf()])?;
    let mut out = BTreeMap::new();
    for report in reports {
        let source = SourceFile::read(&report.path)?;
        for work_order in work_orders_for_source(&source, &report.findings) {
            out.insert(work_order.id.to_owned(), work_order);
        }
    }
    Ok(out)
}

fn reject(patch: &Patch, path: Option<PathBuf>, reason: &str) -> PatchVerification {
    PatchVerification {
        workorder_id: patch.workorder_id.to_owned(),
        path,
        passed: false,
        verdict: VerificationVerdict::Rejected,
        reasons: vec![reason.to_string()],
    }
}

fn region_byte_range(
    source: &SourceFile,
    work_order: &WorkOrder,
) -> Result<std::ops::Range<usize>> {
    let start = source.line_start_byte(work_order.region.start_line);
    let end = source
        .line_start_byte(work_order.region.end_line + 1)
        .min(source.text.len());
    if start > end || end > source.text.len() {
        bail!("invalid workorder region for {}", work_order.path.display());
    }
    Ok(start..end)
}

fn replace_region(text: &str, range: std::ops::Range<usize>, replacement: &str) -> Result<String> {
    if range.start > range.end || range.end > text.len() {
        bail!("invalid replacement range");
    }
    let mut out = text.to_string();
    out.replace_range(range, replacement);
    Ok(out)
}

fn balanced_after_reparse(text: &str) -> bool {
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut chars = text.char_indices().peekable();
    while let Some((_, ch)) = chars.next() {
        if in_string {
            if ch == '\\' {
                chars.next();
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            ';' | '#' => skip_until_newline(&mut chars),
            '(' | '[' | '{' => stack.push(ch),
            ')' | ']' | '}' if !closes_last_open(ch, &mut stack) => return false,
            ')' | ']' | '}' => {}
            _ => {}
        }
    }
    !in_string && stack.is_empty()
}

fn skip_until_newline(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>) {
    while let Some((_, next)) = chars.peek() {
        if *next == '\n' {
            break;
        }
        chars.next();
    }
}

fn closes_last_open(close: char, stack: &mut Vec<char>) -> bool {
    let expected_open = match close {
        ')' => '(',
        ']' => '[',
        '}' => '{',
        _ => return true,
    };
    stack.pop() == Some(expected_open)
}

fn parse_check_passes(lang: Lang, text: &str) -> Result<bool> {
    match parses_without_errors(lang, text)? {
        Some(ok) => Ok(ok),
        None => Ok(balanced_after_reparse(text)),
    }
}

fn deletes_defensive_code(original: &str, replacement: &str) -> bool {
    const PROTECTED: &[&str] = &[
        "try",
        "catch",
        "except",
        "finally",
        "rescue",
        "throw",
        "raise",
        "panic!",
        "bail!",
        "return Err",
        "Err(",
        "assert",
        ":pre",
        "precondition",
        "requires",
        "ensure",
    ];
    PROTECTED
        .iter()
        .any(|needle| count_occurrences(original, needle) > count_occurrences(replacement, needle))
}

fn count_occurrences(text: &str, needle: &str) -> usize {
    text.match_indices(needle).count()
}

fn count_public_defs(text: &str) -> usize {
    let clojure_defs = ["(def ", "(defn ", "(defmacro "]
        .iter()
        .map(|needle| count_occurrences(text, needle))
        .sum::<usize>();
    let rust_defs = ["pub fn ", "pub struct ", "pub enum ", "pub trait "]
        .iter()
        .map(|needle| count_occurrences(text, needle))
        .sum::<usize>();
    let julia_defs = count_occurrences(text, "export ");
    clojure_defs + rust_defs + julia_defs
}

fn run_check_cmd_on_temp_copy(
    options: &VerifyOptions,
    work_order: &WorkOrder,
    candidate: &str,
    command: &str,
    reasons: &mut Vec<String>,
) -> Result<()> {
    run_check_cmd_in_temp_project(&options.root, command, reasons, |temp_root| {
        write_patched_source(&options.root, temp_root, work_order, candidate)
    })
}

fn run_characterization_test_on_current_code(
    options: &VerifyOptions,
    test: &CharacterizationTest,
    command: &str,
    reasons: &mut Vec<String>,
) -> Result<()> {
    run_check_cmd_in_temp_project(&options.root, command, reasons, |temp_root| {
        write_characterization_test(temp_root, test)
    })
}

fn run_characterization_test_on_patched_code(
    options: &VerifyOptions,
    work_order: &WorkOrder,
    candidate: &str,
    test: &CharacterizationTest,
    command: &str,
    reasons: &mut Vec<String>,
) -> Result<()> {
    run_check_cmd_in_temp_project(&options.root, command, reasons, |temp_root| {
        write_patched_source(&options.root, temp_root, work_order, candidate)?;
        write_characterization_test(temp_root, test)
    })
}

fn run_check_cmd_in_temp_project(
    root: &Path,
    command: &str,
    reasons: &mut Vec<String>,
    setup: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let temp = TempDir::new().context("failed to create check tempdir")?;
    copy_project_for_check(root, temp.path())?;
    setup(temp.path())?;
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(temp.path())
        .output()
        .with_context(|| format!("failed to run check command `{command}`"))?;
    if !output.status.success() {
        reasons.push(check_cmd_failure_reason(output.status, &output.stderr));
    }
    Ok(())
}

fn write_patched_source(
    root: &Path,
    temp_root: &Path,
    work_order: &WorkOrder,
    candidate: &str,
) -> Result<()> {
    let relative = relative_to_root(root, &work_order.path)?;
    let temp_file = temp_root.join(relative);
    fs::write(&temp_file, candidate)
        .with_context(|| format!("failed to write temp patched file {}", temp_file.display()))
}

fn write_characterization_test(temp_root: &Path, test: &CharacterizationTest) -> Result<()> {
    let relative = safe_relative_test_path(&test.test_path)?;
    let test_path = temp_root.join(relative);
    if let Some(parent) = test_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&test_path, &test.test_text).with_context(|| {
        format!(
            "failed to write characterization test {}",
            test_path.display()
        )
    })
}

fn safe_relative_test_path(path: &Path) -> Result<&Path> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        bail!("characterization test_path must be relative and stay inside the project");
    }
    Ok(path)
}

fn check_cmd_failure_reason(status: std::process::ExitStatus, stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    format!(
        "check_cmd failed with status {}{}",
        status,
        if stderr.trim().is_empty() {
            String::new()
        } else {
            format!(": {}", stderr.trim())
        }
    )
}

fn copy_project_for_check(root: &Path, target: &Path) -> Result<()> {
    for entry in ignore::WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(name.as_ref(), ".git" | ".jj" | "target")
        })
        .build()
    {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(path);
        let destination = target.join(relative);
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            fs::create_dir_all(&destination)?;
        } else if entry.file_type().is_some_and(|kind| kind.is_file()) {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(path, &destination).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    path.display(),
                    destination.display()
                )
            })?;
        }
    }
    Ok(())
}

fn write_prepared_patches(
    root: &Path,
    prepared: &[PreparedPatch],
    backup: bool,
) -> Result<Vec<PathBuf>> {
    let grouped = group_prepared_patches(root, prepared);
    let mut written = Vec::new();
    for (path, patches) in grouped {
        if write_prepared_patches_to_file(&path, patches, backup)? {
            written.push(path);
        }
    }
    Ok(written)
}

fn group_prepared_patches<'a>(
    root: &Path,
    prepared: &'a [PreparedPatch],
) -> BTreeMap<PathBuf, Vec<&'a PreparedPatch>> {
    let mut grouped: BTreeMap<PathBuf, Vec<&PreparedPatch>> = BTreeMap::new();
    for patch in prepared {
        grouped
            .entry(path_in_root(root, &patch.path))
            .or_default()
            .push(patch);
    }
    grouped
}

fn write_prepared_patches_to_file(
    path: &Path,
    mut patches: Vec<&PreparedPatch>,
    backup: bool,
) -> Result<bool> {
    patches.sort_by(|a, b| b.range.start.cmp(&a.range.start));
    let original = read_to_string_ctx(path)?;
    let mut next_start = original.len() + 1;
    let mut text = original.to_owned();
    for patch in patches {
        if patch.range.end > next_start {
            bail!("overlapping patches for {}", path.display());
        }
        text.replace_range(patch.range.start..patch.range.end, &patch.replacement);
        next_start = patch.range.start;
    }
    if text == original {
        return Ok(false);
    }
    write_replacement_file(path, &original, text, backup)?;
    Ok(true)
}

fn write_replacement_file(path: &Path, original: &str, text: String, backup: bool) -> Result<()> {
    if backup {
        let backup_path = PathBuf::from(format!("{}.deslop.bak", path.display()));
        fs::write(&backup_path, original)
            .with_context(|| format!("failed to write {}", backup_path.display()))?;
    }
    let tmp = deslop_tmp_path(path);
    fs::write(&tmp, text).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("failed to replace {}", path.display()))
}

fn deslop_tmp_path(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}deslop.tmp",
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!("{ext}."))
            .unwrap_or_default()
    ))
}

fn path_in_root(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn relative_to_root(root: &Path, path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        Ok(path
            .strip_prefix(&root)
            .with_context(|| format!("{} is outside {}", path.display(), root.display()))?
            .to_path_buf())
    } else {
        Ok(path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_protocol::{
        CharacterizationTest, Patch, Region, WorkOrderKind, workorder_region_fingerprint,
    };

    struct VerifyFixture {
        temp: tempfile::TempDir,
        work_order: WorkOrder,
    }

    #[derive(Debug, Clone, Copy)]
    enum FixtureKind {
        Clojure,
        Julia,
        Python,
        Rust,
    }

    fn write_fixture(root: &Path, text: &str) -> PathBuf {
        let file = root.join("sample.clj");
        fs::write(&file, text).expect("write");
        file
    }

    fn write_rust_fixture(root: &Path, text: &str) -> PathBuf {
        let file = root.join("sample.rs");
        fs::write(&file, text).expect("write");
        file
    }

    fn write_python_fixture(root: &Path, text: &str) -> PathBuf {
        let file = root.join("sample.py");
        fs::write(&file, text).expect("write");
        file
    }

    fn write_julia_fixture(root: &Path, text: &str) -> PathBuf {
        let file = root.join("sample.jl");
        fs::write(&file, text).expect("write");
        file
    }

    fn only_work_order(root: &Path) -> WorkOrder {
        current_work_orders(root)
            .expect("workorders")
            .into_values()
            .next()
            .expect("workorder")
    }

    fn patch_for(work_order: &WorkOrder, replacement: &str) -> Patch {
        Patch {
            schema: "deslop.patch/1".to_string(),
            workorder_id: work_order.id.to_owned(),
            region_fingerprint: workorder_region_fingerprint(work_order),
            replacement: replacement.to_string(),
            by: "test".to_string(),
        }
    }

    fn characterization_test_for(
        work_order: &WorkOrder,
        test_path: &str,
        test_text: &str,
    ) -> CharacterizationTest {
        CharacterizationTest {
            schema: "deslop.characterization-test/1".to_string(),
            workorder_id: work_order.id.to_owned(),
            region_fingerprint: workorder_region_fingerprint(work_order),
            test_path: PathBuf::from(test_path),
            test_text: test_text.to_string(),
            by: "test".to_string(),
        }
    }

    fn verify_single(root: &Path, patch: Patch) -> VerifyReport {
        verify_single_with_options(
            root,
            patch,
            test_options(root, None, CoverageConfig::Disabled),
        )
    }

    fn verify_single_with_options(
        root: &Path,
        patch: Patch,
        options: VerifyOptions,
    ) -> VerifyReport {
        verify_patches(
            &[patch],
            &VerifyOptions {
                root: root.to_path_buf(),
                ..options
            },
        )
        .expect("verify")
    }

    fn work_order_from_fixture(root: &Path, text: &str) -> WorkOrder {
        write_fixture(root, text);
        only_work_order(root)
    }

    fn rust_work_order_from_fixture(root: &Path, text: &str) -> WorkOrder {
        write_rust_fixture(root, text);
        only_work_order(root)
    }

    fn python_work_order_from_fixture(root: &Path, text: &str) -> WorkOrder {
        write_python_fixture(root, text);
        only_work_order(root)
    }

    fn julia_work_order_from_fixture(root: &Path, text: &str) -> WorkOrder {
        write_julia_fixture(root, text);
        only_work_order(root)
    }

    fn verify_fixture(kind: FixtureKind, text: &str) -> VerifyFixture {
        let temp = tempfile::tempdir().expect("tempdir");
        let work_order = match kind {
            FixtureKind::Clojure => work_order_from_fixture(temp.path(), text),
            FixtureKind::Julia => julia_work_order_from_fixture(temp.path(), text),
            FixtureKind::Python => python_work_order_from_fixture(temp.path(), text),
            FixtureKind::Rust => rust_work_order_from_fixture(temp.path(), text),
        };
        VerifyFixture { temp, work_order }
    }

    fn clojure_fixture(text: &str) -> VerifyFixture {
        verify_fixture(FixtureKind::Clojure, text)
    }

    fn rust_fixture(text: &str) -> VerifyFixture {
        verify_fixture(FixtureKind::Rust, text)
    }

    fn python_fixture(text: &str) -> VerifyFixture {
        verify_fixture(FixtureKind::Python, text)
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
        .expect("write lcov");
        path
    }

    fn cargo_mutants_outcomes_fixture(root: &Path, source: &Path) -> PathBuf {
        let path = root.join("outcomes.json");
        fs::write(
            &path,
            format!(
                r#"{{
  "cargo_mutants_version": "fixture",
  "outcomes": [
    {{
      "outcome": "Missed",
      "mutant": {{
        "file": "{}",
        "line": 2,
        "end_line": 2
      }}
    }},
    {{
      "outcome": "Caught",
      "mutant": {{
        "file": "{}",
        "line": 5,
        "end_line": 5
      }}
    }}
  ]
}}"#,
                source.display(),
                source.display()
            ),
        )
        .expect("write outcomes");
        path
    }

    fn cosmic_ray_outcomes_fixture(root: &Path, source: &Path, outcome: &str) -> PathBuf {
        let path = root.join(format!("cosmic-ray-{outcome}.json"));
        fs::write(
            &path,
            format!(
                r#"{{
  "cosmic_ray_version": "fixture",
  "jobs": [
    {{
      "module_path": "{}",
      "line_number": 2,
      "test_outcome": "{}"
    }}
  ]
}}"#,
                source.display(),
                outcome
            ),
        )
        .expect("write cosmic-ray outcomes");
        path
    }

    fn coverage_report_fixture(root: &Path, name: &str, text: &str) -> PathBuf {
        let path = root.join(name);
        fs::write(&path, text).expect("write coverage fixture");
        path
    }

    fn manual_work_order(source: &SourceFile, line: usize) -> WorkOrder {
        WorkOrder {
            schema: "deslop.workorder/1".to_string(),
            kind: WorkOrderKind::RewriteRegion,
            id: format!("wo_{}_{}", source.lang, line),
            path: source.path.to_path_buf(),
            region: Region {
                start_line: line,
                end_line: line,
                text: source.region_text(line, line),
            },
            findings: Vec::new(),
            instruction: "test".to_string(),
            contract: deslop_protocol::Contract::default(),
        }
    }

    fn assess_provider(
        provider: &mut dyn CoverageProvider,
        root: &Path,
        source: &SourceFile,
        work_order: &WorkOrder,
    ) -> CoverageAssessment {
        provider
            .assess(CoverageRequest {
                root,
                source,
                work_order,
            })
            .expect("coverage assessment")
    }

    fn assert_fixture_coverage(
        provider: &mut dyn CoverageProvider,
        root: &Path,
        source: &SourceFile,
    ) {
        let covered = manual_work_order(source, 2);
        let uncovered = manual_work_order(source, 5);
        let covered_assessment = assess_provider(provider, root, source, &covered);
        assert_eq!(covered_assessment.status, CoverageStatus::Covered);
        assert_eq!(
            verdict_for_passing_patch(
                &patch_for(&covered, "x"),
                covered_assessment.status,
                MutationStatus::Unknown,
                false
            ),
            VerificationVerdict::Removable
        );

        let uncovered_assessment = assess_provider(provider, root, source, &uncovered);
        assert_eq!(uncovered_assessment.status, CoverageStatus::Uncovered);
        assert_eq!(
            verdict_for_passing_patch(
                &patch_for(&uncovered, ""),
                uncovered_assessment.status,
                MutationStatus::Unknown,
                false
            ),
            VerificationVerdict::DeadCandidate
        );
        assert_eq!(
            verdict_for_passing_patch(
                &patch_for(&uncovered, "x"),
                uncovered_assessment.status,
                MutationStatus::Unknown,
                false
            ),
            VerificationVerdict::UntestedRisky
        );
    }

    fn test_options(
        root: &Path,
        check_cmd: Option<&str>,
        coverage: CoverageConfig,
    ) -> VerifyOptions {
        VerifyOptions {
            root: root.to_path_buf(),
            check_cmd: check_cmd.map(ToString::to_string),
            coverage,
            mutation: MutationConfig::Disabled,
            characterization_tests: Vec::new(),
            allow_non_removable: false,
        }
    }

    fn test_options_with_mutation(
        root: &Path,
        check_cmd: Option<&str>,
        mutation: MutationConfig,
    ) -> VerifyOptions {
        VerifyOptions {
            mutation,
            ..test_options(root, check_cmd, CoverageConfig::Disabled)
        }
    }

    fn test_options_with_characterization(
        root: &Path,
        check_cmd: Option<&str>,
        characterization_tests: Vec<CharacterizationTest>,
    ) -> VerifyOptions {
        VerifyOptions {
            characterization_tests,
            ..test_options(root, check_cmd, CoverageConfig::Disabled)
        }
    }

    #[test]
    fn protocol_round_trip_workorder_patch_verify() {
        let fixture = clojure_fixture("(= (count xs) 0)\n");
        let report = verify_single(
            fixture.temp.path(),
            patch_for(&fixture.work_order, "(= (count xs) 0)\n"),
        );
        assert_eq!(report.passed_count(), 1);
        assert_eq!(report.failed_count(), 0);
        assert_eq!(
            report.results[0].verdict,
            VerificationVerdict::CoverageUnknown
        );
    }

    #[test]
    fn patch_deleting_try_catch_is_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_fixture(
            temp.path(),
            "(try (= (count xs) 0) (catch Exception e false))\n",
        );
        let work_order = only_work_order(temp.path());
        let report = verify_single(temp.path(), patch_for(&work_order, "(= (count xs) 0)\n"));
        assert_eq!(report.passed_count(), 0);
        assert!(report.results[0].reasons[0].contains("defensive-code"));
    }

    #[test]
    fn stale_region_fingerprint_is_rejected() {
        let fixture = clojure_fixture("(= (count xs) 0)\n");
        let mut patch = patch_for(&fixture.work_order, "(= (count xs) 0)\n");
        patch.region_fingerprint = "stale".to_string();
        let report = verify_single(fixture.temp.path(), patch);
        assert_eq!(report.passed_count(), 0);
        assert!(report.results[0].reasons[0].contains("stale"));
    }

    #[test]
    fn tree_sitter_error_node_parse_check_rejects_broken_patch() {
        let fixture = clojure_fixture("(= (count xs) 0)\n");
        let report = verify_single(
            fixture.temp.path(),
            patch_for(&fixture.work_order, "(empty? xs\n"),
        );
        assert_eq!(report.passed_count(), 0);
        assert_eq!(report.results[0].verdict, VerificationVerdict::Rejected);
        assert!(report.results[0].reasons[0].contains("tree-sitter ERROR"));
    }

    #[test]
    fn coverage_fixture_verdicts_are_graded() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file = write_rust_fixture(temp.path(), "fn f() -> i32 {\n    return 1;\n}\n");
        let work_order = only_work_order(temp.path());
        let covered = lcov_fixture(temp.path(), "covered.lcov", &file, 2, 1);
        let uncovered = lcov_fixture(temp.path(), "uncovered.lcov", &file, 2, 0);

        let removable = verify_single_with_options(
            temp.path(),
            patch_for(&work_order, "fn f() -> i32 {\n    1\n}\n"),
            test_options(temp.path(), Some("true"), CoverageConfig::LcovFile(covered)),
        );
        assert_eq!(removable.results[0].verdict, VerificationVerdict::Removable);

        let dead = verify_single_with_options(
            temp.path(),
            patch_for(&work_order, ""),
            test_options(
                temp.path(),
                Some("true"),
                CoverageConfig::LcovFile(uncovered),
            ),
        );
        assert_eq!(dead.results[0].verdict, VerificationVerdict::DeadCandidate);

        let risky = verify_single_with_options(
            temp.path(),
            patch_for(&work_order, "fn f() -> i32 {\n    1\n}\n"),
            test_options(
                temp.path(),
                Some("true"),
                CoverageConfig::LcovFile(lcov_fixture(temp.path(), "risky.lcov", &file, 2, 0)),
            ),
        );
        assert_eq!(risky.results[0].verdict, VerificationVerdict::UntestedRisky);

        let rejected = verify_single_with_options(
            temp.path(),
            patch_for(&work_order, "fn f() -> i32 {\n    1\n}\n"),
            test_options(temp.path(), Some("false"), CoverageConfig::Disabled),
        );
        assert_eq!(rejected.results[0].verdict, VerificationVerdict::Rejected);
    }

    #[test]
    fn absent_coverage_tool_degrades_to_unknown() {
        let fixture = rust_fixture("fn f() -> i32 {\n    return 1;\n}\n");
        let report = verify_single_with_options(
            fixture.temp.path(),
            patch_for(&fixture.work_order, "fn f() -> i32 {\n    1\n}\n"),
            test_options(
                fixture.temp.path(),
                Some("true"),
                CoverageConfig::AutoWithCommand("__deslop_missing_cargo__".to_string()),
            ),
        );
        assert_eq!(report.passed_count(), 1);
        assert_eq!(
            report.results[0].verdict,
            VerificationVerdict::CoverageUnknown
        );
        assert!(report.results[0].reasons[0].contains("cargo-llvm-cov"));
    }

    #[test]
    fn non_rust_coverage_auto_commands_are_constructed_deterministically() {
        let output_dir = Path::new("/tmp/deslop-cloverage");
        assert_eq!(
            cloverage_command("lein", output_dir),
            CoverageToolCommand::new(
                "lein",
                [
                    "cloverage".to_string(),
                    "--json".to_string(),
                    "--output".to_string(),
                    output_dir.display().to_string(),
                ],
            )
        );
        assert_eq!(
            cloverage_command("custom-lein", output_dir).program,
            "custom-lein"
        );

        assert_eq!(
            julia_coverage_command("julia"),
            CoverageToolCommand::new(
                "julia",
                [
                    "--startup-file=no".to_string(),
                    "--code-coverage=user".to_string(),
                    "-e".to_string(),
                    "using Pkg; Pkg.test()".to_string(),
                ],
            )
        );
        assert_eq!(
            julia_coverage_command("custom-julia").program,
            "custom-julia"
        );

        let output_path = Path::new("/tmp/deslop-coverage.json");
        assert_eq!(
            coverage_py_run_command("coverage"),
            CoverageToolCommand::new(
                "coverage",
                [
                    "run".to_string(),
                    "-m".to_string(),
                    "unittest".to_string(),
                    "discover".to_string(),
                ],
            )
        );
        assert_eq!(
            coverage_py_json_command("coverage", output_path),
            CoverageToolCommand::new(
                "coverage",
                [
                    "json".to_string(),
                    "-o".to_string(),
                    output_path.display().to_string(),
                ],
            )
        );
        assert_eq!(
            coverage_py_run_command("custom-coverage").program,
            "custom-coverage"
        );
    }

    #[test]
    fn non_rust_coverage_auto_defaults_to_expected_commands() {
        let clojure = ClojureCloverageProvider::new(&CoverageConfig::Auto);
        let julia = JuliaCoverageProvider::new(&CoverageConfig::Auto);
        let python = PythonCoveragePyProvider::new(&CoverageConfig::Auto);
        assert!(matches!(
            clojure.mode,
            CoverageProviderMode::Auto { ref command } if command == "lein"
        ));
        assert!(matches!(
            julia.mode,
            CoverageProviderMode::Auto { ref command } if command == "julia"
        ));
        assert!(matches!(
            python.mode,
            CoverageProviderMode::Auto { ref command } if command == "coverage"
        ));

        let clojure = ClojureCloverageProvider::new(&CoverageConfig::AutoWithCommand(
            "custom-lein".to_string(),
        ));
        let julia = JuliaCoverageProvider::new(&CoverageConfig::AutoWithCommand(
            "custom-julia".to_string(),
        ));
        let python = PythonCoveragePyProvider::new(&CoverageConfig::AutoWithCommand(
            "custom-coverage".to_string(),
        ));
        assert!(matches!(
            clojure.mode,
            CoverageProviderMode::Auto { ref command } if command == "custom-lein"
        ));
        assert!(matches!(
            julia.mode,
            CoverageProviderMode::Auto { ref command } if command == "custom-julia"
        ));
        assert!(matches!(
            python.mode,
            CoverageProviderMode::Auto { ref command } if command == "custom-coverage"
        ));
    }

    #[test]
    fn cloverage_fixture_maps_covered_and_uncovered_regions() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = SourceFile::new(
            temp.path().join("sample.clj"),
            "(ns sample)\n(defn covered [] 1)\n\n\n(defn missed [] 2)\n".into(),
        );
        let fixture = coverage_report_fixture(
            temp.path(),
            "cloverage.json",
            r#"{
  "files": [
    {
      "filename": "sample.clj",
      "lines": [
        { "line": 2, "hits": 1 },
        { "line": 5, "hits": 0 }
      ]
    }
  ]
}"#,
        );
        let mut provider = ClojureCloverageProvider::new(&CoverageConfig::CloverageFile(fixture));
        assert_fixture_coverage(&mut provider, temp.path(), &source);
    }

    #[test]
    fn absent_cloverage_degrades_to_unknown() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = SourceFile::new(
            temp.path().join("sample.clj"),
            "(ns sample)\n(defn f [] 1)\n".into(),
        );
        let work_order = manual_work_order(&source, 2);
        let mut provider = ClojureCloverageProvider::new(&CoverageConfig::AutoWithCommand(
            "__deslop_missing_cloverage__".to_string(),
        ));
        let assessment = assess_provider(&mut provider, temp.path(), &source, &work_order);
        assert_eq!(assessment.status, CoverageStatus::Unknown);
        assert!(assessment.reason.unwrap().contains("cloverage"));
    }

    #[test]
    fn absent_cloverage_auto_command_keeps_patch_coverage_unknown() {
        let fixture = clojure_fixture("(= (count xs) 0)\n");
        let report = verify_single_with_options(
            fixture.temp.path(),
            patch_for(&fixture.work_order, "(empty? xs)\n"),
            test_options(
                fixture.temp.path(),
                Some("true"),
                CoverageConfig::AutoWithCommand("__deslop_missing_cloverage__".to_string()),
            ),
        );
        assert_eq!(report.passed_count(), 1);
        assert_eq!(
            report.results[0].verdict,
            VerificationVerdict::CoverageUnknown
        );
        assert!(report.results[0].reasons[0].contains("cloverage"));
    }

    #[test]
    fn coverage_jl_cov_fixture_maps_covered_and_uncovered_regions() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = SourceFile::new(
            temp.path().join("sample.jl"),
            "module Sample\ncovered() = 1\n\n\nmissed() = 2\nend\n".into(),
        );
        let fixture = coverage_report_fixture(
            temp.path(),
            "sample.jl.cov",
            "        - module Sample\n        1 covered() = 1\n        -\n        -\n    ##### missed() = 2\n        - end\n",
        );
        let mut provider = JuliaCoverageProvider::new(&CoverageConfig::JuliaCovFile(fixture));
        assert_fixture_coverage(&mut provider, temp.path(), &source);
    }

    #[test]
    fn absent_coverage_jl_degrades_to_unknown() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = SourceFile::new(temp.path().join("sample.jl"), "f() = 1\n".into());
        let work_order = manual_work_order(&source, 1);
        let mut provider = JuliaCoverageProvider::new(&CoverageConfig::AutoWithCommand(
            "__deslop_missing_julia__".to_string(),
        ));
        let assessment = assess_provider(&mut provider, temp.path(), &source, &work_order);
        assert_eq!(assessment.status, CoverageStatus::Unknown);
        assert!(assessment.reason.unwrap().contains("julia"));
    }

    #[test]
    fn absent_julia_auto_command_keeps_patch_coverage_unknown() {
        let fixture = verify_fixture(
            FixtureKind::Julia,
            "function f()\n    error(\"TODO: implement\")\nend\n",
        );
        let report = verify_single_with_options(
            fixture.temp.path(),
            patch_for(&fixture.work_order, "function f()\n    1\nend\n"),
            test_options(
                fixture.temp.path(),
                Some("true"),
                CoverageConfig::AutoWithCommand("__deslop_missing_julia__".to_string()),
            ),
        );
        assert_eq!(report.passed_count(), 1);
        assert_eq!(
            report.results[0].verdict,
            VerificationVerdict::CoverageUnknown
        );
        assert!(report.results[0].reasons[0].contains("julia"));
    }

    #[test]
    fn coverage_py_json_fixture_maps_covered_and_uncovered_regions() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = SourceFile::new_with_lang(
            temp.path().join("sample.py"),
            "def setup():\n    return 1\n\n\ndef missed():\n    return 2\n".into(),
            Lang::Python,
        );
        let fixture = coverage_report_fixture(
            temp.path(),
            "coverage.json",
            r#"{
  "files": {
    "sample.py": {
      "executed_lines": [2],
      "missing_lines": [5]
    }
  }
}"#,
        );
        let mut provider = PythonCoveragePyProvider::new(&CoverageConfig::CoveragePyFile(fixture));
        assert_fixture_coverage(&mut provider, temp.path(), &source);
    }

    #[test]
    fn absent_coverage_py_degrades_to_unknown() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = SourceFile::new_with_lang(
            temp.path().join("sample.py"),
            "def f():\n    return 1\n".into(),
            Lang::Python,
        );
        let work_order = manual_work_order(&source, 2);
        let mut provider = PythonCoveragePyProvider::new(&CoverageConfig::AutoWithCommand(
            "__deslop_missing_coverage_py__".to_string(),
        ));
        let assessment = assess_provider(&mut provider, temp.path(), &source, &work_order);
        assert_eq!(assessment.status, CoverageStatus::Unknown);
        assert!(assessment.reason.unwrap().contains("coverage.py"));
    }

    #[test]
    fn absent_coverage_py_auto_command_keeps_patch_coverage_unknown() {
        let fixture = python_fixture("def f():\n    value = 'TODO: implement'\n    return value\n");
        let report = verify_single_with_options(
            fixture.temp.path(),
            patch_for(&fixture.work_order, "    value = 1\n"),
            test_options(
                fixture.temp.path(),
                Some("true"),
                CoverageConfig::AutoWithCommand("__deslop_missing_coverage_py__".to_string()),
            ),
        );
        assert_eq!(report.passed_count(), 1);
        assert_eq!(
            report.results[0].verdict,
            VerificationVerdict::CoverageUnknown
        );
        assert!(report.results[0].reasons[0].contains("coverage.py"));
    }

    #[test]
    fn cargo_mutants_fixture_survivor_feeds_dead_signal() {
        let temp = tempfile::tempdir().expect("tempdir");
        let rust_file = write_rust_fixture(
            temp.path(),
            "fn missed() -> i32 {\n    return 1;\n}\nfn caught() -> i32 {\n    return 2;\n}\n",
        );
        let work_orders: Vec<_> = current_work_orders(temp.path())
            .expect("workorders")
            .into_values()
            .collect();
        let missed = work_orders
            .iter()
            .find(|work_order| work_order.region.text.contains("missed"))
            .expect("missed workorder");
        let caught = work_orders
            .iter()
            .find(|work_order| work_order.region.text.contains("caught"))
            .expect("caught workorder");
        let outcomes = cargo_mutants_outcomes_fixture(temp.path(), &rust_file);

        let report = verify_patches(
            &[
                patch_for(missed, ""),
                patch_for(caught, "fn caught() -> i32 {\n    2\n}\n"),
            ],
            &test_options_with_mutation(
                temp.path(),
                Some("true"),
                MutationConfig::OutcomesFile(outcomes),
            ),
        )
        .expect("verify");

        assert_eq!(
            report.results[0].verdict,
            VerificationVerdict::DeadCandidate
        );
        assert!(
            report.results[0].reasons[1].contains("surviving mutant"),
            "{:#?}",
            report.results[0].reasons
        );
        assert_eq!(
            report.results[1].verdict,
            VerificationVerdict::CoverageUnknown
        );
        assert!(
            report.results[1].reasons[1].contains("no surviving mutant"),
            "{:#?}",
            report.results[1].reasons
        );
    }

    #[test]
    fn cosmic_ray_fixture_survivor_downgrades_python_patch() {
        let fixture = python_fixture("def f():\n    value = 'TODO: implement'\n    return value\n");
        let source = fixture.temp.path().join("sample.py");
        let outcomes = cosmic_ray_outcomes_fixture(fixture.temp.path(), &source, "survived");
        let report = verify_single_with_options(
            fixture.temp.path(),
            patch_for(&fixture.work_order, "    value = 1\n"),
            test_options_with_mutation(
                fixture.temp.path(),
                Some("true"),
                MutationConfig::OutcomesFile(outcomes),
            ),
        );

        assert_eq!(
            report.results[0].verdict,
            VerificationVerdict::UntestedRisky
        );
        assert!(
            report.results[0].reasons[1].contains("cosmic-ray"),
            "{:#?}",
            report.results[0].reasons
        );
        assert!(
            report.results[0].reasons[1].contains("surviving mutant"),
            "{:#?}",
            report.results[0].reasons
        );
    }

    #[test]
    fn cosmic_ray_fixture_without_survivor_does_not_downgrade_python_patch() {
        let fixture = python_fixture("def f():\n    value = 'TODO: implement'\n    return value\n");
        let source = fixture.temp.path().join("sample.py");
        let outcomes = cosmic_ray_outcomes_fixture(fixture.temp.path(), &source, "killed");
        let report = verify_single_with_options(
            fixture.temp.path(),
            patch_for(&fixture.work_order, "    value = 1\n"),
            test_options_with_mutation(
                fixture.temp.path(),
                Some("true"),
                MutationConfig::OutcomesFile(outcomes),
            ),
        );

        assert_eq!(
            report.results[0].verdict,
            VerificationVerdict::CoverageUnknown
        );
        assert!(
            report.results[0].reasons[1].contains("no surviving mutant"),
            "{:#?}",
            report.results[0].reasons
        );
    }

    #[test]
    fn absent_cosmic_ray_degrades_without_rejecting_python_patch() {
        let fixture = python_fixture("def f():\n    value = 'TODO: implement'\n    return value\n");
        let report = verify_single_with_options(
            fixture.temp.path(),
            patch_for(&fixture.work_order, "    value = 1\n"),
            test_options_with_mutation(
                fixture.temp.path(),
                Some("true"),
                MutationConfig::AutoWithCommand("__deslop_missing_cosmic_ray__".to_string()),
            ),
        );

        assert_eq!(report.passed_count(), 1);
        assert_eq!(
            report.results[0].verdict,
            VerificationVerdict::CoverageUnknown
        );
        assert!(report.results[0].reasons[1].contains("cosmic-ray"));
    }

    #[test]
    fn absent_cargo_mutants_degrades_without_rejecting_patch() {
        let fixture = rust_fixture("fn f() -> i32 {\n    return 1;\n}\n");
        let report = verify_single_with_options(
            fixture.temp.path(),
            patch_for(&fixture.work_order, "fn f() -> i32 {\n    1\n}\n"),
            test_options_with_mutation(
                fixture.temp.path(),
                Some("true"),
                MutationConfig::AutoWithCommand("__deslop_missing_cargo__".to_string()),
            ),
        );
        assert_eq!(report.passed_count(), 1);
        assert_eq!(
            report.results[0].verdict,
            VerificationVerdict::CoverageUnknown
        );
        assert!(report.results[0].reasons[1].contains("cargo-mutants"));
    }

    #[test]
    fn weak_verdict_emits_characterization_work_order() {
        let fixture = rust_fixture("fn f() -> i32 {\n    return 1;\n}\n");
        let work_orders = characterization_work_orders_for_patches(
            &[patch_for(
                &fixture.work_order,
                "fn f() -> i32 {\n    1\n}\n",
            )],
            &test_options(fixture.temp.path(), Some("true"), CoverageConfig::Disabled),
        )
        .expect("characterize");

        assert_eq!(work_orders.len(), 1);
        assert_eq!(
            work_orders[0].kind,
            WorkOrderKind::NeedsCharacterizationTest
        );
        assert_eq!(work_orders[0].id, fixture.work_order.id);
        assert!(
            work_orders[0]
                .instruction
                .contains("pins the current observable behavior")
        );
    }

    #[test]
    fn characterization_test_passing_current_code_is_accepted() {
        let fixture = rust_fixture("fn f() -> i32 {\n    return 1;\n}\n");
        let test =
            characterization_test_for(&fixture.work_order, "tests/characterization.txt", "pin");
        let report = verify_characterization_tests(
            &[test],
            &test_options(
                fixture.temp.path(),
                Some("test -f tests/characterization.txt && grep -q 'return 1' sample.rs"),
                CoverageConfig::Disabled,
            ),
        )
        .expect("verify characterization");

        assert_eq!(report.accepted_count(), 1);
        assert_eq!(report.rejected_count(), 0);
    }

    #[test]
    fn characterization_test_failing_current_code_is_rejected() {
        let fixture = rust_fixture("fn f() -> i32 {\n    return 1;\n}\n");
        let test =
            characterization_test_for(&fixture.work_order, "tests/characterization.txt", "pin");
        let report = verify_characterization_tests(
            &[test],
            &test_options(
                fixture.temp.path(),
                Some("test -f tests/characterization.txt && grep -q 'return 2' sample.rs"),
                CoverageConfig::Disabled,
            ),
        )
        .expect("verify characterization");

        assert_eq!(report.accepted_count(), 0);
        assert_eq!(report.rejected_count(), 1);
        assert!(report.results[0].reasons[0].contains("check_cmd failed"));
    }

    #[test]
    fn accepted_characterization_test_gates_patch_verification() {
        let fixture = rust_fixture("fn f() -> i32 {\n    return 1;\n}\n");
        let test =
            characterization_test_for(&fixture.work_order, "tests/characterization.txt", "pin");
        let command = "if [ -f tests/characterization.txt ]; then grep -q 'return 1' sample.rs; else true; fi";

        let rejected = verify_single_with_options(
            fixture.temp.path(),
            patch_for(&fixture.work_order, "fn f() -> i32 {\n    return 2;\n}\n"),
            test_options_with_characterization(
                fixture.temp.path(),
                Some(command),
                vec![test.clone()],
            ),
        );
        assert_eq!(rejected.results[0].verdict, VerificationVerdict::Rejected);
        assert!(rejected.results[0].reasons[0].contains("check_cmd failed"));

        let accepted = verify_single_with_options(
            fixture.temp.path(),
            patch_for(&fixture.work_order, "fn f() -> i32 {\n    return 1;\n}\n"),
            test_options_with_characterization(fixture.temp.path(), Some(command), vec![test]),
        );
        assert_eq!(accepted.results[0].verdict, VerificationVerdict::Removable);
        assert!(
            accepted.results[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("characterization test passed"))
        );
    }

    #[test]
    fn apply_writes_only_removable_patches_by_default() {
        let temp = tempfile::tempdir().expect("tempdir");
        let rust_file = write_rust_fixture(temp.path(), "fn f() -> i32 {\n    return 1;\n}\n");
        let clj_file = write_fixture(
            temp.path(),
            "(= (count xs) 0)\n(assert ok) ; initialize x\n",
        );
        let work_orders: Vec<_> = current_work_orders(temp.path())
            .expect("workorders")
            .into_values()
            .collect();
        assert_eq!(work_orders.len(), 3);
        let passing = work_orders
            .iter()
            .find(|work_order| work_order.path.ends_with("sample.rs"))
            .expect("rust workorder");
        let failing = work_orders
            .iter()
            .find(|work_order| work_order.region.text.contains("assert"))
            .expect("assert workorder");
        let coverage = lcov_fixture(temp.path(), "coverage.lcov", &rust_file, 2, 1);
        let patches = vec![
            patch_for(passing, "fn f() -> i32 {\n    1\n}\n"),
            patch_for(failing, "\n"),
        ];
        let report = apply_patches(
            &patches,
            &test_options(
                temp.path(),
                Some("true"),
                CoverageConfig::LcovFile(coverage),
            ),
            true,
        )
        .expect("apply");
        assert_eq!(report.verified.passed_count(), 1);
        assert_eq!(report.verified.failed_count(), 1);
        let rust_text = fs::read_to_string(&rust_file).expect("read rust");
        let clj_text = fs::read_to_string(&clj_file).expect("read clj");
        assert!(rust_text.contains("    1"));
        assert!(clj_text.contains("(assert ok)"));
        assert!(PathBuf::from(format!("{}.deslop.bak", rust_file.display())).exists());
    }
}
