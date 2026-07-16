use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{SharedWorkOrder, SharedWorkOrderId, WorkOrderResource, WorkOrderResourceKind};

pub const WORK_ORDER_PLAN_SCHEMA: &str = "deslop.work-order-plan/1";
const WORK_ORDER_PLAN_ID_DOMAIN: &str = "deslop work order plan v1";
const ATOMIC_GROUP_ID_DOMAIN: &str = "deslop atomic work group v1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct WorkOrderPlanId(String);

impl WorkOrderPlanId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkOrderPlanId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for WorkOrderPlanId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_digest(&value, "wop1_").map_err(serde::de::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct AtomicWorkGroupId(String);

impl AtomicWorkGroupId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AtomicWorkGroupId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AtomicWorkGroupId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_digest(&value, "awg1_").map_err(serde::de::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplicitPrerequisite {
    pub before: SharedWorkOrderId,
    pub after: SharedWorkOrderId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutuallyExclusiveRecipes {
    pub left: String,
    pub right: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderPlannerConstraints {
    pub prerequisites: Vec<ExplicitPrerequisite>,
    pub atomic_groups: Vec<Vec<SharedWorkOrderId>>,
    pub mutually_exclusive_recipes: Vec<MutuallyExclusiveRecipes>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkOrderEdgeKind {
    Prerequisite,
    Invalidation,
    Conflict,
    MutuallyExclusiveRecipe,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderEdge {
    pub kind: WorkOrderEdgeKind,
    pub from: SharedWorkOrderId,
    pub to: SharedWorkOrderId,
    pub resource: Option<WorkOrderResource>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AtomicWorkGroup {
    pub id: AtomicWorkGroupId,
    pub members: Vec<SharedWorkOrderId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkOrderBlockReason {
    PlanningCycle,
    MutuallyExclusiveChoice,
    BlockedPrerequisite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockedWorkGroup {
    pub group: AtomicWorkGroupId,
    pub reason: WorkOrderBlockReason,
    pub related: Vec<AtomicWorkGroupId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderScheduleWave {
    pub ordinal: usize,
    pub groups: Vec<AtomicWorkGroupId>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderPlan {
    schema: String,
    id: WorkOrderPlanId,
    orders: Vec<SharedWorkOrder>,
    constraints: WorkOrderPlannerConstraints,
    edges: Vec<WorkOrderEdge>,
    groups: Vec<AtomicWorkGroup>,
    blocked: Vec<BlockedWorkGroup>,
    waves: Vec<WorkOrderScheduleWave>,
}

impl WorkOrderPlan {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn id(&self) -> &WorkOrderPlanId {
        &self.id
    }

    pub fn orders(&self) -> &[SharedWorkOrder] {
        &self.orders
    }

    pub fn edges(&self) -> &[WorkOrderEdge] {
        &self.edges
    }

    pub fn groups(&self) -> &[AtomicWorkGroup] {
        &self.groups
    }

    pub fn blocked(&self) -> &[BlockedWorkGroup] {
        &self.blocked
    }

    pub fn waves(&self) -> &[WorkOrderScheduleWave] {
        &self.waves
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != WORK_ORDER_PLAN_SCHEMA {
            bail!("unsupported work-order plan schema `{}`", self.schema);
        }
        let rebuilt = plan_work_orders(self.orders.clone(), self.constraints.clone())?;
        if self != &rebuilt {
            bail!("work-order plan is stale or noncanonical");
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkOrderPlanWire {
    schema: String,
    id: WorkOrderPlanId,
    orders: Vec<SharedWorkOrder>,
    constraints: WorkOrderPlannerConstraints,
    edges: Vec<WorkOrderEdge>,
    groups: Vec<AtomicWorkGroup>,
    blocked: Vec<BlockedWorkGroup>,
    waves: Vec<WorkOrderScheduleWave>,
}

impl<'de> Deserialize<'de> for WorkOrderPlan {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkOrderPlanWire::deserialize(deserializer)?;
        let plan = Self {
            schema: wire.schema,
            id: wire.id,
            orders: wire.orders,
            constraints: wire.constraints,
            edges: wire.edges,
            groups: wire.groups,
            blocked: wire.blocked,
            waves: wire.waves,
        };
        plan.validate().map_err(serde::de::Error::custom)?;
        Ok(plan)
    }
}

pub fn plan_work_orders(
    mut orders: Vec<SharedWorkOrder>,
    mut constraints: WorkOrderPlannerConstraints,
) -> Result<WorkOrderPlan> {
    for order in &orders {
        order.validate()?;
    }
    orders.sort_by(|left, right| left.id().cmp(right.id()));
    if orders.windows(2).any(|pair| pair[0].id() == pair[1].id()) {
        bail!("work-order plan contains duplicate order identities");
    }
    canonicalize_constraints(&mut constraints)?;

    let order_index = orders
        .iter()
        .enumerate()
        .map(|(index, order)| (order.id().clone(), index))
        .collect::<BTreeMap<_, _>>();
    validate_constraint_ids(&constraints, &order_index)?;
    let groups = atomic_groups(&orders, &constraints, &order_index)?;
    let group_by_order = groups
        .iter()
        .flat_map(|group| {
            group
                .members
                .iter()
                .cloned()
                .map(move |member| (member, group.id.clone()))
        })
        .collect::<BTreeMap<_, _>>();

    let edges = derive_edges(&orders, &constraints)?;
    reject_atomic_conflicts(&edges, &group_by_order)?;
    let (blocked, waves) = schedule_groups(&groups, &edges, &group_by_order)?;

    let canonical = PlanPayload {
        orders: &orders,
        constraints: &constraints,
        edges: &edges,
        groups: &groups,
        blocked: &blocked,
        waves: &waves,
    };
    let digest = crate::digest_json(WORK_ORDER_PLAN_ID_DOMAIN, &canonical)?;
    let plan = WorkOrderPlan {
        schema: WORK_ORDER_PLAN_SCHEMA.into(),
        id: WorkOrderPlanId(format!("wop1_{}", &digest[4..])),
        orders,
        constraints,
        edges,
        groups,
        blocked,
        waves,
    };
    Ok(plan)
}

#[derive(Serialize)]
struct PlanPayload<'a> {
    orders: &'a [SharedWorkOrder],
    constraints: &'a WorkOrderPlannerConstraints,
    edges: &'a [WorkOrderEdge],
    groups: &'a [AtomicWorkGroup],
    blocked: &'a [BlockedWorkGroup],
    waves: &'a [WorkOrderScheduleWave],
}

fn canonicalize_constraints(constraints: &mut WorkOrderPlannerConstraints) -> Result<()> {
    for prerequisite in &constraints.prerequisites {
        if prerequisite.reason.trim().is_empty() {
            bail!("explicit prerequisite requires a reason");
        }
    }
    constraints.prerequisites.sort_by(|left, right| {
        (&left.before, &left.after, &left.reason).cmp(&(&right.before, &right.after, &right.reason))
    });
    if constraints
        .prerequisites
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        bail!("duplicate explicit prerequisite");
    }
    for group in &mut constraints.atomic_groups {
        group.sort();
        if group.len() < 2 || group.windows(2).any(|pair| pair[0] == pair[1]) {
            bail!("atomic groups require at least two distinct work orders");
        }
    }
    constraints.atomic_groups.sort();
    if constraints
        .atomic_groups
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        bail!("duplicate atomic group");
    }
    for pair in &mut constraints.mutually_exclusive_recipes {
        if pair.left.trim().is_empty() || pair.right.trim().is_empty() || pair.left == pair.right {
            bail!("mutually exclusive recipes require two distinct names");
        }
        if pair.right < pair.left {
            std::mem::swap(&mut pair.left, &mut pair.right);
        }
    }
    constraints.mutually_exclusive_recipes.sort();
    if constraints
        .mutually_exclusive_recipes
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        bail!("duplicate mutually exclusive recipe pair");
    }
    Ok(())
}

fn validate_constraint_ids(
    constraints: &WorkOrderPlannerConstraints,
    order_index: &BTreeMap<SharedWorkOrderId, usize>,
) -> Result<()> {
    for prerequisite in &constraints.prerequisites {
        if prerequisite.before == prerequisite.after
            || !order_index.contains_key(&prerequisite.before)
            || !order_index.contains_key(&prerequisite.after)
        {
            bail!("explicit prerequisite references a missing or identical work order");
        }
    }
    for id in constraints.atomic_groups.iter().flatten() {
        if !order_index.contains_key(id) {
            bail!("atomic group references an unknown work order");
        }
    }
    Ok(())
}

fn atomic_groups(
    orders: &[SharedWorkOrder],
    constraints: &WorkOrderPlannerConstraints,
    index: &BTreeMap<SharedWorkOrderId, usize>,
) -> Result<Vec<AtomicWorkGroup>> {
    let mut parent = (0..orders.len()).collect::<Vec<_>>();
    for members in &constraints.atomic_groups {
        let first = index[&members[0]];
        for member in &members[1..] {
            union(&mut parent, first, index[member]);
        }
    }
    let mut by_root = BTreeMap::<usize, Vec<SharedWorkOrderId>>::new();
    for (order_index, order) in orders.iter().enumerate() {
        let root = find(&mut parent, order_index);
        by_root.entry(root).or_default().push(order.id().clone());
    }
    let mut groups = by_root
        .into_values()
        .map(|mut members| -> Result<_> {
            members.sort();
            let digest = crate::digest_json(ATOMIC_GROUP_ID_DOMAIN, &members)?;
            Ok(AtomicWorkGroup {
                id: AtomicWorkGroupId(format!("awg1_{}", &digest[4..])),
                members,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    groups.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(groups)
}

fn find(parent: &mut [usize], node: usize) -> usize {
    if parent[node] != node {
        parent[node] = find(parent, parent[node]);
    }
    parent[node]
}

fn union(parent: &mut [usize], left: usize, right: usize) {
    let left_root = find(parent, left);
    let right_root = find(parent, right);
    if left_root != right_root {
        let (keep, merge) = if left_root < right_root {
            (left_root, right_root)
        } else {
            (right_root, left_root)
        };
        parent[merge] = keep;
    }
}

fn derive_edges(
    orders: &[SharedWorkOrder],
    constraints: &WorkOrderPlannerConstraints,
) -> Result<Vec<WorkOrderEdge>> {
    let mut edges = BTreeSet::new();
    let mut writers = BTreeMap::<WorkOrderResource, Vec<&SharedWorkOrder>>::new();
    let mut readers = BTreeMap::<WorkOrderResource, Vec<&SharedWorkOrder>>::new();
    let mut invalidators = BTreeMap::<WorkOrderResource, Vec<&SharedWorkOrder>>::new();
    for order in orders {
        for resource in &order.access().writes {
            writers.entry(resource.clone()).or_default().push(order);
        }
        for resource in order.access().reads.iter().chain(&order.access().requires) {
            readers.entry(resource.clone()).or_default().push(order);
        }
        for resource in &order.access().invalidates {
            invalidators
                .entry(resource.clone())
                .or_default()
                .push(order);
        }
    }

    for (resource, providers) in &writers {
        if let Some(consumers) = readers.get(resource) {
            for provider in providers {
                for consumer in consumers {
                    if provider.id() != consumer.id() {
                        edges.insert(edge(
                            WorkOrderEdgeKind::Prerequisite,
                            provider.id(),
                            consumer.id(),
                            Some(resource.clone()),
                            "required resource is produced by another work order",
                        ));
                    }
                }
            }
        }
        for (index, left) in providers.iter().enumerate() {
            for right in &providers[index + 1..] {
                edges.insert(canonical_undirected_edge(
                    WorkOrderEdgeKind::Conflict,
                    left.id(),
                    right.id(),
                    Some(resource.clone()),
                    "both work orders write the same resource",
                ));
            }
        }
    }
    for (resource, sources) in &invalidators {
        if let Some(consumers) = readers.get(resource) {
            for source in sources {
                for consumer in consumers {
                    if source.id() != consumer.id() {
                        edges.insert(edge(
                            WorkOrderEdgeKind::Invalidation,
                            source.id(),
                            consumer.id(),
                            Some(resource.clone()),
                            "committing the source invalidates a resource read or required by the target",
                        ));
                    }
                }
            }
        }
    }
    derive_overlap_edges(orders, &mut edges);
    for prerequisite in &constraints.prerequisites {
        edges.insert(edge(
            WorkOrderEdgeKind::Prerequisite,
            &prerequisite.before,
            &prerequisite.after,
            None,
            &prerequisite.reason,
        ));
    }
    for pair in &constraints.mutually_exclusive_recipes {
        let left = orders
            .iter()
            .filter(|order| order.recipe().name == pair.left)
            .collect::<Vec<_>>();
        let right = orders
            .iter()
            .filter(|order| order.recipe().name == pair.right)
            .collect::<Vec<_>>();
        for left in &left {
            for right in &right {
                if left.target().path == right.target().path
                    && spans_overlap(left.target().span, right.target().span)
                {
                    edges.insert(canonical_undirected_edge(
                        WorkOrderEdgeKind::MutuallyExclusiveRecipe,
                        left.id(),
                        right.id(),
                        None,
                        "two recipe alternatives target the same owned syntax",
                    ));
                }
            }
        }
    }
    Ok(edges.into_iter().collect())
}

fn derive_overlap_edges(orders: &[SharedWorkOrder], edges: &mut BTreeSet<WorkOrderEdge>) {
    let mut by_path = BTreeMap::<&std::path::Path, Vec<&SharedWorkOrder>>::new();
    for order in orders {
        by_path.entry(&order.target().path).or_default().push(order);
    }
    for path_orders in by_path.values_mut() {
        path_orders.sort_by_key(|order| {
            (
                order.target().span.start_byte,
                order.target().span.end_byte,
                order.id(),
            )
        });
        for left_index in 0..path_orders.len() {
            let left = path_orders[left_index];
            for right in &path_orders[left_index + 1..] {
                if right.target().span.start_byte >= left.target().span.end_byte {
                    break;
                }
                if spans_overlap(left.target().span, right.target().span) {
                    edges.insert(canonical_undirected_edge(
                        WorkOrderEdgeKind::Conflict,
                        left.id(),
                        right.id(),
                        Some(WorkOrderResource {
                            kind: WorkOrderResourceKind::SourceSpan,
                            identity: format!("overlap:{}", left.target().path.display()),
                        }),
                        "exact target spans overlap",
                    ));
                }
            }
        }
    }
}

fn spans_overlap(left: deslop_core::Span, right: deslop_core::Span) -> bool {
    left.start_byte < right.end_byte && right.start_byte < left.end_byte
}

fn edge(
    kind: WorkOrderEdgeKind,
    from: &SharedWorkOrderId,
    to: &SharedWorkOrderId,
    resource: Option<WorkOrderResource>,
    reason: &str,
) -> WorkOrderEdge {
    WorkOrderEdge {
        kind,
        from: from.clone(),
        to: to.clone(),
        resource,
        reason: reason.into(),
    }
}

fn canonical_undirected_edge(
    kind: WorkOrderEdgeKind,
    left: &SharedWorkOrderId,
    right: &SharedWorkOrderId,
    resource: Option<WorkOrderResource>,
    reason: &str,
) -> WorkOrderEdge {
    let (from, to) = if left < right {
        (left, right)
    } else {
        (right, left)
    };
    edge(kind, from, to, resource, reason)
}

fn reject_atomic_conflicts(
    edges: &[WorkOrderEdge],
    group_by_order: &BTreeMap<SharedWorkOrderId, AtomicWorkGroupId>,
) -> Result<()> {
    for edge in edges {
        if matches!(
            edge.kind,
            WorkOrderEdgeKind::Conflict | WorkOrderEdgeKind::MutuallyExclusiveRecipe
        ) && group_by_order[&edge.from] == group_by_order[&edge.to]
        {
            bail!("an atomic work group contains conflicting or mutually exclusive orders");
        }
    }
    Ok(())
}

fn schedule_groups(
    groups: &[AtomicWorkGroup],
    edges: &[WorkOrderEdge],
    group_by_order: &BTreeMap<SharedWorkOrderId, AtomicWorkGroupId>,
) -> Result<(Vec<BlockedWorkGroup>, Vec<WorkOrderScheduleWave>)> {
    let mut prerequisite = BTreeMap::<AtomicWorkGroupId, BTreeSet<AtomicWorkGroupId>>::new();
    let mut conflicts = BTreeMap::<AtomicWorkGroupId, BTreeSet<AtomicWorkGroupId>>::new();
    let mut mutually_exclusive = BTreeSet::new();
    for group in groups {
        prerequisite.entry(group.id.clone()).or_default();
        conflicts.entry(group.id.clone()).or_default();
    }
    for edge in edges {
        let from = group_by_order[&edge.from].clone();
        let to = group_by_order[&edge.to].clone();
        if from == to {
            continue;
        }
        match edge.kind {
            WorkOrderEdgeKind::Prerequisite => {
                prerequisite.entry(from).or_default().insert(to);
            }
            WorkOrderEdgeKind::Conflict | WorkOrderEdgeKind::Invalidation => {
                conflicts
                    .entry(from.clone())
                    .or_default()
                    .insert(to.clone());
                conflicts.entry(to).or_default().insert(from);
            }
            WorkOrderEdgeKind::MutuallyExclusiveRecipe => {
                mutually_exclusive.insert(from.clone());
                mutually_exclusive.insert(to.clone());
                conflicts
                    .entry(from.clone())
                    .or_default()
                    .insert(to.clone());
                conflicts.entry(to).or_default().insert(from);
            }
        }
    }

    let cycles = strongly_connected_components(&prerequisite)
        .into_iter()
        .filter(|component| component.len() > 1)
        .collect::<Vec<_>>();
    let mut blocked_reason =
        BTreeMap::<AtomicWorkGroupId, (WorkOrderBlockReason, Vec<AtomicWorkGroupId>)>::new();
    for component in cycles {
        for group in &component {
            blocked_reason.insert(
                group.clone(),
                (WorkOrderBlockReason::PlanningCycle, component.clone()),
            );
        }
    }
    for group in mutually_exclusive {
        let related = conflicts[&group]
            .iter()
            .filter(|peer| conflicts[*peer].contains(&group))
            .cloned()
            .collect();
        blocked_reason
            .entry(group)
            .or_insert((WorkOrderBlockReason::MutuallyExclusiveChoice, related));
    }
    loop {
        let mut newly_blocked = Vec::new();
        for (from, targets) in &prerequisite {
            if blocked_reason.contains_key(from) {
                for target in targets {
                    if !blocked_reason.contains_key(target) {
                        newly_blocked.push((target.clone(), from.clone()));
                    }
                }
            }
        }
        if newly_blocked.is_empty() {
            break;
        }
        for (target, source) in newly_blocked {
            blocked_reason
                .entry(target)
                .or_insert((WorkOrderBlockReason::BlockedPrerequisite, vec![source]));
        }
    }

    let active = groups
        .iter()
        .map(|group| group.id.clone())
        .filter(|group| !blocked_reason.contains_key(group))
        .collect::<BTreeSet<_>>();
    let mut indegree = active
        .iter()
        .cloned()
        .map(|group| (group, 0usize))
        .collect::<BTreeMap<_, _>>();
    for (from, targets) in &prerequisite {
        if !active.contains(from) {
            continue;
        }
        for target in targets {
            if active.contains(target) {
                *indegree
                    .get_mut(target)
                    .expect("active target has indegree") += 1;
            }
        }
    }

    let mut remaining = active;
    let mut waves = Vec::new();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .filter(|group| indegree[*group] == 0)
            .cloned()
            .collect::<Vec<_>>();
        if ready.is_empty() {
            bail!("active work-order condensation unexpectedly contains a cycle");
        }
        let mut wave = Vec::new();
        for group in ready {
            if wave
                .iter()
                .all(|selected| !conflicts[&group].contains(selected))
            {
                wave.push(group);
            }
        }
        for group in &wave {
            remaining.remove(group);
            for target in &prerequisite[group] {
                if remaining.contains(target) {
                    *indegree
                        .get_mut(target)
                        .expect("remaining target has indegree") -= 1;
                }
            }
        }
        waves.push(WorkOrderScheduleWave {
            ordinal: waves.len(),
            groups: wave,
        });
    }

    let blocked = blocked_reason
        .into_iter()
        .map(|(group, (reason, mut related))| {
            related.sort();
            related.dedup();
            BlockedWorkGroup {
                group,
                reason,
                related,
            }
        })
        .collect();
    Ok((blocked, waves))
}

fn strongly_connected_components(
    graph: &BTreeMap<AtomicWorkGroupId, BTreeSet<AtomicWorkGroupId>>,
) -> Vec<Vec<AtomicWorkGroupId>> {
    struct Tarjan<'a> {
        graph: &'a BTreeMap<AtomicWorkGroupId, BTreeSet<AtomicWorkGroupId>>,
        next: usize,
        index: BTreeMap<AtomicWorkGroupId, usize>,
        low: BTreeMap<AtomicWorkGroupId, usize>,
        stack: Vec<AtomicWorkGroupId>,
        on_stack: BTreeSet<AtomicWorkGroupId>,
        output: Vec<Vec<AtomicWorkGroupId>>,
    }

    fn visit(node: AtomicWorkGroupId, state: &mut Tarjan<'_>) {
        let index = state.next;
        state.next += 1;
        state.index.insert(node.clone(), index);
        state.low.insert(node.clone(), index);
        state.stack.push(node.clone());
        state.on_stack.insert(node.clone());

        for target in &state.graph[&node] {
            if !state.index.contains_key(target) {
                visit(target.clone(), state);
                state
                    .low
                    .insert(node.clone(), state.low[&node].min(state.low[target]));
            } else if state.on_stack.contains(target) {
                state
                    .low
                    .insert(node.clone(), state.low[&node].min(state.index[target]));
            }
        }
        if state.low[&node] == state.index[&node] {
            let mut component = Vec::new();
            while let Some(member) = state.stack.pop() {
                state.on_stack.remove(&member);
                component.push(member.clone());
                if member == node {
                    break;
                }
            }
            component.sort();
            state.output.push(component);
        }
    }

    let mut state = Tarjan {
        graph,
        next: 0,
        index: BTreeMap::new(),
        low: BTreeMap::new(),
        stack: Vec::new(),
        on_stack: BTreeSet::new(),
        output: Vec::new(),
    };
    for node in graph.keys() {
        if !state.index.contains_key(node) {
            visit(node.clone(), &mut state);
        }
    }
    state.output.sort();
    state.output
}

fn validate_digest(value: &str, prefix: &str) -> Result<()> {
    let digest = value
        .strip_prefix(prefix)
        .filter(|digest| digest.len() == 64)
        .ok_or_else(|| anyhow::anyhow!("invalid work-order planner identity"))?;
    if !digest
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("invalid work-order planner identity");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use deslop_recipes::detect_rust_recipes;
    use tempfile::tempdir;

    use super::*;
    use crate::shared_transformation_work_orders;

    fn orders() -> Vec<SharedWorkOrder> {
        let temp = tempdir().unwrap();
        fs::write(
            temp.path().join("sample.rs"),
            "fn sample() { return; 7; }\nfn other() { return; 8; }\n",
        )
        .unwrap();
        let candidates = detect_rust_recipes(temp.path(), &[PathBuf::from("sample.rs")]).unwrap();
        shared_transformation_work_orders(candidates).unwrap()
    }

    #[test]
    fn overlapping_orders_conflict_and_are_serialized_deterministically() {
        let mut orders = orders();
        assert_eq!(orders.len(), 2);
        let mut second = serde_json::to_value(&orders[1]).unwrap();
        second["subject"]["candidate"]["target"]["span"] =
            serde_json::to_value(orders[0].target().span).unwrap();
        assert!(serde_json::from_value::<SharedWorkOrder>(second).is_err());

        // Exact fixture targets are disjoint, but both invalidate their pinned snapshot. Graph
        // commits therefore serialize unless the caller declares one genuine atomic group.
        let plan = plan_work_orders(std::mem::take(&mut orders), Default::default()).unwrap();
        assert!(plan.blocked().is_empty());
        assert_eq!(plan.waves().len(), 2);
        assert!(plan.waves().iter().all(|wave| wave.groups.len() == 1));
        let wire = serde_json::to_value(&plan).unwrap();
        assert_eq!(serde_json::from_value::<WorkOrderPlan>(wire).unwrap(), plan);
    }

    #[test]
    fn unresolved_prerequisite_cycle_blocks_exact_scc() {
        let orders = orders();
        let left = orders[0].id().clone();
        let right = orders[1].id().clone();
        let plan = plan_work_orders(
            orders,
            WorkOrderPlannerConstraints {
                prerequisites: vec![
                    ExplicitPrerequisite {
                        before: left.clone(),
                        after: right.clone(),
                        reason: "first direction".into(),
                    },
                    ExplicitPrerequisite {
                        before: right,
                        after: left,
                        reason: "second direction".into(),
                    },
                ],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(plan.blocked().len(), 2);
        assert!(
            plan.blocked()
                .iter()
                .all(|block| block.reason == WorkOrderBlockReason::PlanningCycle)
        );
        assert!(plan.waves().is_empty());
    }

    #[test]
    fn atomic_group_collapses_without_using_order_to_break_cycles() {
        let orders = orders();
        let members = orders.iter().map(|order| order.id().clone()).collect();
        let plan = plan_work_orders(
            orders,
            WorkOrderPlannerConstraints {
                atomic_groups: vec![members],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(plan.groups().len(), 1);
        assert_eq!(plan.groups()[0].members.len(), 2);
        assert_eq!(plan.waves().len(), 1);
    }
}
