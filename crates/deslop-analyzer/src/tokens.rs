use deslop_core::{DetectedBy, Finding, Lang, SafetyClass, Severity};
use deslop_lang::{LangPack, RegionClass, Registry as LangRegistry};
use deslop_parse::{NodeId, SourceFile, parse_source};
use std::collections::HashMap;
use std::path::PathBuf;
use tree_sitter::Node;

use crate::{AnalyzerConfig, AnalyzerFile, finding};

#[derive(Debug, Clone)]
struct Token {
    exact: String,
    normalized: String,
    start_byte: usize,
    end_byte: usize,
    segment: usize,
    meaningful: bool,
}

#[derive(Debug, Clone, Copy)]
struct Segment {
    id: usize,
    start_byte: usize,
    end_byte: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaskKind {
    Comment,
    Data,
    String,
}

#[derive(Debug, Clone, Copy)]
struct MaskRange {
    start_byte: usize,
    end_byte: usize,
    kind: MaskKind,
}

pub(crate) fn duplicate_token_sequences(
    source: &SourceFile,
    config: &AnalyzerConfig,
) -> Vec<Finding> {
    let min_tokens = config.min_duplication_tokens;
    let min_meaningful_tokens = config.min_meaningful_tokens;
    let tokens = tokenize(source);
    if min_tokens == 0 || tokens.len() < min_tokens * 2 {
        return Vec::new();
    }
    let rust_tree = rust_tree(source);
    let mut out = Vec::new();
    let mut reported_until = 0;
    for i in 0..=(tokens.len() - min_tokens) {
        for j in (i + min_tokens)..=(tokens.len() - min_tokens) {
            if j < reported_until {
                continue;
            }
            let Some(match_info) =
                duplicate_match(&tokens, i, j, min_tokens, min_meaningful_tokens)
            else {
                continue;
            };
            if non_removable_rust_match(source, &rust_tree, match_info.left, match_info.right) {
                continue;
            }
            out.push(duplicate_finding(source, &tokens, match_info));
            reported_until = j + min_tokens;
            break;
        }
    }
    out
}

pub(crate) fn duplicate_token_sequences_analysis(
    file: &AnalyzerFile<'_>,
    config: &AnalyzerConfig,
) -> Vec<Finding> {
    let source = file.source();
    let min_tokens = config.min_duplication_tokens;
    let min_meaningful_tokens = config.min_meaningful_tokens;
    let tokens = tokenize_analysis(file);
    if min_tokens == 0 || tokens.len() < min_tokens * 2 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut reported_until = 0;
    for i in 0..=(tokens.len() - min_tokens) {
        for j in (i + min_tokens)..=(tokens.len() - min_tokens) {
            if j < reported_until {
                continue;
            }
            let Some(match_info) =
                duplicate_match(&tokens, i, j, min_tokens, min_meaningful_tokens)
            else {
                continue;
            };
            if non_removable_rust_match_analysis(file, match_info.left, match_info.right) {
                continue;
            }
            out.push(duplicate_finding(source, &tokens, match_info));
            reported_until = j + min_tokens;
            break;
        }
    }
    out
}

pub(crate) fn cross_file_duplicate_findings_analysis(
    files: &[AnalyzerFile<'_>],
    config: &AnalyzerConfig,
) -> Vec<Finding> {
    let min_tokens = config.min_duplication_tokens;
    if min_tokens == 0 || files.len() < 2 {
        return Vec::new();
    }
    let tokenized = files
        .iter()
        .map(|file| (file.source(), tokenize_analysis(file)))
        .collect::<Vec<_>>();
    let mut exact_windows = HashMap::<Vec<String>, CrossFileWindow>::new();
    let mut normalized_windows = HashMap::<Vec<String>, CrossFileWindow>::new();
    let mut out = Vec::new();
    for (source_index, (source, tokens)) in tokenized.iter().enumerate() {
        if tokens.len() < min_tokens {
            continue;
        }
        for start in 0..=(tokens.len() - min_tokens) {
            let window = &tokens[start..start + min_tokens];
            if !single_segment(window)
                || meaningful_count(window) < config.min_meaningful_tokens
                || already_reported_cross_file(&out, source, window)
            {
                continue;
            }
            let exact_key = token_key(window, |token| token.exact.as_str());
            if let Some(first) = exact_windows.get(&exact_key)
                && first.source_index != source_index
            {
                out.push(cross_file_finding(
                    source,
                    tokens,
                    start,
                    min_tokens,
                    "duplicate-block",
                    first,
                    meaningful_count(window),
                ));
                continue;
            }
            exact_windows.entry(exact_key).or_insert_with(|| {
                CrossFileWindow::new(source_index, source.path.to_path_buf(), source, window)
            });

            let normalized_key = token_key(window, |token| token.normalized.as_str());
            if let Some(first) = normalized_windows.get(&normalized_key)
                && first.source_index != source_index
            {
                out.push(cross_file_finding(
                    source,
                    tokens,
                    start,
                    min_tokens,
                    "near-duplicate",
                    first,
                    meaningful_count(window),
                ));
                continue;
            }
            normalized_windows.entry(normalized_key).or_insert_with(|| {
                CrossFileWindow::new(source_index, source.path.to_path_buf(), source, window)
            });
        }
    }
    out
}

#[derive(Debug, Clone)]
struct CrossFileWindow {
    source_index: usize,
    path: PathBuf,
    start_line: usize,
}

impl CrossFileWindow {
    fn new(source_index: usize, path: PathBuf, source: &SourceFile, tokens: &[Token]) -> Self {
        Self {
            source_index,
            path,
            start_line: source.line_for_byte(tokens[0].start_byte),
        }
    }
}

fn token_key(tokens: &[Token], field: impl Fn(&Token) -> &str) -> Vec<String> {
    tokens
        .iter()
        .map(|token| field(token).to_string())
        .collect()
}

fn already_reported_cross_file(out: &[Finding], source: &SourceFile, window: &[Token]) -> bool {
    let start_line = source.line_for_byte(window[0].start_byte);
    let end_line = source.line_for_byte(window[window.len() - 1].end_byte);
    out.iter().any(|finding| {
        finding.path == source.path
            && finding.span.start_line <= end_line
            && finding.span.end_line >= start_line
            && matches!(finding.rule.as_str(), "duplicate-block" | "near-duplicate")
    })
}

fn cross_file_finding(
    source: &SourceFile,
    tokens: &[Token],
    start: usize,
    len: usize,
    rule: &str,
    first: &CrossFileWindow,
    meaningful_count: usize,
) -> Finding {
    let start_line = source.line_for_byte(tokens[start].start_byte);
    let end_line = source.line_for_byte(tokens[start + len - 1].end_byte);
    finding(
        source,
        start_line,
        end_line,
        rule,
        Severity::Major,
        SafetyClass::LlmOnly,
        DetectedBy::Duplication,
        &format!(
            "{meaningful_count} meaningful tokens duplicate the block at {}:{}",
            first.path.display(),
            first.start_line
        ),
        "extract a shared function or helper",
        None,
        None,
    )
}

struct DuplicateMatch<'a> {
    left_index: usize,
    right_index: usize,
    left: &'a [Token],
    right: &'a [Token],
    rule: &'static str,
    meaningful_count: usize,
}

fn duplicate_match<'a>(
    tokens: &'a [Token],
    left_index: usize,
    right_index: usize,
    min_tokens: usize,
    min_meaningful_tokens: usize,
) -> Option<DuplicateMatch<'a>> {
    let left = &tokens[left_index..left_index + min_tokens];
    let right = &tokens[right_index..right_index + min_tokens];
    if !disjoint_ranges(left, right) || !single_segment(left) || !single_segment(right) {
        return None;
    }
    let meaningful_count = meaningful_count(left).min(meaningful_count(right));
    let rule = duplicate_rule(left, right)?;
    (meaningful_count >= min_meaningful_tokens).then_some(DuplicateMatch {
        left_index,
        right_index,
        left,
        right,
        rule,
        meaningful_count,
    })
}

