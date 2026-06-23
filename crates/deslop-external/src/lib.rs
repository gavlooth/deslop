use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use deslop_core::{
    DetectedBy, Edit, EditKind, Finding, SafetyClass, Severity, Span, Splice, fingerprint,
};
pub use deslop_lang::{ExternalAnalyzer, ExternalFindings};
use deslop_parse::SourceFile;
use serde::Deserialize;

const JULIA_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy)]
pub struct CljKondoAnalyzer;

impl ExternalAnalyzer<SourceFile, Finding> for CljKondoAnalyzer {
    fn name(&self) -> &'static str {
        "clj-kondo"
    }

    fn covered_rules(&self) -> &'static [&'static str] {
        &[
            "unused-binding",
            "unused-private-def",
            "unused-namespace",
            "redundant-do",
        ]
    }

    fn analyze(&self, path: &Path, source: &SourceFile) -> Result<ExternalFindings<Finding>> {
        clj_kondo_file(path, source)
    }
}

#[derive(Debug, Clone)]
pub struct ClippyAnalyzer {
    command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JuliaAnalyzerKind {
    StaticLint,
    Jet,
}

impl JuliaAnalyzerKind {
    fn analyzer_name(self) -> &'static str {
        match self {
            Self::StaticLint => "StaticLint",
            Self::Jet => "JET",
        }
    }

    fn helper(self) -> &'static str {
        match self {
            Self::StaticLint => STATICLINT_HELPER,
            Self::Jet => JET_HELPER,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JuliaAnalyzer {
    command: String,
    project: Option<PathBuf>,
    kind: JuliaAnalyzerKind,
}

impl Default for ClippyAnalyzer {
    fn default() -> Self {
        Self {
            command: "cargo".to_string(),
        }
    }
}

impl ClippyAnalyzer {
    pub fn with_command(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
        }
    }
}

impl JuliaAnalyzer {
    pub fn staticlint(project: Option<PathBuf>) -> Self {
        Self {
            command: "julia".to_string(),
            project,
            kind: JuliaAnalyzerKind::StaticLint,
        }
    }

    pub fn jet(project: Option<PathBuf>) -> Self {
        Self {
            command: "julia".to_string(),
            project,
            kind: JuliaAnalyzerKind::Jet,
        }
    }

    pub fn with_command(
        command: impl Into<String>,
        project: Option<PathBuf>,
        kind: JuliaAnalyzerKind,
    ) -> Self {
        Self {
            command: command.into(),
            project,
            kind,
        }
    }
}

impl ExternalAnalyzer<SourceFile, Finding> for ClippyAnalyzer {
    fn name(&self) -> &'static str {
        "clippy"
    }

    fn covered_rules(&self) -> &'static [&'static str] {
        &[
            "needless-return",
            "needless-clone",
            "let-and-return",
            "useless-format",
            "redundant-closure",
        ]
    }

    fn analyze(&self, path: &Path, source: &SourceFile) -> Result<ExternalFindings<Finding>> {
        clippy_file_with_command(&self.command, path, source)
    }
}

impl ExternalAnalyzer<SourceFile, Finding> for JuliaAnalyzer {
    fn name(&self) -> &'static str {
        match self.kind {
            JuliaAnalyzerKind::StaticLint => "StaticLint.jl",
            JuliaAnalyzerKind::Jet => "JET.jl",
        }
    }

    fn covered_rules(&self) -> &'static [&'static str] {
        match self.kind {
            JuliaAnalyzerKind::StaticLint => &["unused-arg", "unused-binding"],
            JuliaAnalyzerKind::Jet => &["julia-jet"],
        }
    }

    fn analyze(&self, path: &Path, source: &SourceFile) -> Result<ExternalFindings<Finding>> {
        julia_file_with_command(
            &self.command,
            self.project.as_deref(),
            self.kind,
            path,
            source,
        )
    }
}

pub fn clj_kondo_file(path: &Path, source: &SourceFile) -> Result<ExternalFindings<Finding>> {
    clj_kondo_file_with_command("clj-kondo", path, source)
}

