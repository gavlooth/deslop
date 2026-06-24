use anyhow::Result;
use std::collections::BTreeMap;

use deslop_core::{FileReport, Finding, SafetyClass, Severity};
use deslop_parse::SourceFile;
use deslop_protocol::work_orders_for_source;
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
        out.push_str("No findings.\n");
    }
    out
}

pub fn render_json(reports: &[FileReport]) -> Result<String> {
    Ok(serde_json::to_string_pretty(&ReportEnvelope {
        schema: "deslop.findings/1",
        reports,
    })?)
}

pub fn render_sarif(reports: &[FileReport]) -> Result<String> {
    let mut rules: BTreeMap<String, SafetyClass> = BTreeMap::new();
    let mut results = Vec::new();
    for report in reports {
        for finding in &report.findings {
            rules
                .entry(finding.rule.to_owned())
                .or_insert(finding.safety);
            results.push(sarif_result(finding));
        }
    }
    let rules = rules
        .into_iter()
        .map(|(id, safety)| {
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

pub fn render_agent(reports: &[FileReport]) -> Result<String> {
    let mut out = String::new();
    for report in reports {
        let source = SourceFile::read(&report.path)?;
        for work_order in work_orders_for_source(&source, &report.findings) {
            out.push_str(&serde_json::to_string(&work_order)?);
            out.push('\n');
        }
    }
    Ok(out)
}

fn sarif_result(finding: &Finding) -> serde_json::Value {
    json!({
        "ruleId": finding.rule,
        "level": sarif_level(finding.severity),
        "message": { "text": finding.message },
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
