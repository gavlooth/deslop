//! Shared wire contract and integrity checks for research metadata.
//! Citations and evaluation artifacts grant no source proof or write authority.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub const RESEARCH_REGISTRY_SCHEMA: &str = "deslop.research-registry/1";
pub const RESEARCH_REGISTRY_VERSION: &str = "1.0.0";
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
    if registry.registry_version != RESEARCH_REGISTRY_VERSION {
        bail!(
            "unsupported research registry version `{}`",
            registry.registry_version
        );
    }
    if registry.units.is_empty() {
        bail!("research registry declares no claim units");
    }
    let mut known_sources: BTreeSet<&str> = BTreeSet::new();
    let mut source_access: BTreeMap<&str, &str> = BTreeMap::new();
    for source in &registry.sources {
        if !known_sources.insert(source.id.as_str()) {
            bail!("duplicate source id in research registry");
        }
        validate_source(source)?;
        source_access.insert(source.id.as_str(), source.access.as_str());
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
    source_access: &BTreeMap<&str, &str>,
    known_units: &BTreeSet<&str>,
) -> Result<()> {
    if !known_units.contains(entry.unit.as_str()) {
        bail!(
            "claim `{}` declares unknown unit `{}`",
            entry.claim_id,
            entry.unit
        );
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
                match source_access.get(reference.as_str()).copied() {
                    Some("fulltext") => {}
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
                match source_access.get(reference.as_str()).copied() {
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
