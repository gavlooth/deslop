use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use deslop_core::Lang;
use deslop_lang::{LangPack, Registry, detect_lang};
use tree_sitter::{Parser, Tree};

pub use deslop_lang::RegionSpan;

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub lang: Lang,
    pub text: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    pub fn read(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Ok(Self::new(path.to_path_buf(), text))
    }

    pub fn new(path: PathBuf, text: String) -> Self {
        let lang = detect_lang(&path);
        Self::new_with_lang(path, text, lang)
    }

    pub fn new_with_lang(path: PathBuf, text: String, lang: Lang) -> Self {
        let line_starts = line_starts(&text);
        Self {
            path,
            lang,
            text,
            line_starts,
        }
    }

    pub fn lines(&self) -> Vec<&str> {
        self.text.lines().collect()
    }

    pub fn line_start_byte(&self, one_based_line: usize) -> usize {
        self.line_starts
            .get(one_based_line.saturating_sub(1))
            .copied()
            .unwrap_or(self.text.len())
    }

    pub fn line_end_byte(&self, one_based_line: usize) -> usize {
        self.line_starts
            .get(one_based_line)
            .copied()
            .map(|idx| idx.saturating_sub(1))
            .unwrap_or(self.text.len())
    }

    pub fn line_text(&self, one_based_line: usize) -> &str {
        let start = self.line_start_byte(one_based_line);
        let end = self.line_end_byte(one_based_line);
        self.text.get(start..end).unwrap_or("")
    }

    pub fn region_text(&self, start_line: usize, end_line: usize) -> String {
        let start = self.line_start_byte(start_line);
        let end = self
            .line_starts
            .get(end_line)
            .copied()
            .unwrap_or(self.text.len());
        self.text.get(start..end).unwrap_or("").to_string()
    }

    pub fn line_for_byte(&self, byte: usize) -> usize {
        match self.line_starts.binary_search(&byte) {
            Ok(idx) => idx + 1,
            Err(idx) => idx,
        }
        .max(1)
    }

    pub fn enclosing_region_for_span(&self, start_line: usize, end_line: usize) -> RegionSpan {
        let start_byte = self.line_start_byte(start_line);
        let end_byte = self.line_end_byte(end_line).max(start_byte);
        enclosing_region(self.lang, &self.text, start_byte, end_byte).unwrap_or(RegionSpan {
            start_line,
            end_line,
            start_byte,
            end_byte,
        })
    }
}

pub fn parse_tree(lang: Lang, text: &str) -> Result<Option<Tree>> {
    let registry = Registry::default();
    let pack = registry.pack_for_lang(lang);
    let Some(mut parser) = parser_for_pack(pack)? else {
        return Ok(None);
    };
    Ok(parser.parse(text, None))
}

pub fn has_tree_sitter_errors(lang: Lang, text: &str) -> Result<Option<bool>> {
    let Some(tree) = parse_tree(lang, text)? else {
        return Ok(None);
    };
    Ok(Some(tree.root_node().has_error()))
}

pub fn parses_without_errors(lang: Lang, text: &str) -> Result<Option<bool>> {
    Ok(has_tree_sitter_errors(lang, text)?.map(|has_errors| !has_errors))
}

pub fn enclosing_region(
    lang: Lang,
    text: &str,
    start_byte: usize,
    end_byte: usize,
) -> Option<RegionSpan> {
    let registry = Registry::default();
    let pack = registry.pack_for_lang(lang);
    let tree = parse_tree(pack.lang(), text).ok().flatten()?;
    if tree.root_node().has_error() {
        return None;
    }
    let root = tree.root_node();
    let end_byte = end_byte.max(start_byte).min(text.len());
    let node = root.descendant_for_byte_range(start_byte, end_byte)?;
    pack.enclosing_region(node, text)
}

pub fn is_supported_source(path: &Path) -> bool {
    deslop_lang::is_supported_source(path)
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut out = vec![0];
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            out.push(idx + 1);
        }
    }
    out
}

fn parser_for_pack(pack: &dyn LangPack) -> Result<Option<Parser>> {
    let Some(language) = pack.grammar() else {
        return Ok(None);
    };
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .with_context(|| format!("failed to load {} tree-sitter grammar", pack.name()))?;
    Ok(Some(parser))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_clojure_top_level_list_region() {
        let source = SourceFile::new(
            PathBuf::from("sample.clj"),
            "(ns sample)\n\n(defn f [xs]\n  (when xs\n    (= (count xs) 0)))\n\n(defn g [] true)\n"
                .into(),
        );
        let line = 5;
        let region = source.enclosing_region_for_span(line, line);
        assert_eq!(region.start_line, 3);
        assert_eq!(region.end_line, 5);
        assert!(
            source
                .region_text(region.start_line, region.end_line)
                .contains("defn f")
        );
    }

    #[test]
    fn extracts_julia_function_region() {
        let source = SourceFile::new(
            PathBuf::from("sample.jl"),
            "module Demo\n\nfunction f(xs)\n    length(xs) == 0\nend\n\nstruct Box\n    x\nend\nend\n"
                .into(),
        );
        let region = source.enclosing_region_for_span(4, 4);
        assert_eq!(region.start_line, 3);
        assert_eq!(region.end_line, 5);
        assert!(
            source
                .region_text(region.start_line, region.end_line)
                .contains("function f")
        );
    }

    #[test]
    fn extracts_rust_function_region() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "mod demo {\n    fn f(xs: Vec<i32>) -> usize {\n        return xs.len();\n    }\n}\n"
                .into(),
        );
        let region = source.enclosing_region_for_span(3, 3);
        assert_eq!(region.start_line, 2);
        assert_eq!(region.end_line, 4);
        assert!(
            source
                .region_text(region.start_line, region.end_line)
                .contains("fn f")
        );
    }
}
