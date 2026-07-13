use deslop_core::{DetectedBy, Finding, SafetyClass, Severity, Span, baseline_fingerprint};
use deslop_external::{ClippyAnalyzer, ExternalAnalyzer as ExternalAnalyzerTrait};
use deslop_parse::{NodeId, SourceFile};
use regex::Regex;

use crate::{AnalyzerConfig, AnalyzerFile, TextSource};

pub(crate) fn external_analyzer(
    config: &AnalyzerConfig,
) -> Option<Box<dyn ExternalAnalyzerTrait<SourceFile, Finding>>> {
    config.rust_external.then(|| {
        Box::new(ClippyAnalyzer::default()) as Box<dyn ExternalAnalyzerTrait<SourceFile, Finding>>
    })
}

pub(crate) fn rust_findings_analysis(file: &AnalyzerFile<'_>) -> Vec<Finding> {
    let source = file.source();
    let useless_format =
        Regex::new(r#"format!\s*\(\s*"\{\}"\s*,\s*([^)]+)\)"#).expect("valid regex");
    let mut out = Vec::new();
    for (idx, line) in source.lines().iter().enumerate() {
        if useless_format.is_match(line) {
            out.push(finding(
                source,
                idx + 1,
                idx + 1,
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
    out.extend(node_rule_findings_analysis(
        file,
        &RustAnalysisNodeRule {
            rule: "redundant-closure",
            safety: SafetyClass::RiskySuggest,
            message: "closure forwards its argument directly to a function",
            suggestion: "replace with function item only after inference remains valid",
            matches: is_redundant_closure_node_analysis,
        },
    ));
    out.extend(node_rule_findings_analysis(
        file,
        &RustAnalysisNodeRule {
            rule: "needless-clone",
            safety: SafetyClass::LlmOnly,
            message: "clone may be unnecessary if a borrow suffices",
            suggestion: "remove clone only with ownership/typecheck confirmation",
            matches: is_needless_clone_node_analysis,
        },
    ));
    out.extend(let_and_return(source));
    out
}

struct RustAnalysisNodeRule {
    rule: &'static str,
    safety: SafetyClass,
    message: &'static str,
    suggestion: &'static str,
    matches: fn(&AnalyzerFile<'_>, NodeId) -> bool,
}

fn node_rule_findings_analysis(
    file: &AnalyzerFile<'_>,
    rule: &RustAnalysisNodeRule,
) -> Vec<Finding> {
    file.node_ids()
        .filter(|node| (rule.matches)(file, *node))
        .map(|node| {
            let view = file
                .analysis
                .node(node)
                .expect("AnalyzerFile NodeId belongs to its analysis");
            let span = view.span();
            finding(
                file.source(),
                span.start_point().row() + 1,
                span.end_point().row() + 1,
                rule.rule,
                Severity::Minor,
                rule.safety,
                DetectedBy::Idiom,
                rule.message,
                rule.suggestion,
                None,
            )
        })
        .collect()
}

fn is_redundant_closure_node_analysis(file: &AnalyzerFile<'_>, node: NodeId) -> bool {
    file.analysis
        .node(node)
        .is_ok_and(|view| view.raw_kind() == "closure_expression")
        && closure_forwards_single_arg_analysis(file, node)
}

fn closure_forwards_single_arg_analysis(file: &AnalyzerFile<'_>, closure: NodeId) -> bool {
    let Some(parameter) = closure_single_parameter_analysis(file, closure) else {
        return false;
    };
    let Some(body) = closure_body_analysis(file, closure) else {
        return false;
    };
    file.analysis
        .node(body)
        .is_ok_and(|view| view.raw_kind() == "call_expression")
        && call_has_identifier_function_analysis(file, body)
        && call_single_argument_text_analysis(file, body)
            .is_some_and(|argument| argument == parameter)
}

fn closure_single_parameter_analysis(file: &AnalyzerFile<'_>, closure: NodeId) -> Option<String> {
    let closure = file.analysis.node(closure).ok()?;
    let parameters = closure.children().into_iter().find(|child| {
        file.analysis
            .node(*child)
            .is_ok_and(|view| view.raw_kind() == "closure_parameters")
    })?;
    let parameters = file.analysis.node(parameters).ok()?;
    let identifiers = parameters
        .children()
        .into_iter()
        .filter(|child| {
            file.analysis
                .node(*child)
                .is_ok_and(|view| view.raw_kind() == "identifier")
        })
        .collect::<Vec<_>>();
    (identifiers.len() == 1).then(|| {
        file.analysis
            .node(identifiers[0])
            .expect("filtered owned identifier")
            .text()
            .to_string()
    })
}

fn closure_body_analysis(file: &AnalyzerFile<'_>, closure: NodeId) -> Option<NodeId> {
    file.analysis
        .node(closure)
        .ok()?
        .children()
        .into_iter()
        .rfind(|child| {
            file.analysis
                .node(*child)
                .is_ok_and(|view| view.is_named() && view.raw_kind() != "closure_parameters")
        })
}

fn call_has_identifier_function_analysis(file: &AnalyzerFile<'_>, call: NodeId) -> bool {
    file.analysis
        .node(call)
        .ok()
        .and_then(|view| {
            view.children()
                .into_iter()
                .find(|child| file.analysis.node(*child).is_ok_and(|view| view.is_named()))
        })
        .is_some_and(|child| {
            file.analysis
                .node(child)
                .is_ok_and(|view| view.raw_kind() == "identifier")
        })
}

fn call_single_argument_text_analysis(file: &AnalyzerFile<'_>, call: NodeId) -> Option<String> {
    let call = file.analysis.node(call).ok()?;
    let arguments = call.children().into_iter().find(|child| {
        file.analysis
            .node(*child)
            .is_ok_and(|view| view.raw_kind() == "arguments")
    })?;
    let arguments = file.analysis.node(arguments).ok()?;
    let named = arguments
        .children()
        .into_iter()
        .filter(|child| file.analysis.node(*child).is_ok_and(|view| view.is_named()))
        .collect::<Vec<_>>();
    (named.len() == 1).then(|| {
        file.analysis
            .node(named[0])
            .expect("filtered owned argument")
            .text()
            .trim()
            .to_string()
    })
}

fn is_needless_clone_node_analysis(file: &AnalyzerFile<'_>, node: NodeId) -> bool {
    reference_borrows_clone_analysis(file, node) || call_iterates_clone_analysis(file, node)
}

fn reference_borrows_clone_analysis(file: &AnalyzerFile<'_>, node: NodeId) -> bool {
    file.analysis
        .node(node)
        .is_ok_and(|view| view.raw_kind() == "reference_expression")
        && file
            .child_by_field(node, "value")
            .is_some_and(|value| is_clone_method_call_analysis(file, value))
}

fn call_iterates_clone_analysis(file: &AnalyzerFile<'_>, node: NodeId) -> bool {
    if !file
        .analysis
        .node(node)
        .is_ok_and(|view| view.raw_kind() == "call_expression")
    {
        return false;
    }
    let Some(function) = file.child_by_field(node, "function") else {
        return false;
    };
    matches!(
        method_name_analysis(file, function).as_deref(),
        Some("iter" | "iter_mut" | "into_iter")
    ) && file
        .child_by_field(function, "value")
        .is_some_and(|value| is_clone_method_call_analysis(file, value))
}

fn is_clone_method_call_analysis(file: &AnalyzerFile<'_>, node: NodeId) -> bool {
    file.analysis
        .node(node)
        .is_ok_and(|view| view.raw_kind() == "call_expression")
        && file
            .child_by_field(node, "function")
            .is_some_and(|function| {
                method_name_analysis(file, function).as_deref() == Some("clone")
            })
}

fn method_name_analysis(file: &AnalyzerFile<'_>, field_expression: NodeId) -> Option<String> {
    if file.analysis.node(field_expression).ok()?.raw_kind() != "field_expression" {
        return None;
    }
    let field = file.child_by_field(field_expression, "field")?;
    let field = file.analysis.node(field).ok()?;
    (field.raw_kind() == "field_identifier").then(|| field.text().to_string())
}

fn let_and_return(source: &impl TextSource) -> Vec<Finding> {
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
    source: &impl TextSource,
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
        path: source.path().to_path_buf(),
        span,
        rule: rule.to_string(),
        severity,
        safety,
        detected_by,
        message: message.to_string(),
        suggestion: suggestion.to_string(),
        precondition: precondition.map(str::to_string),
        edit: None,
        fingerprint: baseline_fingerprint(source.path(), rule, span, &text),
    }
}
