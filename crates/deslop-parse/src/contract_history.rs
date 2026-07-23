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
//! Contract query text lives in each adapter's `LanguageQueryPack` as the
//! `contract` query family and is read here from the snapshot's stored
//! adapter identity, then compiled through
//! [`ProjectAnalysis::compile_syntax_query`] against the exact grammar of
//! each file. Adapters that declare the family unknown are per-language
//! capability gaps, never silent absences.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use deslop_core::Span;

use crate::{FactCoverage, NodeId, ProjectAnalysis};

/// Wire schema identifier for a contract change history.
pub const CONTRACT_CHANGE_HISTORY_SCHEMA: &str = "deslop.contract-change-history/3";

/// Wire schema identifier for contract facts extracted from one exact
/// project snapshot. Snapshot-native refactor analysis consumes this type
/// directly; ordered history is a composition of these facts.
pub const CONTRACT_SNAPSHOT_SCHEMA: &str = "deslop.contract-snapshot/2";

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

/// Bounded syntactic facts used to compare sibling admission gates.
///
/// These facts describe source shape only. In particular, `zero_nan_admission`
/// records the co-occurrence of a NaN check and a zero equality/carve-out; it
/// does not claim that the branch is semantically reachable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionGuardFacts {
    /// The function contains a fail-loud construct (`throw`, `error`,
    /// `raise`, or `assert`).
    pub fail_loud: bool,
    /// Identifier-shaped metric/field candidates, with call targets,
    /// language keywords, and type-like names removed.
    pub domain_identifiers: BTreeSet<String>,
    /// Coarse predicate features retained for bounded structural comparison.
    pub predicate_features: BTreeSet<String>,
    /// Concrete domain identifiers referenced by predicate lines, including
    /// simple local aliases such as `mean = activity.metric_mean`.
    pub predicate_identifiers: BTreeSet<String>,
    /// A NaN value is conditionally paired with a zero comparison or
    /// `iszero` check, the characteristic undefined-mean carve-out.
    pub zero_nan_admission: bool,
    /// Domain identifiers local to the NaN/zero predicate window. Small
    /// helper functions also retain their bounded symbol arguments so a
    /// caller can connect `mean_name`/`count_name` to concrete fields.
    pub zero_nan_identifiers: BTreeSet<String>,
}

/// One function extracted from one revision: a candidate contract owner,
/// consumer, producer, verifier, or admission gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractFunction {
    pub name: String,
    pub span: Span,
    /// blake3 hex digest of the function's exact source text.
    pub fingerprint: String,
    /// Callee/attribute reference tokens, normalized (leading `self.`/`this.`
    /// dropped).
    pub references: BTreeSet<String>,
    /// String-literal contents (schema tokens).
    pub literals: BTreeSet<String>,
    /// Config keys read by this function from the process-parameter surface
    /// (`os.environ[...]`, `os.getenv(...)`, `os.environ.get(...)`, Julia
    /// `ENV[...]`, JavaScript `process.env`), normalized like literals.
    pub config_keys: BTreeSet<String>,
    /// Loop constructs contained in this function (for/while statements and
    /// comprehension clauses). Partition evidence for scope-collapse
    /// detection; syntax counts only, never a claim about the semantic axis.
    pub loops: u64,
    /// Assertion-bearing statements contained in this function (Python
    /// `assert`/`raise`, Julia `@assert`, JavaScript `throw`). Verifier
    /// classification evidence.
    pub assertions: u64,
    /// Normalized whole-call-expression texts with occurrence counts.
    /// Duplication evidence for hot-path detection; texts longer than
    /// [`CALL_TEXT_LIMIT`] bytes are stored as `blake3:` digests.
    pub call_texts: BTreeMap<String, u64>,
    /// Bounded source-shape facts for sibling admission-gate comparison.
    pub admission_guard: AdmissionGuardFacts,
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

/// Contract facts and coverage for one exact revision-pinned analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractSnapshot {
    pub schema: String,
    pub revision: String,
    pub coverage: FactCoverage,
    /// Empty exactly when coverage is complete.
    pub reasons: Vec<String>,
    pub files: Vec<FileContracts>,
}

