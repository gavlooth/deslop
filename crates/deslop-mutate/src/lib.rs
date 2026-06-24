use std::collections::BTreeSet;
use std::ops::Range;

use anyhow::Result;
use deslop_core::Lang;
use deslop_parse::{SourceFile, parse_tree};
use tree_sitter::Node;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mutant {
    pub line: usize,
    pub byte_span: Range<usize>,
    pub operator: &'static str,
    pub original: String,
    pub mutated: String,
    pub mutated_source: String,
}

pub trait MutationOperator: Send + Sync {
    fn name(&self) -> &'static str;
    fn replacement(&self, token: &str) -> Option<&'static str>;
}

#[derive(Debug, Clone, Copy)]
struct TokenSwap {
    name: &'static str,
    pairs: &'static [(&'static str, &'static str)],
}

impl MutationOperator for TokenSwap {
    fn name(&self) -> &'static str {
        self.name
    }

    fn replacement(&self, token: &str) -> Option<&'static str> {
        self.pairs.iter().find_map(|(left, right)| {
            if token == *left {
                Some(*right)
            } else if token == *right {
                Some(*left)
            } else {
                None
            }
        })
    }
}

trait MutationPack: Send + Sync {
    fn lang(&self) -> Lang;
    fn infix_expression_kinds(&self) -> &'static [&'static str];
    fn prefix_call_kind(&self) -> Option<&'static str> {
        None
    }
    fn condition_kinds(&self) -> &'static [&'static str];
    fn condition_prefix(&self) -> &'static str;
    fn condition_suffix(&self) -> &'static str;
    fn boolean_replacement(&self, token: &str) -> Option<&'static str>;
    fn operators(&self) -> &'static [&'static dyn MutationOperator];
}

struct MutationRegistry {
    packs: Vec<&'static dyn MutationPack>,
}

impl MutationRegistry {
    fn default() -> Self {
        Self {
            packs: vec![
                &CLOJURE_MUTATION_PACK,
                &JULIA_MUTATION_PACK,
                &PYTHON_MUTATION_PACK,
                &RUST_MUTATION_PACK,
            ],
        }
    }

    fn pack_for_lang(&self, lang: Lang) -> Option<&'static dyn MutationPack> {
        self.packs.iter().copied().find(|pack| pack.lang() == lang)
    }
}

const RELATIONAL_INFIX: TokenSwap = TokenSwap {
    name: "relational-swap",
    pairs: &[("<", "<="), (">", ">="), ("==", "!=")],
};
const RELATIONAL_CLOJURE: TokenSwap = TokenSwap {
    name: "relational-swap",
    pairs: &[("<", "<="), (">", ">="), ("=", "not=")],
};
const ARITHMETIC: TokenSwap = TokenSwap {
    name: "arithmetic-swap",
    pairs: &[("+", "-"), ("*", "/")],
};
const LOGICAL_SYMBOLIC: TokenSwap = TokenSwap {
    name: "logical-swap",
    pairs: &[("&&", "||")],
};
const LOGICAL_PYTHON: TokenSwap = TokenSwap {
    name: "logical-swap",
    pairs: &[("and", "or")],
};
const LOGICAL_CLOJURE: TokenSwap = TokenSwap {
    name: "logical-swap",
    pairs: &[("and", "or")],
};

static INFIX_OPERATORS: [&dyn MutationOperator; 3] =
    [&RELATIONAL_INFIX, &ARITHMETIC, &LOGICAL_SYMBOLIC];
static PYTHON_OPERATORS: [&dyn MutationOperator; 3] =
    [&RELATIONAL_INFIX, &ARITHMETIC, &LOGICAL_PYTHON];
static CLOJURE_OPERATORS: [&dyn MutationOperator; 3] =
    [&RELATIONAL_CLOJURE, &ARITHMETIC, &LOGICAL_CLOJURE];

struct ClojureMutationPack;
struct JuliaMutationPack;
struct PythonMutationPack;
struct RustMutationPack;