fn duplicate_rule(left: &[Token], right: &[Token]) -> Option<&'static str> {
    if token_windows_match(left, right, |token| token.exact.as_str()) {
        Some("duplicate-block")
    } else if token_windows_match(left, right, |token| token.normalized.as_str()) {
        Some("near-duplicate")
    } else {
        None
    }
}

fn non_removable_rust_match(
    source: &SourceFile,
    rust_tree: &Option<tree_sitter::Tree>,
    left: &[Token],
    right: &[Token],
) -> bool {
    rust_tree
        .as_ref()
        .is_some_and(|tree| non_removable_rust_mapping_rhyme(source, tree.root_node(), left, right))
}

fn duplicate_finding(
    source: &SourceFile,
    tokens: &[Token],
    match_info: DuplicateMatch<'_>,
) -> Finding {
    let start_line = source.line_for_byte(tokens[match_info.right_index].start_byte);
    let end_line =
        source.line_for_byte(tokens[match_info.right_index + match_info.right.len() - 1].end_byte);
    finding(
        source,
        start_line,
        end_line,
        match_info.rule,
        Severity::Major,
        SafetyClass::LlmOnly,
        DetectedBy::Duplication,
        &format!(
            "{} meaningful tokens duplicate the block at line {}",
            match_info.meaningful_count,
            source.line_for_byte(tokens[match_info.left_index].start_byte)
        ),
        "extract a shared function or helper",
        None,
        None,
    )
}

