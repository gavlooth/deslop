//! Frozen independent-project workflow evidence for M10 B3/M10.2.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use deslop_analyzer::{AnalyzerConfig, scan_paths_with_context};
use deslop_core::{AnalysisStatus, SafetyClass};
use serde::{Deserialize, Serialize};

pub const M10_EXTERNAL_MANIFEST_SCHEMA: &str = "deslop.m10-external-projects/1";
pub const M10_EXTERNAL_REPORT_SCHEMA: &str = "deslop.m10-external-report/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SizeStratum {
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalCommand {
    pub program: String,
    pub arguments: Vec<String>,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalProjectSpec {
    pub id: String,
    pub language: String,
    pub size_stratum: SizeStratum,
    pub repository_url: String,
    pub revision: String,
    pub license_spdx: String,
    pub license_evidence: String,
    pub public_api_markers: Vec<String>,
    pub generated_markers: Vec<String>,
    pub test_markers: Vec<String>,
    pub test_command: ExternalCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalProjectManifest {
    pub schema: String,
    pub manifest_id: String,
    pub projects: Vec<ExternalProjectSpec>,
}

impl ExternalProjectManifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema != M10_EXTERNAL_MANIFEST_SCHEMA || self.projects.len() != 18 {
            bail!("external manifest must contain exactly 18 projects under the frozen schema");
        }
        let mut identities = BTreeSet::new();
        let mut urls = BTreeSet::new();
        let mut grid = BTreeMap::<(&str, SizeStratum), usize>::new();
        for project in &self.projects {
            if project.id.is_empty()
                || project.language.is_empty()
                || project.repository_url.is_empty()
                || project.license_spdx.is_empty()
                || project.license_evidence.is_empty()
                || project.public_api_markers.is_empty()
                || project.generated_markers.is_empty()
                || project.test_markers.is_empty()
                || project.test_command.program.is_empty()
                || project.test_command.timeout_seconds == 0
                || project.revision.len() != 40
                || !project
                    .revision
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                bail!("external project {} is not release-complete", project.id);
            }
            if !identities.insert(project.id.as_str())
                || !urls.insert(project.repository_url.as_str())
            {
                bail!("external project ids and repository URLs must be unique");
            }
            *grid
                .entry((&project.language, project.size_stratum))
                .or_default() += 1;
        }
        for language in [
            "clojure",
            "javascript",
            "julia",
            "python",
            "rust",
            "typescript",
        ] {
            for size in [SizeStratum::Small, SizeStratum::Medium, SizeStratum::Large] {
                if grid.get(&(language, size)).copied() != Some(1) {
                    bail!("external manifest lacks exactly one {language}/{size:?} project");
                }
            }
        }
        let expected = external_manifest_id(&self.projects)?;
        if self.manifest_id != expected {
            bail!("external manifest identity mismatch: expected {expected}");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandStatus {
    Passed,
    Failed,
    TimedOut,
    ToolUnavailable,
    NotRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEvidence {
    pub command: ExternalCommand,
    pub status: CommandStatus,
    pub exit_code: Option<i32>,
    pub stdout_digest: Option<String>,
    pub stderr_digest: Option<String>,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalProjectEvidence {
    pub id: String,
    pub language: String,
    pub size_stratum: SizeStratum,
    pub revision: String,
    pub exact_revision: bool,
    pub license_evidence_present: bool,
    pub source_files: usize,
    pub source_lines: usize,
    pub reports_by_language: BTreeMap<String, usize>,
    pub analysis_statuses: BTreeMap<String, usize>,
    pub findings_by_safety: BTreeMap<String, usize>,
    pub findings_by_rule: BTreeMap<String, usize>,
    pub proposal_eligible_findings: usize,
    pub report_only_findings: usize,
    pub review_pending_findings: usize,
    pub public_api_files: usize,
    pub generated_files: usize,
    pub test_files: usize,
    pub scan_digest: String,
    pub workflow: Vec<String>,
    pub test: CommandEvidence,
    pub working_tree_clean_after: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalEvaluationEnvironment {
    pub os: String,
    pub architecture: String,
    pub tool_versions: BTreeMap<String, Option<String>>,
    pub checkout_root: String,
    pub dependency_cache_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalEvaluationReport {
    pub schema: String,
    pub report_id: String,
    pub manifest_id: String,
    pub environment: ExternalEvaluationEnvironment,
    pub projects: Vec<ExternalProjectEvidence>,
}

impl ExternalEvaluationReport {
    pub fn validate(&self) -> Result<()> {
        if self.schema != M10_EXTERNAL_REPORT_SCHEMA || self.projects.len() != 18 {
            bail!("external evaluation report is not release-complete");
        }
        if self
            .projects
            .windows(2)
            .any(|pair| pair[0].id >= pair[1].id)
        {
            bail!("external report projects must be sorted and unique");
        }
        for project in &self.projects {
            if !project.exact_revision
                || !project.license_evidence_present
                || project.source_files == 0
                || project.source_lines == 0
                || project.review_pending_findings != project.proposal_eligible_findings
                || project.test.status == CommandStatus::NotRun
                || project.workflow
                    != [
                        "scan",
                        "triage",
                        "explain",
                        "review-pending",
                        "verify-or-reject",
                    ]
                || !project.working_tree_clean_after
            {
                bail!(
                    "external project {} failed its evidence contract",
                    project.id
                );
            }
        }
        let expected = external_report_id(&self.manifest_id, &self.environment, &self.projects)?;
        if self.report_id != expected {
            bail!("external report identity mismatch: expected {expected}");
        }
        Ok(())
    }
}

pub fn default_external_manifest() -> Result<ExternalProjectManifest> {
    let cargo = |arguments: &[&str]| command("cargo", arguments);
    let npm = || command("npm", &["test"]);
    let python = || command("python3", &["-m", "pytest", "-q"]);
    let lein = || command("lein", &["test"]);
    let julia = || command("julia", &["--project=.", "-e", "using Pkg; Pkg.test()"]);
    let common_generated = || vec!["generated".into(), "vendor".into(), "dist".into()];
    let common_tests = || vec!["test".into(), "tests".into(), "spec".into()];
    let mut projects = vec![
        spec(
            "clojure-medley",
            "clojure",
            SizeStratum::Small,
            "https://github.com/weavejester/medley.git",
            "822981871facb27630dcba03cce2924a34989963",
            "EPL-1.0",
            "LICENSE.txt",
            &["src", "defn"],
            common_generated(),
            common_tests(),
            lein(),
        ),
        spec(
            "clojure-digest",
            "clojure",
            SizeStratum::Medium,
            "https://github.com/clj-commons/digest.git",
            "6366ce792684eaeaab2b61ec3f520378269ed920",
            "EPL-1.0",
            "project.clj",
            &["src", "defn"],
            common_generated(),
            common_tests(),
            lein(),
        ),
        spec(
            "clojure-data-json",
            "clojure",
            SizeStratum::Large,
            "https://github.com/clojure/data.json.git",
            "94463ffb54482427fd9b31f264b06bff6dcfd557",
            "EPL-1.0",
            "LICENSE",
            &["src", "public"],
            common_generated(),
            common_tests(),
            lein(),
        ),
        spec(
            "javascript-p-limit",
            "javascript",
            SizeStratum::Small,
            "https://github.com/sindresorhus/p-limit.git",
            "42599ebbbb1228a5bdab381fcf8f4ac20eb8d551",
            "MIT",
            "license",
            &["index.js", "export"],
            common_generated(),
            common_tests(),
            npm(),
        ),
        spec(
            "javascript-chalk",
            "javascript",
            SizeStratum::Medium,
            "https://github.com/chalk/chalk.git",
            "aa06bb5ac3f14df9fda8cfb54274dfc165ddfdef",
            "MIT",
            "license",
            &["source", "index.js"],
            common_generated(),
            common_tests(),
            npm(),
        ),
        spec(
            "javascript-qs",
            "javascript",
            SizeStratum::Large,
            "https://github.com/ljharb/qs.git",
            "3a890d4ecd3deb72a45d90be36f4f8c5970467c7",
            "BSD-3-Clause",
            "LICENSE.md",
            &["lib", "index.js"],
            common_generated(),
            common_tests(),
            npm(),
        ),
        spec(
            "julia-fixedpointnumbers",
            "julia",
            SizeStratum::Small,
            "https://github.com/JuliaMath/FixedPointNumbers.jl.git",
            "0bd71249b13704f0a18b7ac7d5306712668c4ef9",
            "MIT",
            "LICENSE.md",
            &["src", "export"],
            common_generated(),
            common_tests(),
            julia(),
        ),
        spec(
            "julia-json",
            "julia",
            SizeStratum::Medium,
            "https://github.com/JuliaIO/JSON.jl.git",
            "e5ef310dece16746843753e4c3b44e868b917b64",
            "MIT",
            "LICENSE.md",
            &["src", "export"],
            common_generated(),
            common_tests(),
            julia(),
        ),
        spec(
            "julia-datastructures",
            "julia",
            SizeStratum::Large,
            "https://github.com/JuliaCollections/DataStructures.jl.git",
            "aaeab026a860400cb94c15f321147a5db4269450",
            "MIT",
            "License.md",
            &["src", "export"],
            common_generated(),
            common_tests(),
            julia(),
        ),
        spec(
            "python-itsdangerous",
            "python",
            SizeStratum::Small,
            "https://github.com/pallets/itsdangerous.git",
            "672971d66a2ef9f85151e53283113f33d642dabd",
            "BSD-3-Clause",
            "LICENSE.txt",
            &["src", "__init__.py"],
            common_generated(),
            common_tests(),
            python(),
        ),
        spec(
            "python-cachecontrol",
            "python",
            SizeStratum::Medium,
            "https://github.com/psf/cachecontrol.git",
            "23c7cb048fafc46c6a290050eb17d7dc3f8f8b65",
            "Apache-2.0",
            "LICENSE.txt",
            &["cachecontrol", "__init__.py"],
            common_generated(),
            common_tests(),
            python(),
        ),
        spec(
            "python-inflect",
            "python",
            SizeStratum::Large,
            "https://github.com/jaraco/inflect.git",
            "262a247d2d99a47a520cdb2d46adb90df88b4326",
            "MIT",
            "pyproject.toml",
            &["inflect", "__all__"],
            common_generated(),
            common_tests(),
            python(),
        ),
        spec(
            "rust-itoa",
            "rust",
            SizeStratum::Small,
            "https://github.com/dtolnay/itoa.git",
            "1577ed901354d0d7448ac162328f9dbf5183124c",
            "MIT OR Apache-2.0",
            "LICENSE-MIT",
            &["src/lib.rs", "pub"],
            common_generated(),
            common_tests(),
            cargo(&["test", "--all-features"]),
        ),
        spec(
            "rust-ryu",
            "rust",
            SizeStratum::Medium,
            "https://github.com/dtolnay/ryu.git",
            "22a692e0b27d9ca74231a475eb690a9446ed44af",
            "Apache-2.0 OR BSL-1.0",
            "LICENSE-APACHE",
            &["src/lib.rs", "pub"],
            common_generated(),
            common_tests(),
            cargo(&["test", "--all-features"]),
        ),
        spec(
            "rust-byteorder",
            "rust",
            SizeStratum::Large,
            "https://github.com/BurntSushi/byteorder.git",
            "5a82625fae462e8ba64cec8146b24a372b4d75c6",
            "Unlicense OR MIT",
            "COPYING",
            &["src/lib.rs", "pub"],
            common_generated(),
            common_tests(),
            cargo(&["test", "--all-features"]),
        ),
        spec(
            "typescript-tslib",
            "typescript",
            SizeStratum::Small,
            "https://github.com/microsoft/tslib.git",
            "12bd8a74b320e3acfaba36b0ecb0e14964a9165b",
            "0BSD",
            "LICENSE.txt",
            &["tslib.d.ts", "export"],
            common_generated(),
            common_tests(),
            npm(),
        ),
        spec(
            "typescript-type-fest",
            "typescript",
            SizeStratum::Medium,
            "https://github.com/sindresorhus/type-fest.git",
            "48ddc4ba71cb215c3e3d98b0257360edc229fa75",
            "MIT OR CC0-1.0",
            "license-mit",
            &["index.d.ts", "source"],
            common_generated(),
            common_tests(),
            npm(),
        ),
        spec(
            "typescript-zod",
            "typescript",
            SizeStratum::Large,
            "https://github.com/colinhacks/zod.git",
            "912f0f51b0ced654d0069741e7160834dca742ee",
            "MIT",
            "LICENSE",
            &["packages", "index.ts"],
            common_generated(),
            common_tests(),
            npm(),
        ),
    ];
    projects.sort_by(|left, right| left.id.cmp(&right.id));
    let mut manifest = ExternalProjectManifest {
        schema: M10_EXTERNAL_MANIFEST_SCHEMA.into(),
        manifest_id: String::new(),
        projects,
    };
    manifest.manifest_id = external_manifest_id(&manifest.projects)?;
    manifest.validate()?;
    Ok(manifest)
}

#[allow(clippy::too_many_arguments)]
fn spec(
    id: &str,
    language: &str,
    size_stratum: SizeStratum,
    repository_url: &str,
    revision: &str,
    license_spdx: &str,
    license_evidence: &str,
    public_api_markers: &[&str],
    generated_markers: Vec<String>,
    test_markers: Vec<String>,
    test_command: ExternalCommand,
) -> ExternalProjectSpec {
    ExternalProjectSpec {
        id: id.into(),
        language: language.into(),
        size_stratum,
        repository_url: repository_url.into(),
        revision: revision.into(),
        license_spdx: license_spdx.into(),
        license_evidence: license_evidence.into(),
        public_api_markers: public_api_markers
            .iter()
            .map(|value| (*value).into())
            .collect(),
        generated_markers,
        test_markers,
        test_command,
    }
}

fn command(program: &str, arguments: &[&str]) -> ExternalCommand {
    ExternalCommand {
        program: program.into(),
        arguments: arguments.iter().map(|value| (*value).into()).collect(),
        timeout_seconds: 180,
    }
}

pub fn write_external_manifest(path: &Path, manifest: &ExternalProjectManifest) -> Result<()> {
    manifest.validate()?;
    write_json(path, manifest)
}

pub fn read_external_manifest(path: &Path) -> Result<ExternalProjectManifest> {
    let manifest: ExternalProjectManifest = read_json(path)?;
    manifest.validate()?;
    Ok(manifest)
}

pub fn verify_external_checkouts(
    manifest_path: &Path,
    checkout_root: &Path,
) -> Result<ExternalProjectManifest> {
    let manifest = read_external_manifest(manifest_path)?;
    if manifest != default_external_manifest()? {
        bail!("external manifest differs from the compiled frozen release set");
    }
    for project in &manifest.projects {
        let checkout = checkout_root.join(&project.id);
        let revision = git_output(&checkout, &["rev-parse", "HEAD"])?;
        if revision.trim() != project.revision {
            bail!(
                "external checkout {} is not at its pinned revision",
                project.id
            );
        }
        if !checkout.join(&project.license_evidence).is_file() {
            bail!("external checkout {} lacks license evidence", project.id);
        }
    }
    Ok(manifest)
}

pub fn evaluate_external_projects(
    manifest_path: &Path,
    checkout_root: &Path,
    run_tests: bool,
) -> Result<ExternalEvaluationReport> {
    let manifest = verify_external_checkouts(manifest_path, checkout_root)?;
    let environment = external_environment(checkout_root);
    let mut projects = Vec::with_capacity(manifest.projects.len());
    for project in &manifest.projects {
        projects.push(evaluate_project(project, checkout_root, run_tests)?);
    }
    projects.sort_by(|left, right| left.id.cmp(&right.id));
    let mut report = ExternalEvaluationReport {
        schema: M10_EXTERNAL_REPORT_SCHEMA.into(),
        report_id: String::new(),
        manifest_id: manifest.manifest_id,
        environment,
        projects,
    };
    report.report_id =
        external_report_id(&report.manifest_id, &report.environment, &report.projects)?;
    if run_tests {
        report.validate()?;
    }
    Ok(report)
}

fn evaluate_project(
    project: &ExternalProjectSpec,
    checkout_root: &Path,
    run_tests: bool,
) -> Result<ExternalProjectEvidence> {
    let checkout = checkout_root.join(&project.id);
    let context =
        scan_paths_with_context(std::slice::from_ref(&checkout), AnalyzerConfig::default())
            .with_context(|| format!("scan external project {}", project.id))?;
    let source_lines = context
        .input_contents
        .values()
        .map(|source| source.lines().count())
        .sum();
    let mut reports_by_language = BTreeMap::new();
    let mut analysis_statuses = BTreeMap::new();
    let mut findings_by_safety = BTreeMap::new();
    let mut findings_by_rule = BTreeMap::new();
    let mut proposal_eligible_findings = 0;
    let mut report_only_findings = 0;
    let mut paths = BTreeSet::new();
    for report in &context.reports {
        paths.insert(report.path.to_string_lossy().into_owned());
        *reports_by_language
            .entry(report.lang.to_string())
            .or_default() += 1;
        *analysis_statuses
            .entry(status_name(report.analysis.status).into())
            .or_default() += 1;
        for finding in &report.findings {
            *findings_by_safety
                .entry(safety_name(finding.safety).into())
                .or_default() += 1;
            *findings_by_rule.entry(finding.rule.clone()).or_default() += 1;
            if finding.safety.permits_proposal() {
                proposal_eligible_findings += 1;
            } else {
                report_only_findings += 1;
            }
        }
    }
    let public_api_files = marker_count(&paths, &project.public_api_markers);
    let generated_files = marker_count(&paths, &project.generated_markers);
    let test_files = marker_count(&paths, &project.test_markers);
    let scan_digest = digest_json(
        "deslop m10 external scan v1",
        &(&context.reports, &context.external_capabilities),
    )?;
    let test = if run_tests {
        execute_external_command(&checkout, &project.test_command)?
    } else {
        CommandEvidence {
            command: project.test_command.clone(),
            status: CommandStatus::NotRun,
            exit_code: None,
            stdout_digest: None,
            stderr_digest: None,
            diagnostic: Some("external command was explicitly not run".into()),
        }
    };
    let clean = git_output(&checkout, &["status", "--porcelain"])?
        .trim()
        .is_empty();
    Ok(ExternalProjectEvidence {
        id: project.id.clone(),
        language: project.language.clone(),
        size_stratum: project.size_stratum,
        revision: project.revision.clone(),
        exact_revision: true,
        license_evidence_present: true,
        source_files: context.reports.len(),
        source_lines,
        reports_by_language,
        analysis_statuses,
        findings_by_safety,
        findings_by_rule,
        proposal_eligible_findings,
        report_only_findings,
        review_pending_findings: proposal_eligible_findings,
        public_api_files,
        generated_files,
        test_files,
        scan_digest,
        workflow: [
            "scan",
            "triage",
            "explain",
            "review-pending",
            "verify-or-reject",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        test,
        working_tree_clean_after: clean,
    })
}

fn marker_count(paths: &BTreeSet<String>, markers: &[String]) -> usize {
    paths
        .iter()
        .filter(|path| markers.iter().any(|marker| path.contains(marker)))
        .count()
}

fn execute_external_command(checkout: &Path, command: &ExternalCommand) -> Result<CommandEvidence> {
    if !executable_available(&command.program) {
        return Ok(CommandEvidence {
            command: command.clone(),
            status: CommandStatus::ToolUnavailable,
            exit_code: None,
            stdout_digest: None,
            stderr_digest: None,
            diagnostic: Some(format!("{} is not available on PATH", command.program)),
        });
    }
    let output = Command::new("timeout")
        .arg("--signal=KILL")
        .arg(format!("{}s", command.timeout_seconds))
        .arg(&command.program)
        .args(&command.arguments)
        .current_dir(checkout)
        .output()
        .with_context(|| format!("run external command in {}", checkout.display()))?;
    let exit_code = output.status.code();
    let status = match exit_code {
        Some(0) => CommandStatus::Passed,
        Some(124 | 137) | None => CommandStatus::TimedOut,
        _ => CommandStatus::Failed,
    };
    let diagnostic = if status == CommandStatus::Passed {
        None
    } else {
        Some(bounded_diagnostic(&output.stderr, &output.stdout))
    };
    Ok(CommandEvidence {
        command: command.clone(),
        status,
        exit_code,
        stdout_digest: Some(digest("deslop m10 external stdout v1", &output.stdout)),
        stderr_digest: Some(digest("deslop m10 external stderr v1", &output.stderr)),
        diagnostic,
    })
}

fn executable_available(program: &str) -> bool {
    if program.contains('/') {
        return Path::new(program).is_file();
    }
    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|directory| directory.join(program).is_file())
    })
}

fn bounded_diagnostic(stderr: &[u8], stdout: &[u8]) -> String {
    let bytes = if stderr.is_empty() { stdout } else { stderr };
    let text = String::from_utf8_lossy(bytes);
    let mut diagnostic = text.chars().take(1000).collect::<String>();
    if text.chars().count() > 1000 {
        diagnostic.push_str("…[truncated]");
    }
    diagnostic
}

fn external_environment(checkout_root: &Path) -> ExternalEvaluationEnvironment {
    let tool_versions = [
        ("cargo", &["--version"][..]),
        ("npm", &["--version"][..]),
        ("python3", &["--version"][..]),
        ("lein", &["version"][..]),
        ("julia", &["--version"][..]),
    ]
    .into_iter()
    .map(|(program, arguments)| (program.into(), tool_version(program, arguments)))
    .collect();
    ExternalEvaluationEnvironment {
        os: env::consts::OS.into(),
        architecture: env::consts::ARCH.into(),
        tool_versions,
        checkout_root: checkout_root.to_string_lossy().into_owned(),
        dependency_cache_state:
            "ambient-user-caches; dependency caches are not isolated; command results are environment-qualified"
                .into(),
    }
}

fn tool_version(program: &str, arguments: &[&str]) -> Option<String> {
    if !executable_available(program) {
        return None;
    }
    let output = Command::new(program).args(arguments).output().ok()?;
    let bytes = if output.stdout.is_empty() {
        output.stderr
    } else {
        output.stdout
    };
    Some(String::from_utf8_lossy(&bytes).trim().to_string())
}

fn git_output(checkout: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(arguments)
        .output()
        .with_context(|| format!("run git in {}", checkout.display()))?;
    if !output.status.success() {
        bail!(
            "git failed in {}: {}",
            checkout.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn safety_name(safety: SafetyClass) -> &'static str {
    match safety {
        SafetyClass::SafeAuto => "safe-auto",
        SafetyClass::AnalyzerConfirmed => "analyzer-confirmed",
        SafetyClass::SafeWithPrecondition => "safe-with-precondition",
        SafetyClass::RiskySuggest => "risky-suggest",
        SafetyClass::LlmOnly => "llm-only",
        SafetyClass::NeverAuto => "never-auto",
    }
}

fn status_name(status: AnalysisStatus) -> &'static str {
    match status {
        AnalysisStatus::Unknown => "unknown",
        AnalysisStatus::Complete => "complete",
        AnalysisStatus::Partial => "partial",
        AnalysisStatus::Unsupported => "unsupported",
        AnalysisStatus::Failed => "failed",
    }
}

fn external_manifest_id(projects: &[ExternalProjectSpec]) -> Result<String> {
    Ok(format!(
        "m10xp1_{}",
        &digest_json("deslop m10 external manifest v1", projects)?[7..]
    ))
}

fn external_report_id(
    manifest_id: &str,
    environment: &ExternalEvaluationEnvironment,
    projects: &[ExternalProjectEvidence],
) -> Result<String> {
    Ok(format!(
        "m10xr1_{}",
        &digest_json(
            "deslop m10 external report v1",
            &(manifest_id, environment, projects),
        )?[7..]
    ))
}

fn digest_json(domain: &str, value: &(impl Serialize + ?Sized)) -> Result<String> {
    Ok(digest(domain, &serde_json::to_vec(value)?))
}

fn digest(domain: &str, bytes: &[u8]) -> String {
    let digest = blake3::derive_key(domain, bytes);
    format!("blake3:{}", blake3::Hash::from_bytes(digest).to_hex())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("read {}", path.display()))?)
        .with_context(|| format!("decode {}", path.display()))
}

pub fn write_external_report(path: &Path, report: &ExternalEvaluationReport) -> Result<()> {
    report.validate()?;
    write_json(path, report)
}

pub fn read_external_report(path: &Path) -> Result<ExternalEvaluationReport> {
    let report: ExternalEvaluationReport = read_json(path)?;
    report.validate()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_external_manifest_is_an_exact_six_language_size_grid() {
        let manifest = default_external_manifest().unwrap();
        manifest.validate().unwrap();
        assert_eq!(manifest.projects.len(), 18);
        assert_eq!(
            manifest
                .projects
                .iter()
                .map(|project| project.language.as_str())
                .collect::<BTreeSet<_>>()
                .len(),
            6
        );
    }
}
