//! P0 research registry: canonical machine-readable claim ledger.
//!
//! The JSON file under `evaluation/research/registry.json` is the canonical
//! source; this module validates it and derives catalog tables from it rather
//! than maintaining a duplicate rule list. Scientific references are ledger
//! metadata only: a reference never grants write authority and never stands in
//! for [`deslop_recipes::ProofState`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const RESEARCH_REGISTRY_SCHEMA: &str = "deslop.research-registry/1";
pub const RESEARCH_REGISTRY_VERSION: &str = "1.0.0";

/// Canonical registry location relative to the `deslop-eval` crate root.
pub const RESEARCH_REGISTRY_PATH: &str = "evaluation/research/registry.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchRegistry {
    pub schema: String,
    pub registry_version: String,
    pub construct: String,
    pub units: Vec<String>,
    pub sources: Vec<RegistrySource>,
    pub metric_exhaustiveness_note: String,
    pub metric_fields: Vec<String>,
    pub entries: Vec<RegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrySource {
    pub id: String,
    pub citation: String,
    pub identifier: String,
    pub identifier_kind: String,
    pub identifier_version: String,
    pub publication_status: String,
    pub peer_reviewed: bool,
    pub dataset_artifact_revision: String,
    pub dataset_artifact_license: String,
    pub studied_population: String,
    pub comparison_baseline: String,
    pub relevant_sections: String,
    pub threats: String,
    pub access: String,
    pub url: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryEntry {
    pub claim_id: String,
    pub kind: String,
    pub support: Vec<String>,
    pub fidelity: String,
    pub references: Vec<String>,
    pub exact_claim: String,
    pub studied_population: String,
    pub construct: String,
    pub unit: String,
    pub facility_ids: Vec<String>,
    pub implementation_symbols: Vec<String>,
    pub estimator_threshold_origin: String,
    pub counterexamples: Vec<String>,
    pub evaluation_artifacts: Vec<String>,
    pub permitted: Vec<String>,
    pub prohibited: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RegistryCheck {
    pub rules: usize,
    pub recipes: usize,
    pub metric_fields: usize,
    pub claims: usize,
}

pub fn registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(RESEARCH_REGISTRY_PATH)
}

pub fn load_registry(path: &Path) -> Result<ResearchRegistry> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read research registry {}", path.display()))?;
    let registry: ResearchRegistry = serde_json::from_str(&text)
        .with_context(|| format!("parse research registry {}", path.display()))?;
    validate_registry(&registry)?;
    Ok(registry)
}

pub fn load_bundled_registry() -> Result<ResearchRegistry> {
    load_registry(&registry_path())
}

/// Validate schema, controlled vocabularies, reference integrity, source
/// fidelity/access consistency, and the minimum P0 fields on every entry.
///
/// Access/fidelity gate: `reproduced` requires fulltext access; `modified`
/// and `inspiration-only` require at least partial access (never
/// metadata-only, abstract-only, record-only, or unavailable); `none`
/// requires heuristic-only support and no references. Reviewed sources are
/// never equated with independently validated facilities.
pub fn validate_registry(registry: &ResearchRegistry) -> Result<()> {
    if registry.schema != RESEARCH_REGISTRY_SCHEMA {
        bail!("unsupported research registry schema `{}`", registry.schema);
    }
    if registry.units.is_empty() {
        bail!("research registry declares no claim units");
    }
    let mut known_sources: BTreeSet<&str> = BTreeSet::new();
    let mut source_access: BTreeMap<String, String> = BTreeMap::new();
    for source in &registry.sources {
        if !known_sources.insert(source.id.as_str()) {
            bail!("duplicate source id in research registry");
        }
        validate_source(source)?;
        source_access.insert(source.id.clone(), source.access.clone());
    }
    let known_units: BTreeSet<&str> = registry.units.iter().map(|unit| unit.as_str()).collect();
    let mut claims = BTreeSet::new();
    for entry in &registry.entries {
        if !claims.insert(entry.claim_id.as_str()) {
            bail!("duplicate claim id `{}`", entry.claim_id);
        }
        validate_entry(entry, &known_sources, &source_access, &known_units)?;
    }
    if registry.metric_fields.is_empty() {
        bail!("research registry covers no metric fields");
    }
    let mut fields = BTreeSet::new();
    for field in &registry.metric_fields {
        if !fields.insert(field.as_str()) {
            bail!("duplicate metric field `{field}`");
        }
    }
    Ok(())
}

