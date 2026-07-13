//! Config boundary analysis: catch "dishonest wiring" — config keys that are declared,
//! parsed, and echoed but never actually govern behavior.
//!
//! Motivating incident class (RelationExtractor, 2026-07): `canvas_top_k` was parsed from
//! config, echoed in banners, serialized into checkpoints — and consumed nowhere, so
//! `canvas_top_k=4` and `=0` behaved identically. A sibling incident hard-overwrote a parsed
//! value with a literal (`k > 3` silently clamped to 3). Both survived review because every
//! *visible* surface (parse, echo, serialize) looked wired.
//!
//! The analysis is deliberately language-agnostic (tree-sitter node-kind heuristics, no
//! per-grammar patterns) and key-oriented rather than dataflow-oriented: for each config KEY
//! it aggregates evidence repo-wide and classifies every occurrence as parse / echo / store /
//! live. Verdicts:
//!
//! - `config-key-unread`: declared in a config artifact (TOML/YAML/JSON), never referenced
//!   by code.
//! - `config-key-unconsumed`: parsed by code, but every use of the key (string or its
//!   convention-named binding identifier) is an echo/serialize sink — nothing behavioral.
//! - `config-key-shadowed`: parsed into a binding that is then reassigned from a
//!   literal-only expression before any live use in that file (the hardcode-over-config class).
//!
//! Precision posture: when a use cannot be confidently classified as an echo, it counts as
//! LIVE (which suppresses findings). False negatives are acceptable; false accusations of
//! dishonest wiring are not. Cross-file matching keys on a normalized form (case/`-`/`_`
//! folded) because config ecosystems mix kebab-case keys with snake_case bindings — the
//! motivating incidents crossed exactly that boundary.
//!
//! Known limitations (each degrades to a hedged finding or a miss, never a wrong-confident
//! claim):
//! - Dynamically CONSTRUCTED key names (`"decode-" * key` prefixing) are invisible to the
//!   string index, so such keys can surface as `config-key-unread`; the finding's
//!   precondition says to verify before acting.
//! - Struct-field/derive-macro configs whose keys never appear as runtime strings are out
//!   of scope for the agnostic pass (language packs can add them later).
//! - Consumption through container round-trips (store into a tuple, read via `cfg.field`)
//!   is credited as a store, not a live use; keys consumed ONLY that way may over-report.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use deslop_core::{DetectedBy, FileReport, Finding, Lang, SafetyClass, Severity};
use deslop_parse::NodeId;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{AnalyzerConfig, AnalyzerFile, AnalyzerText, finding};

/// Tuning for the boundary pass. All fields have working defaults; everything here is
/// surfaced through `[analyzer.boundary]` so behavior is config-governed, never silent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryConfig {
    /// Master switch. `false` skips the pass entirely.
    pub enabled: bool,
    /// Keys shorter than this (after normalization) are ignored: short keys ("id", "env")
    /// collide with ordinary identifiers and drown the signal.
    pub min_key_length: usize,
    /// Additional callee-name fragments treated as echo sinks, merged with the built-ins.
    pub extra_sinks: Vec<String>,
    /// Key names (raw or normalized) exempt from all boundary rules.
    pub ignore_keys: Vec<String>,
    /// File names (exact) of config artifacts to skip for `config-key-unread`. Defaults to
    /// well-known TOOL configs (Cargo.toml, package.json, ...) whose consumers live outside
    /// the repo; flagging their keys as "unread" would be noise, not signal.
    pub skip_artifacts: Vec<String>,
}

impl Default for BoundaryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_key_length: 4,
            extra_sinks: Vec::new(),
            ignore_keys: Vec::new(),
            skip_artifacts: WELL_KNOWN_TOOL_CONFIGS
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        }
    }
}

/// Config files whose keys are consumed by external tooling, not by this repo's code.
const WELL_KNOWN_TOOL_CONFIGS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "package.json",
    "package-lock.json",
    "tsconfig.json",
    "jsconfig.json",
    "pyproject.toml",
    "poetry.lock",
    "Pipfile",
    "composer.json",
    "deno.json",
    "bun.lockb",
    "Project.toml",
    "Manifest.toml",
    "JuliaProject.toml",
    "deslop.toml",
    "rust-toolchain.toml",
    "rustfmt.toml",
    "clippy.toml",
    ".pre-commit-config.yaml",
    "mkdocs.yml",
    "codecov.yml",
    "renovate.json",
];

/// Callee-name fragments that mark a call as an echo/serialize sink. Matching is
/// case-insensitive substring over the callee identifier. Deliberately broad: config keys
/// flowing into logging/printing/serialization is exactly the "looks wired" surface.
const SINK_FRAGMENTS: &[&str] = &[
    "print",
    "log",
    "write",
    "format",
    "serial",
    "echo",
    "banner",
    "show",
    "display",
    "debug",
    "info",
    "warn",
    "error",
    "trace",
    "dump",
    "render",
    "tostring",
    "to_string",
    "to_str",
    "inspect",
    "repr",
    "json",
    "yaml",
    "toml",
    "assert",
    "panic",
    "throw",
    "bail",
    "raise",
    "expect",
    "message",
    "sprint",
    "string",
];

