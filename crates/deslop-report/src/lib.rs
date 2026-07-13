use anyhow::Result;
use std::collections::BTreeMap;

use deslop_core::{
    AnalysisDiagnostic, AnalysisStatus, FileReport, Finding, SafetyClass, Severity,
    reports_analysis_status, reports_permit_rewrites,
};
use deslop_protocol::{WorkOrder, validate_workorder_identity};
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
    Sarif,
    Agent,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            "sarif" => Ok(Self::Sarif),
            "agent" => Ok(Self::Agent),
            other => Err(format!("unknown output format `{other}`")),
        }
    }
}

pub fn render_text(reports: &[FileReport]) -> String {
    let mut out = String::new();
    let mut count = 0;
    for report in reports {
        for diagnostic in &report.analysis.diagnostics {
            let location = diagnostic.span.map_or_else(
                || report.path.display().to_string(),
                |span| format!("{}:{}", report.path.display(), span.start_line),
            );
            out.push_str(&format!(
                "{location} [{}] {}\n",
                diagnostic.code, diagnostic.message
            ));
        }
        for finding in &report.findings {
            count += 1;
            out.push_str(&format!(
                "{} [{}/{:?}/{:?}] {}\n",
                finding.loc(),
                finding.rule,
                finding.severity,
                finding.safety,
                finding.message
            ));
            if !finding.suggestion.is_empty() {
                out.push_str(&format!("  suggestion: {}\n", finding.suggestion));
            }
            if let Some(precondition) = &finding.precondition {
                out.push_str(&format!("  precondition: {precondition}\n"));
            }
        }
    }
    if count == 0 {
        if reports_permit_rewrites(reports) {
            out.push_str("No findings.\n");
        } else {
            out.push_str("No authoritative findings; analysis is incomplete.\n");
        }
    }
    out
}

pub fn render_json(reports: &[FileReport]) -> Result<String> {
    Ok(serde_json::to_string_pretty(&ReportEnvelope {
        schema: "deslop.findings/2",
        status: reports_analysis_status(reports),
        reports,
    })?)
}

pub fn render_sarif(reports: &[FileReport]) -> Result<String> {
    let mut rules: BTreeMap<String, Vec<SafetyClass>> = BTreeMap::new();
    let mut results = Vec::new();
    for report in reports {
        if !report.analysis.permits_rewrites() {
            for diagnostic in &report.analysis.diagnostics {
                let safeties = rules
                    .entry(format!("deslop/{}", diagnostic.code))
                    .or_default();
                if !safeties.contains(&SafetyClass::NeverAuto) {
                    safeties.push(SafetyClass::NeverAuto);
                }
            }
            results.extend(
                report
                    .analysis
                    .diagnostics
                    .iter()
                    .map(|diagnostic| sarif_analysis_result(report, diagnostic)),
            );
        }
        for finding in &report.findings {
            let safeties = rules.entry(finding.rule.to_owned()).or_default();
            if !safeties.contains(&finding.safety) {
                safeties.push(finding.safety);
            }
            results.push(sarif_result(finding));
        }
    }
    let rules = rules
        .into_iter()
        .map(|(id, safeties)| {
            let safety = if safeties.len() == 1 {
                json!(safeties[0])
            } else {
                json!("per-finding")
            };
            json!({
                "id": id,
                "shortDescription": { "text": id },
                "properties": { "safety": safety }
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::to_string_pretty(&json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "deslop",
                    "version": env!("CARGO_PKG_VERSION"),
                    "rules": rules
                }
            },
            "results": results
        }]
    }))?)
}

pub fn render_agent(work_orders: &[WorkOrder]) -> Result<String> {
    let mut out = String::new();
    for work_order in work_orders {
        validate_workorder_identity(work_order).map_err(anyhow::Error::msg)?;
        out.push_str(&serde_json::to_string(&work_order)?);
        out.push('\n');
    }
    Ok(out)
}

fn sarif_result(finding: &Finding) -> serde_json::Value {
    json!({
        "ruleId": finding.rule,
        "level": sarif_level(finding.severity),
        "message": { "text": finding.message },
        "properties": {
            "safety": finding.safety,
            "reportOnly": finding.safety == SafetyClass::NeverAuto
        },
        "locations": [{
            "physicalLocation": {
                "artifactLocation": { "uri": finding.path.to_string_lossy() },
                "region": {
                    "startLine": finding.span.start_line,
                    "endLine": finding.span.end_line
                }
            }
        }]
    })
}

fn sarif_analysis_result(
    report: &FileReport,
    diagnostic: &AnalysisDiagnostic,
) -> serde_json::Value {
    let span = diagnostic
        .span
        .unwrap_or_else(|| deslop_core::Span::new(1, 1, 0, 0));
    json!({
        "ruleId": format!("deslop/{}", diagnostic.code),
        "level": "error",
        "message": { "text": format!("[{}] {}", diagnostic.code, diagnostic.message) },
        "properties": {
            "diagnosticCode": diagnostic.code,
            "analysisStatus": report.analysis.status,
            "rewriteBlocked": true
        },
        "locations": [{
            "physicalLocation": {
                "artifactLocation": { "uri": report.path.to_string_lossy() },
                "region": {
                    "startLine": span.start_line,
                    "endLine": span.end_line
                }
            }
        }]
    })
}