fn tokenize(source: &SourceFile) -> Vec<Token> {
    let registry = LangRegistry::default();
    let pack = registry.pack_for_lang(source.lang);
    let segments = behavioral_segments(source, pack);
    let masks = token_masks(source, pack);
    tokenize_with(source, pack, &segments, &masks)
}

fn tokenize_analysis(file: &AnalyzerFile<'_>) -> Vec<Token> {
    let segments = behavioral_segments_analysis(file);
    let masks = token_masks_analysis(file);
    tokenize_with(file.source(), file.adapter(), &segments, &masks)
}

fn tokenize_with(
    source: &SourceFile,
    pack: &dyn LangPack,
    segments: &[Segment],
    masks: &[MaskRange],
) -> Vec<Token> {
    let mut out = Vec::new();
    let mut iter = source.text.char_indices().peekable();
    while let Some((start, ch)) = iter.next() {
        if ch.is_whitespace() {
            continue;
        }
        if let Some(mask) = mask_for_byte(masks, start) {
            push_masked_token(&mut out, &source.text, segments, mask, start);
            skip_until_byte(&mut iter, mask.end_byte);
            continue;
        }
        if starts_line_comment(source, start, pack) {
            skip_until_newline(&mut iter);
            continue;
        }
        let Some(segment) = segment_for_byte(segments, start) else {
            skip_token(&source.text, &mut iter, start, ch);
            continue;
        };
        out.push(next_token(&source.text, &mut iter, start, ch, segment.id));
    }
    out
}

fn push_masked_token(
    out: &mut Vec<Token>,
    text: &str,
    segments: &[Segment],
    mask: MaskRange,
    start: usize,
) {
    if mask.kind == MaskKind::String
        && start == mask.start_byte
        && let Some(segment) = segment_for_byte(segments, start)
    {
        out.push(string_token_from_range(
            text,
            mask.start_byte,
            mask.end_byte,
            segment.id,
        ));
    }
}

type CharIter<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

fn next_token(
    text: &str,
    iter: &mut CharIter<'_>,
    start: usize,
    ch: char,
    segment: usize,
) -> Token {
    match ch {
        '"' => string_token(text, iter, start, ch, segment),
        _ if is_ident_start(ch) => identifier_token(text, iter, start, ch, segment),
        _ if ch.is_ascii_digit() => number_token(text, iter, start, ch, segment),
        _ => one_char_token(start, ch, segment),
    }
}

fn skip_token(text: &str, iter: &mut CharIter<'_>, start: usize, ch: char) {
    match ch {
        '"' => {
            let _ = string_token(text, iter, start, ch, 0);
        }
        _ if is_ident_start(ch) => {
            let _ = identifier_token(text, iter, start, ch, 0);
        }
        _ if ch.is_ascii_digit() => {
            let _ = number_token(text, iter, start, ch, 0);
        }
        _ => {}
    }
}