/// Callee-name fragments that mark a call as a config parse/lookup site when a key string
/// literal is among its arguments.
const LOOKUP_FRAGMENTS: &[&str] = &[
    "get", "opt", "fetch", "lookup", "read", "load", "parse", "env", "arg", "flag", "setting",
    "config", "param", "option", "key", "value", "attr", "property", "getenv",
];

#[derive(Debug, Default)]
struct KeyEvidence {
    /// Raw spellings seen for this normalized key (for messages).
    spellings: BTreeSet<String>,
    /// (path, line) where the key is declared in a config artifact.
    declared: Vec<(PathBuf, usize)>,
    /// (path, line) of code parse/lookup sites (key string inside a lookup call).
    parsed: Vec<(PathBuf, usize)>,
    /// (path, line) of echo/serialize-only occurrences.
    echoed: Vec<(PathBuf, usize)>,
    /// (path, line) of stores (struct/dict field writes, plain rebinding into other names).
    stored: Vec<(PathBuf, usize)>,
    /// (path, line) of behavioral uses (branch, arithmetic, index, non-sink call, return).
    live: Vec<(PathBuf, usize)>,
    /// (path, line, literal_text) where a parse-site binding is reassigned from a
    /// literal-only expression (shadowing evidence).
    shadowed: Vec<(PathBuf, usize, String)>,
}

/// Entry point: run the repo-wide boundary pass and append findings to `reports`.
///
/// `source_paths` are the already-collected analyzable code files; config artifacts are
/// discovered independently (they are not "supported languages"). New findings for artifact
/// files get their own `FileReport` appended.
pub(crate) fn add_config_boundary_analysis(
    reports: &mut Vec<FileReport>,
    code_files: &[AnalyzerFile<'_>],
    artifact_sources: &[AnalyzerText],
    config: &AnalyzerConfig,
) -> Result<()> {
    let boundary = &config.boundary;
    if !boundary.enabled {
        return Ok(());
    }
    let sinks = sink_matcher(boundary);
    let ignore = ignore_matcher(boundary);

    let mut evidence: BTreeMap<String, KeyEvidence> = BTreeMap::new();

    // Phase 1: config artifacts → declared keys. Artifacts on the skip list (tool configs
    // consumed outside this repo) contribute no `declared` evidence, so their keys can
    // never produce config-key-unread.
    for source in artifact_sources {
        let skipped = source
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| boundary.skip_artifacts.iter().any(|s| s == name));
        if skipped {
            continue;
        }
        for (line, key) in artifact_keys(source) {
            let normalized = normalize_key(&key);
            if normalized.len() < boundary.min_key_length || ignore(&normalized, &key) {
                continue;
            }
            let entry = evidence.entry(normalized).or_default();
            entry.spellings.insert(key);
            entry.declared.push((source.path.clone(), line));
        }
    }

    // Phase 2: code files → parse/echo/store/live/shadow evidence.
    // Pass 2a collects key-string occurrences so identifier matching in 2b can include
    // parse-bound names; key strings and convention-named identifiers share normalization.
    for file in code_files {
        collect_code_evidence_analysis(file, &mut evidence, &sinks, &ignore, boundary)?;
    }

    // Phase 3: verdicts.
    let mut extra_reports: BTreeMap<PathBuf, Vec<Finding>> = BTreeMap::new();
    for (normalized, entry) in &evidence {
        let display = entry
            .spellings
            .iter()
            .next()
            .cloned()
            .unwrap_or_else(|| normalized.clone());

        // config-key-unread: declared, never mentioned by code in any form.
        let code_mentions = entry.parsed.len()
            + entry.echoed.len()
            + entry.stored.len()
            + entry.live.len()
            + entry.shadowed.len();
        if !entry.declared.is_empty() && code_mentions == 0 {
            for (path, line) in &entry.declared {
                if let Some(source) = artifact_sources.iter().find(|s| &s.path == path) {
                    extra_reports.entry(path.clone()).or_default().push(finding(
                        source,
                        *line,
                        *line,
                        "config-key-unread",
                        Severity::Minor,
                        SafetyClass::NeverAuto,
                        DetectedBy::Boundary,
                        &format!("config key '{display}' is declared here but never read by any scanned code"),
                        "wire the key to a consumer, or delete it; a declared-but-unread key misleads operators into thinking it does something",
                        Some("verify the key is not consumed by an external tool or an unscanned component before removing"),
                        None,
                    ));
                }
            }
            continue;
        }

        // config-key-unconsumed: parsed, but no live use anywhere — echo/store only.
        //
        // ANCHOR REQUIREMENT (precision): the key must be recognizably CONFIG, not data —
        // declared in a config artifact, flag-shaped (`--x`), env-shaped (ALL_CAPS), or
        // dotted (`a.b`). Without an anchor, lookup-shaped reads of output/manifest keys
        // (`get(report, "audit_rows", 0)`) flood the rule with data-plumbing noise —
        // measured on the first live shakedown: 155 findings, mostly manifest fields.
        let anchored = !entry.declared.is_empty()
            || entry
                .spellings
                .iter()
                .any(|s| s.starts_with("--") || s.contains('.') || is_env_shaped(s));
        if anchored
            && !entry.parsed.is_empty()
            && entry.live.is_empty()
            && entry.shadowed.is_empty()
        {
            let (path, line) = &entry.parsed[0];
            if let Some(source) = code_files
                .iter()
                .map(AnalyzerFile::source)
                .find(|source| &source.path == path)
            {
                let echoes = entry.echoed.len();
                let stores = entry.stored.len();
                extra_reports.entry(path.clone()).or_default().push(finding(
                    source,
                    *line,
                    *line,
                    "config-key-unconsumed",
                    Severity::Major,
                    SafetyClass::NeverAuto,
                    DetectedBy::Boundary,
                    &format!(
                        "config key '{display}' is parsed here but nothing behavioral consumes it \
                         ({echoes} echo/serialize use(s), {stores} store(s), 0 live uses repo-wide) — \
                         the value cannot change behavior"
                    ),
                    "wire the parsed value into the mechanism it claims to control, or fail loudly on it being set; parsing+echoing without consumption is dishonest wiring",
                    Some("confirm the binding is not consumed via an alias this analysis cannot see (dynamic dispatch, metaprogramming, cross-process)"),
                    None,
                ));
            }
        }

        // config-key-shadowed: parsed binding literal-overwritten before live use.
        for (path, line, literal) in &entry.shadowed {
            if let Some(source) = code_files
                .iter()
                .map(AnalyzerFile::source)
                .find(|source| &source.path == path)
            {
                extra_reports.entry(path.clone()).or_default().push(finding(
                    source,
                    *line,
                    *line,
                    "config-key-shadowed",
                    Severity::Major,
                    SafetyClass::NeverAuto,
                    DetectedBy::Boundary,
                    &format!(
                        "config key '{display}' is parsed but its binding is overwritten by literal expression `{literal}` before any behavioral use — the configured value is silently discarded"
                    ),
                    "remove the hardcoded overwrite, or make the clamp/override explicit config with a loud echo of the effective value",
                    Some("confirm the overwrite is unconditional on the paths that matter (a guarded fallback assignment is legitimate)"),
                    None,
                ));
            }
        }
    }

    // Merge findings into existing reports where possible; append new reports otherwise.
    for (path, mut findings) in extra_reports {
        if let Some(report) = reports.iter_mut().find(|r| r.path == path) {
            if report.analysis.permits_rewrites() {
                report.findings.append(&mut findings);
            }
        } else {
            let lang = code_files
                .iter()
                .map(AnalyzerFile::source)
                .find(|source| source.path == path)
                .map_or(Lang::Generic, |source| source.lang);
            reports.push(FileReport {
                path,
                lang,
                analysis: deslop_core::AnalysisProvenance::complete(),
                findings,
            });
        }
    }
    Ok(())
}

