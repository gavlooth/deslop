//! Contract extraction and change history for refactor-defect detection.
//!
//! Design: `docs/REFACTOR_DEFECT_ACCUMULATION.md`. A [`ContractChangeHistory`]
//! is built from exact per-revision [`ProjectAnalysis`] snapshots and records,
//! per file, the functions (candidate owners/consumers) and the contract
//! tokens they reference: callee/attribute references (owner tokens) and
//! string-literal contents (schema tokens).
//!
//! This is the sibling of [`crate::ModuleChangeHistory`]: module history
//! tracks co-change of dependency nodes, while contract history retains the
//! owner/reference facts the refactor-defect detectors compare across
//! revisions. Detector logic lives in `deslop-analyzer`; this module only
//! extracts revision-pinned facts with explicit coverage.
//!
//! Contract query text currently lives beside the extractor and runs through
//! [`ProjectAnalysis::compile_syntax_query`] against the exact grammar of
//! each file. Languages without a contract query are coverage gaps, never
//! silent absences. Later phases may migrate the text into `deslop-lang`
//! query packs once a detector needs captures these queries cannot express.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use deslop_core::Span;

use crate::{FactCoverage, NodeId, ProjectAnalysis};

/// Wire schema identifier for a contract change history.
pub const CONTRACT_CHANGE_HISTORY_SCHEMA: &str = "deslop.contract-change-history/1";

/// Python contract query: function definitions (owner/consumer candidates),
/// call targets (owner tokens), string literals (schema tokens), and
/// config-key reads (environment accessors carrying a string key).
/// `@config.object`/`@config.accessor` are post-filtered in Rust
/// (see [`is_config_object`]/[`is_config_accessor`]); the query cannot express
/// the name match. Only environment reads count as behavioral config reads:
/// a `config["K"]` subscript cannot be told apart from a write without
/// semantic facts, so config-object literals stay acceptance-surface tokens.
const PYTHON_CONTRACT_QUERY: &str = concat!(
    "(function_definition\n",
    "  name: (identifier) @function.name) @function\n\n",
    "(call\n",
    "  function: [(identifier) (attribute)] @ref)\n\n",
    "(string) @string\n\n",
    "(subscript\n",
    "  value: [(identifier) (attribute)] @config.object\n",
    "  subscript: (string) @config.key)\n\n",
    "(call\n",
    "  function: (attribute) @config.accessor\n",
    "  arguments: (argument_list . (string) @config.key))\n",
);

/// Julia contract query: long- and short-form function definitions, call
/// targets, and `ENV["KEY"]` config reads. Julia schema-token extraction is
/// not yet supported (coverage gap), so no plain string captures here.
const JULIA_CONTRACT_QUERY: &str = concat!(
    "(assignment\n",
    "  (call_expression\n",
    "    (identifier) @function.name)) @function\n\n",
    "(function_definition\n",
    "  (call_expression\n",
    "    (identifier) @function.name)) @function\n\n",
    "(call_expression\n",
    "  (identifier) @ref)\n\n",
    "(index_expression\n",
    "  (identifier) @config.object\n",
    "  (vector_expression\n",
    "    (string_literal) @config.key))\n",
);

/// Errors building a [`ContractChangeHistory`].
#[derive(Debug)]
pub enum ContractHistoryBuildError {
    /// The history violates its own invariants.
    Identity(String),
    /// A contract query failed to compile or run against a file.
    Query(String),
}

impl fmt::Display for ContractHistoryBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(message) => write!(f, "contract history identity error: {message}"),
            Self::Query(message) => write!(f, "contract query error: {message}"),
        }
    }
}

impl std::error::Error for ContractHistoryBuildError {}

/// One function extracted from one revision: a candidate contract owner,
/// consumer, producer, or verifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractFunction {
    pub name: String,
    pub span: Span,
    /// blake3 hex digest of the function's exact source text.
    pub fingerprint: String,
    /// Callee/attribute reference tokens, normalized (leading `self.` dropped).
    pub references: BTreeSet<String>,
    /// String-literal contents (schema tokens).
    pub literals: BTreeSet<String>,
    /// Config keys read by this function from the process-parameter surface
    /// (`os.environ[...]`, `os.getenv(...)`, `os.environ.get(...)`, Julia
    /// `ENV[...]`), normalized like literals.
    pub config_keys: BTreeSet<String>,
}

