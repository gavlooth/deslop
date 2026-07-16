//! Frozen six-language canonical compatibility corpus for M10 B1/B6.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use deslop_parse::{
    CanonicalRole, ControlFlowPolicyId, ProjectAnalysis, ProjectSnapshotBuilder, RepositoryId,
    ScopeSpec, lower_control_flow,
};
use serde::{Deserialize, Serialize};

pub const M10_CANONICAL_CORPUS_SCHEMA: &str = "deslop.m10-canonical-corpus/1";
const CASES_PER_LANGUAGE: usize = 100;
const LANGUAGES: [(&str, &str); 6] = [
    ("clojure", "clj"),
    ("javascript", "js"),
    ("julia", "jl"),
    ("python", "py"),
    ("rust", "rs"),
    ("typescript", "ts"),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalCaseEvidence {
    pub id: String,
    pub language: String,
    pub family: String,
    pub path: String,
    pub source_digest: String,
    pub source_bytes: usize,
    pub malformed_or_opaque: bool,
    pub parse_complete: bool,
    pub node_count: usize,
    pub root_span: [usize; 2],
    pub canonical_role_counts: BTreeMap<String, usize>,
    pub containment_ownership_digest: String,
    pub control_graph_count: usize,
    pub control_point_count: usize,
    pub control_edge_count: usize,
    pub control_flow_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalCorpusManifest {
    pub schema: String,
    pub corpus_id: String,
    pub generator: String,
    pub cases_per_language: usize,
    pub total_cases: usize,
    pub cases: Vec<CanonicalCaseEvidence>,
}

impl CanonicalCorpusManifest {
    pub fn validate_shape(&self) -> Result<()> {
        if self.schema != M10_CANONICAL_CORPUS_SCHEMA
            || self.generator != "deslop-m10-six-language-grid/1"
            || self.cases_per_language < 100
            || self.total_cases != self.cases.len()
            || self.total_cases < 600
        {
            bail!("M10 canonical corpus shape is not release-complete");
        }
        let mut counts = BTreeMap::<&str, usize>::new();
        for case in &self.cases {
            *counts.entry(&case.language).or_default() += 1;
        }
        for (language, _) in LANGUAGES {
            if counts.get(language).copied().unwrap_or_default() < 100 {
                bail!("canonical corpus has fewer than 100 {language} cases");
            }
        }
        if self
            .cases
            .windows(2)
            .any(|window| window[0].id >= window[1].id)
        {
            bail!("canonical corpus cases must be sorted and unique");
        }
        let expected = corpus_id(&self.cases)?;
        if self.corpus_id != expected {
            bail!("canonical corpus identity mismatch: expected {expected}");
        }
        Ok(())
    }
}

pub fn assemble_canonical_corpus(root: &Path) -> Result<CanonicalCorpusManifest> {
    let mut by_language = BTreeMap::<String, Vec<GeneratedSource>>::new();
    for source in generated_sources() {
        by_language
            .entry(source.language.clone())
            .or_default()
            .push(source);
    }
    let mut cases = Vec::with_capacity(LANGUAGES.len() * CASES_PER_LANGUAGE);
    for (language, _) in LANGUAGES {
        cases.extend(evaluate_language_sources(
            root,
            language,
            by_language
                .remove(language)
                .expect("generator covers every declared language"),
        )?);
    }
    cases.sort_by(|left, right| left.id.cmp(&right.id));
    let mut manifest = CanonicalCorpusManifest {
        schema: M10_CANONICAL_CORPUS_SCHEMA.into(),
        corpus_id: String::new(),
        generator: "deslop-m10-six-language-grid/1".into(),
        cases_per_language: CASES_PER_LANGUAGE,
        total_cases: cases.len(),
        cases,
    };
    manifest.corpus_id = corpus_id(&manifest.cases)?;
    manifest.validate_shape()?;
    Ok(manifest)
}

fn evaluate_language_sources(
    root: &Path,
    language: &str,
    generated: Vec<GeneratedSource>,
) -> Result<Vec<CanonicalCaseEvidence>> {
    let paths = generated.iter().map(|source| source.path.clone()).collect();
    let mut builder = ProjectSnapshotBuilder::new(
        root,
        RepositoryId::explicit(format!("m10-canonical-corpus:{language}"))?,
    )?
    .with_scope_spec(ScopeSpec::ExactLogicalFiles(paths));
    for source in &generated {
        builder = builder.with_overlay(&source.path, source.source.as_bytes().to_vec())?;
    }
    let analysis = ProjectAnalysis::build(builder.build()?)?;
    let policy =
        ControlFlowPolicyId::from_parts(&[b"m10-canonical-corpus/1", language.as_bytes()])?;
    let lowering = lower_control_flow(Arc::clone(&analysis), policy)?;
    let mut graphs_by_path = BTreeMap::<PathBuf, Vec<_>>::new();
    if let Some(projection) = lowering.projection() {
        for graph in projection.document().graphs() {
            graphs_by_path
                .entry(graph.owner().file().path.clone())
                .or_default()
                .push(graph);
        }
    }

    let mut cases = Vec::with_capacity(generated.len());
    for source in generated {
        let file = analysis
            .file(&source.path)
            .with_context(|| format!("missing canonical file {}", source.path.display()))?;
        let ids = analysis
            .file_node_ids(&source.path)
            .context("canonical file has no node range")?
            .collect::<Vec<_>>();
        let root_node = ids
            .first()
            .copied()
            .context("canonical file has no root node")?;
        let root_span = analysis.node(root_node)?.span();
        let projection = analysis.canonical_role_projection(&source.path)?;
        let mut roles = CanonicalRole::ALL
            .into_iter()
            .map(|role| (role.as_str().to_string(), 0usize))
            .collect::<BTreeMap<_, _>>();
        for fact in projection.facts() {
            for role in fact.roles().iter() {
                *roles.get_mut(role.as_str()).expect("catalog is total") += 1;
            }
        }

        let shape = ids
            .iter()
            .map(|id| {
                let node = analysis
                    .node(*id)
                    .expect("canonical node belongs to analysis");
                serde_json::json!({
                    "kind": node.raw_kind(),
                    "grammar_kind": node.raw_grammar_kind(),
                    "field": node.field(),
                    "span": [node.span().start_byte(), node.span().end_byte()],
                    "parent": node.parent().and_then(|parent| {
                        ids.iter().position(|candidate| *candidate == parent)
                    }),
                    "error": node.is_error(),
                    "missing": node.is_missing(),
                    "has_error": node.has_error(),
                })
            })
            .collect::<Vec<_>>();
        let graphs = graphs_by_path.remove(&source.path).unwrap_or_default();
        let control = graphs
            .iter()
            .map(|graph| {
                serde_json::json!({
                    "owner": graph.owner(),
                    "kind": graph.owner_kind(),
                    "coverage": graph.coverage(),
                    "points": graph.points(),
                    "edges": graph.edges(),
                })
            })
            .collect::<Vec<_>>();
        cases.push(CanonicalCaseEvidence {
            id: source.id,
            language: source.language,
            family: source.family,
            path: source.path.to_string_lossy().into_owned(),
            source_digest: digest("deslop m10 canonical source v1", source.source.as_bytes()),
            source_bytes: source.source.len(),
            malformed_or_opaque: source.malformed_or_opaque,
            parse_complete: file.provenance().permits_rewrites(),
            node_count: ids.len(),
            root_span: [root_span.start_byte(), root_span.end_byte()],
            canonical_role_counts: roles,
            containment_ownership_digest: digest_json(
                "deslop m10 canonical containment v1",
                &shape,
            )?,
            control_graph_count: graphs.len(),
            control_point_count: graphs.iter().map(|graph| graph.points().len()).sum(),
            control_edge_count: graphs.iter().map(|graph| graph.edges().len()).sum(),
            control_flow_digest: digest_json("deslop m10 canonical control v1", &control)?,
        });
    }
    Ok(cases)
}

pub fn verify_canonical_corpus(root: &Path, path: &Path) -> Result<CanonicalCorpusManifest> {
    let stored: CanonicalCorpusManifest = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read canonical corpus {}", path.display()))?,
    )?;
    stored.validate_shape()?;
    let recomputed = assemble_canonical_corpus(root)?;
    if stored != recomputed {
        bail!("frozen canonical corpus disagrees with current adapters/graphs");
    }
    Ok(stored)
}

pub fn write_canonical_corpus(path: &Path, manifest: &CanonicalCorpusManifest) -> Result<()> {
    manifest.validate_shape()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(manifest)?)?;
    Ok(())
}

