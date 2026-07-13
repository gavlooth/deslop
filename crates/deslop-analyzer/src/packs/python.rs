use deslop_core::{DetectedBy, Finding, SafetyClass, Severity};
use regex::Regex;

use crate::{AnalyzerText, finding};

pub(crate) fn python_findings(source: &AnalyzerText) -> Vec<Finding> {
    let none_cmp = Regex::new(r"(?:==|!=)\s*None\b|\bNone\s*(?:==|!=)").expect("regex");
    let range_len = Regex::new(r"\brange\s*\(\s*len\s*\(").expect("regex");
    let dict_keys = Regex::new(r"\bin\s+[A-Za-z_][A-Za-z0-9_]*\.keys\s*\(\s*\)").expect("regex");
    let list_comp = Regex::new(r"\blist\s*\(\s*\[").expect("regex");
    let mut out = Vec::new();
    for (idx, line) in source.lines().iter().enumerate() {
        let line_no = idx + 1;
        if none_cmp.is_match(line) {
            out.push(py_finding(
                source,
                line_no,
                "py-none-comparison",
                SafetyClass::SafeWithPrecondition,
                "comparison to None should usually use identity",
                "use `is None` or `is not None` when custom equality is not intended",
            ));
        }
        if range_len.is_match(line) {
            out.push(py_finding(
                source,
                line_no,
                "py-range-len",
                SafetyClass::RiskySuggest,
                "range(len(x)) often hides direct iteration",
                "use enumerate or direct iteration when the index is not required",
            ));
        }
        if dict_keys.is_match(line) {
            out.push(py_finding(
                source,
                line_no,
                "py-dict-keys-membership",
                SafetyClass::SafeWithPrecondition,
                "membership in dict.keys() can usually test the dict directly",
                "use `key in mapping` when a normal mapping lookup is intended",
            ));
        }
        if list_comp.is_match(line) {
            out.push(py_finding(
                source,
                line_no,
                "py-list-comprehension-wrapper",
                SafetyClass::RiskySuggest,
                "list([...]) wraps an already materialized list",
                "remove the redundant list() wrapper when a list comprehension is intended",
            ));
        }
    }
    out
}

fn py_finding(
    source: &AnalyzerText,
    line: usize,
    rule: &str,
    safety: SafetyClass,
    message: &str,
    suggestion: &str,
) -> Finding {
    finding(
        source,
        line,
        line,
        rule,
        Severity::Minor,
        safety,
        DetectedBy::Idiom,
        message,
        suggestion,
        None,
        None,
    )
}
