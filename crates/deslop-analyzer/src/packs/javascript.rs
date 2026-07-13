use deslop_core::{DetectedBy, Finding, Lang, SafetyClass, Severity};
use deslop_lang::Rule;
use deslop_parse::SourceFile;
use regex::Regex;

use crate::{AnalysisPack, AnalyzerConfig, finding};

pub static JAVASCRIPT_PACK: JavaScriptPack = JavaScriptPack;
pub static TYPESCRIPT_PACK: TypeScriptPack = TypeScriptPack;

static JAVASCRIPT_RULE: JavaScriptRule = JavaScriptRule;
static JAVASCRIPT_RULES: [&'static dyn Rule<SourceFile, AnalyzerConfig, Finding>; 1] =
    [&JAVASCRIPT_RULE];

pub struct JavaScriptPack;
pub struct TypeScriptPack;

struct JavaScriptRule;

impl AnalysisPack for JavaScriptPack {
    fn name(&self) -> &'static str {
        "javascript"
    }

    fn lang(&self) -> Lang {
        Lang::JavaScript
    }

    fn rules(&self) -> &'static [&'static dyn Rule<SourceFile, AnalyzerConfig, Finding>] {
        &JAVASCRIPT_RULES
    }

    fn external_analyzer(
        &self,
        _config: &AnalyzerConfig,
    ) -> Option<Box<dyn deslop_external::ExternalAnalyzer<SourceFile, Finding>>> {
        None
    }
}

impl AnalysisPack for TypeScriptPack {
    fn name(&self) -> &'static str {
        "typescript"
    }

    fn lang(&self) -> Lang {
        Lang::TypeScript
    }

    fn rules(&self) -> &'static [&'static dyn Rule<SourceFile, AnalyzerConfig, Finding>] {
        &JAVASCRIPT_RULES
    }

    fn external_analyzer(
        &self,
        _config: &AnalyzerConfig,
    ) -> Option<Box<dyn deslop_external::ExternalAnalyzer<SourceFile, Finding>>> {
        None
    }
}

impl Rule<SourceFile, AnalyzerConfig, Finding> for JavaScriptRule {
    fn name(&self) -> &'static str {
        "javascript"
    }

    fn check(&self, source: &SourceFile, _config: &AnalyzerConfig) -> Vec<Finding> {
        javascript_findings(source)
    }
}

pub(crate) fn javascript_findings(source: &SourceFile) -> Vec<Finding> {
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
    source: &SourceFile,
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