pub fn clj_kondo_file_with_command(
    command: &str,
    path: &Path,
    source: &SourceFile,
) -> Result<ExternalFindings<Finding>> {
    findings_from_command(
        clj_kondo_command(command, path),
        source,
        findings_from_clj_kondo_json,
        None,
        "clj-kondo not on PATH; falling back to built-in T1 Clojure rules",
        None,
        clj_kondo_failure_notice,
        "failed to run clj-kondo",
    )
}

pub fn findings_from_clj_kondo_json(source: &SourceFile, json: &str) -> Result<Vec<Finding>> {
    let report: KondoReport =
        serde_json::from_str(json).context("failed to parse clj-kondo JSON")?;
    let mut out = Vec::new();
    for finding in report.findings {
        let Some(rule) = map_type_to_rule(&finding.finding_type) else {
            continue;
        };
        let line = finding.row.unwrap_or(1).max(1);
        let message = finding
            .message
            .unwrap_or_else(|| format!("clj-kondo reported {}", finding.finding_type));
        let edit = if rule == "redundant-do" {
            redundant_do_edit(source, line)
        } else {
            None
        };
        out.push(make_finding(
            source,
            line,
            rule,
            Severity::Minor,
            message,
            suggestion_for(rule),
            DetectedBy::CljKondo,
            edit,
        ));
    }
    Ok(out)
}

pub fn clippy_file_with_command(
    command: &str,
    path: &Path,
    source: &SourceFile,
) -> Result<ExternalFindings<Finding>> {
    let Some(root) = nearest_cargo_root(path) else {
        return Ok(ExternalFindings::Unavailable {
            notice: "clippy skipped: no Cargo.toml found for Rust file".to_string(),
        });
    };
    findings_from_command(
        clippy_command(command, &root),
        source,
        findings_from_clippy_json,
        None,
        "cargo/clippy not on PATH; falling back to built-in Rust rules",
        None,
        clippy_failure_notice,
        "failed to run cargo clippy",
    )
}

pub fn julia_file_with_command(
    command: &str,
    project: Option<&Path>,
    kind: JuliaAnalyzerKind,
    path: &Path,
    source: &SourceFile,
) -> Result<ExternalFindings<Finding>> {
    findings_from_command(
        julia_command(command, project, kind.helper(), path),
        source,
        |source, stdout| julia_findings_from_json(kind, source, stdout),
        Some(JULIA_TIMEOUT),
        "julia not on PATH; falling back to built-in T1 Julia rules",
        Some(julia_unavailable_notice(kind, "timed out".to_string())),
        |output| julia_unavailable_notice(kind, julia_failure_detail(output)),
        "failed to run julia analyzer",
    )
}

fn clj_kondo_command(command: &str, path: &Path) -> Command {
    let mut command = Command::new(command);
    command.args([
        "--lint",
        &path.to_string_lossy(),
        "--config",
        "{:output {:analysis true :format :json}}",
    ]);
    command
}

fn clippy_command(command: &str, root: &Path) -> Command {
    let mut command = Command::new(command);
    command
        .args([
            "clippy",
            "--message-format=json",
            "--quiet",
            "--",
            "-A",
            "warnings",
        ])
        .current_dir(root);
    command
}

fn unavailable_findings(notice: String) -> ExternalFindings<Finding> {
    ExternalFindings::Unavailable { notice }
}

#[allow(clippy::too_many_arguments)]
fn findings_from_command(
    command: Command,
    source: &SourceFile,
    parse_findings: impl FnOnce(&SourceFile, &str) -> Result<Vec<Finding>>,
    timeout: Option<Duration>,
    not_found_notice: &str,
    timeout_notice: Option<String>,
    failure_notice: impl FnOnce(&Output) -> String,
    error_context: &str,
) -> Result<ExternalFindings<Finding>> {
    match capture_external_stdout(
        command,
        timeout,
        not_found_notice,
        timeout_notice,
        failure_notice,
        error_context,
    )? {
        CapturedStdout::Available(stdout) => Ok(ExternalFindings::Available(parse_findings(
            source, &stdout,
        )?)),
        CapturedStdout::Unavailable(notice) => Ok(unavailable_findings(notice)),
    }
}

