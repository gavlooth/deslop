use std::collections::BTreeSet;

use anyhow::{Result, bail};
use deslop_core::RevisionGuard;
use serde::{Deserialize, Serialize};

use crate::{
    SharedWorkOrder, SharedWorkOrderId, WorkOrderPlan, WorkOrderPlanId,
    WorkOrderPlannerConstraints, WorkOrderResource, plan_work_orders,
};

pub const WORK_ORDER_HANDLE_SCHEMA: &str = "deslop.work-order-handle/1";
pub const WORK_ORDER_REPLAN_SCHEMA: &str = "deslop.work-order-replan/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderHandle {
    pub schema: String,
    pub plan: WorkOrderPlanId,
    pub order: SharedWorkOrderId,
    pub revision_guard: RevisionGuard,
}

impl WorkOrderHandle {
    pub fn for_order(plan: &WorkOrderPlan, order: &SharedWorkOrder) -> Result<Self> {
        if !plan.orders().iter().any(|item| item.id() == order.id()) {
            bail!("cannot create a handle for an order outside the plan");
        }
        Ok(Self {
            schema: WORK_ORDER_HANDLE_SCHEMA.into(),
            plan: plan.id().clone(),
            order: order.id().clone(),
            revision_guard: order.target().revision_guard.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpiredWorkOrder {
    pub order: SharedWorkOrderId,
    pub invalidated_resources: Vec<WorkOrderResource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderReplanResult {
    pub schema: String,
    pub prior_plan: WorkOrderPlanId,
    pub committed: SharedWorkOrderId,
    pub invalidated_resources: Vec<WorkOrderResource>,
    pub expired: Vec<ExpiredWorkOrder>,
    pub retained: Vec<SharedWorkOrderId>,
    pub replacement_plan: WorkOrderPlan,
}

/// Expire impacted orders after one pinned commit and plan only independently regenerated orders.
///
/// `regenerated_orders` must come from reanalysis of the post-commit snapshot. Reusing an expired
/// identity is rejected; the lifecycle never adjusts a stale byte span or revision guard.
pub fn replan_after_commit(
    plan: &WorkOrderPlan,
    handle: &WorkOrderHandle,
    mut actual_invalidations: Vec<WorkOrderResource>,
    regenerated_orders: Vec<SharedWorkOrder>,
    replacement_constraints: WorkOrderPlannerConstraints,
) -> Result<WorkOrderReplanResult> {
    plan.validate()?;
    if handle.schema != WORK_ORDER_HANDLE_SCHEMA || handle.plan != *plan.id() {
        bail!("stale work-order handle: plan identity differs");
    }
    let committed = plan
        .orders()
        .iter()
        .find(|order| order.id() == &handle.order)
        .ok_or_else(|| anyhow::anyhow!("stale work-order handle: order is absent"))?;
    if handle.revision_guard != committed.target().revision_guard {
        bail!("stale work-order handle: revision guard differs");
    }

    actual_invalidations.extend(committed.access().invalidates.iter().cloned());
    actual_invalidations.sort();
    actual_invalidations.dedup();
    if actual_invalidations
        .iter()
        .any(|resource| resource.identity.trim().is_empty())
    {
        bail!("commit invalidation resources require nonempty identities");
    }
    let invalidated = actual_invalidations.iter().collect::<BTreeSet<_>>();
    let mut expired = Vec::new();
    let mut retained = Vec::new();
    for order in plan
        .orders()
        .iter()
        .filter(|order| order.id() != committed.id())
    {
        let impacted = order
            .access()
            .reads
            .iter()
            .chain(&order.access().requires)
            .filter(|resource| invalidated.contains(resource))
            .cloned()
            .collect::<Vec<_>>();
        if impacted.is_empty() {
            retained.push(order.id().clone());
        } else {
            expired.push(ExpiredWorkOrder {
                order: order.id().clone(),
                invalidated_resources: impacted,
            });
        }
    }
    expired.sort_by(|left, right| left.order.cmp(&right.order));
    retained.sort();

    let expired_ids = expired
        .iter()
        .map(|item| &item.order)
        .collect::<BTreeSet<_>>();
    if regenerated_orders
        .iter()
        .any(|order| expired_ids.contains(order.id()))
    {
        bail!("reanalysis reused an expired work-order identity instead of binding a new revision");
    }
    let replacement_plan = plan_work_orders(regenerated_orders, replacement_constraints)?;
    Ok(WorkOrderReplanResult {
        schema: WORK_ORDER_REPLAN_SCHEMA.into(),
        prior_plan: plan.id().clone(),
        committed: committed.id().clone(),
        invalidated_resources: actual_invalidations,
        expired,
        retained,
        replacement_plan,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use deslop_recipes::detect_rust_recipes;
    use tempfile::tempdir;

    use super::*;
    use crate::shared_transformation_work_orders;

    fn fixture_plan() -> WorkOrderPlan {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("sample.rs"),
            "fn sample() { return; 7; }\nfn other() { return; 8; }\n",
        )
        .unwrap();
        let candidates = detect_rust_recipes(temp.path(), &[PathBuf::from("sample.rs")]).unwrap();
        plan_work_orders(
            shared_transformation_work_orders(candidates).unwrap(),
            WorkOrderPlannerConstraints::default(),
        )
        .unwrap()
    }

    #[test]
    fn commit_expires_impacted_snapshot_orders_and_never_reuses_their_identity() {
        let plan = fixture_plan();
        let handle = WorkOrderHandle::for_order(&plan, &plan.orders()[0]).unwrap();
        let result =
            replan_after_commit(&plan, &handle, Vec::new(), Vec::new(), Default::default())
                .unwrap();
        assert_eq!(result.expired.len(), 1);
        assert!(result.retained.is_empty());
        assert!(result.replacement_plan.orders().is_empty());

        assert!(
            replan_after_commit(
                &plan,
                &handle,
                Vec::new(),
                vec![plan.orders()[1].clone()],
                Default::default(),
            )
            .unwrap_err()
            .to_string()
            .contains("reused an expired work-order identity")
        );
    }

    #[test]
    fn stale_plan_and_revision_handles_fail_before_replan() {
        let plan = fixture_plan();
        let mut handle = WorkOrderHandle::for_order(&plan, &plan.orders()[0]).unwrap();
        handle.revision_guard = RevisionGuard::from("forged");
        assert!(
            replan_after_commit(&plan, &handle, Vec::new(), Vec::new(), Default::default(),)
                .unwrap_err()
                .to_string()
                .contains("revision guard differs")
        );

        let mut handle = WorkOrderHandle::for_order(&plan, &plan.orders()[0]).unwrap();
        let other = fixture_plan();
        handle.plan = other.id().clone();
        assert!(
            replan_after_commit(&plan, &handle, Vec::new(), Vec::new(), Default::default(),)
                .is_err()
        );
    }
}