static CLOJURE_MUTATION_PACK: ClojureMutationPack = ClojureMutationPack;
static JULIA_MUTATION_PACK: JuliaMutationPack = JuliaMutationPack;
static PYTHON_MUTATION_PACK: PythonMutationPack = PythonMutationPack;
static RUST_MUTATION_PACK: RustMutationPack = RustMutationPack;

impl MutationPack for ClojureMutationPack {
    fn lang(&self) -> Lang {
        Lang::Clojure
    }

    fn infix_expression_kinds(&self) -> &'static [&'static str] {
        &[]
    }

    fn prefix_call_kind(&self) -> Option<&'static str> {
        Some("list_lit")
    }

    fn condition_kinds(&self) -> &'static [&'static str] {
        &["list_lit"]
    }

    fn condition_prefix(&self) -> &'static str {
        "(not "
    }

    fn condition_suffix(&self) -> &'static str {
        ")"
    }

    fn boolean_replacement(&self, token: &str) -> Option<&'static str> {
        match token {
            "true" => Some("false"),
            "false" => Some("true"),
            _ => None,
        }
    }

    fn operators(&self) -> &'static [&'static dyn MutationOperator] {
        &CLOJURE_OPERATORS
    }
}

impl MutationPack for JuliaMutationPack {
    fn lang(&self) -> Lang {
        Lang::Julia
    }

    fn infix_expression_kinds(&self) -> &'static [&'static str] {
        &["binary_expression"]
    }

    fn condition_kinds(&self) -> &'static [&'static str] {
        &["if_statement", "elseif_clause", "while_statement"]
    }

    fn condition_prefix(&self) -> &'static str {
        "!("
    }

    fn condition_suffix(&self) -> &'static str {
        ")"
    }

    fn boolean_replacement(&self, token: &str) -> Option<&'static str> {
        match token {
            "true" => Some("false"),
            "false" => Some("true"),
            _ => None,
        }
    }

    fn operators(&self) -> &'static [&'static dyn MutationOperator] {
        &INFIX_OPERATORS
    }
}

impl MutationPack for PythonMutationPack {
    fn lang(&self) -> Lang {
        Lang::Python
    }

    fn infix_expression_kinds(&self) -> &'static [&'static str] {
        &["binary_operator", "boolean_operator", "comparison_operator"]
    }

    fn condition_kinds(&self) -> &'static [&'static str] {
        &["if_statement", "elif_clause", "while_statement"]
    }

    fn condition_prefix(&self) -> &'static str {
        "not ("
    }

    fn condition_suffix(&self) -> &'static str {
        ")"
    }

    fn boolean_replacement(&self, token: &str) -> Option<&'static str> {
        match token {
            "True" => Some("False"),
            "False" => Some("True"),
            "true" => Some("false"),
            "false" => Some("true"),
            _ => None,
        }
    }

    fn operators(&self) -> &'static [&'static dyn MutationOperator] {
        &PYTHON_OPERATORS
    }
}

impl MutationPack for RustMutationPack {
    fn lang(&self) -> Lang {
        Lang::Rust
    }

    fn infix_expression_kinds(&self) -> &'static [&'static str] {
        &["binary_expression"]
    }

    fn condition_kinds(&self) -> &'static [&'static str] {
        &["if_expression", "while_expression"]
    }

    fn condition_prefix(&self) -> &'static str {
        "!("
    }

    fn condition_suffix(&self) -> &'static str {
        ")"
    }

    fn boolean_replacement(&self, token: &str) -> Option<&'static str> {
        match token {
            "true" => Some("false"),
            "false" => Some("true"),
            _ => None,
        }
    }

    fn operators(&self) -> &'static [&'static dyn MutationOperator] {
        &INFIX_OPERATORS
    }
}