fn string_token(
    text: &str,
    iter: &mut CharIter<'_>,
    start: usize,
    ch: char,
    segment: usize,
) -> Token {
    let mut end = start + ch.len_utf8();
    while let Some((idx, next)) = iter.next() {
        end = idx + next.len_utf8();
        if next == '\\' {
            if let Some((escaped_idx, escaped)) = iter.next() {
                end = escaped_idx + escaped.len_utf8();
            }
        } else if next == '"' {
            break;
        }
    }
    string_token_from_range(text, start, end, segment)
}

fn string_token_from_range(text: &str, start: usize, end: usize, segment: usize) -> Token {
    let exact = text[start..end].to_string();
    Token {
        normalized: exact.clone(),
        exact,
        start_byte: start,
        end_byte: end,
        segment,
        meaningful: true,
    }
}

fn identifier_token(
    text: &str,
    iter: &mut CharIter<'_>,
    start: usize,
    ch: char,
    segment: usize,
) -> Token {
    let end = consume_while(iter, start + ch.len_utf8(), is_ident_continue);
    let exact = text[start..end].to_string();
    let meaningful = is_meaningful_identifier(&exact);
    Token {
        normalized: normalize_identifier(&exact),
        exact,
        start_byte: start,
        end_byte: end,
        segment,
        meaningful,
    }
}

fn number_token(
    text: &str,
    iter: &mut CharIter<'_>,
    start: usize,
    ch: char,
    segment: usize,
) -> Token {
    let end = consume_while(iter, start + ch.len_utf8(), |next| {
        next.is_ascii_digit() || next == '.'
    });
    token_from_slice(text, start, end, "NUM", segment, true)
}

fn one_char_token(start: usize, ch: char, segment: usize) -> Token {
    let text = ch.to_string();
    Token {
        normalized: text.clone(),
        exact: text,
        start_byte: start,
        end_byte: start + ch.len_utf8(),
        segment,
        meaningful: is_meaningful_punctuation(ch),
    }
}

fn token_from_slice(
    text: &str,
    start: usize,
    end: usize,
    normalized: &str,
    segment: usize,
    meaningful: bool,
) -> Token {
    Token {
        exact: text[start..end].to_string(),
        normalized: normalized.to_string(),
        start_byte: start,
        end_byte: end,
        segment,
        meaningful,
    }
}

fn consume_while(
    iter: &mut CharIter<'_>,
    mut end: usize,
    mut accepts: impl FnMut(char) -> bool,
) -> usize {
    while let Some((idx, next)) = iter.peek().copied() {
        if !accepts(next) {
            break;
        }
        iter.next();
        end = idx + next.len_utf8();
    }
    end
}

fn skip_until_newline(iter: &mut CharIter<'_>) {
    while let Some((_, next)) = iter.peek() {
        if *next == '\n' {
            break;
        }
        iter.next();
    }
}

fn skip_until_byte(iter: &mut CharIter<'_>, end: usize) {
    while let Some((idx, _)) = iter.peek() {
        if *idx >= end {
            break;
        }
        iter.next();
    }
}

fn starts_line_comment(source: &SourceFile, start: usize, pack: &dyn LangPack) -> bool {
    let text = &source.text[start..];
    pack.line_comments()
        .iter()
        .any(|token| text.starts_with(token))
}

fn behavioral_segments(source: &SourceFile, pack: &dyn LangPack) -> Vec<Segment> {
    if pack.grammar().is_none() {
        return vec![Segment {
            id: 0,
            start_byte: 0,
            end_byte: source.text.len(),
        }];
    }

    let Some(tree) = parse_source(source).ok().flatten() else {
        return vec![Segment {
            id: 0,
            start_byte: 0,
            end_byte: source.text.len(),
        }];
    };
    if tree.root_node().has_error() {
        return Vec::new();
    }

    let mut segments = Vec::new();
    collect_behavioral_segments(tree.root_node(), &source.text, pack, false, &mut segments);
    segments
        .into_iter()
        .enumerate()
        .map(|(id, mut segment)| {
            segment.id = id;
            segment
        })
        .collect()
}