fn validate_source(source: &RegistrySource) -> Result<()> {
    for required in [
        source.id.as_str(),
        source.citation.as_str(),
        source.identifier.as_str(),
        source.identifier_kind.as_str(),
        source.identifier_version.as_str(),
        source.publication_status.as_str(),
        source.dataset_artifact_revision.as_str(),
        source.dataset_artifact_license.as_str(),
        source.studied_population.as_str(),
        source.comparison_baseline.as_str(),
        source.relevant_sections.as_str(),
        source.threats.as_str(),
        source.access.as_str(),
    ] {
        if required.trim().is_empty() {
            bail!("source `{}` has an empty required field", source.id);
        }
    }
    match source.identifier_kind.as_str() {
        "arxiv" | "doi" | "zenodo" | "local" => {}
        other => bail!(
            "source `{}` has unknown identifier kind `{other}`",
            source.id
        ),
    }
    match source.access.as_str() {
        "fulltext" | "partial" | "metadata-only" | "abstract-only" | "record-only"
        | "unavailable" => {}
        other => bail!("source `{}` has unknown access `{other}`", source.id),
    }
    Ok(())
}

fn validate_entry(
    entry: &RegistryEntry,
    known_sources: &BTreeSet<&str>,
    source_access: &BTreeMap<String, String>,
    known_units: &BTreeSet<&str>,
) -> Result<()> {
    if !known_units.contains(entry.unit.as_str()) {
        bail!("claim `{}` declares unknown unit `{}`", entry.claim_id, entry.unit);
    }
    for required in [
        entry.claim_id.as_str(),
        entry.exact_claim.as_str(),
        entry.studied_population.as_str(),
        entry.construct.as_str(),
        entry.unit.as_str(),
        entry.estimator_threshold_origin.as_str(),
    ] {
        if required.trim().is_empty() {
            bail!("claim `{}` has an empty required field", entry.claim_id);
        }
    }
    if entry.permitted.is_empty() || entry.prohibited.is_empty() {
        bail!(
            "claim `{}` lacks permitted/prohibited interpretations",
            entry.claim_id
        );
    }
    match entry.kind.as_str() {
        "construct" | "rule-group" | "recipe-group" | "metric" | "quality-claim" => {}
        other => bail!("claim `{}` has unknown kind `{other}`", entry.claim_id),
    }
    if entry.support.is_empty() {
        bail!("claim `{}` declares no support category", entry.claim_id);
    }
    for support in &entry.support {
        match support.as_str() {
            "empirical" | "algorithmic" | "heuristic" => {}
            other => bail!("claim `{}` has unknown support `{other}`", entry.claim_id),
        }
    }
    // Plan P0 asks exactly: paper-reproduced, modified, or inspiration-only.
    // `none` marks entries with no paper at all. Typed consistency only, no
    // prose matching: reproduced requires fulltext access on every cited
    // source; modified/inspiration-only require at least partial access;
    // none forbids references and requires heuristic-only support.
    match entry.fidelity.as_str() {
        "reproduced" => {
            if entry.references.is_empty() {
                bail!(
                    "claim `{}` needs a reference for fidelity `reproduced`",
                    entry.claim_id
                );
            }
            for reference in &entry.references {
                match source_access.get(reference) {
                    Some(access) if access == "fulltext" => {}
                    Some(access) => bail!(
                        "claim `{}` fidelity `reproduced` needs fulltext access, source `{reference}` is `{access}`",
                        entry.claim_id
                    ),
                    None => bail!(
                        "claim `{}` cites unknown source `{reference}`",
                        entry.claim_id
                    ),
                }
            }
        }
        "modified" | "inspiration-only" => {
            if entry.references.is_empty() {
                bail!(
                    "claim `{}` needs a reference for fidelity `{}`",
                    entry.claim_id,
                    entry.fidelity
                );
            }
            for reference in &entry.references {
                match source_access.get(reference) {
                    Some(access) if access == "fulltext" || access == "partial" => {}
                    Some(access) => bail!(
                        "claim `{}` fidelity `{}` needs at least partial access, source `{reference}` is `{access}`",
                        entry.claim_id,
                        entry.fidelity
                    ),
                    None => bail!(
                        "claim `{}` cites unknown source `{reference}`",
                        entry.claim_id
                    ),
                }
            }
        }
        "none" => {
            if !entry.references.is_empty() {
                bail!(
                    "claim `{}` with fidelity `none` must not list references",
                    entry.claim_id
                );
            }
            if entry.support.iter().any(|support| support != "heuristic") {
                bail!(
                    "claim `{}` with fidelity `none` must be heuristic-only",
                    entry.claim_id
                );
            }
        }
        other => bail!("claim `{}` has unknown fidelity `{other}`", entry.claim_id),
    }
    for reference in &entry.references {
        if !known_sources.contains(reference.as_str()) {
            bail!(
                "claim `{}` cites unknown source `{reference}`",
                entry.claim_id
            );
        }
    }
    Ok(())
}

