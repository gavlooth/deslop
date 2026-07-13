use deslop_core::{DetectedBy, Edit, EditKind, Finding, SafetyClass, Severity, Splice};
use regex::Regex;

use crate::{AnalyzerText, finding};

pub(crate) fn findings(source: &AnalyzerText) -> Vec<Finding> {
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

fn simple_safe_rule(source: &AnalyzerText, rule: SimpleRule) -> Vec<Finding> {
    let regex = Regex::new(rule.pattern).expect("valid regex");
    findings_from_captures(source, &regex, |line_no, caps, matched| {
        let edit = safe_auto_edit(source, line_no, matched, (rule.replacement)(caps));
        Some(finding(
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
        ))
    })
}

fn findings_from_captures(
    source: &AnalyzerText,
    regex: &Regex,
    mut build: impl FnMut(usize, &regex::Captures<'_>, regex::Match<'_>) -> Option<Finding>,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for (line_no, code) in code_lines(source) {
        for caps in regex.captures_iter(&code) {
            if let Some(matched) = caps.get(0)
                && let Some(finding) = build(line_no, &caps, matched)
            {
                out.push(finding);
            }
        }
    }
    out
}

fn redundant_do(source: &AnalyzerText) -> Vec<Finding> {
    let regex =
        Regex::new(r"\((when(?:-not)?)\s+([^()\n]+?)\s+\(do\s+(.+?)\)\)").expect("valid regex");
    findings_from_captures(source, &regex, |line_no, caps, matched| {
        let body = caps[3].trim();
        if body.is_empty() {
            return None;
        }
        let replacement = format!("({} {} {})", &caps[1], caps[2].trim(), body);
        let edit = safe_auto_edit(source, line_no, matched, replacement);
        Some(redundant_do_finding(source, line_no, edit))
    })
}

fn precondition_rules(source: &AnalyzerText) -> Vec<Finding> {
    let rules = [
        PreconditionRule::new(
            r"\(=\s+\(count\s+([^()]+?)\)\s+0\)",
            "reimpl-empty?",
            "(= (count x) 0) reimplements empty?",
            "use (empty? x) only for finite/countable collections",
        ),
        PreconditionRule::new(
            r"\(>\s+\(count\s+([^()]+?)\)\s+0\)",
            "reimpl-seq",
            "(> (count x) 0) reimplements seq",
            "use (seq x) only for finite/countable collections",
        ),
        PreconditionRule::new(
            r"\(reduce\s+conj\s+\[\]\s",
            "reimpl-vec",
            "(reduce conj [] coll) reimplements vec/into",
            "use (vec coll) or (into [] coll) only for finite collections",
        ),
    ];
    let mut out = Vec::new();
    for (line_no, code) in code_lines(source) {
        for rule in &rules {
            if rule.regex.is_match(&code) {
                out.push(rule.finding(source, line_no));
            }
        }
    }
    out
}

fn single_use_let(source: &AnalyzerText) -> Vec<Finding> {
    let regex = Regex::new(r"\(let\s+\[\s*([A-Za-z_][\w\-?!*+./<>=]*)\s+([^\]\n]+)\]\s+([^)]+)\)")
        .expect("valid regex");
    let mut out = Vec::new();
    for (line_no, code) in code_lines(source) {
        for caps in regex.captures_iter(&code) {
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

struct PreconditionRule {
    regex: Regex,
    rule: &'static str,
    message: &'static str,
    suggestion: &'static str,
}

impl PreconditionRule {
    fn new(
        pattern: &'static str,
        rule: &'static str,
        message: &'static str,
        suggestion: &'static str,
    ) -> Self {
        Self {
            regex: Regex::new(pattern).expect("valid regex"),
            rule,
            message,
            suggestion,
        }
    }

    fn finding(&self, source: &AnalyzerText, line_no: usize) -> Finding {
        finding(
            source,
            line_no,
            line_no,
            self.rule,
            Severity::Minor,
            SafetyClass::SafeWithPrecondition,
            DetectedBy::Idiom,
            self.message,
            self.suggestion,
            Some("collection is finite/countable and strictness change is acceptable"),
            None,
        )
    }
}

fn code_lines(source: &AnalyzerText) -> Vec<(usize, String)> {
    source
        .lines()
        .iter()
        .enumerate()
        .map(|(idx, line)| (idx + 1, strip_comment(line).to_string()))
        .collect()
}

fn safe_auto_edit(
    source: &AnalyzerText,
    line_no: usize,
    matched: regex::Match<'_>,
    replacement: String,
) -> Edit {
    let start = source.line_start_byte(line_no) + matched.start();
    let end = source.line_start_byte(line_no) + matched.end();
    Edit {
        kind: EditKind::SafeAuto,
        splices: vec![Splice {
            start_byte: start,
            end_byte: end,
            replacement,
        }],
    }
}

fn redundant_do_finding(source: &AnalyzerText, line_no: usize, edit: Edit) -> Finding {
    finding(
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
    )
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