fn behavioral_segments_analysis(file: &AnalyzerFile<'_>) -> Vec<Segment> {
    let Some(root) = file.node_ids().next() else {
        return Vec::new();
    };
    let mut segments = Vec::new();
    collect_behavioral_segments_analysis(file, root, false, &mut segments);
    segments
        .into_iter()
        .enumerate()
        .map(|(id, mut segment)| {
            segment.id = id;
            segment
        })
        .collect()
}

fn token_masks(source: &SourceFile, pack: &dyn LangPack) -> Vec<MaskRange> {
    let Some(tree) = parse_source(source).ok().flatten() else {
        return Vec::new();
    };
    if tree.root_node().has_error() {
        return Vec::new();
    }
    let mut masks = Vec::new();
    collect_token_masks(tree.root_node(), &source.text, pack, &mut masks);
    masks.sort_by_key(|mask| (mask.start_byte, mask.end_byte));
    masks
}

fn token_masks_analysis(file: &AnalyzerFile<'_>) -> Vec<MaskRange> {
    let Some(root) = file.node_ids().next() else {
        return Vec::new();
    };
    let mut masks = Vec::new();
    collect_token_masks_analysis(file, root, &mut masks);
    masks.sort_by_key(|mask| (mask.start_byte, mask.end_byte));
    masks
}

fn collect_token_masks_analysis(file: &AnalyzerFile<'_>, node: NodeId, masks: &mut Vec<MaskRange>) {
    let Ok(view) = file.analysis.node(node) else {
        return;
    };
    let kind = view.raw_kind();
    let mask_kind = if kind.contains("comment") {
        Some(MaskKind::Comment)
    } else if file.fact(node).is_duplication_data_region() {
        Some(MaskKind::Data)
    } else if kind.contains("string") || kind.contains("str_lit") {
        Some(MaskKind::String)
    } else {
        None
    };
    if let Some(kind) = mask_kind {
        let span = view.span();
        masks.push(MaskRange {
            start_byte: span.start_byte(),
            end_byte: span.end_byte().min(file.source().text.len()),
            kind,
        });
        return;
    }
    for child in view.children() {
        collect_token_masks_analysis(file, child, masks);
    }
}

fn collect_token_masks(
    node: Node<'_>,
    text: &str,
    pack: &dyn LangPack,
    masks: &mut Vec<MaskRange>,
) {
    if let Some(kind) = token_mask_kind(node, text, pack) {
        masks.push(MaskRange {
            start_byte: node.start_byte(),
            end_byte: node.end_byte().min(text.len()),
            kind,
        });
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_token_masks(child, text, pack, masks);
    }
}

fn token_mask_kind(node: Node<'_>, text: &str, pack: &dyn LangPack) -> Option<MaskKind> {
    let kind = node.kind();
    if kind.contains("comment") {
        return Some(MaskKind::Comment);
    }
    if pack.is_duplication_data_region(node, text) {
        return Some(MaskKind::Data);
    }
    if kind.contains("string") || kind.contains("str_lit") {
        return Some(MaskKind::String);
    }
    None
}

fn collect_behavioral_segments(
    node: Node<'_>,
    text: &str,
    pack: &dyn LangPack,
    in_body: bool,
    segments: &mut Vec<Segment>,
) {
    match pack.region_class(node, text) {
        RegionClass::Declaration if !pack.is_behavioral_container(node, text) => return,
        RegionClass::Behavioral if !in_body => {
            segments.push(Segment {
                id: 0,
                start_byte: node.start_byte(),
                end_byte: node.end_byte().min(text.len()),
            });
            return;
        }
        RegionClass::Behavioral | RegionClass::Declaration | RegionClass::Other => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_behavioral_segments(
            child,
            text,
            pack,
            in_body || pack.region_class(node, text) == RegionClass::Behavioral,
            segments,
        );
    }
}