impl ContractSnapshot {
    /// Extract one snapshot without constructing a synthetic history window.
    pub fn from_analysis(
        revision: impl Into<String>,
        analysis: &Arc<ProjectAnalysis>,
    ) -> Result<Self, ContractHistoryBuildError> {
        let revision = revision.into();
        let mut reasons = Vec::new();
        let contract_sources: BTreeMap<PathBuf, String> = analysis
            .snapshot()
            .entries()
            .filter_map(|entry| {
                let identity = entry.language_adapter_identity()?;
                let declaration = identity.queries().queries().iter().find(|declaration| {
                    declaration.family() == deslop_lang::QueryFamily::Contract
                })?;
                if declaration.support() != deslop_lang::CapabilitySupport::Provided {
                    return None;
                }
                Some((
                    entry.path().to_path_buf(),
                    declaration.source()?.to_string(),
                ))
            })
            .collect();
        let mut files = Vec::new();
        for parsed in analysis.files() {
            let path = parsed.key().path.clone();
            let Some(query_source) = contract_sources.get(&path) else {
                reasons.push(format!(
                    "{}: the language adapter declares no contract query family \
                     (revision {revision})",
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
            if parsed.text().is_some_and(is_generated_source) {
                reasons.push(format!(
                    "{}: generated file excluded by explicit provenance marker \
                     (revision {revision})",
                    path.display()
                ));
                continue;
            }
            files.push(
                extract_file(analysis, &path, query_source).map_err(|error| {
                    ContractHistoryBuildError::Query(format!(
                        "{} (revision {revision}): {error}",
                        path.display()
                    ))
                })?,
            );
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let coverage = if reasons.is_empty() {
            FactCoverage::Complete
        } else {
            FactCoverage::Partial
        };
        let snapshot = Self {
            schema: CONTRACT_SNAPSHOT_SCHEMA.to_string(),
            revision,
            coverage,
            reasons,
            files,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), ContractHistoryBuildError> {
        if self.schema != CONTRACT_SNAPSHOT_SCHEMA {
            return Err(ContractHistoryBuildError::Identity(format!(
                "expected schema `{CONTRACT_SNAPSHOT_SCHEMA}`, got `{}`",
                self.schema
            )));
        }
        if (self.coverage == FactCoverage::Complete) != self.reasons.is_empty() {
            return Err(ContractHistoryBuildError::Identity(
                "complete snapshot coverage must carry no reasons; incomplete coverage must carry at least one"
                    .to_string(),
            ));
        }
        Ok(())
    }

    pub fn revision_contracts(&self) -> RevisionContracts {
        RevisionContracts {
            revision: self.revision.clone(),
            files: self.files.clone(),
        }
    }
}

/// Contract facts over an ordered revision window
/// (`deslop.contract-change-history/3`).
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
            let snapshot = ContractSnapshot::from_analysis(revision.clone(), analysis)?;
            reasons.extend(snapshot.reasons.iter().cloned());
            extracted.push(snapshot.revision_contracts());
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

/// Maximum stored length for a normalized call-expression text; longer texts
/// are stored as `blake3:` digests so the wire stays bounded while equal
/// calls still compare equal.
pub const CALL_TEXT_LIMIT: usize = 200;

/// Normalize a reference token: drop a leading `self.`/`this.` so
/// `self.model.score` and `model.score` identify the same contract
/// dependency.
fn normalize_reference(text: &str) -> String {
    text.strip_prefix("self.")
        .or_else(|| text.strip_prefix("this."))
        .unwrap_or(text)
        .to_string()
}

/// Normalize a whole call-expression text: collapse whitespace runs so
/// formatting-only differences compare equal, and digest texts longer than
/// [`CALL_TEXT_LIMIT`].
fn normalize_call_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len().min(CALL_TEXT_LIMIT));
    let mut in_gap = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            in_gap = true;
            continue;
        }
        if in_gap && !normalized.is_empty() {
            normalized.push(' ');
        }
        in_gap = false;
        normalized.push(ch);
    }
    if normalized.len() > CALL_TEXT_LIMIT {
        format!("blake3:{}", blake3::hash(normalized.as_bytes()).to_hex())
    } else {
        normalized
    }
}

/// Mask strings and comments while preserving token order. Guard extraction
/// must not treat words such as "zero" or "error" inside diagnostics as
/// executable predicates.
fn executable_text(text: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Code,
        Quote(char),
        LineComment,
        BlockComment,
    }

    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut state = State::Code;
    let mut escaped = false;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        let next = chars.get(index + 1).copied();
        match state {
            State::Code if ch == '#' => {
                state = State::LineComment;
                out.push(' ');
            }
            State::Code if ch == '/' && next == Some('/') => {
                state = State::LineComment;
                out.push(' ');
                out.push(' ');
                index += 1;
            }
            State::Code if ch == '/' && next == Some('*') => {
                state = State::BlockComment;
                out.push(' ');
                out.push(' ');
                index += 1;
            }
            State::Code if matches!(ch, '\'' | '"' | '`') => {
                state = State::Quote(ch);
                escaped = false;
                out.push(' ');
            }
            State::Code => out.push(ch),
            State::Quote(quote) if escaped => {
                escaped = false;
                out.push(if ch == '\n' { '\n' } else { ' ' });
                if ch == '\n' && quote != '`' {
                    state = State::Code;
                }
            }
            State::Quote(_) if ch == '\\' => {
                escaped = true;
                out.push(' ');
            }
            State::Quote(quote) if ch == quote => {
                state = State::Code;
                out.push(' ');
            }
            State::Quote(_) => out.push(if ch == '\n' { '\n' } else { ' ' }),
            State::LineComment if ch == '\n' => {
                state = State::Code;
                out.push('\n');
            }
            State::LineComment => out.push(' '),
            State::BlockComment if ch == '*' && next == Some('/') => {
                state = State::Code;
                out.push(' ');
                out.push(' ');
                index += 1;
            }
            State::BlockComment => out.push(if ch == '\n' { '\n' } else { ' ' }),
        }
        index += 1;
    }
    out
}