enum CapturedStdout {
    Available(String),
    Unavailable(String),
}

fn capture_external_stdout(
    command: Command,
    timeout: Option<Duration>,
    not_found_notice: &str,
    timeout_notice: Option<String>,
    failure_notice: impl FnOnce(&Output) -> String,
    error_context: &str,
) -> Result<CapturedStdout> {
    let output = match run_command(command, timeout) {
        Ok(Some(output)) => output,
        Ok(None) => {
            return Ok(CapturedStdout::Unavailable(
                timeout_notice.unwrap_or_else(|| "external analyzer timed out".to_string()),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CapturedStdout::Unavailable(not_found_notice.to_string()));
        }
        Err(error) => return Err(error).context(error_context.to_string()),
    };

    if !output.status.success() && output.stdout.is_empty() {
        return Ok(CapturedStdout::Unavailable(failure_notice(&output)));
    }

    Ok(CapturedStdout::Available(
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}

fn julia_command(command: &str, project: Option<&Path>, helper: &str, path: &Path) -> Command {
    let mut command = Command::new(command);
    command.arg("--startup-file=no");
    if let Some(project) = project {
        command.arg(format!("--project={}", project.display()));
    }
    command.arg("-e").arg(helper).arg("--").arg(path);
    command
}

fn julia_findings_from_json(
    kind: JuliaAnalyzerKind,
    source: &SourceFile,
    json: &str,
) -> Result<Vec<Finding>> {
    match kind {
        JuliaAnalyzerKind::StaticLint => findings_from_staticlint_json(source, json),
        JuliaAnalyzerKind::Jet => findings_from_jet_json(source, json),
    }
}

fn julia_failure_detail(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.lines().next().unwrap_or("").trim();
    if detail.is_empty() {
        format!("status {}", output.status)
    } else {
        detail.to_string()
    }
}

fn clj_kondo_failure_notice(output: &Output) -> String {
    format!(
        "clj-kondo failed with status {}; falling back to built-in T1 Clojure rules",
        output.status
    )
}

fn clippy_failure_notice(output: &Output) -> String {
    format!(
        "cargo clippy failed with status {}; falling back to built-in Rust rules",
        output.status
    )
}

fn julia_unavailable_notice(kind: JuliaAnalyzerKind, detail: String) -> String {
    format!(
        "Julia {} unavailable ({detail}); falling back to built-in T1 Julia rules",
        kind.analyzer_name()
    )
}

pub fn findings_from_staticlint_json(source: &SourceFile, json: &str) -> Result<Vec<Finding>> {
    let report = parse_julia_diagnostics(json, "StaticLint")?;
    let mut out = Vec::new();
    for diagnostic in report.diagnostics {
        let rule = diagnostic.rule.as_deref().unwrap_or_default();
        let Some(rule) = map_staticlint_rule(rule, diagnostic.message.as_deref()) else {
            continue;
        };
        let (line, message) =
            diagnostic_location_message(diagnostic, || format!("StaticLint reported {rule}"));
        let (safety, suggestion) = match rule {
            "unused-arg" => (
                SafetyClass::AnalyzerConfirmed,
                "remove or inline the unused argument after semantic review",
            ),
            "unused-binding" => (
                SafetyClass::AnalyzerConfirmed,
                "remove or inline the unused binding after semantic review",
            ),
            "missing-reference" => (
                SafetyClass::NeverAuto,
                "review the unresolved reference; this is report-only",
            ),
            _ => (SafetyClass::NeverAuto, "review StaticLint finding"),
        };
        out.push(make_finding_with_safety(
            source,
            line,
            rule,
            Severity::Minor,
            safety,
            message,
            suggestion,
            DetectedBy::JuliaAnalyzer,
            None,
        ));
    }
    Ok(out)
}

pub fn findings_from_jet_json(source: &SourceFile, json: &str) -> Result<Vec<Finding>> {
    let report = parse_julia_diagnostics(json, "JET")?;
    let mut out = Vec::new();
    for diagnostic in report.diagnostics {
        let (line, message) = diagnostic_location_message(diagnostic, || {
            "JET reported a possible Julia issue".to_string()
        });
        out.push(make_finding_with_safety(
            source,
            line,
            "julia-jet",
            Severity::Minor,
            SafetyClass::NeverAuto,
            message,
            "review JET report; correctness diagnostics are report-only",
            DetectedBy::JuliaAnalyzer,
            None,
        ));
    }
    Ok(out)
}

fn parse_julia_diagnostics(json: &str, name: &str) -> Result<JuliaDiagnostics> {
    serde_json::from_str(json).with_context(|| format!("failed to parse {name} JSON"))
}

fn diagnostic_location_message(
    diagnostic: JuliaDiagnostic,
    fallback: impl FnOnce() -> String,
) -> (usize, String) {
    let line = diagnostic.line.unwrap_or(1).max(1);
    let message = diagnostic.message.unwrap_or_else(fallback);
    (line, message)
}

pub fn findings_from_clippy_json(source: &SourceFile, jsonl: &str) -> Result<Vec<Finding>> {
    let mut out = Vec::new();
    for (idx, line) in jsonl.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)
            .with_context(|| format!("failed to parse clippy JSON line {}", idx + 1))?;
        if value.get("reason").and_then(|reason| reason.as_str()) != Some("compiler-message") {
            continue;
        }
        let message = &value["message"];
        let Some(code) = message
            .get("code")
            .and_then(|code| code.get("code"))
            .and_then(|code| code.as_str())
        else {
            continue;
        };
        let Some(rule) = map_clippy_code(code) else {
            continue;
        };
        let line = message
            .get("spans")
            .and_then(|spans| spans.as_array())
            .and_then(|spans| spans.first())
            .and_then(|span| span.get("line_start"))
            .and_then(|line| line.as_u64())
            .map(|line| line as usize)
            .unwrap_or(1);
        let text = message
            .get("message")
            .and_then(|message| message.as_str())
            .unwrap_or(rule)
            .to_string();
        out.push(make_finding(
            source,
            line,
            rule,
            Severity::Minor,
            text,
            suggestion_for_rust(rule),
            DetectedBy::RustAnalyzer,
            None,
        ));
    }
    Ok(out)
}

fn run_command(mut command: Command, timeout: Option<Duration>) -> std::io::Result<Option<Output>> {
    let Some(timeout) = timeout else {
        return command.output().map(Some);
    };

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output().map(Some);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn map_staticlint_rule(rule: &str, message: Option<&str>) -> Option<&'static str> {
    let normalized = rule.trim().to_ascii_lowercase().replace(['_', ' '], "-");
    match normalized.as_str() {
        "unused-arg" | "unused-argument" | "unused-function-argument" => Some("unused-arg"),
        "unused-binding" | "unused-variable" | "unused-local" | "unused-var" => {
            Some("unused-binding")
        }
        "missing-reference" | "undefined-var" | "undefined-variable" | "unresolved-reference" => {
            Some("missing-reference")
        }
        _ => message.and_then(|message| {
            let message = message.to_ascii_lowercase();
            if message.contains("unused") && message.contains("argument") {
                Some("unused-arg")
            } else if message.contains("unused") {
                Some("unused-binding")
            } else if message.contains("missing reference")
                || message.contains("undefined")
                || message.contains("unresolved")
            {
                Some("missing-reference")
            } else {
                None
            }
        }),
    }
}

fn map_clippy_code(code: &str) -> Option<&'static str> {
    match code.strip_prefix("clippy::").unwrap_or(code) {
        "needless_return" => Some("needless-return"),
        "redundant_clone" => Some("needless-clone"),
        "let_and_return" => Some("let-and-return"),
        "useless_format" => Some("useless-format"),
        "redundant_closure" => Some("redundant-closure"),
        _ => None,
    }
}