fn collect_behavioral_segments_analysis(
    file: &AnalyzerFile<'_>,
    node: NodeId,
    in_body: bool,
    segments: &mut Vec<Segment>,
) {
    let Ok(view) = file.analysis.node(node) else {
        return;
    };
    let fact = file.fact(node);
    match fact.region_class() {
        RegionClass::Declaration if !fact.is_behavioral_container() => return,
        RegionClass::Behavioral if !in_body => {
            let span = view.span();
            segments.push(Segment {
                id: 0,
                start_byte: span.start_byte(),
                end_byte: span.end_byte().min(file.source().text.len()),
            });
            return;
        }
        RegionClass::Behavioral | RegionClass::Declaration | RegionClass::Other => {}
    }
    let child_in_body = in_body || fact.region_class() == RegionClass::Behavioral;
    for child in view.children() {
        collect_behavioral_segments_analysis(file, child, child_in_body, segments);
    }
}

fn segment_for_byte(segments: &[Segment], byte: usize) -> Option<Segment> {
    segments
        .iter()
        .copied()
        .find(|segment| byte >= segment.start_byte && byte < segment.end_byte)
}

fn single_segment(tokens: &[Token]) -> bool {
    tokens
        .first()
        .is_some_and(|first| tokens.iter().all(|token| token.segment == first.segment))
}

fn meaningful_count(tokens: &[Token]) -> usize {
    tokens.iter().filter(|token| token.meaningful).count()
}

fn token_windows_match(left: &[Token], right: &[Token], field: impl Fn(&Token) -> &str) -> bool {
    left.iter()
        .zip(right)
        .all(|(left, right)| field(left) == field(right))
}

fn rust_tree(source: &SourceFile) -> Option<tree_sitter::Tree> {
    (source.lang == Lang::Rust)
        .then(|| parse_source(source).ok().flatten())?
        .filter(|tree| !tree.root_node().has_error())
}

fn non_removable_rust_mapping_rhyme(
    source: &SourceFile,
    root: Node<'_>,
    left: &[Token],
    right: &[Token],
) -> bool {
    let Some((left_start, left_end)) = token_window_range(left) else {
        return false;
    };
    let Some((right_start, right_end)) = token_window_range(right) else {
        return false;
    };
    is_in_pure_path_mapping_context(source, root, left_start, left_end)
        && is_in_pure_path_mapping_context(source, root, right_start, right_end)
}

fn non_removable_rust_match_analysis(
    file: &AnalyzerFile<'_>,
    left: &[Token],
    right: &[Token],
) -> bool {
    if file.source().lang != Lang::Rust {
        return false;
    }
    let Some(root) = file.node_ids().next() else {
        return false;
    };
    let Some((left_start, left_end)) = token_window_range(left) else {
        return false;
    };
    let Some((right_start, right_end)) = token_window_range(right) else {
        return false;
    };
    is_in_pure_path_mapping_context_analysis(file, root, left_start, left_end)
        && is_in_pure_path_mapping_context_analysis(file, root, right_start, right_end)
}

fn smallest_enclosing_node_analysis(
    file: &AnalyzerFile<'_>,
    node: NodeId,
    start_byte: usize,
    end_byte: usize,
    kind: &str,
) -> Option<NodeId> {
    let view = file.analysis.node(node).ok()?;
    let span = view.span();
    if span.start_byte() > start_byte || end_byte > span.end_byte() {
        return None;
    }
    for child in view.children() {
        let child_view = file.analysis.node(child).ok()?;
        if child_view.is_named()
            && let Some(enclosing) =
                smallest_enclosing_node_analysis(file, child, start_byte, end_byte, kind)
        {
            return Some(enclosing);
        }
    }
    (view.raw_kind() == kind).then_some(node)
}

fn is_in_pure_path_mapping_context_analysis(
    file: &AnalyzerFile<'_>,
    root: NodeId,
    start_byte: usize,
    end_byte: usize,
) -> bool {
    if smallest_enclosing_node_analysis(file, root, start_byte, end_byte, "match_expression")
        .is_some_and(|node| is_pure_path_mapping_match_analysis(file, node))
    {
        return true;
    }
    ["function_item", "impl_item"].into_iter().any(|kind| {
        smallest_enclosing_node_analysis(file, root, start_byte, end_byte, kind)
            .is_some_and(|node| contains_pure_path_mapping_match_analysis(file, node))
    })
}