struct GeneratedSource {
    id: String,
    language: String,
    family: String,
    path: PathBuf,
    source: String,
    malformed_or_opaque: bool,
}

fn generated_sources() -> Vec<GeneratedSource> {
    let mut sources = Vec::with_capacity(LANGUAGES.len() * CASES_PER_LANGUAGE);
    for (language, extension) in LANGUAGES {
        for variant in 0..CASES_PER_LANGUAGE {
            let family_index = variant % 5;
            let family = [
                "callable",
                "branch",
                "loop",
                "call-import",
                "opaque-malformed",
            ][family_index];
            let source = source(language, family_index, variant);
            let payload = format!("{language}\0{family}\0{variant}\0{source}");
            let id = format!(
                "m10c1_{}",
                &digest("deslop m10 canonical case v1", payload.as_bytes())[7..]
            );
            sources.push(GeneratedSource {
                id,
                language: language.into(),
                family: family.into(),
                path: PathBuf::from(format!("{language}/case_{variant:03}.{extension}")),
                source,
                malformed_or_opaque: family_index == 4,
            });
        }
    }
    sources
}

fn source(language: &str, family: usize, variant: usize) -> String {
    match (language, family) {
        ("rust", 0) => format!("pub fn value_{variant}(x: i32) -> i32 {{ x + {variant} }}\n"),
        ("rust", 1) => format!(
            "fn choose_{variant}(x: i32) -> i32 {{ if x > {variant} {{ x }} else {{ {variant} }} }}\n"
        ),
        ("rust", 2) => format!(
            "fn loop_{variant}(mut x: i32) -> i32 {{ while x < {variant} {{ x += 1; }} x }}\n"
        ),
        ("rust", 3) => {
            format!("use std::cmp::max; fn call_{variant}(x: i32) -> i32 {{ max(x, {variant}) }}\n")
        }
        ("rust", 4) => format!(
            "macro_rules! opaque_{variant} {{ ($x:expr) => {{ $x + }} }} fn broken_{variant}() {{ opaque_{variant}!(1);\n"
        ),
        ("python", 0) => format!("def value_{variant}(x):\n    return x + {variant}\n"),
        ("python", 1) => format!(
            "def choose_{variant}(x):\n    if x > {variant}:\n        return x\n    return {variant}\n"
        ),
        ("python", 2) => format!(
            "def loop_{variant}(x):\n    while x < {variant}:\n        x += 1\n    return x\n"
        ),
        ("python", 3) => format!(
            "from math import floor\ndef call_{variant}(x):\n    return floor(x) + {variant}\n"
        ),
        ("python", 4) => {
            format!("def opaque_{variant}(x):\n    return eval(x)\ndef broken_{variant}(:\n")
        }
        ("javascript", 0) => {
            format!("export function value_{variant}(x) {{ return x + {variant}; }}\n")
        }
        ("javascript", 1) => format!(
            "function choose_{variant}(x) {{ if (x > {variant}) {{ return x; }} return {variant}; }}\n"
        ),
        ("javascript", 2) => format!(
            "function loop_{variant}(x) {{ while (x < {variant}) {{ x += 1; }} return x; }}\n"
        ),
        ("javascript", 3) => format!(
            "import {{ max }} from './math.js'; export const call_{variant} = x => max(x, {variant});\n"
        ),
        ("javascript", 4) => format!(
            "export function opaque_{variant}(x) {{ return eval(x); }} function broken_{variant}( {{\n"
        ),
        ("typescript", 0) => format!(
            "export function value_{variant}(x: number): number {{ return x + {variant}; }}\n"
        ),
        ("typescript", 1) => format!(
            "function choose_{variant}(x: number): number {{ if (x > {variant}) {{ return x; }} return {variant}; }}\n"
        ),
        ("typescript", 2) => format!(
            "function loop_{variant}(x: number): number {{ while (x < {variant}) {{ x += 1; }} return x; }}\n"
        ),
        ("typescript", 3) => format!(
            "import {{ max }} from './math'; export const call_{variant} = (x: number): number => max(x, {variant});\n"
        ),
        ("typescript", 4) => format!(
            "export function opaque_{variant}(x: string): unknown {{ return eval(x); }} function broken_{variant}(: {{\n"
        ),
        ("clojure", 0) => {
            format!("(ns corpus.case-{variant})\n(defn value-{variant} [x] (+ x {variant}))\n")
        }
        ("clojure", 1) => format!(
            "(ns corpus.case-{variant})\n(defn choose-{variant} [x] (if (> x {variant}) x {variant}))\n"
        ),
        ("clojure", 2) => format!(
            "(ns corpus.case-{variant})\n(defn loop-{variant} [x] (loop [n x] (if (< n {variant}) (recur (inc n)) n)))\n"
        ),
        ("clojure", 3) => format!(
            "(ns corpus.case-{variant} (:require [clojure.string :as str]))\n(defn call-{variant} [x] (str/trim (str x {variant})))\n"
        ),
        ("clojure", 4) => format!(
            "(ns corpus.case-{variant})\n(defmacro opaque-{variant} [x] `(eval ~x))\n(defn broken-{variant} [x] (+ x\n"
        ),
        ("julia", 0) => format!("module Case{variant}\nvalue_{variant}(x) = x + {variant}\nend\n"),
        ("julia", 1) => format!(
            "function choose_{variant}(x)\n    if x > {variant}\n        return x\n    end\n    return {variant}\nend\n"
        ),
        ("julia", 2) => format!(
            "function loop_{variant}(x)\n    while x < {variant}\n        x += 1\n    end\n    x\nend\n"
        ),
        ("julia", 3) => format!("using Base: max\ncall_{variant}(x) = max(x, {variant})\n"),
        ("julia", 4) => format!(
            "macro opaque_{variant}(x)\n    :(eval($x))\nend\nfunction broken_{variant}(x\n"
        ),
        _ => unreachable!("language/family grid is exhaustive"),
    }
}

fn corpus_id(cases: &[CanonicalCaseEvidence]) -> Result<String> {
    Ok(format!(
        "m10mc1_{}",
        &digest_json("deslop m10 canonical corpus v1", cases)?[7..]
    ))
}

fn digest_json(domain: &str, value: &(impl Serialize + ?Sized)) -> Result<String> {
    Ok(digest(domain, &serde_json::to_vec(value)?))
}

fn digest(domain: &str, bytes: &[u8]) -> String {
    let digest = blake3::derive_key(domain, bytes);
    format!("blake3:{}", blake3::Hash::from_bytes(digest).to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_generator_is_exactly_balanced_and_content_addressed() {
        let root = tempfile::tempdir().unwrap();
        let manifest = assemble_canonical_corpus(root.path()).unwrap();
        manifest.validate_shape().unwrap();
        assert_eq!(manifest.total_cases, 600);
        assert_eq!(
            manifest
                .cases
                .iter()
                .filter(|case| case.malformed_or_opaque)
                .count(),
            120
        );
    }
}