fn suggestion_for_rust(rule: &str) -> &'static str {
    match rule {
        "needless-return" => "remove the tail-position return after checks pass",
        "needless-clone" => "remove clone only when borrow/lifetime checks pass",
        "let-and-return" => "return the expression directly after checks pass",
        "useless-format" => "use to_string when type semantics are acceptable",
        "redundant-closure" => "use the function directly when inference remains valid",
        _ => "review clippy finding",
    }
}

fn nearest_cargo_root(path: &Path) -> Option<PathBuf> {
    let mut current = path.parent()?;
    loop {
        if current.join("Cargo.toml").exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn map_type_to_rule(value: &str) -> Option<&'static str> {
    match value {
        "unused-binding" => Some("unused-binding"),
        "unused-private-var" => Some("unused-private-def"),
        "unused-namespace" => Some("unused-namespace"),
        "redundant-do" => Some("redundant-do"),
        _ => None,
    }
}

fn suggestion_for(rule: &str) -> &'static str {
    match rule {
        "unused-binding" => "remove or inline the unused binding after semantic review",
        "unused-private-def" => "remove the private var only after project tests pass",
        "unused-namespace" => "remove the unused namespace require/import",
        "redundant-do" => "drop the inner do; clj-kondo confirmed it is redundant",
        _ => "review clj-kondo finding",
    }
}