pub fn generate_mutants(
    source: &SourceFile,
    restrict_lines: Option<&BTreeSet<usize>>,
) -> Result<Vec<Mutant>> {
    let registry = MutationRegistry::default();
    let Some(pack) = registry.pack_for_lang(source.lang) else {
        return Ok(Vec::new());
    };
    let Some(tree) = parse_tree(source.lang, &source.text)? else {
        return Ok(Vec::new());
    };
    if tree.root_node().has_error() {
        return Ok(Vec::new());
    }
    let mut mutants = Vec::new();
    collect_mutants(
        tree.root_node(),
        &source.text,
        pack,
        restrict_lines,
        &mut mutants,
    );
    mutants.sort_by(|left, right| {
        left.byte_span
            .start
            .cmp(&right.byte_span.start)
            .then(left.byte_span.end.cmp(&right.byte_span.end))
            .then(left.operator.cmp(right.operator))
            .then(left.mutated.cmp(&right.mutated))
    });
    mutants.dedup_by(|left, right| {
        left.byte_span == right.byte_span
            && left.operator == right.operator
            && left.mutated == right.mutated
    });
    Ok(mutants)
}

fn collect_mutants(
    node: Node<'_>,
    text: &str,
    pack: &'static dyn MutationPack,
    restrict_lines: Option<&BTreeSet<usize>>,
    mutants: &mut Vec<Mutant>,
) {
    collect_infix_mutants(node, text, pack, restrict_lines, mutants);
    collect_prefix_mutants(node, text, pack, restrict_lines, mutants);
    collect_boolean_mutant(node, text, pack, restrict_lines, mutants);
    collect_condition_negation_mutant(node, text, pack, restrict_lines, mutants);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_mutants(child, text, pack, restrict_lines, mutants);
    }
}

fn collect_infix_mutants(
    node: Node<'_>,
    text: &str,
    pack: &'static dyn MutationPack,
    restrict_lines: Option<&BTreeSet<usize>>,
    mutants: &mut Vec<Mutant>,
) {
    if !pack.infix_expression_kinds().contains(&node.kind()) {
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let token = node_text(child, text);
        if token.is_empty() || child.is_named() && child.kind() != "operator" {
            continue;
        }
        push_operator_mutants(child, text, token, pack, restrict_lines, mutants);
    }
}

fn collect_prefix_mutants(
    node: Node<'_>,
    text: &str,
    pack: &'static dyn MutationPack,
    restrict_lines: Option<&BTreeSet<usize>>,
    mutants: &mut Vec<Mutant>,
) {
    if pack.prefix_call_kind() != Some(node.kind()) {
        return;
    }
    let Some(operator_node) = first_named_child(node) else {
        return;
    };
    let token = node_text(operator_node, text);
    push_operator_mutants(operator_node, text, token, pack, restrict_lines, mutants);
}

fn push_operator_mutants(
    node: Node<'_>,
    text: &str,
    token: &str,
    pack: &'static dyn MutationPack,
    restrict_lines: Option<&BTreeSet<usize>>,
    mutants: &mut Vec<Mutant>,
) {
    for operator in pack.operators() {
        if let Some(replacement) = operator.replacement(token) {
            push_mutant(
                text,
                node.start_byte()..node.end_byte(),
                operator.name(),
                token,
                replacement,
                restrict_lines,
                mutants,
            );
        }
    }
}

fn collect_boolean_mutant(
    node: Node<'_>,
    text: &str,
    pack: &'static dyn MutationPack,
    restrict_lines: Option<&BTreeSet<usize>>,
    mutants: &mut Vec<Mutant>,
) {
    let token = node_text(node, text);
    let Some(replacement) = pack.boolean_replacement(token) else {
        return;
    };
    push_mutant(
        text,
        node.start_byte()..node.end_byte(),
        "boolean-flip",
        token,
        replacement,
        restrict_lines,
        mutants,
    );
}

