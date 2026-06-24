use deslop_core::{DetectedBy, Finding, SafetyClass, Severity};
use deslop_parse::SourceFile;
use regex::Regex;

use crate::finding;

pub(crate) fn findings(source: &SourceFile) -> Vec<Finding> {
    let rules = julia_rules();
    let lines = source.lines();
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        for rule in &rules {
            if rule.regex.is_match(line) {
                out.push(rule.finding(source, line_no));
            }
        }
    }
    out.extend(eachindex_findings(source, &lines));
    out
}

struct JuliaRule {
    regex: Regex,
    rule: &'static str,
    severity: Severity,
    safety: SafetyClass,
    message: &'static str,
    suggestion: &'static str,
    precondition: Option<&'static str>,
}

impl JuliaRule {
    fn new(
        pattern: &'static str,
        rule: &'static str,
        severity: Severity,
        safety: SafetyClass,
        message: &'static str,
        suggestion: &'static str,
        precondition: Option<&'static str>,
    ) -> Self {
        Self {
            regex: Regex::new(pattern).expect("valid regex"),
            rule,
            severity,
            safety,
            message,
            suggestion,
            precondition,
        }
    }

    fn finding(&self, source: &SourceFile, line_no: usize) -> Finding {
        finding(
            source,
            line_no,
            line_no,
            self.rule,
            self.severity,
            self.safety,
            DetectedBy::Idiom,
            self.message,
            self.suggestion,
            self.precondition,
            None,
        )
    }
}

fn julia_rules() -> Vec<JuliaRule> {
    vec![
        JuliaRule::new(
            r"length\(([^()\n]+)\)\s*==\s*0",
            "reimpl-isempty",
            Severity::Minor,
            SafetyClass::SafeWithPrecondition,
            "length(x) == 0 reimplements isempty",
            "use isempty(x) when collection semantics are well behaved",
            Some("collection has standard length/isempty semantics"),
        ),
        JuliaRule::new(
            r"([^=!<>\n]+)\s*==\s*nothing",
            "reimpl-isnothing",
            Severity::Info,
            SafetyClass::RiskySuggest,
            "x == nothing is usually isnothing(x), but == can be overloaded",
            "consider isnothing(x)",
            None,
        ),
    ]
}

fn eachindex_findings(source: &SourceFile, lines: &[&str]) -> Vec<Finding> {
    let header = Regex::new(r"^\s*for\s+([A-Za-z_]\w*)\s+in\s+1:length\(([^()\n]+)\)\s*(?:#.*)?$")
        .expect("valid regex");
    let mut findings = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let Some(captures) = header.captures(line) else {
            continue;
        };
        let index = captures.get(1).expect("index capture").as_str();
        let collection = captures.get(2).expect("collection capture").as_str().trim();
        let Some(body) = loop_body_range(lines, idx) else {
            continue;
        };
        if !body_uses_index_only_for_collection(lines, body, collection, index) {
            continue;
        }
        let line_no = idx + 1;
        let message = format!(
            "1:length({collection}) is used to index {collection}; eachindex preserves custom indices"
        );
        let suggestion = format!(
            "replace the iterator with eachindex({collection}); keep 1:length({collection}) when {index} is an ordinal counter"
        );
        findings.push(finding(
            source,
            line_no,
            line_no,
            "reimpl-eachindex",
            Severity::Minor,
            SafetyClass::SafeWithPrecondition,
            DetectedBy::Idiom,
            &message,
            &suggestion,
            Some("loop variable is used only to index the same collection"),
            None,
        ));
    }
    findings
}

fn loop_body_range(lines: &[&str], header_idx: usize) -> Option<std::ops::Range<usize>> {
    let mut depth = 1usize;
    for (idx, line) in lines.iter().enumerate().skip(header_idx + 1) {
        let trimmed = code_before_comment(line).trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_julia_end(trimmed) {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(header_idx + 1..idx);
            }
            continue;
        }
        if starts_julia_block(trimmed) {
            depth += 1;
        }
    }
    None
}

fn body_uses_index_only_for_collection(
    lines: &[&str],
    body: std::ops::Range<usize>,
    collection: &str,
    index: &str,
) -> bool {
    let collection_index = Regex::new(&format!(
        r"(^|[^A-Za-z0-9_!]){}\s*\[\s*{}\s*\]",
        regex::escape(collection),
        regex::escape(index)
    ))
    .expect("valid collection index regex");
    let index_word =
        Regex::new(&format!(r"\b{}\b", regex::escape(index))).expect("valid index regex");
    let mut saw_collection_index = false;
    for line in &lines[body] {
        let code = code_before_comment(line);
        if collection_index.is_match(code) {
            saw_collection_index = true;
        }
        let remaining = collection_index.replace_all(code, "");
        if index_word.is_match(&remaining) {
            return false;
        }
    }
    saw_collection_index
}

fn code_before_comment(line: &str) -> &str {
    line.split('#').next().unwrap_or("")
}

fn is_julia_end(trimmed: &str) -> bool {
    trimmed == "end" || trimmed.starts_with("end ")
}

fn starts_julia_block(trimmed: &str) -> bool {
    trimmed.starts_with("for ")
        || trimmed.starts_with("while ")
        || trimmed.starts_with("if ")
        || trimmed.starts_with("function ")
        || trimmed == "let"
        || trimmed.starts_with("let ")
        || trimmed == "begin"
        || trimmed == "try"
        || trimmed.starts_with("module ")
        || trimmed.starts_with("baremodule ")
        || trimmed.starts_with("struct ")
        || trimmed.starts_with("mutable struct ")
        || trimmed.starts_with("macro ")
}