fn redundant_do_edit(source: &SourceFile, line: usize) -> Option<Edit> {
    let line_text = source.line_text(line);
    let do_col = line_text.find("(do ")?;
    let close_col = line_text.rfind(')')?;
    if close_col <= do_col {
        return None;
    }
    let start = source.line_start_byte(line) + do_col;
    let end = source.line_start_byte(line) + do_col + 4;
    let last_close = source.line_start_byte(line) + close_col;
    Some(Edit {
        kind: EditKind::AnalyzerConfirmed,
        splices: vec![
            Splice {
                start_byte: last_close,
                end_byte: last_close + 1,
                replacement: String::new(),
            },
            Splice {
                start_byte: start,
                end_byte: end,
                replacement: String::new(),
            },
        ],
    })
}

#[allow(clippy::too_many_arguments)]
fn make_finding(
    source: &SourceFile,
    line: usize,
    rule: &str,
    severity: Severity,
    message: String,
    suggestion: &str,
    detected_by: DetectedBy,
    edit: Option<Edit>,
) -> Finding {
    make_finding_with_safety(
        source,
        line,
        rule,
        severity,
        SafetyClass::AnalyzerConfirmed,
        message,
        suggestion,
        detected_by,
        edit,
    )
}

#[allow(clippy::too_many_arguments)]
fn make_finding_with_safety(
    source: &SourceFile,
    line: usize,
    rule: &str,
    severity: Severity,
    safety: SafetyClass,
    message: String,
    suggestion: &str,
    detected_by: DetectedBy,
    edit: Option<Edit>,
) -> Finding {
    let span = Span::new(
        line,
        line,
        source.line_start_byte(line),
        source.line_end_byte(line),
    );
    let text = source.region_text(line, line);
    Finding {
        path: source.path.to_path_buf(),
        span,
        rule: rule.to_string(),
        severity,
        safety,
        detected_by,
        message,
        suggestion: suggestion.to_string(),
        precondition: None,
        edit,
        fingerprint: fingerprint(&source.path, rule, span, &text),
    }
}

#[derive(Debug, Deserialize)]
struct KondoReport {
    #[serde(default)]
    findings: Vec<KondoFinding>,
}

