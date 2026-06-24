use deslop_core::{DetectedBy, Finding, SafetyClass, Severity};
use deslop_parse::SourceFile;
use regex::Regex;

use crate::finding;

pub(crate) fn findings(source: &SourceFile) -> Vec<Finding> {
    let rules = julia_rules();
    let mut out = Vec::new();
    for (idx, line) in source.lines().iter().enumerate() {
        let line_no = idx + 1;
        for rule in &rules {
            if rule.regex.is_match(line) {
                out.push(rule.finding(source, line_no));
            }
        }
    }
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
            r"for\s+\w+\s+in\s+1:length\(([^()\n]+)\)",
            "reimpl-eachindex",
            Severity::Minor,
            SafetyClass::SafeWithPrecondition,
            "1:length(x) may reimplement eachindex(x)",
            "use eachindex(x) only when ordinal indexing is not required",
            Some("indices are 1-based and the loop variable is positional"),
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