fn contains_pure_path_mapping_match_analysis(file: &AnalyzerFile<'_>, node: NodeId) -> bool {
    let Ok(view) = file.analysis.node(node) else {
        return false;
    };
    if view.raw_kind() == "match_expression" && is_pure_path_mapping_match_analysis(file, node) {
        return true;
    }
    view.children().into_iter().any(|child| {
        file.analysis.node(child).is_ok_and(|view| view.is_named())
            && contains_pure_path_mapping_match_analysis(file, child)
    })
}

fn is_pure_path_mapping_match_analysis(file: &AnalyzerFile<'_>, node: NodeId) -> bool {
    let Some(body) = file.child_by_field(node, "body") else {
        return false;
    };
    let Ok(body_view) = file.analysis.node(body) else {
        return false;
    };
    let arms = body_view
        .children()
        .into_iter()
        .filter(|child| {
            file.analysis
                .node(*child)
                .is_ok_and(|view| view.raw_kind() == "match_arm")
        })
        .collect::<Vec<_>>();
    arms.len() >= 2
        && arms
            .into_iter()
            .all(|arm| is_pure_path_mapping_arm_analysis(file, arm))
}

fn is_pure_path_mapping_arm_analysis(file: &AnalyzerFile<'_>, arm: NodeId) -> bool {
    let Some(pattern) = file.child_by_field(arm, "pattern") else {
        return false;
    };
    let Some(value) = file.child_by_field(arm, "value") else {
        return false;
    };
    is_path_like_pattern_analysis(file, pattern) && is_path_like_value_analysis(file, value)
}

fn is_path_like_pattern_analysis(file: &AnalyzerFile<'_>, node: NodeId) -> bool {
    let Ok(view) = file.analysis.node(node) else {
        return false;
    };
    match view.raw_kind() {
        "identifier" | "scoped_identifier" => true,
        "match_pattern" => {
            if file.child_by_field(node, "condition").is_some() {
                return false;
            }
            let named = view
                .children()
                .into_iter()
                .filter(|child| file.analysis.node(*child).is_ok_and(|view| view.is_named()))
                .collect::<Vec<_>>();
            named.len() == 1 && is_path_like_pattern_analysis(file, named[0])
        }
        _ => {
            let text = view.text();
            text.contains("::") && !text.contains([' ', '\n', '\t'])
        }
    }
}

fn is_path_like_value_analysis(file: &AnalyzerFile<'_>, node: NodeId) -> bool {
    file.analysis
        .node(node)
        .is_ok_and(|view| matches!(view.raw_kind(), "identifier" | "scoped_identifier"))
}

fn token_window_range(tokens: &[Token]) -> Option<(usize, usize)> {
    Some((tokens.first()?.start_byte, tokens.last()?.end_byte))
}

fn smallest_enclosing_node<'tree>(
    node: Node<'tree>,
    start_byte: usize,
    end_byte: usize,
    kind: &str,
) -> Option<Node<'tree>> {
    if !node_contains_range(node, start_byte, end_byte) {
        return None;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor).filter(|child| child.is_named()) {
        if let Some(enclosing) = smallest_enclosing_node(child, start_byte, end_byte, kind) {
            return Some(enclosing);
        }
    }

    (node.kind() == kind).then_some(node)
}

fn node_contains_range(node: Node<'_>, start_byte: usize, end_byte: usize) -> bool {
    node.start_byte() <= start_byte && end_byte <= node.end_byte()
}

fn is_in_pure_path_mapping_context(
    source: &SourceFile,
    root: Node<'_>,
    start_byte: usize,
    end_byte: usize,
) -> bool {
    if smallest_enclosing_node(root, start_byte, end_byte, "match_expression")
        .is_some_and(|node| is_pure_path_mapping_match(source, node))
    {
        return true;
    }

    ["function_item", "impl_item"].into_iter().any(|kind| {
        smallest_enclosing_node(root, start_byte, end_byte, kind)
            .is_some_and(|node| contains_pure_path_mapping_match(source, node))
    })
}

