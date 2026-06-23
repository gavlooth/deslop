use deslop_core::{DetectedBy, Finding, Lang, SafetyClass, Severity};
use deslop_external::ExternalAnalyzer as ExternalAnalyzerTrait;
use deslop_lang::{RegionSpan, Rule};
use deslop_parse::SourceFile;

use crate::{AnalysisPack, AnalyzerConfig, finding};

pub static TEST_LANG_PACK: TestLangPack = TestLangPack;
pub static TEST_ANALYSIS_PACK: TestAnalysisPack = TestAnalysisPack;

pub struct TestLangPack;
pub struct TestAnalysisPack;

struct TestRule;

static TEST_RULE: TestRule = TestRule;
static TEST_RULES: [&'static dyn Rule<SourceFile, AnalyzerConfig, Finding>; 1] = [&TEST_RULE];

impl deslop_lang::LangPack for TestLangPack {
    fn name(&self) -> &'static str {
        "test"
    }

    fn lang(&self) -> Lang {
        Lang::Generic
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["testpack"]
    }

    fn grammar(&self) -> Option<tree_sitter::Language> {
        None
    }

    fn line_comments(&self) -> &'static [&'static str] {
        &["#"]
    }

    fn metrics_regions(&self) -> &'static [&'static str] {
        &[]
    }

    fn metrics_branches(&self) -> &'static [&'static str] {
        &[]
    }

    fn metrics_nesting(&self) -> &'static [&'static str] {
        &[]
    }

    fn metrics_flow_breaks(&self) -> &'static [&'static str] {
        &[]
    }

    fn halstead_operator_tokens(&self) -> &'static [&'static str] {
        &[]
    }

    fn enclosing_region(&self, _node: tree_sitter::Node<'_>, _text: &str) -> Option<RegionSpan> {
        None
    }
}

impl AnalysisPack for TestAnalysisPack {
    fn name(&self) -> &'static str {
        "test"
    }

    fn lang(&self) -> Lang {
        Lang::Generic
    }

    fn rules(&self) -> &'static [&'static dyn Rule<SourceFile, AnalyzerConfig, Finding>] {
        &TEST_RULES
    }

    fn external_analyzer(
        &self,
        _config: &AnalyzerConfig,
    ) -> Option<Box<dyn ExternalAnalyzerTrait<SourceFile, Finding>>> {
        None
    }
}

impl Rule<SourceFile, AnalyzerConfig, Finding> for TestRule {
    fn name(&self) -> &'static str {
        "test-rule"
    }

    fn check(&self, source: &SourceFile, _config: &AnalyzerConfig) -> Vec<Finding> {
        vec![finding(
            source,
            1,
            1,
            "test-pack-rule",
            Severity::Info,
            SafetyClass::NeverAuto,
            DetectedBy::Text,
            "test pack was dispatched",
            "none",
            None,
            None,
        )]
    }
}
