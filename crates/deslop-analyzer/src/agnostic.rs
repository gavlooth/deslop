use deslop_core::{DetectedBy, Edit, EditKind, Finding, SafetyClass, Severity, Splice};
use deslop_lang::TailPositionClass;
use regex::Regex;

use crate::{AnalyzerConfig, AnalyzerFile, AnalyzerText, finding, tokens};

pub(crate) fn findings_analysis(file: &AnalyzerFile<'_>, config: &AnalyzerConfig) -> Vec<Finding> {
    let source = file.source();
    let comments = file.adapter().line_comments();
    let mut out = Vec::new();
    out.extend(blank_runs(source));
    out.extend(incompleteness_analysis(file));
    out.extend(magic_numbers_analysis(file));
    out.extend(long_methods_analysis(
        file,
        config.long_method_nloc_for(source.lang),
    ));
    out.extend(narrating_comments(source, comments));
    out.extend(comment_blocks(source, comments));
    out.extend(needless_tail_returns_analysis(file));
    out.extend(tokens::duplicate_token_sequences_analysis(file, config));
    out
}

fn incompleteness_analysis(file: &AnalyzerFile<'_>) -> Vec<Finding> {
    incompleteness_with_ranges(file.source(), string_comment_ranges_analysis(file))
}

fn incompleteness_with_ranges(source: &AnalyzerText, masked: Vec<(usize, usize)>) -> Vec<Finding> {
    let mut out = Vec::new();
    let stub = Regex::new(
        r#"(?i)(todo!\s*\(|unimplemented!\s*\(|TODO\s*:?\s*implement|throw\b.*TODO|error\s*\(\s*["']TODO|@assert\s+false|not\s+implemented|\bplaceholder\b)"#,
    )
    .expect("valid regex");
    for (idx, line) in source.lines().iter().enumerate() {
        let line_start = source.line_start_byte(idx + 1);
        if stub
            .find_iter(line)
            .any(|matched| !byte_in_ranges(line_start + matched.start(), &masked))
        {
            out.push(finding(
                source,
                idx + 1,
                idx + 1,
                "incompleteness",
                Severity::Major,
                SafetyClass::LlmOnly,
                DetectedBy::Text,
                "placeholder or unimplemented code remains",
                "replace the stub with real behavior or remove the unreachable path",
                None,
                None,
            ));
        }
    }
    out
}

fn string_comment_ranges_analysis(file: &AnalyzerFile<'_>) -> Vec<(usize, usize)> {
    file.node_ids()
        .filter_map(|node| {
            let view = file
                .analysis
                .node(node)
                .expect("AnalyzerFile NodeId belongs to its analysis");
            let kind = view.raw_kind();
            (kind.contains("string") || kind.contains("str_lit") || kind.contains("comment")).then(
                || {
                    let span = view.span();
                    (span.start_byte(), span.end_byte())
                },
            )
        })
        .collect()
}

fn byte_in_ranges(byte: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| byte >= start && byte < end)
}

fn magic_numbers_analysis(file: &AnalyzerFile<'_>) -> Vec<Finding> {
    let source = file.source();
    let mut masked = string_comment_ranges_analysis(file);
    masked.extend(file.node_ids().filter_map(|node| {
        if !file.fact(node).is_constant_definition_region() {
            return None;
        }
        let span = file
            .analysis
            .node(node)
            .expect("AnalyzerFile NodeId belongs to its analysis")
            .span();
        Some((span.start_byte(), span.end_byte()))
    }));
    let mut out = Vec::new();
    for (idx, line) in source.lines().iter().enumerate() {
        let code = code_before_comment_with_tokens(line, file.adapter().line_comments());
        if should_skip_magic_number_line(code) {
            continue;
        }
        if let Some(offset) = first_magic_number(code) {
            let byte = source.line_start_byte(idx + 1) + offset;
            if byte_in_ranges(byte, &masked) {
                continue;
            }
            out.push(finding(
                source,
                idx + 1,
                idx + 1,
                "magic-number",
                Severity::Minor,
                SafetyClass::RiskySuggest,
                DetectedBy::Text,
                "inline numeric literal should probably be a named constant",
                "introduce a named constant if the number encodes domain policy",
                None,
                None,
            ));
            break;
        }
    }
    out
}

fn should_skip_magic_number_line(code: &str) -> bool {
    let trimmed = code.trim();
    trimmed.is_empty()
        || trimmed.starts_with("const ")
        || trimmed.starts_with("static ")
        || trimmed.starts_with("(def ")
        || trimmed.starts_with(':')
        || trimmed.starts_with("const ")
        || trimmed.starts_with("struct ")
        || trimmed.starts_with("enum ")
        || trimmed.contains("#[")
        || trimmed.contains("::")
}

/// Byte offset (within `code`) of the first inline magic number, if any.
fn first_magic_number(code: &str) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        let start = idx;
        let negative = bytes[idx] == b'-';
        if negative {
            idx += 1;
        }
        if idx >= bytes.len() || !bytes[idx].is_ascii_digit() {
            idx = start + 1;
            continue;
        }
        let digit_start = idx;
        while idx < bytes.len() && (bytes[idx].is_ascii_digit() || bytes[idx] == b'.') {
            idx += 1;
        }
        if is_identifier_byte_before(bytes, start) || is_identifier_byte_after(bytes, idx) {
            continue;
        }
        let literal = &code[start..idx];
        if !is_allowed_small_number(literal) {
            return Some(digit_start);
        }
    }
    None
}

