use std::path::Path;

use anyhow::Result;
use deslop_core::Lang;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionSpan {
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionClass {
    Behavioral,
    Declaration,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailPositionClass {
    Return,
    FunctionBody,
    Other,
}

pub trait LangPack: Send + Sync {
    fn name(&self) -> &'static str;
    fn lang(&self) -> Lang;
    fn extensions(&self) -> &'static [&'static str];
    fn grammar(&self) -> Option<tree_sitter::Language>;
    fn line_comments(&self) -> &'static [&'static str];
    fn metrics_regions(&self) -> &'static [&'static str];
    fn metrics_branches(&self) -> &'static [&'static str];
    fn metrics_nesting(&self) -> &'static [&'static str];
    fn metrics_flow_breaks(&self) -> &'static [&'static str];
    fn halstead_operator_tokens(&self) -> &'static [&'static str];
    fn region_class(&self, _node: Node<'_>, _text: &str) -> RegionClass {
        RegionClass::Other
    }
    fn is_long_method_region(&self, _node: Node<'_>, _text: &str) -> bool {
        false
    }
    fn is_duplication_data_region(&self, _node: Node<'_>, _text: &str) -> bool {
        false
    }
    fn tail_position_class(&self, _node: Node<'_>, _text: &str) -> TailPositionClass {
        TailPositionClass::Other
    }

    fn enclosing_region(&self, node: Node<'_>, text: &str) -> Option<RegionSpan>;

    fn detect(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| self.extensions().contains(&extension))
    }
}

pub trait Rule<Source, Config, Output>: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, source: &Source, config: &Config) -> Vec<Output>;
}

#[derive(Debug, Clone)]
pub enum ExternalFindings<Output> {
    Available(Vec<Output>),
    Unavailable { notice: String },
}

pub trait ExternalAnalyzer<Source, Output>: Send + Sync {
    fn name(&self) -> &'static str;
    fn covered_rules(&self) -> &'static [&'static str];
    fn analyze(&self, path: &Path, source: &Source) -> Result<ExternalFindings<Output>>;
}

pub struct Registry {
    packs: Vec<&'static dyn LangPack>,
    generic: &'static dyn LangPack,
}

impl Registry {
    pub fn new(generic: &'static dyn LangPack) -> Self {
        Self {
            packs: Vec::new(),
            generic,
        }
    }

    pub fn with_default_packs() -> Self {
        let mut registry = Self::new(&GENERIC_PACK);
        registry.register(&CLOJURE_PACK);
        registry.register(&JULIA_PACK);
        registry.register(&RUST_PACK);
        registry
    }

    pub fn register(&mut self, pack: &'static dyn LangPack) {
        self.packs.push(pack);
    }

    pub fn pack_for_path(&self, path: &Path) -> &'static dyn LangPack {
        self.packs
            .iter()
            .copied()
            .find(|pack| pack.detect(path))
            .unwrap_or(self.generic)
    }

    pub fn pack_for_lang(&self, lang: Lang) -> &'static dyn LangPack {
        self.packs
            .iter()
            .copied()
            .find(|pack| pack.lang() == lang)
            .unwrap_or(self.generic)
    }

    pub fn supported_pack_for_path(&self, path: &Path) -> Option<&'static dyn LangPack> {
        self.packs.iter().copied().find(|pack| pack.detect(path))
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::with_default_packs()
    }
}

pub fn detect_lang(path: &Path) -> Lang {
    Registry::default().pack_for_path(path).lang()
}

pub fn is_supported_source(path: &Path) -> bool {
    Registry::default().supported_pack_for_path(path).is_some()
}

pub static GENERIC_PACK: GenericPack = GenericPack;
pub static CLOJURE_PACK: ClojurePack = ClojurePack;
pub static JULIA_PACK: JuliaPack = JuliaPack;
pub static RUST_PACK: RustPack = RustPack;

pub struct GenericPack;
pub struct ClojurePack;
pub struct JuliaPack;
pub struct RustPack;

impl LangPack for GenericPack {
    fn name(&self) -> &'static str {
        "generic"
    }

    fn lang(&self) -> Lang {
        Lang::Generic
    }

    fn extensions(&self) -> &'static [&'static str] {
        &[]
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
        &[
            "=", "+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">=", "&&", "||", "!",
        ]
    }

    fn enclosing_region(&self, _node: Node<'_>, _text: &str) -> Option<RegionSpan> {
        None
    }
}