/// Live rule inventory: the authoritative public catalog definition.
pub fn live_rule_ids() -> Vec<String> {
    deslop_core::rules::RULES
        .iter()
        .map(|rule| format!("rule:{}", rule.name))
        .collect()
}

/// Live recipe inventory: the authoritative enabled production catalog.
pub fn live_recipe_ids() -> Result<Vec<String>> {
    Ok(deslop_recipes::enabled_rust_recipe_catalog()?
        .iter()
        .map(|recipe| format!("recipe:{}", recipe.name()))
        .collect())
}

/// Serialize one live `MetricsReport` computed from real code on disk.
///
/// `dir` defaults to the eval crate's own `src/` directory so the check
/// always exercises the real serializer (not a hand-written fixture): any
/// newly added serialized field appears in the JSON and must be declared in
/// `metric_fields`. Pass an explicit directory to cover other shapes.
pub struct LiveMetricsShape {
    pub metrics_json: serde_json::Value,
    pub slop_json: Option<serde_json::Value>,
}

pub fn live_metrics_report_json(dir: Option<&Path>) -> Result<LiveMetricsShape> {
    let root = match dir {
        Some(dir) => dir.to_path_buf(),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src"),
    };
    let report =
        deslop_metrics::metrics_paths(&[root], deslop_metrics::MetricsConfig { sigma: 1.0 })?;
    let text = deslop_metrics::render_json(&report)?;
    let metrics_json: serde_json::Value = serde_json::from_str(&text)?;
    Ok(LiveMetricsShape {
        metrics_json,
        slop_json: None,
    })
}

/// Every `rule:`/`recipe:` facility id the registry claims to cover.
pub fn registry_facility_ids(registry: &ResearchRegistry) -> BTreeSet<String> {
    registry
        .entries
        .iter()
        .flat_map(|entry| entry.facility_ids.iter().cloned())
        .filter(|id| id.starts_with("rule:") || id.starts_with("recipe:"))
        .collect()
}

/// Every `metric:` facility path the registry claims to cover, in the JSON
/// field path form used by `metric_fields`. Leaf paths are listed verbatim;
/// a trailing `.*` expands against `metric_fields` by prefix.
fn registry_metric_paths(registry: &ResearchRegistry) -> Result<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    for entry in &registry.entries {
        for id in &entry.facility_ids {
            let Some(path) = id.strip_prefix("metric:") else {
                continue;
            };
            if let Some(prefix) = path.strip_suffix(".*") {
                let covered: Vec<String> = registry
                    .metric_fields
                    .iter()
                    .filter(|field| {
                        field.as_str() == prefix || field.starts_with(&format!("{prefix}."))
                    })
                    .cloned()
                    .collect();
                if covered.is_empty() {
                    bail!("metric wildcard `{id}` covers no metric_fields entry");
                }
                paths.extend(covered);
                continue;
            }
            paths.insert(path.to_string());
        }
    }
    Ok(paths)
}

/// Fail on unregistered rule/recipe drift and on metric fields the registry
/// does not cover. Every live `RULES` entry — including `slop-score` — must be
/// named in some entry's `facility_ids` via its covering claim; there is no
/// bypass. Metric coverage is bidirectional (see `check_live_metrics_shape`):
/// declared-but-unobserved paths fail as made-up entries, and
/// observed-but-undeclared paths fail as unregistered drift.
pub fn check_inventory(registry: &ResearchRegistry) -> Result<RegistryCheck> {
    let live_rules: BTreeSet<String> = live_rule_ids().into_iter().collect();
    let live_recipes: BTreeSet<String> = live_recipe_ids()?.into_iter().collect();
    let covered = registry_facility_ids(registry);
    let missing: Vec<String> = live_rules
        .iter()
        .chain(live_recipes.iter())
        .filter(|id| !covered.contains(id.as_str()))
        .cloned()
        .collect();
    if !missing.is_empty() {
        bail!("unregistered facilities: {}", missing.join(", "));
    }
    let unknown: Vec<String> = covered
        .iter()
        .filter(|id| !live_rules.contains(id.as_str()) && !live_recipes.contains(id.as_str()))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        bail!("registry lists unknown facilities: {}", unknown.join(", "));
    }
    let metric_paths = registry_metric_paths(registry)?;
    let declared: BTreeSet<String> = registry.metric_fields.iter().cloned().collect();
    let uncovered: Vec<String> = declared.difference(&metric_paths).cloned().collect();
    if !uncovered.is_empty() {
        bail!(
            "metric fields without covering claim: {}",
            uncovered.join(", ")
        );
    }
    Ok(RegistryCheck {
        rules: live_rules.len(),
        recipes: live_recipes.len(),
        metric_fields: declared.len(),
        claims: registry.entries.len(),
    })
}