/// Contract facts for one file at one revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileContracts {
    pub path: PathBuf,
    /// Sorted by name, then span.
    pub functions: Vec<ContractFunction>,
    /// String literals outside any function (module-level acceptance
    /// surfaces such as defaults dicts), token -> first span.
    pub module_literals: BTreeMap<String, Span>,
    /// Config-key reads outside any function, token -> first span.
    pub module_config_keys: BTreeMap<String, Span>,
}

/// Contract facts for one revision in the analysis window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionContracts {
    pub revision: String,
    pub files: Vec<FileContracts>,
}

/// Contract facts over an ordered revision window
/// (`deslop.contract-change-history/1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractChangeHistory {
    pub schema: String,
    pub coverage: FactCoverage,
    /// Why coverage is not `Complete` (unsupported languages, unparsed
    /// files). Must be empty exactly when coverage is `Complete`.
    pub reasons: Vec<String>,
    /// Oldest first, matching the input order.
    pub revisions: Vec<RevisionContracts>,
}

impl ContractChangeHistory {
    /// Extract contract facts from exact per-revision analyses.
    ///
    /// `revisions` is `(revision label, analysis)` pairs, oldest first. Every
    /// parsed file whose extension has a contract query contributes facts;
    /// every other source file is recorded as a coverage reason.
    pub fn from_analyses(
        revisions: &[(String, Arc<ProjectAnalysis>)],
    ) -> Result<Self, ContractHistoryBuildError> {
        let mut reasons = Vec::new();
        let mut extracted = Vec::with_capacity(revisions.len());
        for (revision, analysis) in revisions {
            let mut files = Vec::new();
            for parsed in analysis.files() {
                let path = parsed.key().path.clone();
                let Some(query_source) = contract_query_for(&path) else {
                    reasons.push(format!(
                        "{}: no contract query for this language (revision {revision})",
                        path.display()
                    ));
                    continue;
                };
                if !parsed.has_tree() {
                    reasons.push(format!(
                        "{}: parse unavailable (revision {revision})",
                        path.display()
                    ));
                    continue;
                }
                files.push(extract_file(analysis, &path, query_source).map_err(
                    |error| {
                        ContractHistoryBuildError::Query(format!(
                            "{} (revision {revision}): {error}",
                            path.display()
                        ))
                    },
                )?);
            }
            files.sort_by(|left, right| left.path.cmp(&right.path));
            extracted.push(RevisionContracts {
                revision: revision.clone(),
                files,
            });
        }
        let coverage = if reasons.is_empty() {
            FactCoverage::Complete
        } else {
            FactCoverage::Partial
        };
        let history = Self {
            schema: CONTRACT_CHANGE_HISTORY_SCHEMA.to_string(),
            coverage,
            reasons,
            revisions: extracted,
        };
        history.validate()?;
        Ok(history)
    }

