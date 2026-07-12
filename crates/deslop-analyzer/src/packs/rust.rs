use deslop_core::{DetectedBy, Finding, Lang, SafetyClass, Severity, Span, fingerprint};
use deslop_external::{ClippyAnalyzer, ExternalAnalyzer as ExternalAnalyzerTrait};
use deslop_parse::{SourceFile, parse_source};
use regex::Regex;
use tree_sitter::Node;

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
    }
    out.extend(redundant_closures(source));
    out.extend(needless_clones(source));
    out.extend(let_and_return(source));
    out
}

fn redundant_closures(source: &SourceFile) -> Vec<Finding> {
    node_rule_findings(
        source,
        &RustNodeRule {
            rule: "redundant-closure",
            safety: SafetyClass::RiskySuggest,
            message: "closure forwards its argument directly to a function",
            suggestion: "replace with function item only after inference remains valid",
            matches: is_redundant_closure_node,
        },
    )
}

fn needless_clones(source: &SourceFile) -> Vec<Finding> {
    node_rule_findings(
        source,
        &RustNodeRule {
            rule: "needless-clone",
            safety: SafetyClass::LlmOnly,
            message: "clone may be unnecessary if a borrow suffices",
            suggestion: "remove clone only with ownership/typecheck confirmation",
            matches: is_needless_clone_node,
        },
    )
}

struct RustNodeRule {
    rule: &'static str,
    safety: SafetyClass,
    message: &'static str,
    suggestion: &'static str,
    matches: for<'tree> fn(&SourceFile, Node<'tree>) -> bool,
}

fn node_rule_findings(source: &SourceFile, rule: &RustNodeRule) -> Vec<Finding> {
    let Some(tree) = parse_source(source).ok().flatten() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_node_rule_findings(source, tree.root_node(), rule, &mut out);
    out
}

fn collect_node_rule_findings(
    source: &SourceFile,
    node: Node<'_>,
    rule: &RustNodeRule,
    out: &mut Vec<Finding>,
) {
    if (rule.matches)(source, node) {
        let start_line = node.start_position().row + 1;
        let end_line = node.end_position().row + 1;
        out.push(finding(
            source,
            start_line,
            end_line,
            rule.rule,
            Severity::Minor,
            rule.safety,
            DetectedBy::Idiom,
            rule.message,
            rule.suggestion,
            None,
        ));
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_node_rule_findings(source, child, rule, out);
    }
}

fn is_redundant_closure_node(source: &SourceFile, node: Node<'_>) -> bool {
    node.kind() == "closure_expression" && closure_forwards_single_arg(source, node)
}

fn closure_forwards_single_arg(source: &SourceFile, closure: Node<'_>) -> bool {
    let Some(param) = closure_single_parameter(source, closure) else {
        return false;
    };
    let Some(body) = closure_body(closure) else {
        return false;
    };
    body.kind() == "call_expression"
        && call_has_identifier_function(body)
        && call_single_argument_text(source, body).is_some_and(|arg| arg == param)
}

fn closure_single_parameter(source: &SourceFile, closure: Node<'_>) -> Option<String> {
    let mut cursor = closure.walk();
    let params = closure
        .children(&mut cursor)
        .find(|child| child.kind() == "closure_parameters")?;
    let mut cursor = params.walk();
    let mut identifiers = params
        .children(&mut cursor)
        .filter(|child| child.kind() == "identifier");
    let param = identifiers.next()?;
    if identifiers.next().is_some() {
        return None;
    }
    param
        .utf8_text(source.text.as_bytes())
        .ok()
        .map(str::to_string)
}

fn closure_body(closure: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = closure.walk();
    let mut body = None;
    for child in closure.children(&mut cursor) {
        if child.is_named() && child.kind() != "closure_parameters" {
            body = Some(child);
        }
    }
    body
}

fn call_has_identifier_function(call: Node<'_>) -> bool {
    let mut cursor = call.walk();
    call.children(&mut cursor)
        .find(|child| child.is_named())
        .is_some_and(|child| child.kind() == "identifier")
}

fn call_single_argument_text(source: &SourceFile, call: Node<'_>) -> Option<String> {
    let mut cursor = call.walk();
    let arguments = call
        .children(&mut cursor)
        .find(|child| child.kind() == "arguments")?;
    let mut cursor = arguments.walk();
    let mut args = arguments
        .children(&mut cursor)
        .filter(|child| child.is_named());
    let arg = args.next()?;
    if args.next().is_some() {
        return None;
    }
    arg.utf8_text(source.text.as_bytes())
        .ok()
        .map(str::trim)
        .map(str::to_string)
}

fn is_needless_clone_node(source: &SourceFile, node: Node<'_>) -> bool {
    reference_borrows_clone(source, node) || call_iterates_clone(source, node)
}

fn reference_borrows_clone(source: &SourceFile, node: Node<'_>) -> bool {
    node.kind() == "reference_expression"
        && node
            .child_by_field_name("value")
            .is_some_and(|value| is_clone_method_call(source, value))
}

fn call_iterates_clone(source: &SourceFile, node: Node<'_>) -> bool {
    if node.kind() != "call_expression" {
        return false;
    }
    let Some(function) = node.child_by_field_name("function") else {
        return false;
    };
    matches!(
        method_name(source, function).as_deref(),
        Some("iter" | "iter_mut" | "into_iter")
    ) && function
        .child_by_field_name("value")
        .is_some_and(|value| is_clone_method_call(source, value))
}

fn is_clone_method_call(source: &SourceFile, node: Node<'_>) -> bool {
    if node.kind() != "call_expression" {
        return false;
    }
    node.child_by_field_name("function")
        .is_some_and(|function| method_name(source, function).as_deref() == Some("clone"))
}

fn method_name(source: &SourceFile, field_expression: Node<'_>) -> Option<String> {
    if field_expression.kind() != "field_expression" {
        return None;
    }
    let field = field_expression.child_by_field_name("field")?;
    (field.kind() == "field_identifier").then(|| {
        field
            .utf8_text(source.text.as_bytes())
            .ok()
            .map(str::to_string)
    })?
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