/// Flatten a JSON value into dot paths with one boring, consistent
/// convention: objects recurse by key; every array records its container
/// path with `[]` (so empty arrays stay shape-visible) and object items
/// recurse under the same container path (so `reasons: ["a"]` yields
/// `reasons[]`, never `reasons[][]`); scalar items add nothing beyond the
/// container; null leaves record just the plain path (the parent path
/// conveys null — no manufactured `:null` marker copies). Feature-axis
/// `measurements` map keys enumerate as full leaf paths (exported
/// per-estimator evidence); only `slop.*.rule_counts` map contents collapse
/// to their parent because rule-count keys are finding data, not shape.
fn flatten_paths(value: &serde_json::Value, prefix: String, out: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_paths(child, path, out);
            }
        }
        serde_json::Value::Array(items) => {
            if !prefix.is_empty() {
                out.insert(format!("{prefix}[]"));
            }
            for item in items {
                match item {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        flatten_paths(item, format!("{prefix}[]"), out);
                    }
                    _ => {
                        out.insert(format!("{prefix}[]"));
                    }
                }
            }
        }
        _ => {
            out.insert(prefix);
        }
    }
}

pub fn check_live_metrics_shape(
    registry: &ResearchRegistry,
    metrics_json: &serde_json::Value,
    slop_json: Option<&serde_json::Value>,
) -> Result<()> {
    // Observed = live report plus the typed change-dispersion output (pure
    // helper, deterministic synthetic counts). Both directions fail.
    let dispersion = deslop_metrics::change_dispersion_metrics(
        "base".to_string(),
        "target".to_string(),
        vec![
            deslop_metrics::ChangedFileMetrics {
                path: "src/a.rs".into(),
                added_lines: 3,
                deleted_lines: 1,
                changed_lines: 4,
                binary: false,
            },
            deslop_metrics::ChangedFileMetrics {
                path: "src/b.rs".into(),
                added_lines: 6,
                deleted_lines: 0,
                changed_lines: 6,
                binary: false,
            },
        ],
    );
    let dispersion_json =
        serde_json::to_value(&dispersion).context("serialize populated change dispersion")?;
    // Single pass: live report plus the typed dispersion output flattened
    // directly under `change_dispersion`. The original report's null arm
    // records the plain path; no report clone or second full scan.
    let mut observed = BTreeSet::new();
    flatten_paths(metrics_json, String::new(), &mut observed);
    flatten_paths(
        &dispersion_json,
        "change_dispersion".to_string(),
        &mut observed,
    );
    if let Some(slop) = slop_json {
        let mut slop_paths = BTreeSet::new();
        flatten_paths(slop, String::new(), &mut slop_paths);
        observed.extend(slop_paths.into_iter().map(|path| format!("slop.{path}")));
    }
    // Collapse only rule-count map contents: keys are finding data, not shape.
    // All array containers record their `[]` path (see `flatten_paths`), so
    // every container is declared uniformly — no per-name exemption list.
    let mut normalized = BTreeSet::new();
    for path in observed {
        let mut path = path;
        for parent in ["slop.files[].rule_counts", "slop.rule_counts"] {
            if path == parent || path.starts_with(&format!("{parent}.")) {
                path = parent.to_string();
                break;
            }
        }
        normalized.insert(path);
    }
    let declared: BTreeSet<String> = registry.metric_fields.iter().cloned().collect();
    let undeclared: Vec<String> = normalized.difference(&declared).cloned().collect();
    if !undeclared.is_empty() {
        bail!(
            "live metrics fields missing from registry: {}",
            undeclared.join(", ")
        );
    }
    let invented: Vec<String> = declared.difference(&normalized).cloned().collect();
    if !invented.is_empty() {
        bail!(
            "registry declares unobserved metric fields: {}",
            invented.join(", ")
        );
    }
    Ok(())
}
/// Derive the construct-to-facility markdown table from the registry.
/// Used by the CLI `inventory` command; docs consume this output instead of
/// maintaining a duplicate rule list.
pub fn render_inventory_markdown(registry: &ResearchRegistry) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Research registry inventory (v{})\n\n",
        registry.registry_version
    ));
    out.push_str("| claim | kind | fidelity | facilities |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for entry in &registry.entries {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            entry.claim_id,
            entry.kind,
            entry.fidelity,
            if entry.facility_ids.is_empty() {
                "—".to_string()
            } else {
                entry.facility_ids.join(", ")
            }
        ));
    }
    out
}