    /// The invariants every history must satisfy.
    pub fn validate(&self) -> Result<(), ContractHistoryBuildError> {
        if self.schema != CONTRACT_CHANGE_HISTORY_SCHEMA {
            return Err(ContractHistoryBuildError::Identity(format!(
                "expected schema `{CONTRACT_CHANGE_HISTORY_SCHEMA}`, got `{}`",
                self.schema
            )));
        }
        if (self.coverage == FactCoverage::Complete) != self.reasons.is_empty() {
            return Err(ContractHistoryBuildError::Identity(
                "complete coverage must carry no reasons; incomplete coverage must carry at least one"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

/// The contract query for a path, keyed on its extension. Unsupported
/// extensions are coverage gaps reported by the caller.
fn contract_query_for(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("py") => Some(PYTHON_CONTRACT_QUERY),
        Some("jl") => Some(JULIA_CONTRACT_QUERY),
        _ => None,
    }
}

/// Normalize a reference token: drop a leading `self.` so `self.model.score`
/// and `model.score` identify the same contract dependency.
fn normalize_reference(text: &str) -> String {
    text.strip_prefix("self.").unwrap_or(text).to_string()
}

/// Normalize a string-literal token: strip one layer of quotes.
fn normalize_literal(text: &str) -> String {
    text.trim_matches(|quote| quote == '"' || quote == '\'')
        .to_string()
}

/// Whether a subscript object names a process-parameter surface: a bare or
/// dotted name whose leaf is `env` or `environ` (case-insensitive, so Python
/// `os.environ` and Julia `ENV` both qualify). Deliberately narrow: a
/// `config["K"]` subscript cannot be told apart from a write without
/// semantic facts, so only environment reads count as config reads.
fn is_config_object(text: &str) -> bool {
    let text = text.strip_prefix("self.").unwrap_or(text);
    let leaf = text.rsplit('.').next().unwrap_or(text);
    let leaf = leaf.to_ascii_lowercase();
    matches!(leaf.as_str(), "env" | "environ")
}

/// Whether a call target is a known config accessor: `os.getenv` or
/// `os.environ.get` (leading `self.` already dropped).
fn is_config_accessor(text: &str) -> bool {
    let text = text.strip_prefix("self.").unwrap_or(text);
    text == "os.getenv" || text == "os.environ.get"
}

/// Run the contract query for one file and join captures into functions.
fn extract_file(
    analysis: &Arc<ProjectAnalysis>,
    path: &Path,
    query_source: &str,
) -> Result<FileContracts, ContractHistoryBuildError> {
    let root = analysis
        .file_node_ids(path)
        .and_then(|mut ids| ids.next())
        .ok_or_else(|| {
            ContractHistoryBuildError::Query(format!("{}: no syntax nodes", path.display()))
        })?;
    let query = analysis
        .compile_syntax_query(path, query_source)
        .map_err(|error| ContractHistoryBuildError::Query(error.to_string()))?;
    let matches = analysis
        .syntax_query_matches(&query, root)
        .map_err(|error| ContractHistoryBuildError::Query(error.to_string()))?;

    // First pass: collect function definitions (name + span).
    let mut functions: Vec<ContractFunction> = Vec::new();
    // Second pass inputs: token captures with their spans. Tokens that no
    // function contains become module-level facts, never silent drops.
    let mut references: Vec<(Span, String)> = Vec::new();
    let mut literals: Vec<(Span, String)> = Vec::new();
    let mut config_keys: Vec<(Span, String)> = Vec::new();

    for one_match in &matches {
        let mut function_name: Option<NodeId> = None;
        let mut function_node: Option<NodeId> = None;
        // Config surfaces named within this one match, with the key captures
        // pending until a surface qualifies.
        let mut config_surface = false;
        let mut pending_keys: Vec<(Span, String)> = Vec::new();
        for capture in one_match.captures().iter() {
            let node = analysis.node(capture.node()).map_err(|error| {
                ContractHistoryBuildError::Query(error.to_string())
            })?;
            let node_span = node.span();
            let span = Span::new(
                node_span.start_point().row() + 1,
                node_span.end_point().row() + 1,
                node_span.start_byte(),
                node_span.end_byte(),
            );
            match capture.capture_name() {
                "function.name" => function_name = Some(capture.node()),
                "function" => function_node = Some(capture.node()),
                "ref" => references.push((span, normalize_reference(node.text()))),
                "string" => literals.push((span, normalize_literal(node.text()))),
                "config.object" | "config.accessor" => {
                    let text = node.text();
                    if is_config_object(text) || is_config_accessor(text) {
                        config_surface = true;
                    }
                }
                "config.key" => pending_keys.push((span, normalize_literal(node.text()))),
                _ => {}
            }
        }
        if config_surface {
            config_keys.append(&mut pending_keys);
        }
        if let (Some(name_id), Some(function_id)) = (function_name, function_node) {
            let name_node = analysis.node(name_id).map_err(|error| {
                ContractHistoryBuildError::Query(error.to_string())
            })?;
            let function_node = analysis.node(function_id).map_err(|error| {
                ContractHistoryBuildError::Query(error.to_string())
            })?;
            let span = function_node.span();
            functions.push(ContractFunction {
                name: name_node.text().to_string(),
                span: Span::new(
                    span.start_point().row() + 1,
                    span.end_point().row() + 1,
                    span.start_byte(),
                    span.end_byte(),
                ),
                fingerprint: blake3::hash(function_node.text().as_bytes())
                    .to_hex()
                    .to_string(),
                references: BTreeSet::new(),
                literals: BTreeSet::new(),
                config_keys: BTreeSet::new(),
            });
        }
    }

    // Assign each token to the innermost function whose span contains it;
    // tokens outside every function are module-level facts.
    let mut module_literals = BTreeMap::new();
    let mut module_config_keys = BTreeMap::new();
    for (span, token) in references {
        if let Some(function) = innermost(&mut functions, &span) {
            function.references.insert(token);
        }
    }
    for (span, token) in literals {
        if let Some(function) = innermost(&mut functions, &span) {
            function.literals.insert(token);
        } else {
            module_literals.entry(token).or_insert(span);
        }
    }
    for (span, token) in config_keys {
        if let Some(function) = innermost(&mut functions, &span) {
            function.config_keys.insert(token);
        } else {
            module_config_keys.entry(token).or_insert(span);
        }
    }

    functions.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.span.start_byte.cmp(&right.span.start_byte))
    });
    functions.dedup_by(|left, right| {
        left.name == right.name && left.span == right.span
    });
    Ok(FileContracts {
        path: path.to_path_buf(),
        functions,
        module_literals,
        module_config_keys,
    })
}

/// The function with the smallest span containing `span`, if any.
fn innermost<'f>(
    functions: &'f mut [ContractFunction],
    span: &Span,
) -> Option<&'f mut ContractFunction> {
    functions
        .iter_mut()
        .filter(|function| {
            function.span.start_byte <= span.start_byte && span.end_byte <= function.span.end_byte
        })
        .min_by_key(|function| function.span.end_byte - function.span.start_byte)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{ProjectSnapshotBuilder, RepositoryId};

    fn analysis(files: &[(&str, &[u8])]) -> Arc<ProjectAnalysis> {
        let temp = tempfile::tempdir().unwrap();
        let mut builder = ProjectSnapshotBuilder::new(
            temp.path(),
            RepositoryId::explicit("contract-history-test").unwrap(),
        )
        .unwrap();
        for (path, source) in files {
            builder = builder.with_overlay(path, source.to_vec()).unwrap();
        }
        ProjectAnalysis::build(builder.build().unwrap()).unwrap()
    }

    fn history(files: &[(&str, &[u8])]) -> ContractChangeHistory {
        let analysis = analysis(files);
        ContractChangeHistory::from_analyses(&[("rev-a".to_string(), analysis)]).unwrap()
    }

    #[test]
    fn python_extraction_captures_functions_references_and_literals() {
        let source = br#"class Scorer:
    def __init__(self, model):
        self.model = model

    def decide(self, candidates):
        return max(candidates, key=lambda c: self.model.raw_score(c))

    def public_score(self, candidate):
        return self.model.raw_score(candidate)


def build_manifest(run):
    return {"run_id": run.id, "metric": run.final_loss}
"#;
        let history = history(&[("scoring.py", source)]);
        assert_eq!(history.coverage, FactCoverage::Complete);
        assert!(history.reasons.is_empty());
        let file = &history.revisions[0].files[0];
        let by_name = |name: &str| {
            file.functions
                .iter()
                .find(|function| function.name == name)
                .unwrap_or_else(|| panic!("missing function {name}"))
        };
        let decide = by_name("decide");
        assert!(decide.references.contains("max"));
        assert!(decide.references.contains("model.raw_score"));
        assert!(!decide.references.contains("self.model.raw_score"));
        let public = by_name("public_score");
        assert!(public.references.contains("model.raw_score"));
        let build = by_name("build_manifest");
        assert!(build.literals.contains("run_id"));
        assert!(build.literals.contains("metric"));
        // __init__ performs assignments only; it references no call target.
        assert!(by_name("__init__").references.is_empty());
    }

    #[test]
    fn julia_extraction_captures_short_form_definitions_and_calls() {
        let source = b"decide(model, candidates) = argmax(c -> raw_score(model, c), candidates)\n\npublic_score(model, c) = raw_score(model, c)\n";
        let history = history(&[("scoring.jl", source)]);
        assert_eq!(history.coverage, FactCoverage::Complete);
        let file = &history.revisions[0].files[0];
        let by_name = |name: &str| {
            file.functions
                .iter()
                .find(|function| function.name == name)
                .unwrap_or_else(|| panic!("missing function {name}"))
        };
        let decide = by_name("decide");
        assert!(decide.references.contains("argmax"));
        assert!(decide.references.contains("raw_score"));
        let public = by_name("public_score");
        assert!(public.references.contains("raw_score"));
    }

    #[test]
    fn module_level_tokens_are_file_facts_not_silent_drops() {
        let source = br#"import os

DEFAULTS = {"threshold": 0.5}
LIMIT = os.environ["LIMIT"]
"#;
        let history = history(&[("settings.py", source)]);
        let file = &history.revisions[0].files[0];
        assert!(file.module_literals.contains_key("threshold"));
        assert!(file.module_literals.contains_key("LIMIT"));
        assert_eq!(file.module_config_keys.len(), 1);
        assert!(file.module_config_keys.contains_key("LIMIT"));
    }

    #[test]
    fn unsupported_language_is_a_coverage_reason_not_a_silent_skip() {
        // Rust is a supported source (the builder accepts it) but has no
        // contract query yet: it must surface as a coverage reason.
        let history = history(&[("lib.rs", b"fn main() {}\n")]);
        assert_eq!(history.coverage, FactCoverage::Partial);
        assert_eq!(history.reasons.len(), 1);
        assert!(history.reasons[0].contains("no contract query"));
    }

    #[test]
    fn python_config_key_extraction() {
        let source = br#"import os

DEFAULTS = {"threshold": 0.5}


def load_config():
    config = {}
    config["threshold"] = os.environ["THRESHOLD"]
    config["seed"] = os.getenv("SEED")
    config["lr"] = os.environ.get("LEARNING_RATE")
    return config


def fetch(url):
    return requests.get("https://example.com")
"#;
        let history = history(&[("config.py", source)]);
        assert_eq!(history.coverage, FactCoverage::Complete);
        let file = &history.revisions[0].files[0];
        let load = file
            .functions
            .iter()
            .find(|function| function.name == "load_config")
            .expect("missing load_config");
        assert!(load.config_keys.contains("THRESHOLD"));
        assert!(load.config_keys.contains("SEED"));
        assert!(load.config_keys.contains("LEARNING_RATE"));
        // A `config["K"]` subscript is not a config read (reads and writes
        // are indistinguishable without semantic facts).
        assert!(!load.config_keys.contains("threshold"));
        // A generic attribute call with a string argument is not a config read.
        let fetch = file
            .functions
            .iter()
            .find(|function| function.name == "fetch")
            .expect("missing fetch");
        assert!(fetch.config_keys.is_empty());
    }

    #[test]
    fn julia_config_key_extraction() {
        let source = b"load_config() = ENV[\"THRESHOLD\"]\n";
        let history = history(&[("config.jl", source)]);
        assert_eq!(history.coverage, FactCoverage::Complete);
        let file = &history.revisions[0].files[0];
        let load = file
            .functions
            .iter()
            .find(|function| function.name == "load_config")
            .expect("missing load_config");
        assert!(load.config_keys.contains("THRESHOLD"));
    }

    #[test]
    fn history_roundtrips_through_json() {
        let history = history(&[("scoring.py", b"def f():\n    return g()\n")]);
        let json = serde_json::to_string(&history).unwrap();
        let back: ContractChangeHistory = serde_json::from_str(&json).unwrap();
        assert_eq!(history, back);
        back.validate().unwrap();
    }

    #[test]
    fn validate_rejects_complete_coverage_with_reasons() {
        let mut history = history(&[("scoring.py", b"def f():\n    return 1\n")]);
        history.reasons.push("bogus".to_string());
        assert!(history.validate().is_err());
    }
}