fn identifier_tokens(text: &str) -> Vec<(String, usize)> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            tokens.push((text[start..index].to_string(), index));
        } else {
            index += 1;
        }
    }
    tokens
}

fn is_domain_identifier(token: &str) -> bool {
    const EXCLUDED: &[&str] = &[
        "activity",
        "assert",
        "begin",
        "break",
        "catch",
        "class",
        "const",
        "continue",
        "else",
        "elseif",
        "end",
        "error",
        "false",
        "finally",
        "for",
        "function",
        "if",
        "let",
        "local",
        "missing",
        "new",
        "nothing",
        "null",
        "raise",
        "return",
        "status",
        "struct",
        "throw",
        "true",
        "try",
        "undefined",
        "verify",
        "while",
    ];
    if token.len() < 6
        || token.starts_with('_')
        || token
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_uppercase())
        || EXCLUDED.contains(&token.to_ascii_lowercase().as_str())
    {
        return false;
    }
    token.contains('_')
        || token
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0].is_ascii_lowercase() && pair[1].is_ascii_uppercase())
}

fn admission_guard_facts(source: &str) -> AdmissionGuardFacts {
    let executable = executable_text(source);
    let compact = executable
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let tokens = identifier_tokens(&executable);
    let token_names: BTreeSet<String> = tokens
        .iter()
        .map(|(token, _)| token.to_ascii_lowercase())
        .collect();
    let fail_loud = token_names
        .iter()
        .any(|token| matches!(token.as_str(), "throw" | "error" | "raise" | "assert"));

    let mut domain_identifiers = BTreeSet::new();
    for (token, end) in tokens {
        let called = executable[end..].chars().find(|ch| !ch.is_whitespace()) == Some('(');
        if !called && is_domain_identifier(&token) {
            domain_identifiers.insert(token);
        }
    }
    // Keep the wire bounded on generated/adversarial functions.
    domain_identifiers = domain_identifiers.into_iter().take(96).collect();

    let mut aliases = BTreeMap::new();
    for line in executable.lines() {
        let Some((left, right)) = line.split_once('=') else {
            continue;
        };
        if right.starts_with('=') || left.ends_with(['<', '>', '!', '=']) {
            continue;
        }
        let Some(alias) = identifier_tokens(left)
            .last()
            .map(|(token, _)| token.clone())
        else {
            continue;
        };
        let Some(target) = identifier_tokens(right)
            .into_iter()
            .map(|(token, _)| token)
            .find(|token| is_domain_identifier(token))
        else {
            continue;
        };
        aliases.insert(alias, target);
    }
    let mut predicate_identifiers = BTreeSet::new();
    for line in executable.lines() {
        let compact_line = line
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        let is_predicate = compact_line.contains("&&")
            || compact_line.contains("||")
            || compact_line.contains("==")
            || compact_line.contains("!=")
            || compact_line.contains("<=")
            || compact_line.contains(">=")
            || compact_line.contains(">0")
            || compact_line.contains("0<")
            || compact_line.contains("isfinite(")
            || compact_line.contains("isnan(")
            || compact_line.contains("iszero(")
            || compact_line.starts_with("if")
            || compact_line.starts_with("assert");
        if !is_predicate {
            continue;
        }
        for (token, _) in identifier_tokens(line) {
            if is_domain_identifier(&token) {
                predicate_identifiers.insert(token.clone());
            }
            if let Some(target) = aliases.get(&token) {
                predicate_identifiers.insert(target.clone());
            }
        }
    }
    predicate_identifiers = predicate_identifiers.into_iter().take(64).collect();

    let mut predicate_features = BTreeSet::new();
    let has_zero_equality = compact.contains("==0")
        || compact.contains("===0")
        || compact.contains("0==")
        || compact.contains("0===");
    let has_iszero = compact.contains("iszero(");
    if compact.contains("isfinite(") {
        predicate_features.insert("finite".to_string());
    }
    if compact.contains("isnan(") {
        predicate_features.insert("nan".to_string());
    }
    if has_zero_equality || has_iszero {
        predicate_features.insert("zero-equality".to_string());
    }
    if compact.contains(">0") || compact.contains("0<") {
        predicate_features.insert("positive".to_string());
    }
    if compact.contains("<=") || compact.contains(">=") {
        predicate_features.insert("bounded".to_string());
    }
    if compact.contains("==") || compact.contains("===") {
        predicate_features.insert("equality".to_string());
    }
    if compact.contains("hasproperty(")
        || compact.contains("haskey(")
        || compact.contains("inkeys(")
    {
        predicate_features.insert("presence".to_string());
    }
    if token_names
        .iter()
        .any(|token| matches!(token.as_str(), "nothing" | "null" | "none" | "missing"))
    {
        predicate_features.insert("missing-value".to_string());
    }
    let mut zero_nan_identifiers = BTreeSet::new();
    let lines: Vec<&str> = executable.lines().collect();
    let mut has_local_zero_nan = false;
    for start in 0..lines.len() {
        let window = lines[start..lines.len().min(start + 5)].join(" ");
        let compact_window = window
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        let zero = compact_window.contains("iszero(")
            || compact_window.contains("==0")
            || compact_window.contains("===0")
            || compact_window.contains("0==")
            || compact_window.contains("0===");
        if !compact_window.contains("isnan(") || !zero {
            continue;
        }
        has_local_zero_nan = true;
        for (identifier, end) in identifier_tokens(&window) {
            let called = window[end..].chars().find(|ch| !ch.is_whitespace()) == Some('(');
            if !called && is_domain_identifier(&identifier) {
                zero_nan_identifiers.insert(identifier);
            }
        }
    }
    if has_local_zero_nan && executable.len() <= 3_000 {
        zero_nan_identifiers.extend(domain_identifiers.iter().cloned());
    }
    zero_nan_identifiers = zero_nan_identifiers.into_iter().take(32).collect();
    let zero_nan_admission = !zero_nan_identifiers.is_empty();

    AdmissionGuardFacts {
        fail_loud,
        domain_identifiers,
        predicate_features,
        predicate_identifiers,
        zero_nan_admission,
        zero_nan_identifiers,
    }
}