/// Per-entry detail for reviewers: claim, references, and interpretations.
pub fn render_claims_markdown(registry: &ResearchRegistry) -> String {
    let mut out = String::new();
    for entry in &registry.entries {
        out.push_str(&format!("## {}\n\n", entry.claim_id));
        out.push_str(&format!("- kind: {}\n", entry.kind));
        out.push_str(&format!("- support: {}\n", entry.support.join(", ")));
        out.push_str(&format!("- fidelity: {}\n", entry.fidelity));
        out.push_str(&format!(
            "- references: {}\n",
            if entry.references.is_empty() {
                "none".to_string()
            } else {
                entry.references.join(", ")
            }
        ));
        out.push_str(&format!("- claim: {}\n", entry.exact_claim));
        out.push_str(&format!(
            "- studied population: {}\n",
            entry.studied_population
        ));
        out.push_str(&format!(
            "- construct/unit: {} / {}\n",
            entry.construct, entry.unit
        ));
        out.push_str(&format!(
            "- estimator/threshold origin: {}\n",
            entry.estimator_threshold_origin
        ));
        out.push_str(&format!("- permitted: {}\n", entry.permitted.join("; ")));
        out.push_str(&format!("- prohibited: {}\n", entry.prohibited.join("; ")));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_registry_covers_live_catalogs() {
        let registry = load_bundled_registry().expect("bundled registry loads");
        check_inventory(&registry).expect("inventory covers live catalogs");
    }

    #[test]
    fn live_metrics_shape_matches_registry() {
        let registry = load_bundled_registry().expect("bundled registry loads");
        let live = live_metrics_report_json(None).expect("live metrics report");
        check_live_metrics_shape(&registry, &live.metrics_json, live.slop_json.as_ref())
            .expect("live metrics shape matches registry");
    }

    #[test]
    fn new_metric_field_fails() {
        let registry = load_bundled_registry().expect("bundled registry loads");
        let live = live_metrics_report_json(None).expect("live metrics report");
        check_live_metrics_shape(&registry, &live.metrics_json, live.slop_json.as_ref())
            .expect("same input is valid before mutation");
        let mut mutated = live.metrics_json.clone();
        mutated["brand_new_metric"] = serde_json::json!(1);
        assert!(check_live_metrics_shape(&registry, &mutated, live.slop_json.as_ref()).is_err());
    }

    #[test]
    fn invented_registry_field_fails() {
        let registry = load_bundled_registry().expect("bundled registry loads");
        let live = live_metrics_report_json(None).expect("live metrics report");
        check_live_metrics_shape(&registry, &live.metrics_json, live.slop_json.as_ref())
            .expect("same input is valid before mutation");
        let mut registry = registry;
        registry
            .metric_fields
            .push("functions[].made_up_metric".to_string());
        assert!(
            check_live_metrics_shape(&registry, &live.metrics_json, live.slop_json.as_ref())
                .is_err()
        );
    }

    #[test]
    fn missing_slop_score_coverage_fails() {
        let mut registry = load_bundled_registry().expect("bundled registry loads");
        for entry in &mut registry.entries {
            entry.facility_ids.retain(|id| id != "rule:slop-score");
        }
        assert!(check_inventory(&registry).is_err());
    }

    #[test]
    fn unknown_reference_fails() {
        let mut registry = load_bundled_registry().expect("bundled registry loads");
        registry.entries[0]
            .references
            .push("no-such-source".to_string());
        registry.entries[0].fidelity = "inspiration-only".to_string();
        assert!(validate_registry(&registry).is_err());
    }

    #[test]
    fn unknown_fidelity_fails() {
        let mut registry = load_bundled_registry().expect("bundled registry loads");
        registry.entries[0].fidelity = "proved".to_string();
        assert!(validate_registry(&registry).is_err());
    }

    #[test]
    fn none_fidelity_with_reference_fails() {
        let mut registry = load_bundled_registry().expect("bundled registry loads");
        let entry = registry
            .entries
            .iter_mut()
            .find(|entry| entry.fidelity == "none")
            .expect("a none-fidelity entry");
        entry.references.push("paul-2025-smells".to_string());
        assert!(validate_registry(&registry).is_err());
    }
}
