//! P0 research registry: canonical machine-readable claim ledger.
//!
//! The JSON file under `evaluation/research/registry.json` is the canonical
//! source; this module validates it and derives catalog tables from it rather
//! than maintaining a duplicate rule list. Scientific references are ledger
//! metadata only: a reference never grants write authority and never stands in
//! for [`deslop_recipes::ProofState`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
pub use deslop_core::research::{
    RESEARCH_REGISTRY_SCHEMA, RESEARCH_REGISTRY_VERSION, RegistryEntry, RegistrySource,
    ResearchRegistry, validate_registry,
};

/// Canonical registry location relative to the `deslop-eval` crate root.
pub const RESEARCH_REGISTRY_PATH: &str = "evaluation/research/registry.json";

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
