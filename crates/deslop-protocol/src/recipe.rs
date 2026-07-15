use std::collections::BTreeSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, bail};
use deslop_recipes::{TransformationCandidate, ValidationStep};
use serde::{Deserialize, Deserializer, Serialize};

pub const RECIPE_WORK_ORDER_SCHEMA: &str = "deslop.recipe-workorder/1";
const RECIPE_WORK_ORDER_ID_DOMAIN: &str = "deslop recipe work order id v1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RecipeWorkOrderId(String);

impl RecipeWorkOrderId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecipeWorkOrderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RecipeWorkOrderId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_id(&value).map_err(serde::de::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeResourceKind {
    Candidate,
    SourceBytes,
    GraphProjection,
    SourceSpan,
    ProjectSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeResource {
    pub kind: RecipeResourceKind,
    pub identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipePatchBudget {
    pub maximum_files: usize,
    pub maximum_edits: usize,
    pub maximum_removed_bytes: usize,
    pub maximum_added_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeVerificationContract {
    pub required_steps: Vec<ValidationStep>,
    pub protect_undeclared_bytes: bool,
    pub protect_undeclared_files: bool,
    pub protect_public_api: bool,
    pub rollback_checks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeWorkOrder {
    schema: String,
    id: RecipeWorkOrderId,
    candidate: TransformationCandidate,
    target_path: PathBuf,
    reads: Vec<RecipeResource>,
    writes: Vec<RecipeResource>,
    requires: Vec<RecipeResource>,
    invalidates: Vec<RecipeResource>,
    patch_budget: RecipePatchBudget,
    verification: RecipeVerificationContract,
}

impl RecipeWorkOrder {
    pub fn from_candidate(candidate: TransformationCandidate) -> Result<Self> {
        let canonical = CanonicalRecipeWorkOrder::for_candidate(&candidate)?;
        let id = derive_id(&canonical)?;
        let work_order = Self {
            schema: RECIPE_WORK_ORDER_SCHEMA.to_string(),
            id,
            candidate,
            target_path: canonical.target_path,
            reads: canonical.reads,
            writes: canonical.writes,
            requires: canonical.requires,
            invalidates: canonical.invalidates,
            patch_budget: canonical.patch_budget,
            verification: canonical.verification,
        };
        work_order.validate()?;
        Ok(work_order)
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn id(&self) -> &RecipeWorkOrderId {
        &self.id
    }

    pub fn candidate(&self) -> &TransformationCandidate {
        &self.candidate
    }

    pub fn target_path(&self) -> &PathBuf {
        &self.target_path
    }

    pub fn reads(&self) -> &[RecipeResource] {
        &self.reads
    }

    pub fn writes(&self) -> &[RecipeResource] {
        &self.writes
    }

    pub fn requires(&self) -> &[RecipeResource] {
        &self.requires
    }

    pub fn invalidates(&self) -> &[RecipeResource] {
        &self.invalidates
    }

    pub fn patch_budget(&self) -> &RecipePatchBudget {
        &self.patch_budget
    }

    pub fn verification(&self) -> &RecipeVerificationContract {
        &self.verification
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != RECIPE_WORK_ORDER_SCHEMA {
            bail!("unsupported recipe work-order schema `{}`", self.schema);
        }
        validate_path(&self.target_path)?;
        let canonical = CanonicalRecipeWorkOrder::for_candidate(&self.candidate)?;
        if self.target_path != canonical.target_path
            || self.reads != canonical.reads
            || self.writes != canonical.writes
            || self.requires != canonical.requires
            || self.invalidates != canonical.invalidates
            || self.patch_budget != canonical.patch_budget
            || self.verification != canonical.verification
        {
            bail!("recipe work order diverges from its exact candidate authority");
        }
        if self.id != derive_id(&canonical)? {
            bail!("recipe work-order identity is stale");
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeWorkOrderWire {
    schema: String,
    id: RecipeWorkOrderId,
    candidate: TransformationCandidate,
    target_path: PathBuf,
    reads: Vec<RecipeResource>,
    writes: Vec<RecipeResource>,
    requires: Vec<RecipeResource>,
    invalidates: Vec<RecipeResource>,
    patch_budget: RecipePatchBudget,
    verification: RecipeVerificationContract,
}

impl<'de> Deserialize<'de> for RecipeWorkOrder {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RecipeWorkOrderWire::deserialize(deserializer)?;
        let work_order = Self {
            schema: wire.schema,
            id: wire.id,
            candidate: wire.candidate,
            target_path: wire.target_path,
            reads: wire.reads,
            writes: wire.writes,
            requires: wire.requires,
            invalidates: wire.invalidates,
            patch_budget: wire.patch_budget,
            verification: wire.verification,
        };
        work_order.validate().map_err(serde::de::Error::custom)?;
        Ok(work_order)
    }
}

pub fn recipe_work_orders(
    candidates: impl IntoIterator<Item = TransformationCandidate>,
) -> Result<Vec<RecipeWorkOrder>> {
    let mut candidate_ids = BTreeSet::new();
    let mut orders = Vec::new();
    for candidate in candidates {
        if !candidate_ids.insert(candidate.id().as_str().to_string()) {
            bail!("duplicate transformation candidate `{}`", candidate.id());
        }
        orders.push(RecipeWorkOrder::from_candidate(candidate)?);
    }
    orders.sort_by(|left, right| left.id.cmp(&right.id));
    let mut order_ids = BTreeSet::new();
    if orders
        .iter()
        .any(|order| !order_ids.insert(order.id.clone()))
    {
        bail!("distinct transformation candidates produced a duplicate work order");
    }
    Ok(orders)
}

#[derive(Serialize)]
struct CanonicalRecipeWorkOrder {
    candidate: TransformationCandidate,
    target_path: PathBuf,
    reads: Vec<RecipeResource>,
    writes: Vec<RecipeResource>,
    requires: Vec<RecipeResource>,
    invalidates: Vec<RecipeResource>,
    patch_budget: RecipePatchBudget,
    verification: RecipeVerificationContract,
}

impl CanonicalRecipeWorkOrder {
    fn for_candidate(candidate: &TransformationCandidate) -> Result<Self> {
        if candidate.edits().is_empty() {
            bail!("recipe work order requires at least one exact edit");
        }
        let target_path = candidate.target().node.file().path.clone();
        validate_path(&target_path)?;
        if candidate
            .edits()
            .iter()
            .any(|edit| edit.target.file().path != target_path)
        {
            bail!("the production recipe work order cannot span multiple files");
        }

        let mut reads = vec![
            resource(RecipeResourceKind::Candidate, candidate.id().as_str()),
            resource(
                RecipeResourceKind::SourceBytes,
                candidate.edits()[0].revision_guard.as_str(),
            ),
            resource(
                RecipeResourceKind::GraphProjection,
                &candidate.source().program_dependence_projection,
            ),
        ];
        let mut writes = candidate
            .edits()
            .iter()
            .map(|edit| {
                resource(
                    RecipeResourceKind::SourceSpan,
                    &format!(
                        "{}:{}..{}:{}",
                        target_path.display(),
                        edit.span.start_byte,
                        edit.span.end_byte,
                        edit.revision_guard
                    ),
                )
            })
            .collect::<Vec<_>>();
        let mut requires = vec![
            resource(
                RecipeResourceKind::Candidate,
                candidate.recipe().id().as_str(),
            ),
            resource(
                RecipeResourceKind::ProjectSnapshot,
                &candidate.source().project_snapshot,
            ),
            resource(
                RecipeResourceKind::GraphProjection,
                &candidate.source().analysis,
            ),
        ];
        requires.extend(candidate.validation_plan().steps.iter().map(|step| {
            resource(
                RecipeResourceKind::GraphProjection,
                &format!("validation:{}", step.key),
            )
        }));
        let mut invalidates = vec![
            resource(
                RecipeResourceKind::ProjectSnapshot,
                &candidate.source().project_snapshot,
            ),
            resource(
                RecipeResourceKind::GraphProjection,
                &candidate.source().program_dependence_projection,
            ),
        ];
        reads.sort();
        writes.sort();
        requires.sort();
        invalidates.sort();
        ensure_unique("reads", &reads)?;
        ensure_unique("writes", &writes)?;
        ensure_unique("requires", &requires)?;
        ensure_unique("invalidates", &invalidates)?;

        let patch_budget = RecipePatchBudget {
            maximum_files: 1,
            maximum_edits: candidate.edits().len(),
            maximum_removed_bytes: candidate.edits().iter().map(|edit| edit.before.len()).sum(),
            maximum_added_bytes: candidate.edits().iter().map(|edit| edit.after.len()).sum(),
        };
        let verification = RecipeVerificationContract {
            required_steps: candidate.validation_plan().steps.clone(),
            protect_undeclared_bytes: true,
            protect_undeclared_files: true,
            protect_public_api: true,
            rollback_checks: candidate.rollback_plan().validation_steps.clone(),
        };
        Ok(Self {
            candidate: candidate.clone(),
            target_path,
            reads,
            writes,
            requires,
            invalidates,
            patch_budget,
            verification,
        })
    }
}

fn resource(kind: RecipeResourceKind, identity: &str) -> RecipeResource {
    RecipeResource {
        kind,
        identity: identity.to_string(),
    }
}

fn derive_id(canonical: &CanonicalRecipeWorkOrder) -> Result<RecipeWorkOrderId> {
    let digest = super::digest_json(RECIPE_WORK_ORDER_ID_DOMAIN, canonical)?;
    Ok(RecipeWorkOrderId(format!("rwo1_{}", &digest[4..])))
}

fn ensure_unique(label: &str, resources: &[RecipeResource]) -> Result<()> {
    if resources.windows(2).any(|pair| pair[0] == pair[1]) {
        bail!("recipe work-order {label} contain duplicate resources");
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<()> {
    let payload = value
        .strip_prefix("rwo1_")
        .filter(|payload| payload.len() == 64)
        .ok_or_else(|| anyhow::anyhow!("invalid recipe work-order id"))?;
    if !payload
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("invalid recipe work-order id");
    }
    Ok(())
}

fn validate_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("recipe work-order target path must be a nonempty logical path");
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::CurDir | Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("recipe work-order target path is not canonical");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use deslop_recipes::detect_rust_recipes;
    use serde_json::json;

    use super::*;

    fn candidate() -> TransformationCandidate {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("fixture.rs"), "fn run() { return; 1; }\n").unwrap();
        detect_rust_recipes(root.path(), &[PathBuf::from("fixture.rs")])
            .unwrap()
            .remove(0)
    }

    #[test]
    fn candidate_maps_to_one_strict_content_bound_work_order() {
        let order = RecipeWorkOrder::from_candidate(candidate()).unwrap();
        let encoded = serde_json::to_value(&order).unwrap();
        let decoded: RecipeWorkOrder = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(decoded, order);
        assert_eq!(order.writes().len(), 1);
        assert!(order.verification().protect_undeclared_bytes);

        let mut stale = encoded;
        stale["patch_budget"]["maximum_removed_bytes"] = json!(999);
        assert!(serde_json::from_value::<RecipeWorkOrder>(stale).is_err());
    }

    #[test]
    fn candidate_patch_validation_and_rollback_payload_mutation_is_rejected() {
        let order = RecipeWorkOrder::from_candidate(candidate()).unwrap();
        let encoded = serde_json::to_value(order).unwrap();

        let mut patch = encoded.clone();
        patch["candidate"]["edits"][0]["after"] = json!("2;");
        assert!(serde_json::from_value::<RecipeWorkOrder>(patch).is_err());

        let mut validation = encoded.clone();
        validation["candidate"]["validation_plan"]["steps"] = json!([]);
        assert!(serde_json::from_value::<RecipeWorkOrder>(validation).is_err());

        let mut rollback = encoded;
        rollback["candidate"]["rollback_plan"]["require_revision_guards"] = json!(false);
        assert!(serde_json::from_value::<RecipeWorkOrder>(rollback).is_err());
    }

    #[test]
    fn duplicate_candidates_and_foreign_fields_are_rejected() {
        let candidate = candidate();
        assert!(recipe_work_orders([candidate.clone(), candidate.clone()]).is_err());
        let order = RecipeWorkOrder::from_candidate(candidate).unwrap();
        let mut value = serde_json::to_value(order).unwrap();
        value["foreign"] = json!(true);
        assert!(serde_json::from_value::<RecipeWorkOrder>(value).is_err());
    }
}
