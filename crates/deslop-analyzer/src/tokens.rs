use deslop_core::{DetectedBy, Finding, SafetyClass, Severity};
use deslop_lang::{LangPack, RegionClass, Registry as LangRegistry};
use deslop_parse::{SourceFile, parse_tree};
use tree_sitter::Node;

use crate::finding;

const MIN_MEANINGFUL_TOKENS: usize = 8;

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

pub(crate) fn duplicate_token_sequences(source: &SourceFile, min_tokens: usize) -> Vec<Finding> {
    let tokens = tokenize(source);
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
            let left = &tokens[i..i + min_tokens];
            let right = &tokens[j..j + min_tokens];
            if !disjoint_ranges(left, right) {
                continue;
            }
            let meaningful_count = meaningful_count(left).min(meaningful_count(right));
            if meaningful_count < MIN_MEANINGFUL_TOKENS
                || !single_segment(left)
                || !single_segment(right)
            {
                continue;
            }
            let exact_match = left
                .iter()
                .zip(right)
                .all(|(left, right)| left.exact == right.exact);
            let normalized_match = left
                .iter()
                .zip(right)
                .all(|(left, right)| left.normalized == right.normalized);
            let rule = if exact_match {
                "duplicate-block"
            } else if normalized_match {
                "near-duplicate"
            } else {
                continue;
            };
            let start_line = source.line_for_byte(tokens[j].start_byte);
            let end_line = source.line_for_byte(tokens[j + min_tokens - 1].end_byte);
            out.push(finding(
                source,
                start_line,
                end_line,
                rule,
                Severity::Major,
                SafetyClass::LlmOnly,
                DetectedBy::Duplication,
                &format!(
                    "{} meaningful tokens duplicate the block at line {}",
                    meaningful_count,
                    source.line_for_byte(tokens[i].start_byte)
                ),
                "extract a shared function or helper",
                None,
                None,
            ));
            reported_until = j + min_tokens;
            break;
        }
    }
    out
}

fn tokenize(source: &SourceFile) -> Vec<Token> {
    let registry = LangRegistry::default();
    let pack = registry.pack_for_lang(source.lang);
    let segments = behavioral_segments(source, pack);
    let masks = token_masks(source, pack);
    let mut out = Vec::new();
    let mut iter = source.text.char_indices().peekable();
    while let Some((start, ch)) = iter.next() {
        if ch.is_whitespace() {
            continue;
        }
        if let Some(mask) = mask_for_byte(&masks, start) {
            match mask.kind {
                MaskKind::String if start == mask.start_byte => {
                    if let Some(segment) = segment_for_byte(&segments, start) {
                        out.push(string_token_from_range(
                            &source.text,
                            mask.start_byte,
                            mask.end_byte,
                            segment.id,
                        ));
                    }
                }
                _ => {}
            }
            skip_until_byte(&mut iter, mask.end_byte);
            continue;
        }
        if starts_line_comment(source, start, pack) {
            skip_until_newline(&mut iter);
            continue;
        }
        let Some(segment) = segment_for_byte(&segments, start) else {
            skip_token(&source.text, &mut iter, start, ch);
            continue;
        };
        out.push(next_token(&source.text, &mut iter, start, ch, segment.id));
    }
    out
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

    let Some(tree) = parse_tree(source.lang, &source.text).ok().flatten() else {
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

fn token_masks(source: &SourceFile, pack: &dyn LangPack) -> Vec<MaskRange> {
    let Some(tree) = parse_tree(source.lang, &source.text).ok().flatten() else {
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
        RegionClass::Declaration => return,
        RegionClass::Behavioral if !in_body => {
            segments.push(Segment {
                id: 0,
                start_byte: node.start_byte(),
                end_byte: node.end_byte().min(text.len()),
            });
            return;
        }
        RegionClass::Behavioral | RegionClass::Other => {}
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
