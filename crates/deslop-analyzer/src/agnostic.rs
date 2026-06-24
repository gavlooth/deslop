use deslop_core::{DetectedBy, Edit, EditKind, Finding, Lang, SafetyClass, Severity, Splice};
use deslop_lang::{LangPack, Registry as LangRegistry, TailPositionClass};
use deslop_parse::{SourceFile, parse_tree};
use regex::Regex;
use tree_sitter::Node;

use crate::{AnalyzerConfig, finding, tokens};

pub(crate) fn findings(source: &SourceFile, config: &AnalyzerConfig) -> Vec<Finding> {
    let mut out = Vec::new();
    out.extend(blank_runs(source));
    out.extend(incompleteness(source));
    out.extend(magic_numbers(source));
    out.extend(long_methods(source, config.long_method_nloc));
    out.extend(narrating_comments(source));
    out.extend(comment_blocks(source));
    out.extend(needless_tail_returns(source));
    out.extend(tokens::duplicate_token_sequences(source, config));
    out
}

fn incompleteness(source: &SourceFile) -> Vec<Finding> {
    let mut out = Vec::new();
    let stub = Regex::new(
        r#"(?i)(todo!\s*\(|unimplemented!\s*\(|TODO\s*:?\s*implement|throw\b.*TODO|error\s*\(\s*["']TODO|@assert\s+false|not\s+implemented|placeholder)"#,
    )
    .expect("valid regex");
    let masked = string_comment_ranges(source);
    for (idx, line) in source.lines().iter().enumerate() {
        let line_start = source.line_start_byte(idx + 1);
        let has_real_stub = stub
            .find_iter(line)
            .any(|m| !byte_in_ranges(line_start + m.start(), &masked));
        if has_real_stub {
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

/// Byte ranges of string-literal and comment nodes, so text-based rules can
/// skip trigger words that appear inside strings/comments (e.g. a log message
/// or this rule's own pattern definition containing "TODO"). Empty when the
/// language has no tree-sitter grammar.
fn string_comment_ranges(source: &SourceFile) -> Vec<(usize, usize)> {
    let Some(tree) = parse_tree(source.lang, &source.text).ok().flatten() else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    collect_string_comment(tree.root_node(), &mut ranges);
    ranges
}

fn collect_string_comment(node: Node, ranges: &mut Vec<(usize, usize)>) {
    let kind = node.kind();
    if kind.contains("string") || kind.contains("str_lit") || kind.contains("comment") {
        ranges.push((node.start_byte(), node.end_byte()));
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_string_comment(child, ranges);
    }
}

fn byte_in_ranges(byte: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| byte >= start && byte < end)
}

fn magic_numbers(source: &SourceFile) -> Vec<Finding> {
    let mut out = Vec::new();
    for (idx, line) in source.lines().iter().enumerate() {
        let code = code_before_comment(line, source.lang);
        if should_skip_magic_number_line(code) {
            continue;
        }
        if first_magic_number(code).is_some() {
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

fn first_magic_number(code: &str) -> Option<&str> {
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
            return Some(&code[digit_start..idx]);
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

fn long_methods(source: &SourceFile, long_method_nloc: usize) -> Vec<Finding> {
    let registry = LangRegistry::default();
    let pack = registry.pack_for_lang(source.lang);
    if pack.metrics_regions().is_empty() {
        return Vec::new();
    }
    let Some(tree) = parse_tree(source.lang, &source.text).ok().flatten() else {
        return Vec::new();
    };
    if tree.root_node().has_error() {
        return Vec::new();
    }
    let mut out = Vec::new();
    collect_long_methods(source, tree.root_node(), pack, long_method_nloc, &mut out);
    out
}

fn collect_long_methods(
    source: &SourceFile,
    node: Node<'_>,
    pack: &dyn LangPack,
    long_method_nloc: usize,
    out: &mut Vec<Finding>,
) {
    if pack.is_long_method_region(node, &source.text) {
        let start_line = node.start_position().row + 1;
        let end_line = node.end_position().row + 1;
        let text = source.region_text(start_line, end_line);
        let nloc = nloc(&text, pack.line_comments());
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
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_long_methods(source, child, pack, long_method_nloc, out);
    }
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

fn needless_tail_returns(source: &SourceFile) -> Vec<Finding> {
    let registry = LangRegistry::default();
    let pack = registry.pack_for_lang(source.lang);
    if pack.grammar().is_none() {
        return Vec::new();
    }

    let Some(tree) = parse_tree(source.lang, &source.text).ok().flatten() else {
        return Vec::new();
    };
    if tree.root_node().has_error() {
        return Vec::new();
    }

    let mut out = Vec::new();
    collect_tail_returns(source, tree.root_node(), pack, &mut out);
    out
}

fn collect_tail_returns(
    source: &SourceFile,
    node: Node<'_>,
    pack: &dyn LangPack,
    out: &mut Vec<Finding>,
) {
    if pack.tail_position_class(node, &source.text) == TailPositionClass::Return
        && is_function_tail_return(source, node, pack)
    {
        let start_line = node.start_position().row + 1;
        let end_line = node.end_position().row + 1;
        out.push(finding(
            source,
            start_line,
            end_line,
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

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tail_returns(source, child, pack, out);
    }
}

fn is_function_tail_return(source: &SourceFile, node: Node<'_>, pack: &dyn LangPack) -> bool {
    let Some(body) = nearest_function_body(node, pack, &source.text) else {
        return false;
    };
    source
        .text
        .get(node.end_byte()..body.end_byte())
        .is_some_and(|tail| tail.chars().all(is_tail_padding))
}

fn nearest_function_body<'tree>(
    mut node: Node<'tree>,
    pack: &dyn LangPack,
    text: &str,
) -> Option<Node<'tree>> {
    loop {
        let parent = node.parent()?;
        if pack.tail_position_class(parent, text) == TailPositionClass::FunctionBody {
            return Some(parent);
        }
        node = parent;
    }
}

fn is_tail_padding(ch: char) -> bool {
    ch.is_whitespace() || ch == ';' || ch == '}'
}

fn blank_runs(source: &SourceFile) -> Vec<Finding> {
    let mut out = Vec::new();
    let lines = source.lines();
    let mut run_start: Option<usize> = None;
    for (idx, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            run_start.get_or_insert(idx + 1);
            continue;
        }
        if let Some(start) = run_start.take() {
            let end = idx;
            if end > start {
                out.push(blank_run_finding(source, start, end));
            }
        }
    }
    if let Some(start) = run_start {
        let end = lines.len();
        if end > start {
            out.push(blank_run_finding(source, start, end));
        }
    }
    out
}

fn blank_run_finding(source: &SourceFile, start_line: usize, end_line: usize) -> Finding {
    let start_byte = source.line_start_byte(start_line + 1);
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
        "collapse to a single blank line",
        None,
        Some(edit),
    )
}

fn narrating_comments(source: &SourceFile) -> Vec<Finding> {
    let narration = Regex::new(
        r"(?i)^(import|initialize|define|create|loop|iterate|return|set|get|check|increment|call|assign|print|update|add|remove|now|first|then|next|finally|step\s*\d+|handle|store|compute|calculate|convert|build|setup|start|end|begin|this\s+(function|method|block|loop|line|variable|code)|we\s+(now|then|will|need)|let'?s)\b",
    )
    .expect("valid regex");
    let mut out = Vec::new();
    let comment_block_lines = full_comment_block_lines(source);
    for (idx, line) in source.lines().iter().enumerate() {
        let line_no = idx + 1;
        if comment_block_lines.contains(&line_no) {
            continue;
        }
        let Some((comment, _col)) = line_comment(line, source.lang) else {
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

fn full_comment_block_lines(source: &SourceFile) -> std::collections::BTreeSet<usize> {
    let lines = source.lines();
    let mut out = std::collections::BTreeSet::new();
    let mut run = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let is_full_comment =
            line_comment(line, source.lang).is_some_and(|(_, col)| line[..col].trim().is_empty());
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

fn comment_blocks(source: &SourceFile) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut run_start: Option<usize> = None;
    let mut seen_code = false;
    for (idx, line) in source.lines().iter().enumerate() {
        let line_no = idx + 1;
        let is_full_comment =
            line_comment(line, source.lang).is_some_and(|(_, col)| line[..col].trim().is_empty());
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

fn comment_block_finding(source: &SourceFile, start_line: usize, end_line: usize) -> Finding {
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

fn line_comment(line: &str, lang: Lang) -> Option<(&str, usize)> {
    LangRegistry::default()
        .pack_for_lang(lang)
        .line_comments()
        .iter()
        .filter_map(|token| {
            line.find(token)
                .map(|idx| (&line[idx + token.len()..], idx))
        })
        .min_by_key(|(_, idx)| *idx)
}

fn code_before_comment(line: &str, lang: Lang) -> &str {
    let registry = LangRegistry::default();
    code_before_comment_with_tokens(line, registry.pack_for_lang(lang).line_comments())
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