fn is_identifier_byte_before(bytes: &[u8], idx: usize) -> bool {
    idx > 0 && (bytes[idx - 1].is_ascii_alphanumeric() || bytes[idx - 1] == b'_')
}

fn is_identifier_byte_after(bytes: &[u8], idx: usize) -> bool {
    idx < bytes.len() && (bytes[idx].is_ascii_alphabetic() || bytes[idx] == b'_')
}

fn is_allowed_small_number(value: &str) -> bool {
    matches!(value, "-1" | "0" | "1" | "2" | "0.0" | "1.0" | "2.0")
}

fn long_methods_analysis(file: &AnalyzerFile<'_>, long_method_nloc: usize) -> Vec<Finding> {
    if file.adapter().metrics_regions().is_empty() {
        return Vec::new();
    }
    let source = file.source();
    let mut out = Vec::new();
    for node in file.node_ids() {
        if !file.fact(node).is_long_method_region() {
            continue;
        }
        let view = file
            .analysis
            .node(node)
            .expect("AnalyzerFile NodeId belongs to its analysis");
        let span = view.span();
        let start_line = span.start_point().row() + 1;
        let end_line = span.end_point().row() + 1;
        let text = source.region_text(start_line, end_line);
        let nloc = nloc(&text, file.adapter().line_comments());
        if nloc >= long_method_nloc {
            out.push(finding(
                source,
                start_line,
                end_line,
                "long-method",
                Severity::Major,
                SafetyClass::LlmOnly,
                DetectedBy::Complexity,
                &format!("method has {nloc} non-comment line(s)"),
                "extract cohesive helpers or reduce unstructured statement bloat",
                None,
                None,
            ));
        }
    }
    out
}

fn nloc(text: &str, comments: &[&str]) -> usize {
    text.lines()
        .filter(|line| {
            !code_before_comment_with_tokens(line, comments)
                .trim()
                .is_empty()
        })
        .count()
}

fn needless_tail_returns_analysis(file: &AnalyzerFile<'_>) -> Vec<Finding> {
    let source = file.source();
    let mut out = Vec::new();
    for node in file.node_ids() {
        if file.fact(node).tail_position_class() != TailPositionClass::Return
            || !is_function_tail_return_analysis(file, node)
        {
            continue;
        }
        let view = file
            .analysis
            .node(node)
            .expect("AnalyzerFile NodeId belongs to its analysis");
        let span = view.span();
        out.push(finding(
            source,
            span.start_point().row() + 1,
            span.end_point().row() + 1,
            "needless-return",
            Severity::Minor,
            SafetyClass::SafeWithPrecondition,
            DetectedBy::Idiom,
            "tail-position return can usually be an expression",
            "remove return only after tests/typecheck pass",
            Some("return is in tail position and control flow remains unchanged"),
            None,
        ));
    }
    out
}

fn is_function_tail_return_analysis(file: &AnalyzerFile<'_>, node: deslop_parse::NodeId) -> bool {
    let view = file
        .analysis
        .node(node)
        .expect("AnalyzerFile NodeId belongs to its analysis");
    let mut ancestor = view.parent();
    let body = loop {
        let Some(candidate) = ancestor else {
            return false;
        };
        if file.fact(candidate).tail_position_class() == TailPositionClass::FunctionBody {
            break candidate;
        }
        ancestor = file
            .analysis
            .node(candidate)
            .expect("AnalyzerFile ancestor belongs to its analysis")
            .parent();
    };
    let body = file
        .analysis
        .node(body)
        .expect("AnalyzerFile function body belongs to its analysis");
    file.source()
        .text
        .get(view.span().end_byte()..body.span().end_byte())
        .is_some_and(|tail| tail.chars().all(is_tail_padding))
}

fn is_tail_padding(ch: char) -> bool {
    ch.is_whitespace() || ch == ';' || ch == '}'
}

fn blank_runs(source: &AnalyzerText) -> Vec<Finding> {
    let mut out = Vec::new();
    let lines = source.lines();
    // Python requires two blank lines between top-level definitions. This text-only rule cannot
    // prove whether a run is nested, so its SafeAuto boundary must preserve both rather than
    // "fixing" valid module layout. Other adapters retain the established one-blank-line policy.
    let allowed = usize::from(source.lang == deslop_core::Lang::Python) + 1;
    let mut run_start: Option<usize> = None;
    for (idx, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            run_start.get_or_insert(idx + 1);
            continue;
        }
        if let Some(start) = run_start.take() {
            let end = idx;
            if end + 1 - start > allowed {
                out.push(blank_run_finding(source, start, end, allowed));
            }
        }
    }
    if let Some(start) = run_start {
        let end = lines.len();
        if end + 1 - start > allowed {
            out.push(blank_run_finding(source, start, end, allowed));
        }
    }
    out
}