fn collect_condition_negation_mutant(
    node: Node<'_>,
    text: &str,
    pack: &'static dyn MutationPack,
    restrict_lines: Option<&BTreeSet<usize>>,
    mutants: &mut Vec<Mutant>,
) {
    if !pack.condition_kinds().contains(&node.kind()) {
        return;
    }
    let Some(condition) = condition_node(node, text, pack.lang()) else {
        return;
    };
    let original = node_text(condition, text);
    if original.is_empty() || is_already_negated(original, pack) {
        return;
    }
    let mutated = format!(
        "{}{}{}",
        pack.condition_prefix(),
        original,
        pack.condition_suffix()
    );
    push_mutant(
        text,
        condition.start_byte()..condition.end_byte(),
        "condition-negation",
        original,
        &mutated,
        restrict_lines,
        mutants,
    );
}

fn condition_node<'tree>(node: Node<'tree>, text: &str, lang: Lang) -> Option<Node<'tree>> {
    match lang {
        Lang::Clojure => clojure_condition_node(node, text),
        _ => first_named_child_after_keyword(node),
    }
}

fn clojure_condition_node<'tree>(node: Node<'tree>, text: &str) -> Option<Node<'tree>> {
    if node.kind() != "list_lit" {
        return None;
    }
    let named = named_children(node);
    let mut named = named.into_iter();
    let head = named.next()?;
    if !matches!(node_text(head, text), "if" | "when" | "while") {
        return None;
    }
    named.next()
}

fn first_named_child_after_keyword(node: Node<'_>) -> Option<Node<'_>> {
    first_named_child(node)
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.is_named())
        .collect()
}

