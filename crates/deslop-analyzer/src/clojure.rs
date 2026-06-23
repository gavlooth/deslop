use deslop_core::{DetectedBy, Edit, EditKind, Finding, SafetyClass, Severity, Splice};
use deslop_parse::SourceFile;
use regex::Regex;

use crate::finding;

pub(crate) fn findings(source: &SourceFile) -> Vec<Finding> {
    let mut out = Vec::new();
    let rules = [
        SimpleRule {
            rule: "reimpl-not=",
            pattern: r"\(not\s+\(=\s+([^()\n]+?)\)\)",
            replacement: |caps: &regex::Captures<'_>| format!("(not= {})", caps[1].trim()),
            message: "(not (= ...)) reimplements not=",
            suggestion: "use (not= ...)",
        },
        SimpleRule {
            rule: "reimpl-some?",
            pattern: r"\(not\s+\(nil\?\s+([^()\n]+?)\)\)",
            replacement: |caps: &regex::Captures<'_>| format!("(some? {})", caps[1].trim()),
            message: "(not (nil? x)) reimplements some?",
            suggestion: "use (some? x)",
        },
        SimpleRule {
            rule: "reimpl-boolean",
            pattern: r"\(if\s+([^()\n]+?)\s+true\s+false\)",
            replacement: |caps: &regex::Captures<'_>| format!("(boolean {})", caps[1].trim()),
            message: "(if x true false) is just a boolean coercion",
            suggestion: "use (boolean x) or x directly",
        },
    ];

    for rule in rules {
        out.extend(simple_safe_rule(source, rule));
    }
    out.extend(redundant_do(source));
    out.extend(precondition_rules(source));
    out.extend(single_use_let(source));
    out
}

struct SimpleRule {
    rule: &'static str,
    pattern: &'static str,
    replacement: fn(&regex::Captures<'_>) -> String,
    message: &'static str,
    suggestion: &'static str,
}

fn simple_safe_rule(source: &SourceFile, rule: SimpleRule) -> Vec<Finding> {
    let regex = Regex::new(rule.pattern).expect("valid regex");
    let mut out = Vec::new();
    for (idx, line) in source.lines().iter().enumerate() {
        let line_no = idx + 1;
        let code = strip_comment(line);
        for caps in regex.captures_iter(code) {
            let Some(matched) = caps.get(0) else {
                continue;
            };
            let start = source.line_start_byte(line_no) + matched.start();
            let end = source.line_start_byte(line_no) + matched.end();
            let edit = Edit {
                kind: EditKind::SafeAuto,
                splices: vec![Splice {
                    start_byte: start,
                    end_byte: end,
                    replacement: (rule.replacement)(&caps),
                }],
            };
            out.push(finding(
                source,
                line_no,
                line_no,
                rule.rule,
                Severity::Minor,
                SafetyClass::SafeAuto,
                DetectedBy::Idiom,
                rule.message,
                rule.suggestion,
                None,
                Some(edit),
            ));
        }
    }
    out
}

fn redundant_do(source: &SourceFile) -> Vec<Finding> {
    let regex =
        Regex::new(r"\((when(?:-not)?)\s+([^()\n]+?)\s+\(do\s+(.+?)\)\)").expect("valid regex");
    let mut out = Vec::new();
    for (idx, line) in source.lines().iter().enumerate() {
        let line_no = idx + 1;
        let code = strip_comment(line);
        for caps in regex.captures_iter(code) {
            let Some(matched) = caps.get(0) else {
                continue;
            };
            let body = caps[3].trim();
            if body.is_empty() {
                continue;
            }
            let start = source.line_start_byte(line_no) + matched.start();
            let end = source.line_start_byte(line_no) + matched.end();
            let edit = Edit {
                kind: EditKind::SafeAuto,
                splices: vec![Splice {
                    start_byte: start,
                    end_byte: end,
                    replacement: format!("({} {} {})", &caps[1], caps[2].trim(), body),
                }],
            };
            out.push(finding(
                source,
                line_no,
                line_no,
                "redundant-do",
                Severity::Minor,
                SafetyClass::SafeAuto,
                DetectedBy::Idiom,
                "(when ... (do ...)) uses a redundant do",
                "drop the inner (do ...)",
                None,
                Some(edit),
            ));
        }
    }
    out
}

fn precondition_rules(source: &SourceFile) -> Vec<Finding> {
    let rules = [
        (
            Regex::new(r"\(=\s+\(count\s+([^()]+?)\)\s+0\)").expect("valid regex"),
            "reimpl-empty?",
            "(= (count x) 0) reimplements empty?",
            "use (empty? x) only for finite/countable collections",
        ),
        (
            Regex::new(r"\(>\s+\(count\s+([^()]+?)\)\s+0\)").expect("valid regex"),
            "reimpl-seq",
            "(> (count x) 0) reimplements seq",
            "use (seq x) only for finite/countable collections",
        ),
        (
            Regex::new(r"\(reduce\s+conj\s+\[\]\s").expect("valid regex"),
            "reimpl-vec",
            "(reduce conj [] coll) reimplements vec/into",
            "use (vec coll) or (into [] coll) only for finite collections",
        ),
    ];
    let mut out = Vec::new();
    for (idx, line) in source.lines().iter().enumerate() {
        let line_no = idx + 1;
        let code = strip_comment(line);
        for (regex, rule, message, suggestion) in &rules {
            if regex.is_match(code) {
                out.push(finding(
                    source,
                    line_no,
                    line_no,
                    rule,
                    Severity::Minor,
                    SafetyClass::SafeWithPrecondition,
                    DetectedBy::Idiom,
                    message,
                    suggestion,
                    Some("collection is finite/countable and strictness change is acceptable"),
                    None,
                ));
            }
        }
    }
    out
}

fn single_use_let(source: &SourceFile) -> Vec<Finding> {
    let regex = Regex::new(r"\(let\s+\[\s*([A-Za-z_][\w\-?!*+./<>=]*)\s+([^\]\n]+)\]\s+([^)]+)\)")
        .expect("valid regex");
    let mut out = Vec::new();
    for (idx, line) in source.lines().iter().enumerate() {
        let line_no = idx + 1;
        let code = strip_comment(line);
        for caps in regex.captures_iter(code) {
            let sym = &caps[1];
            let body = &caps[3];
            if count_symbol_uses(body, sym) == 1 {
                out.push(finding(
                    source,
                    line_no,
                    line_no,
                    "single-use-binding",
                    Severity::Minor,
                    SafetyClass::RiskySuggest,
                    DetectedBy::Complexity,
                    &format!("let binding `{sym}` is used only once"),
                    "inline the expression only after semantic review",
                    None,
                    None,
                ));
            }
        }
    }
    out
}

fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut chars = line.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if in_string {
            if ch == '\\' {
                chars.next();
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == ';' {
            return &line[..idx];
        }
    }
    line
}

fn count_symbol_uses(body: &str, sym: &str) -> usize {
    body.match_indices(sym)
        .filter(|(idx, _)| {
            let before = body[..*idx].chars().next_back();
            let after = body[*idx + sym.len()..].chars().next();
            !is_symbol_char(before) && !is_symbol_char(after)
        })
        .count()
}

fn is_symbol_char(ch: Option<char>) -> bool {
    ch.is_some_and(|ch| {
        ch.is_ascii_alphanumeric()
            || matches!(
                ch,
                '_' | '-' | '?' | '!' | '*' | '+' | '.' | '/' | '<' | '>' | '='
            )
    })
}
