//! Read-only research metadata from the canonical, build-embedded ledger.
//! This module has no verifier dependency and grants no source or write authority.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use anyhow::{Result, anyhow, bail};
use serde::Serialize;
use serde_json::Value;

const LEDGER: &str = include_str!("../../deslop-eval/evaluation/research/registry.json");
static REGISTRY: LazyLock<std::result::Result<Value, String>> = LazyLock::new(|| {
    let value: Value = serde_json::from_str(LEDGER).map_err(|error| error.to_string())?;
    if value["schema"] != "deslop.research-registry/1"
        || value["registry_version"] != "1.0.0"
        || !value["entries"].is_array()
        || !value["sources"].is_array()
    {
        return Err("unsupported bundled research registry".into());
    }
    Ok(value)
});

#[derive(Debug, Serialize)]
pub struct ResearchEvidence {
    pub schema: &'static str,
    pub registry_version: &'static str,
    pub authority: &'static str,
    pub validation_status: &'static str,
    pub claims: Vec<&'static Value>,
    pub sources: Vec<&'static Value>,
}

fn registry() -> Result<&'static Value> {
    REGISTRY.as_ref().map_err(|error| anyhow!("{error}"))
}

/// Explain one catalog rule without interpreting its research as source proof.
/// Evaluation artifact links may be engineering fixtures, not independent studies.
pub fn explain_rule(rule: &str) -> Result<ResearchEvidence> {
    if !deslop_core::rules::is_known(rule) {
        bail!("unknown rule `{rule}`");
    }
    let registry = registry()?;
    let facility = format!("rule:{rule}");
    let claims: Vec<_> = registry["entries"]
        .as_array()
        .expect("validated entries")
        .iter()
        .filter(|entry| {
            entry["facility_ids"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(&facility)))
        })
        .collect();
    if claims.is_empty() {
        bail!("bundled research registry does not cover rule `{rule}`");
    }
    let reference_ids: BTreeSet<_> = claims
        .iter()
        .filter_map(|claim| claim["references"].as_array())
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let sources: Vec<_> = registry["sources"]
        .as_array()
        .expect("validated sources")
        .iter()
        .filter(|source| {
            source["id"]
                .as_str()
                .is_some_and(|id| reference_ids.contains(id))
        })
        .collect();
    if sources.len() != reference_ids.len() {
        bail!("bundled research registry contains unresolved references");
    }
    Ok(ResearchEvidence {
        schema: "deslop.rule-research/1",
        registry_version: "1.0.0",
        authority: "metadata-only; citations and evaluation artifacts grant no source proof or write authority",
        validation_status: "not-independently-validated; engineering fixtures are not a frozen confirmatory evaluation",
        claims,
        sources,
    })
}