#[derive(Debug, Deserialize)]
struct KondoFinding {
    #[serde(rename = "type")]
    finding_type: String,
    #[serde(default)]
    row: Option<usize>,
    #[serde(default)]
    message: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    filename: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct JuliaDiagnostics {
    #[serde(default)]
    diagnostics: Vec<JuliaDiagnostic>,
}

#[derive(Debug, Deserialize)]
struct JuliaDiagnostic {
    #[serde(default, alias = "code", alias = "kind", alias = "lint")]
    rule: Option<String>,
    #[serde(default, alias = "row")]
    line: Option<usize>,
    #[serde(default)]
    message: Option<String>,
}

const STATICLINT_HELPER: &str = r#"
path = only(ARGS)
try
    @eval using StaticLint
catch err
    println(stderr, "StaticLint unavailable: ", sprint(showerror, err))
    exit(86)
end

function json_escape(value)
    value = replace(String(value), "\\" => "\\\\")
    value = replace(value, "\"" => "\\\"")
    value = replace(value, "\n" => "\\n")
    value = replace(value, "\r" => "\\r")
    value
end

diagnostics = Any[]
try
    if isdefined(StaticLint, :lint_file)
        result = StaticLint.lint_file(path)
        if result isa AbstractArray
            append!(diagnostics, result)
        elseif result !== nothing
            push!(diagnostics, result)
        end
    end
catch err
    println(stderr, "StaticLint failed: ", sprint(showerror, err))
    exit(87)
end

parts = String[]
for diagnostic in diagnostics
    rule = hasproperty(diagnostic, :code) ? getproperty(diagnostic, :code) :
           hasproperty(diagnostic, :kind) ? getproperty(diagnostic, :kind) :
           typeof(diagnostic)
    line = hasproperty(diagnostic, :line) ? getproperty(diagnostic, :line) : 1
    message = hasproperty(diagnostic, :message) ? getproperty(diagnostic, :message) : sprint(show, diagnostic)
    push!(parts, "{\"rule\":\"$(json_escape(rule))\",\"line\":$(line),\"message\":\"$(json_escape(message))\"}")
end
print("{\"diagnostics\":[" * join(parts, ",") * "]}")
"#;

const JET_HELPER: &str = r#"
path = only(ARGS)
try
    @eval using JET
catch err
    println(stderr, "JET unavailable: ", sprint(showerror, err))
    exit(86)
end

function json_escape(value)
    value = replace(String(value), "\\" => "\\\\")
    value = replace(value, "\"" => "\\\"")
    value = replace(value, "\n" => "\\n")
    value = replace(value, "\r" => "\\r")
    value
end

try
    report = sprint() do io
        JET.report_file(path; target_modules=(@__MODULE__,))
    end
    print("{\"diagnostics\":[{\"rule\":\"jet-report\",\"line\":1,\"message\":\"$(json_escape(report))\"}]}")
catch err
    println(stderr, "JET failed: ", sprint(showerror, err))
    exit(87)
end
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_recorded_clj_kondo_json_fixture() {
        let source = SourceFile::new(
            PathBuf::from("sample.clj"),
            "(ns sample)\n(defn f [x] (let [unused 1] x))\n(when ok (do (work)))\n".into(),
        );
        let json = r#"{
          "findings": [
            {"type":"unused-binding","filename":"sample.clj","row":2,"col":22,"message":"unused binding unused"},
            {"type":"unused-private-var","filename":"sample.clj","row":2,"col":1,"message":"unused private var"},
            {"type":"unused-namespace","filename":"sample.clj","row":1,"col":5,"message":"unused namespace"},
            {"type":"redundant-do","filename":"sample.clj","row":3,"col":10,"message":"redundant do"}
          ],
          "analysis": {"namespace-definitions": []}
        }"#;
        let findings = findings_from_clj_kondo_json(&source, json).expect("mapped");
        let rules: Vec<_> = findings
            .iter()
            .map(|finding| finding.rule.as_str())
            .collect();
        assert_eq!(
            rules,
            vec![
                "unused-binding",
                "unused-private-def",
                "unused-namespace",
                "redundant-do"
            ]
        );
        assert!(
            findings
                .iter()
                .all(|finding| finding.safety == SafetyClass::AnalyzerConfirmed)
        );
        assert!(findings.iter().any(|finding| finding.edit.is_some()));
    }

    #[test]
    fn absent_clj_kondo_degrades_cleanly() {
        let source = SourceFile::new(PathBuf::from("sample.clj"), "(ns sample)\n".into());
        let result =
            clj_kondo_file_with_command("__deslop_missing_clj_kondo__", &source.path, &source)
                .expect("no hard error");
        match result {
            ExternalFindings::Unavailable { notice } => {
                assert!(notice.contains("clj-kondo not on PATH"));
            }
            ExternalFindings::Available(_) => panic!("expected unavailable"),
        }
    }

    #[test]
    fn maps_recorded_clippy_json_fixture() {
        let source = SourceFile::new(PathBuf::from("src/lib.rs"), "fn f() { return 1; }\n".into());
        let json = r#"{"reason":"compiler-message","message":{"code":{"code":"clippy::needless_return"},"message":"unneeded return statement","spans":[{"file_name":"src/lib.rs","line_start":1,"line_end":1}]}}"#;
        let findings = findings_from_clippy_json(&source, json).expect("mapped");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "needless-return");
        assert_eq!(findings[0].safety, SafetyClass::AnalyzerConfirmed);
        assert_eq!(findings[0].detected_by, DetectedBy::RustAnalyzer);
    }

    #[test]
    fn maps_recorded_staticlint_json_fixture() {
        let source = SourceFile::new(
            PathBuf::from("sample.jl"),
            "function f(x, unused_arg)\n    unused_binding = 1\n    return x + missing_ref\nend\n"
                .into(),
        );
        let json = r#"{
          "diagnostics": [
            {"rule":"unused-arg","line":1,"message":"unused argument unused_arg"},
            {"rule":"unused-binding","line":2,"message":"unused binding unused_binding"},
            {"rule":"missing-reference","line":3,"message":"missing reference missing_ref"}
          ]
        }"#;
        let findings = findings_from_staticlint_json(&source, json).expect("mapped");
        let rules: Vec<_> = findings
            .iter()
            .map(|finding| finding.rule.as_str())
            .collect();
        assert_eq!(
            rules,
            vec!["unused-arg", "unused-binding", "missing-reference"]
        );
        assert_eq!(findings[0].safety, SafetyClass::AnalyzerConfirmed);
        assert_eq!(findings[1].safety, SafetyClass::AnalyzerConfirmed);
        assert_eq!(findings[2].safety, SafetyClass::NeverAuto);
        assert!(
            findings
                .iter()
                .all(|finding| finding.detected_by == DetectedBy::JuliaAnalyzer)
        );
    }

    #[test]
    fn absent_julia_degrades_cleanly() {
        let source = SourceFile::new(PathBuf::from("sample.jl"), "x = nothing\n".into());
        let result = julia_file_with_command(
            "__deslop_missing_julia__",
            None,
            JuliaAnalyzerKind::StaticLint,
            &source.path,
            &source,
        )
        .expect("no hard error");
        match result {
            ExternalFindings::Unavailable { notice } => {
                assert!(notice.contains("julia not on PATH"));
            }
            ExternalFindings::Available(_) => panic!("expected unavailable"),
        }
    }

    #[test]
    fn absent_clippy_degrades_cleanly() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file = temp.path().join("src/lib.rs");
        std::fs::create_dir_all(file.parent().unwrap()).expect("mkdir");
        std::fs::write(
            temp.path().join("Cargo.toml"),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
        )
        .expect("cargo toml");
        std::fs::write(&file, "fn f() {}\n").expect("file");
        let source = SourceFile::new(file.to_path_buf(), "fn f() {}\n".into());
        let result =
            clippy_file_with_command("__deslop_missing_cargo__", &file, &source).expect("result");
        match result {
            ExternalFindings::Unavailable { notice } => {
                assert!(notice.contains("cargo/clippy not on PATH"));
            }
            ExternalFindings::Available(_) => panic!("expected unavailable"),
        }
    }
}