impl LangPack for ClojurePack {
    fn name(&self) -> &'static str {
        "clojure"
    }

    fn lang(&self) -> Lang {
        Lang::Clojure
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["clj", "cljs", "cljc", "edn"]
    }

    fn grammar(&self) -> Option<tree_sitter::Language> {
        Some(tree_sitter_clojure::LANGUAGE.into())
    }

    fn line_comments(&self) -> &'static [&'static str] {
        &[";"]
    }

    fn metrics_regions(&self) -> &'static [&'static str] {
        &["list_lit"]
    }

    fn metrics_branches(&self) -> &'static [&'static str] {
        &[
            "if", "when", "cond", "case", "for", "doseq", "loop", "recur",
        ]
    }

    fn metrics_nesting(&self) -> &'static [&'static str] {
        &["list_lit"]
    }

    fn metrics_flow_breaks(&self) -> &'static [&'static str] {
        &["throw", "recur"]
    }

    fn halstead_operator_tokens(&self) -> &'static [&'static str] {
        &[
            "defn", "fn", "let", "if", "when", "cond", "case", "for", "doseq", "loop", "recur",
            "=", "not=", "+", "-", "*", "/", ">", "<", ">=", "<=",
        ]
    }

    fn region_class(&self, node: Node<'_>, text: &str) -> RegionClass {
        if node.kind() != "list_lit" {
            return RegionClass::Other;
        }
        match node_head_token(node, text) {
            Some("defn" | "fn") => RegionClass::Behavioral,
            Some(
                "ns" | "require" | "import" | "def" | "defrecord" | "deftype" | "defprotocol"
                | "definterface" | "defmulti" | "defmethod",
            ) => RegionClass::Declaration,
            _ => RegionClass::Other,
        }
    }

    fn is_long_method_region(&self, node: Node<'_>, text: &str) -> bool {
        self.region_class(node, text) == RegionClass::Behavioral
    }

    fn is_duplication_data_region(&self, node: Node<'_>, _text: &str) -> bool {
        matches!(node.kind(), "map_lit" | "set_lit")
    }

    fn enclosing_region(&self, node: Node<'_>, text: &str) -> Option<RegionSpan> {
        top_level_clojure_list(node, text)
    }
}

impl LangPack for JuliaPack {
    fn name(&self) -> &'static str {
        "julia"
    }

    fn lang(&self) -> Lang {
        Lang::Julia
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["jl"]
    }

    fn grammar(&self) -> Option<tree_sitter::Language> {
        Some(tree_sitter_julia::LANGUAGE.into())
    }

    fn line_comments(&self) -> &'static [&'static str] {
        &["#"]
    }

    fn metrics_regions(&self) -> &'static [&'static str] {
        &[
            "function_definition",
            "struct_definition",
            "module_definition",
        ]
    }

    fn metrics_branches(&self) -> &'static [&'static str] {
        &[
            "if_statement",
            "elseif_clause",
            "for_statement",
            "while_statement",
            "try_statement",
            "catch_clause",
        ]
    }

    fn metrics_nesting(&self) -> &'static [&'static str] {
        &[
            "if_statement",
            "for_statement",
            "while_statement",
            "try_statement",
            "function_definition",
        ]
    }

    fn metrics_flow_breaks(&self) -> &'static [&'static str] {
        &["return_statement", "break_statement", "continue_statement"]
    }

    fn halstead_operator_tokens(&self) -> &'static [&'static str] {
        &[
            "=", "+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">=", "&&", "||", "!", "if",
            "elseif", "else", "for", "while", "return", "break", "continue",
        ]
    }

    fn region_class(&self, node: Node<'_>, text: &str) -> RegionClass {
        match node.kind() {
            "function_definition" | "do_clause" => RegionClass::Behavioral,
            "struct_definition" => RegionClass::Declaration,
            _ => match node_head_token(node, text) {
                Some("using" | "import" | "const") => RegionClass::Declaration,
                _ => RegionClass::Other,
            },
        }
    }

    fn is_long_method_region(&self, node: Node<'_>, text: &str) -> bool {
        self.region_class(node, text) == RegionClass::Behavioral
    }

    fn is_duplication_data_region(&self, node: Node<'_>, _text: &str) -> bool {
        matches!(
            node.kind(),
            "vect_expression" | "matrix_expression" | "tuple_expression"
        )
    }

    fn enclosing_region(&self, node: Node<'_>, text: &str) -> Option<RegionSpan> {
        enclosing_julia_block(node, text)
    }
}

impl LangPack for RustPack {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn lang(&self) -> Lang {
        Lang::Rust
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn grammar(&self) -> Option<tree_sitter::Language> {
        Some(tree_sitter_rust::LANGUAGE.into())
    }

    fn line_comments(&self) -> &'static [&'static str] {
        &["//"]
    }