fn first_named_child(node: Node<'_>) -> Option<Node<'_>> {
    named_children(node).into_iter().next()
}

fn push_mutant(
    text: &str,
    span: Range<usize>,
    operator: &'static str,
    original: &str,
    mutated: &str,
    restrict_lines: Option<&BTreeSet<usize>>,
    mutants: &mut Vec<Mutant>,
) {
    let line = line_for_byte(text, span.start);
    if restrict_lines.is_some_and(|lines| !lines.contains(&line)) {
        return;
    }
    let Some(mutated_source) = replace_range(text, span.clone(), mutated) else {
        return;
    };
    mutants.push(Mutant {
        line,
        byte_span: span,
        operator,
        original: original.to_string(),
        mutated: mutated.to_string(),
        mutated_source,
    });
}

fn replace_range(text: &str, span: Range<usize>, replacement: &str) -> Option<String> {
    if span.start > span.end || span.end > text.len() {
        return None;
    }
    let mut out = text.to_string();
    out.replace_range(span, replacement);
    Some(out)
}

fn node_text<'a>(node: Node<'_>, text: &'a str) -> &'a str {
    text.get(node.start_byte()..node.end_byte()).unwrap_or("")
}

fn is_already_negated(original: &str, pack: &dyn MutationPack) -> bool {
    let trimmed = original.trim_start();
    trimmed.starts_with(pack.condition_prefix())
        || trimmed.starts_with("!")
        || trimmed.starts_with("not ")
        || trimmed.starts_with("(not ")
}

fn line_for_byte(text: &str, byte: usize) -> usize {
    let mut line = 1;
    for (idx, ch) in text.char_indices() {
        if idx >= byte {
            break;
        }
        if ch == '\n' {
            line += 1;
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mutated_texts(path: &str, text: &str) -> Vec<(usize, &'static str, String, String)> {
        let source = SourceFile::new(PathBuf::from(path), text.to_string());
        generate_mutants(&source, None)
            .expect("mutants")
            .into_iter()
            .map(|mutant| {
                (
                    mutant.line,
                    mutant.operator,
                    mutant.original,
                    mutant.mutated,
                )
            })
            .collect()
    }

    #[test]
    fn rust_generates_exact_portable_mutants() {
        let mutants = mutated_texts(
            "sample.rs",
            "fn f(a: i32, b: i32) -> bool {\n    if a < b && true { a + b } else { a * b } == 0\n}\n",
        );
        assert_eq!(
            mutants,
            vec![
                (
                    2,
                    "condition-negation",
                    "a < b && true".into(),
                    "!(a < b && true)".into()
                ),
                (2, "relational-swap", "<".into(), "<=".into()),
                (2, "logical-swap", "&&".into(), "||".into()),
                (2, "boolean-flip", "true".into(), "false".into()),
                (2, "arithmetic-swap", "+".into(), "-".into()),
                (2, "arithmetic-swap", "*".into(), "/".into()),
                (2, "relational-swap", "==".into(), "!=".into()),
            ]
        );
    }

    #[test]
    fn clojure_generates_exact_prefix_mutants() {
        let mutants = mutated_texts(
            "sample.clj",
            "(defn f [a b]\n  (if (and (< a b) true) (+ a b) (* a b)))\n",
        );
        assert_eq!(
            mutants,
            vec![
                (
                    2,
                    "condition-negation",
                    "(and (< a b) true)".into(),
                    "(not (and (< a b) true))".into()
                ),
                (2, "logical-swap", "and".into(), "or".into()),
                (2, "relational-swap", "<".into(), "<=".into()),
                (2, "boolean-flip", "true".into(), "false".into()),
                (2, "arithmetic-swap", "+".into(), "-".into()),
                (2, "arithmetic-swap", "*".into(), "/".into()),
            ]
        );
    }

    #[test]
    fn julia_generates_exact_portable_mutants() {
        let mutants = mutated_texts(
            "sample.jl",
            "function f(a, b)\n    if a < b && true\n        a + b\n    else\n        a * b\n    end\nend\n",
        );
        assert_eq!(
            mutants,
            vec![
                (
                    2,
                    "condition-negation",
                    "a < b && true".into(),
                    "!(a < b && true)".into()
                ),
                (2, "relational-swap", "<".into(), "<=".into()),
                (2, "logical-swap", "&&".into(), "||".into()),
                (2, "boolean-flip", "true".into(), "false".into()),
                (3, "arithmetic-swap", "+".into(), "-".into()),
                (5, "arithmetic-swap", "*".into(), "/".into()),
            ]
        );
    }

    #[test]
    fn python_generates_exact_portable_mutants() {
        let mutants = mutated_texts(
            "sample.py",
            "def f(a, b):\n    if a < b and True:\n        return a + b\n    return a * b == 0\n",
        );
        assert_eq!(
            mutants,
            vec![
                (
                    2,
                    "condition-negation",
                    "a < b and True".into(),
                    "not (a < b and True)".into()
                ),
                (2, "relational-swap", "<".into(), "<=".into()),
                (2, "logical-swap", "and".into(), "or".into()),
                (2, "boolean-flip", "True".into(), "False".into()),
                (3, "arithmetic-swap", "+".into(), "-".into()),
                (4, "arithmetic-swap", "*".into(), "/".into()),
                (4, "relational-swap", "==".into(), "!=".into()),
            ]
        );
    }

    #[test]
    fn restrict_lines_filters_mutants() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn f(a: i32, b: i32) -> i32 {\n    let _ = a + b;\n    a * b\n}\n".to_string(),
        );
        let restrict_lines = BTreeSet::from([3]);
        let mutants = generate_mutants(&source, Some(&restrict_lines)).expect("mutants");
        assert_eq!(mutants.len(), 1);
        assert_eq!(mutants[0].line, 3);
        assert_eq!(mutants[0].original, "*");
        assert_eq!(mutants[0].mutated, "/");
    }

    #[test]
    fn mutant_contains_mutated_source() {
        let source = SourceFile::new(
            PathBuf::from("sample.clj"),
            "(defn f [a b] (< a b))\n".to_string(),
        );
        let mutants = generate_mutants(&source, None).expect("mutants");
        assert_eq!(mutants[0].mutated_source, "(defn f [a b] (<= a b))\n");
    }
}