fn sarif_level(severity: Severity) -> &'static str {
    match severity {
        Severity::Major => "error",
        Severity::Minor => "warning",
        Severity::Info => "note",
    }
}

#[derive(Serialize)]
struct ReportEnvelope<'a> {
    schema: &'static str,
    status: AnalysisStatus,
    reports: &'a [FileReport],
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use deslop_core::{DetectedBy, SafetyClass, Span};

    use super::*;

    #[test]
    fn sarif_render_has_required_shape_and_locations() {
        let reports = vec![FileReport {
            path: PathBuf::from("src/sample.clj"),
            lang: deslop_core::Lang::Clojure,
            analysis: deslop_core::AnalysisProvenance::complete(),
            findings: vec![
                finding("duplicate-block", Severity::Major, 2),
                finding("narrating-comment", Severity::Minor, 4),
                finding("incompleteness", Severity::Info, 6),
            ],
        }];

        let value: serde_json::Value =
            serde_json::from_str(&render_sarif(&reports).expect("sarif")).expect("json");

        assert_eq!(value["version"], "2.1.0");
        assert!(value.get("$schema").is_some());
        assert_eq!(value["runs"][0]["tool"]["driver"]["name"], "deslop");
        assert_eq!(
            value["runs"][0]["results"]
                .as_array()
                .expect("results")
                .len(),
            reports[0].findings.len()
        );
        assert_json_eq(&value, &["runs", "0", "results", "0", "level"], "error");
        assert_json_eq(&value, &["runs", "0", "results", "1", "level"], "warning");
        assert_json_eq(&value, &["runs", "0", "results", "2", "level"], "note");
        assert_eq!(
            value["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]
                ["uri"],
            "src/sample.clj"
        );
        assert_eq!(
            value["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"]["startLine"],
            2
        );
        assert_eq!(
            value["runs"][0]["tool"]["driver"]["rules"][0]["properties"]["safety"],
            "llm-only"
        );
    }

    #[test]
    fn partial_report_is_explicit_and_never_renders_agent_workorders() {
        let reports = vec![FileReport {
            path: PathBuf::from("malformed.ts"),
            lang: deslop_core::Lang::TypeScript,
            analysis: deslop_core::AnalysisProvenance::partial(vec![AnalysisDiagnostic {
                code: "tree-sitter-error".to_string(),
                message: "syntax recovery".to_string(),
                span: Some(deslop_core::Span::new(2, 2, 10, 11)),
            }]),
            findings: Vec::new(),
        }];

        let text = render_text(&reports);
        assert!(text.contains("malformed.ts:2 [tree-sitter-error]"));
        assert!(text.contains("No authoritative findings"));
        assert!(!text.contains("No findings.\n"));

        let json: serde_json::Value =
            serde_json::from_str(&render_json(&reports).expect("json")).expect("value");
        assert_eq!(json["schema"], "deslop.findings/2");
        assert_eq!(json["status"], "partial");

        let sarif: serde_json::Value =
            serde_json::from_str(&render_sarif(&reports).expect("sarif")).expect("value");
        assert_eq!(
            sarif["runs"][0]["results"][0]["ruleId"],
            "deslop/tree-sitter-error"
        );
        assert_eq!(
            sarif["runs"][0]["results"][0]["properties"]["rewriteBlocked"],
            true
        );
        assert_eq!(render_agent(&[]).expect("empty agent output"), "");
    }

    #[test]
    fn sarif_preserves_per_finding_safety_when_one_rule_has_mixed_evidence() {
        let mut report_only = finding("mixed-rule", Severity::Minor, 2);
        report_only.safety = SafetyClass::NeverAuto;
        let proposal_eligible = finding("mixed-rule", Severity::Minor, 4);
        let reports = [FileReport {
            path: PathBuf::from("src/sample.clj"),
            lang: deslop_core::Lang::Clojure,
            analysis: deslop_core::AnalysisProvenance::complete(),
            findings: vec![proposal_eligible, report_only],
        }];

        let value: serde_json::Value =
            serde_json::from_str(&render_sarif(&reports).expect("sarif")).expect("json");
        let results = value["runs"][0]["results"].as_array().expect("results");
        assert_eq!(results[0]["properties"]["safety"], "llm-only");
        assert_eq!(results[0]["properties"]["reportOnly"], false);
        assert_eq!(results[1]["properties"]["safety"], "never-auto");
        assert_eq!(results[1]["properties"]["reportOnly"], true);
        assert_eq!(
            value["runs"][0]["tool"]["driver"]["rules"][0]["properties"]["safety"],
            "per-finding"
        );
    }

    fn assert_json_eq(value: &serde_json::Value, path: &[&str], expected: &str) {
        let mut current = value;
        for segment in path {
            current = match segment.parse::<usize>() {
                Ok(index) => &current[index],
                Err(_) => &current[*segment],
            };
        }
        assert_eq!(current, expected);
    }

    fn finding(rule: &str, severity: Severity, line: usize) -> Finding {
        Finding {
            path: PathBuf::from("src/sample.clj"),
            span: Span::new(line, line, 0, 10),
            rule: rule.to_string(),
            severity,
            safety: SafetyClass::LlmOnly,
            detected_by: DetectedBy::Text,
            message: format!("{rule} message"),
            suggestion: String::new(),
            precondition: None,
            edit: None,
            fingerprint: format!("{rule}-{line}"),
        }
    }
}