fn sink_matcher(boundary: &BoundaryConfig) -> impl Fn(&str) -> bool + '_ {
    move |callee: &str| {
        let lower = callee.to_ascii_lowercase();
        SINK_FRAGMENTS.iter().any(|f| lower.contains(f))
            || boundary
                .extra_sinks
                .iter()
                .any(|f| lower.contains(&f.to_ascii_lowercase()))
    }
}

fn ignore_matcher(boundary: &BoundaryConfig) -> impl Fn(&str, &str) -> bool + '_ {
    move |normalized: &str, raw: &str| {
        boundary
            .ignore_keys
            .iter()
            .any(|k| normalize_key(k) == normalized || k == raw)
    }
}

/// Fold case and separator style so `canvas-top-k`, `canvas_top_k`, and `canvasTopK`
/// all meet at one normalized form.
pub(crate) fn normalize_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut prev_lower = false;
    for ch in key.chars() {
        if ch == '-' || ch == '_' || ch == '.' || ch == ':' {
            prev_lower = false;
            continue;
        }
        if ch.is_ascii_uppercase() && prev_lower {
            // camelCase boundary — separator already dropped, just lowercase.
            out.push(ch.to_ascii_lowercase());
            prev_lower = false;
            continue;
        }
        prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        out.push(ch.to_ascii_lowercase());
    }
    out
}

/// Does this string literal look like a config key at all? Filters interpolations, paths,
/// sentences, and URLs so ordinary strings don't enter the evidence map.
fn looks_like_key(text: &str) -> bool {
    if text.is_empty() || text.len() > 64 {
        return false;
    }
    let has_word = text.chars().any(|c| c.is_ascii_alphabetic());
    has_word
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
        && !text.contains("::")
        && !text.starts_with('.')
        && !text.ends_with('.')
}