fn contains_pure_path_mapping_match(source: &SourceFile, node: Node<'_>) -> bool {
    if node.kind() == "match_expression" && is_pure_path_mapping_match(source, node) {
        return true;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.is_named())
        .any(|child| contains_pure_path_mapping_match(source, child))
}

fn is_pure_path_mapping_match(source: &SourceFile, match_node: Node<'_>) -> bool {
    let Some(body) = match_node.child_by_field_name("body") else {
        return false;
    };
    let mut cursor = body.walk();
    let arms: Vec<_> = body
        .children(&mut cursor)
        .filter(|child| child.kind() == "match_arm")
        .collect();
    arms.len() >= 2
        && arms
            .into_iter()
            .all(|arm| is_pure_path_mapping_arm(source, arm))
}

fn is_pure_path_mapping_arm(source: &SourceFile, arm: Node<'_>) -> bool {
    let Some(pattern) = arm.child_by_field_name("pattern") else {
        return false;
    };
    let Some(value) = arm.child_by_field_name("value") else {
        return false;
    };
    is_path_like_pattern(source, pattern) && is_path_like_value(value)
}

fn is_path_like_pattern(source: &SourceFile, node: Node<'_>) -> bool {
    match node.kind() {
        "identifier" | "scoped_identifier" => true,
        "match_pattern" => {
            if node.child_by_field_name("condition").is_some() {
                return false;
            }
            let mut cursor = node.walk();
            let mut children = node.children(&mut cursor).filter(|child| child.is_named());
            let Some(child) = children.next() else {
                return false;
            };
            children.next().is_none() && is_path_like_pattern(source, child)
        }
        _ => node
            .utf8_text(source.text.as_bytes())
            .is_ok_and(|text| text.contains("::") && !text.contains([' ', '\n', '\t'])),
    }
}

fn is_path_like_value(node: Node<'_>) -> bool {
    matches!(node.kind(), "identifier" | "scoped_identifier")
}

fn disjoint_ranges(left: &[Token], right: &[Token]) -> bool {
    let Some(left_start) = left.first().map(|token| token.start_byte) else {
        return false;
    };
    let Some(left_end) = left.last().map(|token| token.end_byte) else {
        return false;
    };
    let Some(right_start) = right.first().map(|token| token.start_byte) else {
        return false;
    };
    let Some(right_end) = right.last().map(|token| token.end_byte) else {
        return false;
    };
    left_end <= right_start || right_end <= left_start
}

fn mask_for_byte(masks: &[MaskRange], byte: usize) -> Option<MaskRange> {
    masks
        .iter()
        .copied()
        .find(|mask| byte >= mask.start_byte && byte < mask.end_byte)
}

fn is_meaningful_identifier(value: &str) -> bool {
    !matches!(
        value,
        "as" | "catch"
            | "const"
            | "def"
            | "defrecord"
            | "derive"
            | "else"
            | "end"
            | "enum"
            | "export"
            | "fn"
            | "for"
            | "function"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "mod"
            | "module"
            | "mut"
            | "ns"
            | "pub"
            | "return"
            | "self"
            | "struct"
            | "trait"
            | "try"
            | "type"
            | "use"
            | "where"
            | "while"
    )
}

fn is_meaningful_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '+' | '-' | '*' | '/' | '%' | '=' | '<' | '>' | '!' | '&' | '|'
    )
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic()
        || matches!(
            ch,
            '_' | '-' | '?' | '!' | '*' | '+' | '/' | '<' | '>' | '='
        )
}

fn is_ident_continue(ch: char) -> bool {
    is_ident_start(ch) || ch.is_ascii_digit() || ch == '.'
}

fn normalize_identifier(value: &str) -> String {
    if value
        .chars()
        .any(|ch| ch.is_ascii_alphabetic() || ch == '_')
    {
        "ID".to_string()
    } else {
        value.to_string()
    }
}
