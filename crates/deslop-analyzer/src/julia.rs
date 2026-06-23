use deslop_core::{DetectedBy, Finding, SafetyClass, Severity};
use deslop_parse::SourceFile;
use regex::Regex;

use crate::finding;

pub(crate) fn findings(source: &SourceFile) -> Vec<Finding> {
    let mut out = Vec::new();
    let isempty = Regex::new(r"length\(([^()\n]+)\)\s*==\s*0").expect("valid regex");
    let eachindex = Regex::new(r"for\s+\w+\s+in\s+1:length\(([^()\n]+)\)").expect("valid regex");
    let isnothing = Regex::new(r"([^=!<>\n]+)\s*==\s*nothing").expect("valid regex");
    for (idx, line) in source.lines().iter().enumerate() {
        let line_no = idx + 1;
        if isempty.is_match(line) {
            out.push(finding(
                source,
                line_no,
                line_no,
                "reimpl-isempty",
                Severity::Minor,
                SafetyClass::SafeWithPrecondition,
                DetectedBy::Idiom,
                "length(x) == 0 reimplements isempty",
                "use isempty(x) when collection semantics are well behaved",
                Some("collection has standard length/isempty semantics"),
                None,
            ));
        }
        if eachindex.is_match(line) {
            out.push(finding(
                source,
                line_no,
                line_no,
                "reimpl-eachindex",
                Severity::Minor,
                SafetyClass::SafeWithPrecondition,
                DetectedBy::Idiom,
                "1:length(x) may reimplement eachindex(x)",
                "use eachindex(x) only when ordinal indexing is not required",
                Some("indices are 1-based and the loop variable is positional"),
                None,
            ));
        }
        if isnothing.is_match(line) {
            out.push(finding(
                source,
                line_no,
                line_no,
                "reimpl-isnothing",
                Severity::Info,
                SafetyClass::RiskySuggest,
                DetectedBy::Idiom,
                "x == nothing is usually isnothing(x), but == can be overloaded",
                "consider isnothing(x)",
                None,
                None,
            ));
        }
    }
    out
}