pub(crate) fn discover_config_artifacts(scan_roots: &[PathBuf]) -> Result<Vec<PathBuf>> {
    use ignore::WalkBuilder;
    let mut out = Vec::new();
    for root in scan_roots {
        if root.is_file() {
            if is_config_artifact(root) {
                out.push(root.clone());
            }
            continue;
        }
        let walker = WalkBuilder::new(root)
            .hidden(false)
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                !matches!(
                    name.as_ref(),
                    ".git" | ".jj" | "target" | "__pycache__" | "node_modules"
                )
            })
            .build();
        for entry in walker {
            let entry = entry.with_context(|| {
                format!(
                    "failed to enumerate config artifacts below {}",
                    root.display()
                )
            })?;
            if !entry.file_type().is_some_and(|k| k.is_file()) {
                continue;
            }
            let path = entry.into_path();
            if is_config_artifact(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn is_config_artifact(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("toml") | Some("yaml") | Some("yml") | Some("json")
    )
}

/// Extract `(line, key)` pairs from a config artifact with cheap line-based parsing.
/// Deterministic and dependency-free; tolerant of partially invalid files (a broken line
/// yields no key rather than an error).
fn artifact_keys(source: &AnalyzerText) -> Vec<(usize, String)> {
    let ext = source
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let mut out = Vec::new();
    match ext {
        "toml" => {
            for (idx, line) in source.lines().iter().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with('#') || trimmed.starts_with('[') {
                    continue;
                }
                if let Some(eq) = trimmed.find('=') {
                    let key = trimmed[..eq].trim().trim_matches('"').trim_matches('\'');
                    if looks_like_key(key) {
                        out.push((idx + 1, key.to_string()));
                    }
                }
            }
        }
        "yaml" | "yml" => {
            for (idx, line) in source.lines().iter().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with('#') || trimmed.starts_with('-') {
                    continue;
                }
                if let Some(colon) = trimmed.find(':') {
                    let key = trimmed[..colon].trim().trim_matches('"').trim_matches('\'');
                    if looks_like_key(key) {
                        out.push((idx + 1, key.to_string()));
                    }
                }
            }
        }
        "json" => {
            let key_re = Regex::new(r#""([^"\\]{1,64})"\s*:"#).expect("valid regex");
            for (idx, line) in source.lines().iter().enumerate() {
                for cap in key_re.captures_iter(line) {
                    let key = &cap[1];
                    if looks_like_key(key) {
                        out.push((idx + 1, key.to_string()));
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// Occurrence classification for one identifier/string node in a code file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UseClass {
    Parse,
    Echo,
    Store,
    Live,
}

fn collect_code_evidence_analysis(
    file: &AnalyzerFile<'_>,
    evidence: &mut BTreeMap<String, KeyEvidence>,
    sinks: &impl Fn(&str) -> bool,
    ignore: &impl Fn(&str, &str) -> bool,
    boundary: &BoundaryConfig,
) -> Result<()> {
    let source = file.source();
    let Some(root) = file.node_ids().next() else {
        return Ok(());
    };

    // 2a: key-string occurrences (string literals that look like keys).
    let mut string_nodes = Vec::new();
    collect_key_strings_analysis(file, root, &mut string_nodes);
    // Track, per normalized key, the identifiers that parse-site results bind to in this
    // file (so 2b can attribute those identifiers' uses to the key even when the identifier
    // spelling does not match the key) plus the earliest parse-site line (so shadowing in
    // 2c only counts reassignments AFTER the parse — an `x = {}` initializer BEFORE the
    // parse is ordinary code, not a shadow).
    let mut bound_aliases: BTreeMap<String, (BTreeSet<String>, usize)> = BTreeMap::new();
    // Byte span of each parse site's enclosing function, per key: shadow evidence (2c) only
    // counts inside it. A same-named binding reassigned in a DIFFERENT function is a
    // different variable, not a shadow (measured FP class on the first live shakedown).
    let mut alias_scopes: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();

    for (node, raw_key) in &string_nodes {
        let normalized = normalize_key(raw_key);
        if normalized.len() < boundary.min_key_length || ignore(&normalized, raw_key) {
            continue;
        }
        // Only strings that participate in a call are classified; a bare string in a list
        // is inert data and proves nothing about wiring.
        let Some((callee, call_node)) = enclosing_call_analysis(file, *node) else {
            continue;
        };
        let line = node_view(file, *node).span().start_point().row() + 1;
        let entry = evidence.entry(normalized.clone()).or_default();
        entry.spellings.insert(raw_key.clone());
        if sinks(&callee) {
            entry.echoed.push((source.path.clone(), line));
            continue;
        }
        let is_lookup = {
            let lower = callee.to_ascii_lowercase();
            LOOKUP_FRAGMENTS.iter().any(|f| lower.contains(f))
        };
        if is_lookup {
            entry.parsed.push((source.path.clone(), line));
            if let Some(alias) = binding_target_analysis(file, call_node) {
                let slot = bound_aliases
                    .entry(normalized.clone())
                    .or_insert_with(|| (BTreeSet::new(), line));
                slot.0.insert(alias);
                slot.1 = slot.1.min(line);
                alias_scopes
                    .entry(normalized)
                    .or_default()
                    .push(enclosing_function_span_analysis(file, call_node));
            } else {
                // The lookup's result is not bound to a name: if it flows directly into
                // another expression (an argument to a further call, a condition, an
                // arithmetic operand), that IS consumption — classify it in place so
                // `f(x=env_flag("KEY"))` does not read as parsed-and-dropped.
                match classify_identifier_use_analysis(file, call_node, sinks) {
                    UseClass::Echo => entry.echoed.push((source.path.clone(), line)),
                    UseClass::Store => entry.stored.push((source.path.clone(), line)),
                    UseClass::Live => entry.live.push((source.path.clone(), line)),
                    UseClass::Parse => {}
                }
            }
        } else {
            // A key string inside an unknown call: treat as live consumption (a routing
            // table, a dispatch, a lookup we do not recognize). Conservative = no finding.
            entry.live.push((source.path.clone(), line));
        }
    }

    // 2b: identifier occurrences. An identifier counts as key evidence when its normalized
    // spelling equals the key (convention naming) or it was bound from a parse site above.
    let keys: BTreeSet<String> = evidence.keys().cloned().collect();
    let mut ident_nodes = Vec::new();
    collect_identifiers_analysis(file, root, &mut ident_nodes);
    for (node, ident) in &ident_nodes {
        let normalized = normalize_key(ident);
        // An identifier can witness SEVERAL keys at once: nested fallback lookups like
        // `x = get(ENV, "A", get(ENV, "B", d))` bind one name for two keys, and both keys'
        // fates merge into that binding — its uses are evidence for ALL of them.
        let mut attributed: Vec<&String> = Vec::new();
        for (key, (aliases, _)) in &bound_aliases {
            if aliases.contains(ident) {
                attributed.push(key);
            }
        }
        if keys.contains(&normalized)
            && let Some((key, _)) = evidence.get_key_value(&normalized)
            && !attributed.contains(&key)
        {
            attributed.push(key);
        }
        if attributed.is_empty() {
            continue;
        }
        let attributed: Vec<String> = attributed.into_iter().cloned().collect();
        let line = node_view(file, *node).span().start_point().row() + 1;
        let class = classify_identifier_use_analysis(file, *node, sinks);
        for key in attributed {
            let Some(entry) = evidence.get_mut(&key) else {
                continue;
            };
            match class {
                UseClass::Echo => entry.echoed.push((source.path.clone(), line)),
                UseClass::Store => entry.stored.push((source.path.clone(), line)),
                UseClass::Live => entry.live.push((source.path.clone(), line)),
                UseClass::Parse => {}
            }
        }
    }

    // 2c: shadowing — a parse-bound alias reassigned from a literal-only RHS in this file,
    // AFTER the parse site (an initializer before the parse is ordinary code) and OUTSIDE
    // guarded blocks (a fallback assignment inside if/try is legitimate error handling).
    for (key, (aliases, parse_line)) in &bound_aliases {
        for (node, ident) in &ident_nodes {
            if !aliases.contains(ident) && normalize_key(ident) != *key {
                continue;
            }
            let line = node_view(file, *node).span().start_point().row() + 1;
            if line <= *parse_line || inside_guarded_block_analysis(file, *node) {
                continue;
            }
            // Scope check: only shadows inside a function that actually parses this key.
            let byte = node_view(file, *node).span().start_byte();
            let in_scope = alias_scopes
                .get(key)
                .is_some_and(|spans| spans.iter().any(|(s, e)| byte >= *s && byte < *e));
            if !in_scope {
                continue;
            }
            if let Some(literal) = literal_reassignment_analysis(file, *node)
                && let Some(entry) = evidence.get_mut(key)
            {
                entry.shadowed.push((source.path.clone(), line, literal));
            }
        }
    }
    Ok(())
}

/// True when the node sits inside a conditional/exception construct (up to the enclosing
/// function): a literal assignment there is a guarded fallback, not an unconditional shadow.
/// Boolean-operator ancestors count as guards too — `cond && (x = "lit")` (Julia) and
/// `x = maybe() or "lit"` (Python) are short-circuit conditionals, not unconditional
/// overwrites.
fn inside_guarded_block_analysis(file: &AnalyzerFile<'_>, node: NodeId) -> bool {
    let mut current = node_view(file, node).parent();
    while let Some(parent) = current {
        let view = node_view(file, parent);
        let kind = view.raw_kind();
        if kind.contains("function") || kind.contains("method") {
            return false;
        }
        if kind.contains("if")
            || kind.contains("try")
            || kind.contains("except")
            || kind.contains("catch")
            || kind.contains("rescue")
            || kind.contains("case")
            || kind.contains("match")
            || kind.contains("cond")
            || kind.contains("unless")
            || kind.contains("switch")
            || kind.contains("conditional")
            || kind.contains("binary")
            || kind.contains("boolean")
            || kind.contains("short_circuit")
        {
            return true;
        }
        current = view.parent();
    }
    false
}

/// Byte span of the function/method enclosing `node`; whole file when at top level (a
/// top-level parse site legitimately scopes shadow detection to the entire script).
fn enclosing_function_span_analysis(file: &AnalyzerFile<'_>, node: NodeId) -> (usize, usize) {
    let mut current = node_view(file, node).parent();
    while let Some(parent) = current {
        let view = node_view(file, parent);
        let kind = view.raw_kind();
        if kind.contains("function") || kind.contains("method") {
            let span = view.span();
            return (span.start_byte(), span.end_byte());
        }
        current = view.parent();
    }
    (0, file.source().text.len())
}

/// `ALL_CAPS_WITH_UNDERSCORES` — the environment-variable naming shape.
fn is_env_shaped(key: &str) -> bool {
    key.len() >= 4
        && key.chars().any(|c| c.is_ascii_uppercase())
        && key
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// String literal nodes whose inner text looks like a config key.
fn collect_key_strings_analysis(
    file: &AnalyzerFile<'_>,
    node: NodeId,
    out: &mut Vec<(NodeId, String)>,
) {
    let view = node_view(file, node);
    let kind = view.raw_kind();
    if kind.contains("comment") {
        return;
    }
    if kind.contains("string") || kind.contains("str_lit") {
        let raw = view.text();
        let inner = raw.trim_matches(|c| matches!(c, '"' | '\'' | '`'));
        // Interpolated strings are echo surfaces, not keys.
        if !inner.contains('$') && !inner.contains('{') && looks_like_key(inner) {
            out.push((node, inner.to_string()));
        }
        return;
    }
    for child in view.children() {
        collect_key_strings_analysis(file, child, out);
    }
}

fn collect_identifiers_analysis(
    file: &AnalyzerFile<'_>,
    node: NodeId,
    out: &mut Vec<(NodeId, String)>,
) {
    let view = node_view(file, node);
    let kind = view.raw_kind();
    if kind.contains("comment") || kind.contains("string") || kind.contains("str_lit") {
        return;
    }
    if kind == "identifier"
        || kind.ends_with("_identifier")
        || kind == "symbol"
        || kind == "simple_symbol"
        || kind == "variable_name"
        || kind == "name"
    {
        out.push((node, view.text().to_string()));
        return;
    }
    for child in view.children() {
        collect_identifiers_analysis(file, child, out);
    }
}

/// Nearest enclosing call expression and its callee name.
fn enclosing_call_analysis(file: &AnalyzerFile<'_>, node: NodeId) -> Option<(String, NodeId)> {
    let mut current = node_view(file, node).parent();
    while let Some(parent) = current {
        let view = node_view(file, parent);
        let kind = view.raw_kind();
        if kind.contains("call") || kind.contains("invocation") || kind == "macro_expression" {
            if let Some(callee) = callee_name_analysis(file, parent) {
                return Some((callee, parent));
            }
            return None;
        }
        // Do not escape a whole function/method definition looking for a call.
        if kind.contains("function") || kind.contains("method") || kind.contains("class") {
            return None;
        }
        current = view.parent();
    }
    None
}

/// Callee name of a call node: the text of its first identifier-ish child (covers
/// `foo(...)`, `obj.foo(...)`, `Module.foo(...)` — the last path segment wins).
fn callee_name_analysis(file: &AnalyzerFile<'_>, call: NodeId) -> Option<String> {
    let target = node_view(file, call).children().first().copied()?;
    let target_view = node_view(file, target);
    let raw = target_view.text();
    let head = raw.lines().next().unwrap_or(raw);
    let name = head.rsplit(['.', ':', '/']).next().unwrap_or(head);
    let cleaned: String = name
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '!' || *c == '?')
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// If `call`'s value is assigned to a simple identifier, return that identifier.
fn binding_target_analysis(file: &AnalyzerFile<'_>, call: NodeId) -> Option<String> {
    let mut current = call;
    for _ in 0..4 {
        let parent = node_view(file, current).parent()?;
        let parent_view = node_view(file, parent);
        let kind = parent_view.raw_kind();
        if kind.contains("assignment")
            || kind.contains("variable_declarat")
            || kind.contains("let")
            || kind.contains("binding")
            || kind.contains("short_var")
        {
            let lhs = parent_view.children().first().copied()?;
            let lhs_view = node_view(file, lhs);
            let name = lhs_view.text().trim();
            let simple =
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            return simple.then(|| name.to_string());
        }
        // Passing through wrappers like `parse(Int, get(...))` and parenthesized exprs.
        if kind.contains("call") || kind.contains("paren") || kind.contains("argument") {
            current = parent;
            continue;
        }
        return None;
    }
    None
}

/// Classify one identifier occurrence. Unknown contexts classify LIVE (conservative:
/// suppresses findings rather than fabricating them).
///
/// Store-ish containers (kwargs, pairs, tuples, dict entries) do NOT terminate the walk:
/// a keyword argument inside a CALL is consumption by that call (live/echo depending on the
/// callee), while the same shape with no enclosing call is a data-structure store. The
/// motivating incident's `driver_config = (canvas_top_k=canvas_top_k,)` is a store; the
/// superficially identical `f(canvas_top_k=canvas_top_k)` is live.
fn classify_identifier_use_analysis(
    file: &AnalyzerFile<'_>,
    node: NodeId,
    sinks: &impl Fn(&str) -> bool,
) -> UseClass {
    // Assignment LHS is a (re)definition, not a use.
    if let Some(parent) = node_view(file, node).parent() {
        let parent_view = node_view(file, parent);
        let kind = parent_view.raw_kind();
        if (kind.contains("assignment")
            || kind.contains("variable_declarat")
            || kind.contains("let"))
            && parent_view.children().first().copied() == Some(node)
        {
            return UseClass::Parse; // definitions carry no consumption evidence
        }
    }
    let mut current = node_view(file, node).parent();
    let mut hops = 0;
    let mut saw_store_shape = false;
    while let Some(parent) = current {
        let parent_view = node_view(file, parent);
        let kind = parent_view.raw_kind();
        if hops > 8 {
            break;
        }
        // Interpolation inside a string = echo surface.
        if kind.contains("interpolat") || kind.contains("string") {
            return UseClass::Echo;
        }
        if kind.contains("call") || kind.contains("invocation") || kind == "macro_expression" {
            return match callee_name_analysis(file, parent) {
                Some(callee) if sinks(&callee) => UseClass::Echo,
                _ => UseClass::Live,
            };
        }
        if kind.contains("if")
            || kind.contains("condition")
            || kind.contains("while")
            || kind.contains("binary")
            || kind.contains("comparison")
            || kind.contains("unary")
            || kind.contains("index")
            || kind.contains("subscript")
            || kind.contains("range")
            || kind.contains("return")
            || kind.contains("ternary")
            || kind.contains("for")
            || kind.contains("match")
            || kind.contains("case")
            || kind.contains("switch")
        {
            return UseClass::Live;
        }
        if kind.contains("pair")
            || kind.contains("field")
            || kind.contains("keyword_arg")
            || kind.contains("named_field")
            || kind.contains("dictionary")
            || kind.contains("tuple")
            || kind.contains("struct")
        {
            saw_store_shape = true;
            // keep walking: an enclosing call reclassifies this as consumption
        }
        // A statement boundary without an enclosing call resolves the store shape.
        if saw_store_shape
            && (kind.contains("assignment") || kind.contains("statement") || kind.contains("block"))
        {
            return UseClass::Store;
        }
        current = parent_view.parent();
        hops += 1;
    }
    if saw_store_shape {
        UseClass::Store
    } else {
        UseClass::Live
    }
}

/// If this identifier node is the LHS of an assignment whose RHS is literal-only
/// (numbers/strings/booleans and pure literal arithmetic, or min/max over the identifier
/// and literals), return the RHS text.
fn literal_reassignment_analysis(file: &AnalyzerFile<'_>, node: NodeId) -> Option<String> {
    let parent = node_view(file, node).parent()?;
    let parent_view = node_view(file, parent);
    let kind = parent_view.raw_kind();
    if !(kind.contains("assignment") || kind.contains("variable_declarat")) {
        return None;
    }
    let children = parent_view.children();
    if children.first().copied() != Some(node) {
        return None;
    }
    let rhs = children.last().copied()?;
    if rhs == node {
        return None;
    }
    let ident_text = node_view(file, node).text().to_string();
    if rhs_is_literal_only_analysis(file, rhs, &ident_text) {
        Some(node_view(file, rhs).text().trim().to_string())
    } else {
        None
    }
}

fn rhs_is_literal_only_analysis(file: &AnalyzerFile<'_>, node: NodeId, self_ident: &str) -> bool {
    let view = node_view(file, node);
    let kind = view.raw_kind();
    if kind.contains("comment") {
        return true;
    }
    if kind.contains("number")
        || kind.contains("integer")
        || kind.contains("float")
        || kind.contains("string")
        || kind.contains("bool")
        || kind == "true"
        || kind == "false"
    {
        return true;
    }
    if kind == "identifier" || kind.ends_with("_identifier") {
        // The binding itself may appear (e.g. `x = min(x, 3)`); any OTHER identifier
        // means the RHS is not literal-only.
        let text = view.text();
        return text == self_ident || text == "min" || text == "max" || text == "clamp";
    }
    if view.child_count() == 0 {
        // Operators, parens, commas.
        return true;
    }
    view.children()
        .into_iter()
        .all(|child| rhs_is_literal_only_analysis(file, child, self_ident))
}

fn node_view<'a>(file: &'a AnalyzerFile<'a>, node: NodeId) -> deslop_parse::NodeView<'a> {
    file.analysis
        .node(node)
        .expect("boundary node belongs to its prepared project analysis")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, text).expect("write fixture");
        path
    }

    fn scan(dir: &Path) -> Vec<Finding> {
        let reports =
            crate::scan_paths_with_config(&[dir.to_path_buf()], AnalyzerConfig::default())
                .expect("scan");
        reports
            .into_iter()
            .flat_map(|report| report.findings)
            .filter(|finding| finding.rule.starts_with("config-key-"))
            .collect()
    }

    fn rules(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.rule.as_str()).collect()
    }

    #[test]
    fn normalize_folds_kebab_snake_camel() {
        assert_eq!(normalize_key("canvas-top-k"), "canvastopk");
        assert_eq!(normalize_key("canvas_top_k"), "canvastopk");
        assert_eq!(normalize_key("canvasTopK"), "canvastopk");
        assert_eq!(normalize_key("decode.canvas_top_k"), "decodecanvastopk");
    }

    #[test]
    fn declared_but_never_read_key_is_flagged() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            "settings.toml",
            "[decode]\nphantom_knob = 4\nused_knob = 2\n",
        );
        write(
            tmp.path(),
            "main.jl",
            "used_knob = parse(Int, get(options, \"used-knob\", \"2\"))\nif used_knob > 1\n    run(used_knob)\nend\n",
        );
        let findings = scan(tmp.path());
        assert!(
            rules(&findings).contains(&"config-key-unread"),
            "expected config-key-unread, got {findings:?}"
        );
        assert!(
            findings
                .iter()
                .any(|f| f.rule == "config-key-unread" && f.message.contains("phantom_knob")),
            "unread finding should name phantom_knob: {findings:?}"
        );
        assert!(
            !findings
                .iter()
                .any(|f| f.message.contains("used_knob") || f.message.contains("used-knob")),
            "used_knob is consumed and must not be flagged: {findings:?}"
        );
    }

    /// Ground truth: the RelationExtractor canvas_top_k incident shape — parsed, echoed
    /// in a banner, serialized into a config tuple, consumed by NOTHING.
    #[test]
    fn parsed_and_echoed_only_key_is_flagged_unconsumed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(tmp.path(), "config.toml", "canvas_top_k = 4\n");
        write(
            tmp.path(),
            "driver.jl",
            concat!(
                "canvas_top_k = parse(Int, get(options, \"canvas-top-k\", \"4\"))\n",
                "println(\"canvas_top_k=$(canvas_top_k)\")\n",
                "driver_config = (canvas_top_k=canvas_top_k,)\n",
            ),
        );
        let findings = scan(tmp.path());
        assert!(
            findings
                .iter()
                .any(|f| f.rule == "config-key-unconsumed" && f.message.contains("canvas")),
            "expected config-key-unconsumed for the echo-only key, got {findings:?}"
        );
    }

    /// The FIXED shape of the same incident: the parsed value now feeds a real call.
    /// The analyzer must stay quiet.
    #[test]
    fn parsed_key_with_live_consumer_is_not_flagged() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(tmp.path(), "config.toml", "canvas_top_k = 4\n");
        write(
            tmp.path(),
            "driver.jl",
            concat!(
                "canvas_top_k = parse(Int, get(options, \"canvas-top-k\", \"4\"))\n",
                "println(\"canvas_top_k=$(canvas_top_k)\")\n",
                "result = topk_feedback(scores, canvas_top_k)\n",
            ),
        );
        let findings = scan(tmp.path());
        assert!(
            !findings.iter().any(|f| f.rule == "config-key-unconsumed"),
            "live consumer must suppress unconsumed, got {findings:?}"
        );
    }

    /// Cross-file consumption through the convention-named identifier: parsed in one file,
    /// consumed in another. Exactly the opts-tuple shape that manual audits had to chase.
    #[test]
    fn cross_file_identifier_consumption_suppresses_finding() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(tmp.path(), "config.toml", "relation_top_k = 3\n");
        write(
            tmp.path(),
            "parse.jl",
            "relation_top_k = parse(Int, get(options, \"relation-top-k\", \"3\"))\n",
        );
        write(
            tmp.path(),
            "consume.jl",
            "function decode(x, relation_top_k)\n    if relation_top_k > 0\n        x + relation_top_k\n    end\nend\n",
        );
        let findings = scan(tmp.path());
        assert!(
            !findings.iter().any(|f| f.rule == "config-key-unconsumed"),
            "cross-file live use must suppress unconsumed, got {findings:?}"
        );
    }

    /// Ground truth: the k>3 -> 3 incident shape — parsed, then hard-clamped by a literal.
    #[test]
    fn literal_clamp_after_parse_is_flagged_shadowed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(tmp.path(), "config.toml", "relation_top_k = 8\n");
        write(
            tmp.path(),
            "sweep.jl",
            concat!(
                "relation_top_k = parse(Int, get(options, \"relation-top-k\", \"3\"))\n",
                "relation_top_k = min(relation_top_k, 3)\n",
                "run_sweep(relation_top_k)\n",
            ),
        );
        let findings = scan(tmp.path());
        assert!(
            findings
                .iter()
                .any(|f| f.rule == "config-key-shadowed"
                    && f.message.contains("min(relation_top_k, 3)")),
            "expected config-key-shadowed for the literal clamp, got {findings:?}"
        );
    }

    #[test]
    fn well_known_tool_configs_are_skipped_for_unread() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        write(tmp.path(), "lib.rs", "pub fn noop() {}\n");
        let findings = scan(tmp.path());
        assert!(
            findings.is_empty(),
            "tool-config keys must not be flagged, got {findings:?}"
        );
    }

    #[test]
    fn disabled_boundary_pass_emits_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(tmp.path(), "settings.toml", "phantom_knob = 4\n");
        write(tmp.path(), "main.jl", "run()\n");
        let config = AnalyzerConfig {
            boundary: BoundaryConfig {
                enabled: false,
                ..BoundaryConfig::default()
            },
            ..AnalyzerConfig::default()
        };
        let reports =
            crate::scan_paths_with_config(&[tmp.path().to_path_buf()], config).expect("scan");
        let boundary_findings: Vec<_> = reports
            .into_iter()
            .flat_map(|r| r.findings)
            .filter(|f| f.rule.starts_with("config-key-"))
            .collect();
        assert!(boundary_findings.is_empty());
    }
}