fn blank_run_finding(
    source: &AnalyzerText,
    start_line: usize,
    end_line: usize,
    allowed: usize,
) -> Finding {
    let start_byte = source.line_start_byte(start_line + allowed);
    let end_byte = if end_line < source.lines().len() {
        source.line_start_byte(end_line + 1)
    } else {
        source.text.len()
    };
    let edit = Edit {
        kind: EditKind::SafeAuto,
        splices: vec![Splice {
            start_byte,
            end_byte,
            replacement: String::new(),
        }],
    };
    finding(
        source,
        start_line,
        end_line,
        "consecutive-blank-lines",
        Severity::Info,
        SafetyClass::SafeAuto,
        DetectedBy::Text,
        &format!("{} consecutive blank lines", end_line - start_line + 1),
        if allowed == 1 {
            "collapse to a single blank line"
        } else {
            "collapse to two blank lines"
        },
        None,
        Some(edit),
    )
}

fn narrating_comments(source: &AnalyzerText, comments: &[&str]) -> Vec<Finding> {
    let narration = Regex::new(
        r"(?i)^(import|initialize|define|create|loop|iterate|return|set|get|check|increment|call|assign|print|update|add|remove|now|first|then|next|finally|step\s*\d+|handle|store|compute|calculate|convert|build|setup|start|end|begin|this\s+(function|method|block|loop|line|variable|code)|we\s+(now|then|will|need)|let'?s)\b",
    )
    .expect("valid regex");
    let mut out = Vec::new();
    let comment_block_lines = full_comment_block_lines(source, comments);
    for (idx, line) in source.lines().iter().enumerate() {
        let line_no = idx + 1;
        if comment_block_lines.contains(&line_no) {
            continue;
        }
        let Some((comment, _col)) = line_comment(line, comments) else {
            continue;
        };
        let text = comment.trim();
        if text.is_empty() || text.split_whitespace().count() > 9 || is_banner(text) {
            continue;
        }
        if narration.is_match(text) {
            out.push(finding(
                source,
                line_no,
                line_no,
                "narrating-comment",
                Severity::Minor,
                SafetyClass::LlmOnly,
                DetectedBy::Text,
                &format!("comment restates the code: \"{text}\""),
                "drop narration while preserving useful why-comments",
                None,
                None,
            ));
        }
    }
    out
}

fn full_comment_block_lines(
    source: &AnalyzerText,
    comments: &[&str],
) -> std::collections::BTreeSet<usize> {
    let lines = source.lines();
    let mut out = std::collections::BTreeSet::new();
    let mut run = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let is_full_comment =
            line_comment(line, comments).is_some_and(|(_, col)| line[..col].trim().is_empty());
        if is_full_comment {
            run.push(idx + 1);
            continue;
        }
        if run.len() >= 2 {
            out.extend(run.iter().copied());
        }
        run.clear();
    }
    if run.len() >= 2 {
        out.extend(run);
    }
    out
}

fn comment_blocks(source: &AnalyzerText, comments: &[&str]) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut run_start: Option<usize> = None;
    let mut seen_code = false;
    for (idx, line) in source.lines().iter().enumerate() {
        let line_no = idx + 1;
        let is_full_comment =
            line_comment(line, comments).is_some_and(|(_, col)| line[..col].trim().is_empty());
        if is_full_comment {
            run_start.get_or_insert(line_no);
            continue;
        }
        if let Some(start) = run_start.take()
            && seen_code
            && line_no - start >= 4
        {
            out.push(comment_block_finding(source, start, line_no - 1));
        }
        if !line.trim().is_empty() {
            seen_code = true;
        }
    }
    if let Some(start) = run_start {
        let end = source.lines().len();
        if seen_code && end - start + 1 >= 4 {
            out.push(comment_block_finding(source, start, end));
        }
    }
    out
}

fn comment_block_finding(source: &AnalyzerText, start_line: usize, end_line: usize) -> Finding {
    finding(
        source,
        start_line,
        end_line,
        "comment-block",
        Severity::Info,
        SafetyClass::LlmOnly,
        DetectedBy::Text,
        &format!(
            "{}-line comment block is likely narration",
            end_line - start_line + 1
        ),
        "keep the why; remove play-by-play",
        None,
        None,
    )
}

fn line_comment<'a>(line: &'a str, comments: &[&str]) -> Option<(&'a str, usize)> {
    comments
        .iter()
        .filter_map(|token| {
            line.find(token)
                .map(|idx| (&line[idx + token.len()..], idx))
        })
        .min_by_key(|(_, idx)| *idx)
}

fn code_before_comment_with_tokens<'a>(line: &'a str, comment_tokens: &[&str]) -> &'a str {
    let comment_at = comment_tokens
        .iter()
        .filter_map(|token| line.find(token))
        .min();
    match comment_at {
        Some(idx) => &line[..idx],
        None => line,
    }
}

fn is_banner(text: &str) -> bool {
    text.len() >= 6
        && text
            .chars()
            .all(|ch| ch.is_whitespace() || matches!(ch, '-' | '=' | '*' | '#' | '/' | '~' | '_'))
}
