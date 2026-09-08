//! Read-only research metadata from the canonical, build-embedded ledger.
//! This module has no verifier dependency and grants no source or write authority.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use anyhow::{Result, anyhow, bail};
use deslop_core::research::{
    RESEARCH_REGISTRY_VERSION, RegistryEntry, RegistrySource, ResearchRegistry, validate_registry,
};
use serde::Serialize;

const LEDGER: &str = include_str!("../../deslop-eval/evaluation/research/registry.json");
static REGISTRY: LazyLock<std::result::Result<ResearchRegistry, String>> =
    LazyLock::new(|| parse_registry(LEDGER).map_err(|error| error.to_string()));

fn parse_registry(ledger: &str) -> Result<ResearchRegistry> {
    let registry: ResearchRegistry = serde_json::from_str(ledger)?;
    validate_registry(&registry)?;
    Ok(registry)
}

#[derive(Debug, Serialize)]
pub struct ResearchEvidence {
    pub schema: &'static str,
    pub registry_version: &'static str,
    pub authority: &'static str,
    pub validation_status: &'static str,
    pub claims: Vec<&'static RegistryEntry>,
    pub sources: Vec<&'static RegistrySource>,
}

fn registry() -> Result<&'static ResearchRegistry> {
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
    let claims: Vec<_> = registry
        .entries
        .iter()
        .filter(|entry| entry.facility_ids.contains(&facility))
        .collect();
    if claims.is_empty() {
        bail!("bundled research registry does not cover rule `{rule}`");
    }
    let reference_ids: BTreeSet<_> = claims
        .iter()
        .flat_map(|claim| claim.references.iter().map(String::as_str))
        .collect();
    let sources: Vec<_> = registry
        .sources
        .iter()
        .filter(|source| reference_ids.contains(source.id.as_str()))
        .collect();
    Ok(ResearchEvidence {
        schema: "deslop.rule-research/1",
        registry_version: RESEARCH_REGISTRY_VERSION,
        authority: "metadata-only; citations and evaluation artifacts grant no source proof or write authority",
        validation_status: "not-independently-validated; engineering fixtures are not a frozen confirmatory evaluation",
        claims,
        sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn malformed_registry_fields_fail_closed() {
        let ledger: Value = serde_json::from_str(LEDGER).unwrap();
        parse_registry(LEDGER).expect("valid ledger before mutation");
        for (object, field) in [
            ("", "construct"),
            ("/entries/0", "references"),
            ("/entries/0", "exact_claim"),
            ("/sources/0", "citation"),
        ] {
            let mut malformed = ledger.clone();
            malformed
                .pointer_mut(object)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(field)
                .unwrap();
            assert!(
                parse_registry(&malformed.to_string()).is_err(),
                "missing required field {object}/{field} must fail"
            );
        }
        for object in ["", "/entries/0", "/sources/0"] {
            let mut malformed = ledger.clone();
            malformed
                .pointer_mut(object)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("undeclared_field".into(), json!(true));
            assert!(
                parse_registry(&malformed.to_string()).is_err(),
                "unknown field in {object} must fail"
            );
        }
    }

    #[test]
    fn unresolved_registry_references_fail_before_rule_selection() {
        let mut ledger: Value = serde_json::from_str(LEDGER).unwrap();
        parse_registry(LEDGER).expect("valid ledger before mutation");
        ledger["entries"][0]["facility_ids"] = json!([]);
        ledger["entries"][0]["fidelity"] = json!("inspiration-only");
        ledger["entries"][0]["references"] = json!(["no-such-source"]);
        assert!(parse_registry(&ledger.to_string()).is_err());
    }
}