/// Whether file bytes carry an explicit generated-code provenance marker.
/// Only the leading bytes are examined: the marker convention places it in a
/// file header. Generated files are excluded from source-owner findings by
/// provenance, never by content guessing.
fn is_generated_source(text: &str) -> bool {
    let head_end = text
        .char_indices()
        .nth(4096)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    text[..head_end].contains("@generated")
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
    let mut loops: Vec<Span> = Vec::new();
    let mut assertions: Vec<Span> = Vec::new();
    let mut call_texts: Vec<(Span, String)> = Vec::new();

    for one_match in &matches {
        let mut function_name: Option<NodeId> = None;
        let mut function_node: Option<NodeId> = None;
        // Config surfaces named within this one match, with the key captures
        // pending until a surface qualifies.
        let mut config_surface = false;
        let mut pending_keys: Vec<(Span, String)> = Vec::new();
        for capture in one_match.captures().iter() {
            let node = analysis
                .node(capture.node())
                .map_err(|error| ContractHistoryBuildError::Query(error.to_string()))?;
            let node_span = node.span();
            let span = Span::new(
                node_span.start_point().row() + 1,
                node_span.end_point().row() + 1,
                node_span.start_byte(),
                node_span.end_byte(),
            );
            match capture.capture_name() {
                "function.name" => function_name = Some(capture.node()),
                // `function.assign` is Julia's short-form definition (an
                // assignment node); `function.value` is a JavaScript
                // function-valued binding (the arrow/function expression).
                "function" | "function.assign" | "function.value" => {
                    function_node = Some(capture.node());
                }
                "ref" => references.push((span, normalize_reference(node.text()))),
                "string" => literals.push((span, normalize_literal(node.text()))),
                "config.object" | "config.accessor" => {
                    let text = node.text();
                    if is_config_object(text) || is_config_accessor(text) {
                        config_surface = true;
                    }
                }
                // `config.prop` is a JavaScript dotted `process.env.KEY`
                // property; `config.key` is a string subscript key.
                "config.key" | "config.prop" => {
                    pending_keys.push((span, normalize_literal(node.text())));
                }
                "loop" => loops.push(span),
                "assertion" => assertions.push(span),
                "assert.macro" => {
                    if node.text().trim_start_matches('@') == "assert" {
                        assertions.push(span);
                    }
                }
                "call.expr" => call_texts.push((span, normalize_call_text(node.text()))),
                _ => {}
            }
        }
        if config_surface {
            config_keys.append(&mut pending_keys);
        }
        if let (Some(name_id), Some(function_id)) = (function_name, function_node) {
            let name_node = analysis
                .node(name_id)
                .map_err(|error| ContractHistoryBuildError::Query(error.to_string()))?;
            let function_node = analysis
                .node(function_id)
                .map_err(|error| ContractHistoryBuildError::Query(error.to_string()))?;
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
                loops: 0,
                assertions: 0,
                call_texts: BTreeMap::new(),
                admission_guard: admission_guard_facts(function_node.text()),
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
    // Loop, assertion, and call-text facts matter only inside candidate
    // owner/consumer functions; module-level occurrences are outside every
    // detector's evidence contract and are deliberately not extracted.
    for span in loops {
        if let Some(function) = innermost(&mut functions, &span) {
            function.loops += 1;
        }
    }
    for span in assertions {
        if let Some(function) = innermost(&mut functions, &span) {
            function.assertions += 1;
        }
    }
    for (span, text) in call_texts {
        if let Some(function) = innermost(&mut functions, &span) {
            *function.call_texts.entry(text).or_insert(0) += 1;
        }
    }

    functions.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.span.start_byte.cmp(&right.span.start_byte))
    });
    functions.dedup_by(|left, right| left.name == right.name && left.span == right.span);
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
    fn python_loop_assertion_and_call_text_extraction() {
        let source = br#"def rank_documents(docs):
    results = []
    for doc in docs:
        scores = [score(c) for c in doc.candidates]
        results.append(scores)
    assert len(results) == len(docs)
    return results


def flatten_rank(docs):
    merged = expensive_transform(docs)
    other = expensive_transform(docs)
    if not merged:
        raise ValueError("empty")
    return merged + other
"#;
        let history = history(&[("rank.py", source)]);
        assert_eq!(history.coverage, FactCoverage::Complete);
        let file = &history.revisions[0].files[0];
        let by_name = |name: &str| {
            file.functions
                .iter()
                .find(|function| function.name == name)
                .unwrap_or_else(|| panic!("missing function {name}"))
        };
        let rank = by_name("rank_documents");
        // One for-statement plus one comprehension clause.
        assert_eq!(rank.loops, 2);
        assert_eq!(rank.assertions, 1);
        let flatten = by_name("flatten_rank");
        assert_eq!(flatten.loops, 0);
        // The raise statement counts as assertion evidence.
        assert_eq!(flatten.assertions, 1);
        assert_eq!(
            flatten.call_texts.get("expensive_transform(docs)"),
            Some(&2),
            "duplicate call text should count twice: {:?}",
            flatten.call_texts
        );
    }

    #[test]
    fn julia_loop_and_assert_macro_extraction() {
        let source = b"function rank(docs)\n    for doc in docs\n        score(doc)\n    end\n    @assert length(docs) > 0\n    docs\nend\n";
        let history = history(&[("rank.jl", source)]);
        assert_eq!(history.coverage, FactCoverage::Complete);
        let file = &history.revisions[0].files[0];
        let rank = file
            .functions
            .iter()
            .find(|function| function.name == "rank")
            .expect("missing rank");
        assert_eq!(rank.loops, 1);
        assert_eq!(rank.assertions, 1);
    }

    #[test]
    fn admission_guard_facts_ignore_diagnostics_and_capture_zero_nan_carve_out() {
        let source = br#"
function require_resume(activity)
    observations = activity.controller_lambda_observations
    lambda_mean = activity.controller_lambda_mean
    input_mean = activity.input_effect_mean
    isnan(lambda_mean) && iszero(observations) && return true
    isfinite(lambda_mean) && lambda_mean > 0 ||
        throw(ArgumentError("error: zero diagnostic_only_field is rejected"))
    return input_mean > 0
end
"#;
        let history = history(&[("gates.jl", source)]);
        let function = &history.revisions[0].files[0].functions[0];
        assert!(function.admission_guard.fail_loud);
        assert!(function.admission_guard.zero_nan_admission);
        assert!(
            function
                .admission_guard
                .zero_nan_identifiers
                .contains("controller_lambda_observations")
        );
        assert!(
            function
                .admission_guard
                .domain_identifiers
                .contains("controller_lambda_observations")
        );
        assert!(
            function
                .admission_guard
                .domain_identifiers
                .contains("controller_lambda_mean")
        );
        assert!(
            function
                .admission_guard
                .predicate_features
                .contains("positive")
        );
        assert!(
            function
                .admission_guard
                .predicate_identifiers
                .contains("controller_lambda_mean")
        );
        assert!(
            !function
                .admission_guard
                .domain_identifiers
                .contains("diagnostic_only_field")
        );
    }

    #[test]
    fn javascript_extraction_captures_contract_facts() {
        let source = br#"const LIMIT = process.env.LIMIT;

function decide(candidates) {
  return candidates.map((c) => this.model.rawScore(c));
}

class Scorer {
  publicScore(candidate) {
    if (candidate == null) {
      throw new Error("missing candidate");
    }
    return this.model.rawScore(candidate);
  }
}

const loadConfig = () => {
  return { threshold: process.env["THRESHOLD"], seed: process.env.SEED };
};

function sweep(batch) {
  for (const item of batch) {
    expensiveTransform(item);
  }
  while (batch.pending) {
    drain(batch);
  }
}
"#;
        let history = history(&[("scoring.js", source)]);
        assert_eq!(history.coverage, FactCoverage::Complete);
        let file = &history.revisions[0].files[0];
        let by_name = |name: &str| {
            file.functions
                .iter()
                .find(|function| function.name == name)
                .unwrap_or_else(|| panic!("missing function {name}"))
        };
        let decide = by_name("decide");
        assert!(decide.references.contains("candidates.map"));
        assert!(decide.references.contains("model.rawScore"));
        let public = by_name("publicScore");
        assert!(public.references.contains("model.rawScore"));
        assert_eq!(public.assertions, 1);
        assert!(public.literals.contains("missing candidate"));
        let config = by_name("loadConfig");
        assert!(config.config_keys.contains("THRESHOLD"));
        assert!(config.config_keys.contains("SEED"));
        let sweep = by_name("sweep");
        assert_eq!(sweep.loops, 2);
        assert_eq!(sweep.call_texts.get("expensiveTransform(item)"), Some(&1));
        // Module-level config read is a file fact.
        assert!(file.module_config_keys.contains_key("LIMIT"));
    }

    #[test]
    fn generated_file_is_excluded_by_provenance() {
        let source = b"# @generated by scripts/make_scoring.py\n\ndef decide(candidates):\n    return max(candidates)\n";
        let history = history(&[("generated_scoring.py", &source[..])]);
        assert_eq!(history.coverage, FactCoverage::Partial);
        assert_eq!(history.reasons.len(), 1);
        assert!(history.reasons[0].contains("generated file excluded"));
        assert!(history.revisions[0].files.is_empty());
    }

    #[test]
    fn long_call_texts_are_digested_not_truncated() {
        let long_arg = "x".repeat(300);
        let source = format!("def f():\n    return g({long_arg})\n");
        let history = history(&[("long.py", source.as_bytes())]);
        let file = &history.revisions[0].files[0];
        let function = &file.functions[0];
        let digested = function
            .call_texts
            .keys()
            .find(|key| key.starts_with("blake3:"))
            .expect("long call text should be digested");
        assert!(digested.len() < 80);
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
