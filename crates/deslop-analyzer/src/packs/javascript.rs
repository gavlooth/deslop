use deslop_core::{DetectedBy, Finding, SafetyClass, Severity};
use regex::Regex;

use crate::{AnalyzerText, finding};

pub(crate) fn javascript_findings(source: &AnalyzerText) -> Vec<Finding> {
    let loose_eq = Regex::new(r"(^|[^=!])(?:==|!=)([^=]|$)").expect("regex");
    let var_decl = Regex::new(r"^\s*var\s+").expect("regex");
    let unnecessary_await = Regex::new(r"\breturn\s+await\b").expect("regex");
    let mut out = Vec::new();
    for (idx, line) in source.lines().iter().enumerate() {
        let line_no = idx + 1;
        if loose_eq.is_match(line) {
            out.push(js_finding(
                source,
                line_no,
                "js-loose-equality",
                SafetyClass::SafeWithPrecondition,
                "loose equality coerces operands",
                "use === or !== when coercion is not intentional",
            ));
        }
        if var_decl.is_match(line) {
            out.push(js_finding(
                source,
                line_no,
                "js-var-declaration",
                SafetyClass::SafeWithPrecondition,
                "var has function scope and can be replaced in most modern code",
                "use let or const when hoisting semantics are not required",
            ));
        }
        if unnecessary_await.is_match(line) {
            out.push(js_finding(
                source,
                line_no,
                "js-unnecessary-await",
                SafetyClass::RiskySuggest,
                "return await is often redundant outside try/catch",
                "return the promise directly when stack-trace or error timing is not needed",
            ));
        }
    }
    out
}

fn js_finding(
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