    fn metrics_regions(&self) -> &'static [&'static str] {
        &["function_item", "impl_item"]
    }

    fn metrics_branches(&self) -> &'static [&'static str] {
        &[
            "if_expression",
            "match_arm",
            "while_expression",
            "for_expression",
            "loop_expression",
        ]
    }

    fn metrics_nesting(&self) -> &'static [&'static str] {
        &[
            "if_expression",
            "match_expression",
            "while_expression",
            "for_expression",
            "loop_expression",
        ]
    }

    fn metrics_flow_breaks(&self) -> &'static [&'static str] {
        &[
            "return_expression",
            "break_expression",
            "continue_expression",
        ]
    }

    fn halstead_operator_tokens(&self) -> &'static [&'static str] {
        &[
            "=", "+", "-", "*", "/", "%", "==", "!=", "<", ">", "<=", ">=", "&&", "||", "!", "&",
            "|", "^", "<<", ">>", "+=", "-=", "*=", "/=", "%=", "if", "else", "match", "for",
            "while", "loop", "return", "break", "continue", "let",
        ]
    }

    fn region_class(&self, node: Node<'_>, _text: &str) -> RegionClass {
        match node.kind() {
            "block" => RegionClass::Behavioral,
            "attribute_item"
            | "enum_item"
            | "field_declaration"
            | "field_declaration_list"
            | "struct_item"
            | "trait_item"
            | "use_declaration" => RegionClass::Declaration,
            _ => RegionClass::Other,
        }
    }

    fn is_long_method_region(&self, node: Node<'_>, _text: &str) -> bool {
        node.kind() == "function_item"
    }

    fn is_duplication_data_region(&self, node: Node<'_>, _text: &str) -> bool {
        matches!(
            node.kind(),
            "array_expression" | "struct_expression" | "field_initializer_list"
        ) || is_rust_data_macro_token_tree(node, _text)
    }

    fn tail_position_class(&self, node: Node<'_>, _text: &str) -> TailPositionClass {
        match node.kind() {
            "return_expression" => TailPositionClass::Return,
            "block"
                if node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "function_item") =>
            {
                TailPositionClass::FunctionBody
            }
            _ => TailPositionClass::Other,
        }
    }

    fn enclosing_region(&self, node: Node<'_>, text: &str) -> Option<RegionSpan> {
        enclosing_rust_item(node, text)
    }
}

fn top_level_clojure_list(mut node: Node<'_>, text: &str) -> Option<RegionSpan> {
    let mut best = None;
    loop {
        if node.kind() == "list_lit" {
            best = Some(node);
        }
        let Some(parent) = node.parent() else {
            break;
        };
        if parent.kind() == "source" {
            break;
        }
        node = parent;
    }
    best.map(|node| region_from_node(node, text))
}

fn enclosing_julia_block(mut node: Node<'_>, text: &str) -> Option<RegionSpan> {
    loop {
        if matches!(
            node.kind(),
            "function_definition" | "struct_definition" | "module_definition"
        ) {
            return Some(region_from_node(node, text));
        }
        let parent = node.parent()?;
        node = parent;
    }
}

fn enclosing_rust_item(mut node: Node<'_>, text: &str) -> Option<RegionSpan> {
    loop {
        if matches!(node.kind(), "function_item" | "impl_item" | "mod_item") {
            return Some(region_from_node(node, text));
        }
        let parent = node.parent()?;
        node = parent;
    }
}

fn is_rust_data_macro_token_tree(node: Node<'_>, text: &str) -> bool {
    if node.kind() != "token_tree" {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != "macro_invocation" {
        return false;
    }
    let Some(invocation) = text.get(parent.start_byte()..parent.end_byte()) else {
        return false;
    };
    let invocation = invocation.trim_start();
    invocation.starts_with("json!") || invocation.starts_with("vec!")
}

fn node_head_token<'a>(node: Node<'_>, text: &'a str) -> Option<&'a str> {
    let slice = text.get(node.start_byte()..node.end_byte())?;
    let trimmed = slice.trim_start_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '#' | '\'' | '`')
    });
    let end = trimmed
        .char_indices()
        .find_map(|(idx, ch)| (!is_head_continue(ch)).then_some(idx))
        .unwrap_or(trimmed.len());
    (!trimmed[..end].is_empty()).then_some(&trimmed[..end])
}

fn is_head_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || matches!(
            ch,
            '_' | '-' | '?' | '!' | '*' | '+' | '/' | '<' | '>' | '=' | '.'
        )
}

fn region_from_node(node: Node<'_>, text: &str) -> RegionSpan {
    let start_position = node.start_position();
    let end_position = node.end_position();
    let mut end_line = end_position.row + 1;
    if end_position.column == 0 && end_line > start_position.row + 1 {
        end_line -= 1;
    }
    RegionSpan {
        start_line: start_position.row + 1,
        end_line,
        start_byte: node.start_byte(),
        end_byte: node.end_byte().min(text.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_detects_pack_by_extension() {
        let registry = Registry::default();
        assert_eq!(
            registry.pack_for_path(Path::new("sample.rs")).lang(),
            Lang::Rust
        );
        assert_eq!(
            registry.pack_for_path(Path::new("sample.unknown")).lang(),
            Lang::Generic
        );
    }
}
