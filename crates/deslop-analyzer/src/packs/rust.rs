use deslop_core::{DetectedBy, Finding, Lang, SafetyClass, Severity, Span, fingerprint};
use deslop_external::{ClippyAnalyzer, ExternalAnalyzer as ExternalAnalyzerTrait};
use deslop_parse::SourceFile;
use regex::Regex;

use crate::{AnalysisPack, AnalyzerConfig};
use deslop_lang::Rule;

pub static RUST_PACK: RustPack = RustPack;

static RUST_RULE: RustRule = RustRule;
static RUST_RULES: [&'static dyn Rule<SourceFile, AnalyzerConfig, Finding>; 1] = [&RUST_RULE];

pub struct RustPack;

struct RustRule;

impl AnalysisPack for RustPack {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn lang(&self) -> Lang {
        Lang::Rust
    }

    fn rules(&self) -> &'static [&'static dyn Rule<SourceFile, AnalyzerConfig, Finding>] {
        &RUST_RULES
    }

    fn external_analyzer(
        &self,
        config: &AnalyzerConfig,
    ) -> Option<Box<dyn ExternalAnalyzerTrait<SourceFile, Finding>>> {
        config.rust_external.then(|| {
            Box::new(ClippyAnalyzer::default())
                as Box<dyn ExternalAnalyzerTrait<SourceFile, Finding>>
        })
    }
}

impl Rule<SourceFile, AnalyzerConfig, Finding> for RustRule {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn check(&self, source: &SourceFile, _config: &AnalyzerConfig) -> Vec<Finding> {
        rust_findings(source)
    }
}

fn rust_findings(source: &SourceFile) -> Vec<Finding> {
    let mut out = Vec::new();
    let useless_format =
        Regex::new(r#"format!\s*\(\s*"\{\}"\s*,\s*([^)]+)\)"#).expect("valid regex");
    let redundant_closure =
        Regex::new(r"\|\s*([A-Za-z_][A-Za-z0-9_]*)\s*\|\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*\)")
            .expect("valid regex");
    let clone = Regex::new(r"\.clone\s*\(\s*\)").expect("valid regex");
    let lines = source.lines();
    for (idx, line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        if useless_format.is_match(line) {
            out.push(finding(
                source,
                line_no,
                line_no,
                "useless-format",
                Severity::Minor,
                SafetyClass::SafeWithPrecondition,
                DetectedBy::Idiom,
                "format!(\"{}\", x) can often be x.to_string()",
                "use to_string only when formatting semantics remain equivalent",
                Some("Display formatting is equivalent to ToString for this value"),
            ));
        }
        if redundant_closure
            .captures(line)
            .is_some_and(|caps| caps.get(1).map(|m| m.as_str()) == caps.get(3).map(|m| m.as_str()))
        {
            out.push(finding(
                source,
                line_no,
                line_no,
                "redundant-closure",
                Severity::Minor,
                SafetyClass::RiskySuggest,
                DetectedBy::Idiom,
                "closure forwards its argument directly to a function",
                "replace with function item only after inference remains valid",
                None,
            ));
        }
        if clone.is_match(line) {
            out.push(finding(
                source,
                line_no,
                line_no,
                "needless-clone",
                Severity::Minor,
                SafetyClass::LlmOnly,
                DetectedBy::Idiom,
                "clone may be unnecessary if a borrow suffices",
                "remove clone only with ownership/typecheck confirmation",
                None,
            ));
        }
    }
    out.extend(let_and_return(source));
    out
}

fn let_and_return(source: &SourceFile) -> Vec<Finding> {
    let lines = source.lines();
    let let_re = Regex::new(r"^\s*let\s+([A-Za-z_][A-Za-z0-9_]*)\s*=").expect("valid regex");
    let mut out = Vec::new();
    for idx in 0..lines.len().saturating_sub(1) {
        let Some(caps) = let_re.captures(lines[idx]) else {
            continue;
        };
        let name = &caps[1];
        let next = lines[idx + 1].trim().trim_end_matches(';');
        if next == name {
            out.push(finding(
                source,
                idx + 1,
                idx + 2,
                "let-and-return",
                Severity::Minor,
                SafetyClass::RiskySuggest,
                DetectedBy::Idiom,
                "binding is immediately returned",
                "return the expression directly only after typecheck confirms behavior",
                None,
            ));
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn finding(
    source: &SourceFile,
    start_line: usize,
    end_line: usize,
    rule: &str,
    severity: Severity,
    safety: SafetyClass,
    detected_by: DetectedBy,
    message: &str,
    suggestion: &str,
    precondition: Option<&str>,
) -> Finding {
    let start_byte = source.line_start_byte(start_line);
    let end_byte = source.line_end_byte(end_line);
    let span = Span::new(start_line, end_line, start_byte, end_byte);
    let text = source.region_text(start_line, end_line);
    Finding {
        path: source.path.clone(),
        span,
        rule: rule.to_string(),
        severity,
        safety,
        detected_by,
        message: message.to_string(),
        suggestion: suggestion.to_string(),
        precondition: precondition.map(str::to_string),
        edit: None,
        fingerprint: fingerprint(&source.path, rule, span, &text),
    }
}
