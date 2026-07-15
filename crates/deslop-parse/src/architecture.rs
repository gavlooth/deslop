use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::Arc;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    DependencyEdgeKey, DependencyEdgeKind, DependencyGapKey, DependencyNodeKey, DependencyNodeKind,
    DependencyProjection, FactCoverage, ProjectionId,
};

pub const ARCHITECTURE_SCHEMA: &str = "deslop.architecture/1";
pub const ARCHITECTURE_POLICY_SCHEMA: &str = "deslop.architecture-policy/1";

const POLICY_DOMAIN: &str = "deslop architecture policy v1";
const RULE_DOMAIN: &str = "deslop architecture rule v1";
const COMPONENT_DOMAIN: &str = "deslop architecture component v1";
const EDGE_DOMAIN: &str = "deslop architecture condensation edge v1";
const VIOLATION_DOMAIN: &str = "deslop architecture violation v1";
const GAP_DOMAIN: &str = "deslop architecture gap v1";

macro_rules! digest_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                validate_digest(&value, $prefix).map_err(D::Error::custom)?;
                Ok(Self(value))
            }
        }
    };
}

digest_id!(ArchitecturePolicyId, "arp1_");
digest_id!(ArchitectureRuleKey, "arr1_");
digest_id!(ArchitectureComponentKey, "arc1_");
digest_id!(ArchitectureCondensationEdgeKey, "are1_");
digest_id!(ArchitectureViolationKey, "arv1_");
digest_id!(ArchitectureGapKey, "arg1_");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArchitectureLevel {
    File,
    Module,
    Package,
    BuildTarget,
}

impl ArchitectureLevel {
    const ALL: [Self; 4] = [Self::File, Self::Module, Self::Package, Self::BuildTarget];
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ArchitectureNodeClass {
    Structural { level: ArchitectureLevel },
    LocalApi,
    ExternalApi,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureRatio {
    numerator: usize,
    denominator: usize,
}

impl ArchitectureRatio {
    pub fn numerator(&self) -> usize {
        self.numerator
    }

    pub fn denominator(&self) -> usize {
        self.denominator
    }

    pub fn as_f64(&self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }

    fn new(numerator: usize, denominator: usize) -> Option<Self> {
        (denominator != 0).then_some(Self {
            numerator,
            denominator,
        })
    }

    fn validate(&self) -> Result<(), ArchitectureBuildError> {
        if self.denominator == 0 || self.numerator > self.denominator {
            Err(invalid("architecture ratio is not canonical"))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureLayerAssignment {
    pub node: DependencyNodeKey,
    pub layer: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ArchitectureRuleKind {
    ForbidDependency {
        from: DependencyNodeKey,
        to: DependencyNodeKey,
        transitive: bool,
    },
    ForbidCycles {
        level: ArchitectureLevel,
    },
    RequireLayerDescent {
        level: ArchitectureLevel,
        require_total: bool,
    },
    RequireStableDependencies {
        level: ArchitectureLevel,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchitectureRuleDraft {
    pub name: String,
    pub kind: ArchitectureRuleKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureRule {
    key: ArchitectureRuleKey,
    name: String,
    kind: ArchitectureRuleKind,
}

impl ArchitectureRule {
    pub fn key(&self) -> &ArchitectureRuleKey {
        &self.key
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> &ArchitectureRuleKind {
        &self.kind
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitecturePolicy {
    schema: String,
    id: ArchitecturePolicyId,
    layer_assignments: Vec<ArchitectureLayerAssignment>,
    rules: Vec<ArchitectureRule>,
}

impl ArchitecturePolicy {
    pub fn new(
        mut layer_assignments: Vec<ArchitectureLayerAssignment>,
        drafts: Vec<ArchitectureRuleDraft>,
    ) -> Result<Self, ArchitectureBuildError> {
        layer_assignments.sort();
        if layer_assignments
            .windows(2)
            .any(|pair| pair[0].node >= pair[1].node)
        {
            return Err(invalid("architecture layer assignments are duplicated"));
        }
        let mut rules = drafts
            .into_iter()
            .map(|draft| make_rule(draft.name, draft.kind))
            .collect::<Result<Vec<_>, _>>()?;
        rules.sort_by(|left, right| left.key.cmp(&right.key));
        let mut policy = Self {
            schema: ARCHITECTURE_POLICY_SCHEMA.into(),
            id: ArchitecturePolicyId(String::new()),
            layer_assignments,
            rules,
        };
        policy.id = make_policy_id(&policy.layer_assignments, &policy.rules)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn id(&self) -> &ArchitecturePolicyId {
        &self.id
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn layer_assignments(&self) -> &[ArchitectureLayerAssignment] {
        &self.layer_assignments
    }

    pub fn rules(&self) -> &[ArchitectureRule] {
        &self.rules
    }

    fn validate(&self) -> Result<(), ArchitectureBuildError> {
        if self.schema != ARCHITECTURE_POLICY_SCHEMA {
            return Err(invalid("unsupported architecture policy schema"));
        }
        if self
            .layer_assignments
            .windows(2)
            .any(|pair| pair[0].node >= pair[1].node)
        {
            return Err(invalid("architecture layer assignments are not canonical"));
        }
        validate_sorted("architecture rules", &self.rules, |rule| rule.key.as_str())?;
        let mut names = BTreeSet::new();
        for rule in &self.rules {
            validate_text(&rule.name)?;
            if !names.insert(&rule.name) {
                return Err(invalid("architecture rule names are duplicated"));
            }
            if rule.key != make_rule_key(&rule.name, &rule.kind)? {
                return Err(invalid("architecture rule key does not match payload"));
            }
            if matches!(
                &rule.kind,
                ArchitectureRuleKind::ForbidDependency { from, to, .. } if from == to
            ) {
                return Err(invalid(
                    "a forbidden dependency rule must name distinct endpoints; use a cycle rule",
                ));
            }
        }
        if self.id != make_policy_id(&self.layer_assignments, &self.rules)? {
            return Err(invalid(
                "architecture policy identity does not match payload",
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchitecturePolicyWire {
    schema: String,
    id: ArchitecturePolicyId,
    layer_assignments: Vec<ArchitectureLayerAssignment>,
    rules: Vec<ArchitectureRule>,
}

impl<'de> Deserialize<'de> for ArchitecturePolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ArchitecturePolicyWire::deserialize(deserializer)?;
        let policy = Self {
            schema: wire.schema,
            id: wire.id,
            layer_assignments: wire.layer_assignments,
            rules: wire.rules,
        };
        policy.validate().map_err(D::Error::custom)?;
        Ok(policy)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureNodeMetrics {
    node: DependencyNodeKey,
    class: ArchitectureNodeClass,
    dependency_fan_in: usize,
    dependency_fan_out: usize,
    api_users: usize,
    api_uses: usize,
    instability: Option<ArchitectureRatio>,
    coverage: FactCoverage,
}

impl ArchitectureNodeMetrics {
    pub fn node(&self) -> &DependencyNodeKey {
        &self.node
    }
    pub fn class(&self) -> &ArchitectureNodeClass {
        &self.class
    }
    pub fn dependency_fan_in(&self) -> usize {
        self.dependency_fan_in
    }
    pub fn dependency_fan_out(&self) -> usize {
        self.dependency_fan_out
    }
    pub fn api_users(&self) -> usize {
        self.api_users
    }
    pub fn api_uses(&self) -> usize {
        self.api_uses
    }
    pub fn instability(&self) -> Option<&ArchitectureRatio> {
        self.instability.as_ref()
    }
    pub fn coverage(&self) -> FactCoverage {
        self.coverage
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureComponent {
    key: ArchitectureComponentKey,
    level: ArchitectureLevel,
    members: Vec<DependencyNodeKey>,
    cyclic: bool,
    layer: u32,
    coverage: FactCoverage,
}

impl ArchitectureComponent {
    pub fn key(&self) -> &ArchitectureComponentKey {
        &self.key
    }
    pub fn level(&self) -> ArchitectureLevel {
        self.level
    }
    pub fn members(&self) -> &[DependencyNodeKey] {
        &self.members
    }
    pub fn cyclic(&self) -> bool {
        self.cyclic
    }
    pub fn layer(&self) -> u32 {
        self.layer
    }
    pub fn coverage(&self) -> FactCoverage {
        self.coverage
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureCondensationEdge {
    key: ArchitectureCondensationEdgeKey,
    level: ArchitectureLevel,
    from: ArchitectureComponentKey,
    to: ArchitectureComponentKey,
    source_edges: Vec<DependencyEdgeKey>,
}

impl ArchitectureCondensationEdge {
    pub fn key(&self) -> &ArchitectureCondensationEdgeKey {
        &self.key
    }
    pub fn level(&self) -> ArchitectureLevel {
        self.level
    }
    pub fn from(&self) -> &ArchitectureComponentKey {
        &self.from
    }
    pub fn to(&self) -> &ArchitectureComponentKey {
        &self.to
    }
    pub fn source_edges(&self) -> &[DependencyEdgeKey] {
        &self.source_edges
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ArchitectureViolationKind {
    ForbiddenDependency {
        from: DependencyNodeKey,
        to: DependencyNodeKey,
        path: Vec<DependencyNodeKey>,
        edges: Vec<DependencyEdgeKey>,
    },
    Cycle {
        component: ArchitectureComponentKey,
    },
    LayerDirection {
        edge: DependencyEdgeKey,
        from: DependencyNodeKey,
        to: DependencyNodeKey,
        from_layer: u32,
        to_layer: u32,
    },
    StableDirection {
        edge: DependencyEdgeKey,
        from: DependencyNodeKey,
        to: DependencyNodeKey,
        from_instability: ArchitectureRatio,
        to_instability: ArchitectureRatio,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureViolation {
    key: ArchitectureViolationKey,
    rule: ArchitectureRuleKey,
    kind: ArchitectureViolationKind,
}

impl ArchitectureViolation {
    pub fn key(&self) -> &ArchitectureViolationKey {
        &self.key
    }
    pub fn rule(&self) -> &ArchitectureRuleKey {
        &self.rule
    }
    pub fn kind(&self) -> &ArchitectureViolationKind {
        &self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ArchitectureGapKind {
    SourceDependency {
        gap: DependencyGapKey,
    },
    LayerAssignmentEndpointMissing {
        node: DependencyNodeKey,
    },
    RuleEndpointMissing {
        rule: ArchitectureRuleKey,
        node: DependencyNodeKey,
    },
    MissingLayerAssignment {
        rule: ArchitectureRuleKey,
        node: DependencyNodeKey,
    },
    RuleRequiresCompleteTopology {
        rule: ArchitectureRuleKey,
        actual: FactCoverage,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureGap {
    key: ArchitectureGapKey,
    kind: ArchitectureGapKind,
}

impl ArchitectureGap {
    pub fn key(&self) -> &ArchitectureGapKey {
        &self.key
    }
    pub fn kind(&self) -> &ArchitectureGapKind {
        &self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureCoverageEvidence {
    status: FactCoverage,
    reasons: Vec<String>,
}

impl ArchitectureCoverageEvidence {
    pub fn status(&self) -> FactCoverage {
        self.status
    }
    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureDocument {
    schema: String,
    projection_id: ProjectionId,
    dependency_projection_id: ProjectionId,
    dependency_policy: crate::DependencyPolicyId,
    policy: ArchitecturePolicy,
    source_coverage: FactCoverage,
    source_reasons: Vec<String>,
    coverage: ArchitectureCoverageEvidence,
    metrics: Vec<ArchitectureNodeMetrics>,
    components: Vec<ArchitectureComponent>,
    condensation_edges: Vec<ArchitectureCondensationEdge>,
    violations: Vec<ArchitectureViolation>,
    gaps: Vec<ArchitectureGap>,
}

impl ArchitectureDocument {
    pub fn schema(&self) -> &str {
        &self.schema
    }
    pub fn projection_id(&self) -> &ProjectionId {
        &self.projection_id
    }
    pub fn dependency_projection_id(&self) -> &ProjectionId {
        &self.dependency_projection_id
    }
    pub fn dependency_policy(&self) -> &crate::DependencyPolicyId {
        &self.dependency_policy
    }
    pub fn policy(&self) -> &ArchitecturePolicy {
        &self.policy
    }
    pub fn source_coverage(&self) -> FactCoverage {
        self.source_coverage
    }
    pub fn source_reasons(&self) -> &[String] {
        &self.source_reasons
    }
    pub fn coverage(&self) -> &ArchitectureCoverageEvidence {
        &self.coverage
    }
    pub fn metrics(&self) -> &[ArchitectureNodeMetrics] {
        &self.metrics
    }
    pub fn components(&self) -> &[ArchitectureComponent] {
        &self.components
    }
    pub fn condensation_edges(&self) -> &[ArchitectureCondensationEdge] {
        &self.condensation_edges
    }
    pub fn violations(&self) -> &[ArchitectureViolation] {
        &self.violations
    }
    pub fn gaps(&self) -> &[ArchitectureGap] {
        &self.gaps
    }

    fn validate(&self) -> Result<(), ArchitectureBuildError> {
        if self.schema != ARCHITECTURE_SCHEMA {
            return Err(invalid("unsupported architecture schema"));
        }
        validate_digest(self.projection_id.as_str(), "pj1_")?;
        validate_digest(self.dependency_projection_id.as_str(), "pj1_")?;
        self.policy.validate()?;
        if !strictly_sorted(&self.source_reasons) && !self.source_reasons.is_empty() {
            return Err(invalid("architecture source reasons are not canonical"));
        }
        validate_sorted("architecture metrics", &self.metrics, |metric| {
            metric.node.as_str()
        })?;
        validate_sorted("architecture components", &self.components, |component| {
            component.key.as_str()
        })?;
        validate_sorted(
            "architecture condensation edges",
            &self.condensation_edges,
            |edge| edge.key.as_str(),
        )?;
        validate_sorted("architecture violations", &self.violations, |violation| {
            violation.key.as_str()
        })?;
        validate_sorted("architecture gaps", &self.gaps, |gap| gap.key.as_str())?;
        let metric_nodes = self
            .metrics
            .iter()
            .map(|metric| &metric.node)
            .collect::<BTreeSet<_>>();
        let metric_classes = self
            .metrics
            .iter()
            .map(|metric| (&metric.node, &metric.class))
            .collect::<BTreeMap<_, _>>();
        for metric in &self.metrics {
            if let Some(ratio) = &metric.instability {
                ratio.validate()?;
            }
            let expected = ArchitectureRatio::new(
                metric.dependency_fan_out,
                metric.dependency_fan_in + metric.dependency_fan_out,
            );
            if metric.instability != expected {
                return Err(invalid(
                    "architecture instability does not match fan counts",
                ));
            }
            if metric.coverage != self.source_coverage {
                return Err(invalid(
                    "architecture metric coverage does not match source",
                ));
            }
        }
        let mut member_owner = BTreeMap::new();
        let component_keys = self
            .components
            .iter()
            .map(|component| &component.key)
            .collect::<BTreeSet<_>>();
        for component in &self.components {
            if component.members.is_empty()
                || (!strictly_sorted(&component.members) && component.members.len() > 1)
            {
                return Err(invalid("architecture component members are not canonical"));
            }
            if component.cyclic != (component.members.len() > 1) {
                return Err(invalid("architecture component cycle flag is invalid"));
            }
            if component.coverage != self.source_coverage {
                return Err(invalid(
                    "architecture component coverage does not match source",
                ));
            }
            if component.key != make_component_key(self.dependency_projection_id(), component)? {
                return Err(invalid("architecture component key does not match payload"));
            }
            for member in &component.members {
                if !metric_nodes.contains(member)
                    || metric_classes.get(member)
                        != Some(&&ArchitectureNodeClass::Structural {
                            level: component.level,
                        })
                    || member_owner.insert(member, &component.key).is_some()
                {
                    return Err(invalid("architecture component membership is invalid"));
                }
            }
        }
        let structural = self
            .metrics
            .iter()
            .filter_map(|metric| match metric.class {
                ArchitectureNodeClass::Structural { .. } => Some(&metric.node),
                ArchitectureNodeClass::LocalApi | ArchitectureNodeClass::ExternalApi => None,
            })
            .collect::<BTreeSet<_>>();
        if structural != member_owner.keys().copied().collect() {
            return Err(invalid(
                "architecture components do not partition structural nodes",
            ));
        }
        let mut outgoing =
            BTreeMap::<&ArchitectureComponentKey, Vec<&ArchitectureComponentKey>>::new();
        let component_levels = self
            .components
            .iter()
            .map(|component| (&component.key, component.level))
            .collect::<BTreeMap<_, _>>();
        let mut condensation_pairs = BTreeSet::new();
        for edge in &self.condensation_edges {
            if edge.from == edge.to
                || !component_keys.contains(&edge.from)
                || !component_keys.contains(&edge.to)
            {
                return Err(invalid(
                    "architecture condensation edge endpoints are invalid",
                ));
            }
            if component_levels[&edge.from] != edge.level
                || component_levels[&edge.to] != edge.level
                || !condensation_pairs.insert((&edge.from, &edge.to))
            {
                return Err(invalid(
                    "architecture condensation edge level or uniqueness is invalid",
                ));
            }
            if edge.source_edges.is_empty()
                || (!strictly_sorted(&edge.source_edges) && edge.source_edges.len() > 1)
            {
                return Err(invalid(
                    "architecture condensation evidence is not canonical",
                ));
            }
            if edge.key != make_condensation_edge_key(self.dependency_projection_id(), edge)? {
                return Err(invalid(
                    "architecture condensation edge key does not match payload",
                ));
            }
            outgoing.entry(&edge.from).or_default().push(&edge.to);
        }
        let layers = self
            .components
            .iter()
            .map(|component| (&component.key, component.layer))
            .collect::<BTreeMap<_, _>>();
        for component in &self.components {
            let expected = outgoing
                .get(&component.key)
                .map(|targets| {
                    targets
                        .iter()
                        .map(|target| layers[target] + 1)
                        .max()
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            if component.layer != expected {
                return Err(invalid("architecture component layer is invalid"));
            }
        }
        let rule_keys = self
            .policy
            .rules
            .iter()
            .map(|rule| &rule.key)
            .collect::<BTreeSet<_>>();
        for violation in &self.violations {
            if !rule_keys.contains(&violation.rule)
                || violation.key
                    != make_violation_key(&self.policy.id, &violation.rule, &violation.kind)?
            {
                return Err(invalid(
                    "architecture violation identity or rule is invalid",
                ));
            }
        }
        for gap in &self.gaps {
            match &gap.kind {
                ArchitectureGapKind::RuleEndpointMissing { rule, .. }
                | ArchitectureGapKind::MissingLayerAssignment { rule, .. }
                | ArchitectureGapKind::RuleRequiresCompleteTopology { rule, .. }
                    if !rule_keys.contains(rule) =>
                {
                    return Err(invalid("architecture gap references an absent rule"));
                }
                ArchitectureGapKind::SourceDependency { .. }
                | ArchitectureGapKind::LayerAssignmentEndpointMissing { .. }
                | ArchitectureGapKind::RuleEndpointMissing { .. }
                | ArchitectureGapKind::MissingLayerAssignment { .. }
                | ArchitectureGapKind::RuleRequiresCompleteTopology { .. } => {}
            }
            if gap.key != make_gap_key(&self.policy.id, &gap.kind)? {
                return Err(invalid("architecture gap key does not match payload"));
            }
        }
        if self.coverage != make_coverage(self.source_coverage, &self.source_reasons, &self.gaps) {
            return Err(invalid("architecture coverage does not match gaps"));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchitectureDocumentWire {
    schema: String,
    projection_id: ProjectionId,
    dependency_projection_id: ProjectionId,
    dependency_policy: crate::DependencyPolicyId,
    policy: ArchitecturePolicy,
    source_coverage: FactCoverage,
    source_reasons: Vec<String>,
    coverage: ArchitectureCoverageEvidence,
    metrics: Vec<ArchitectureNodeMetrics>,
    components: Vec<ArchitectureComponent>,
    condensation_edges: Vec<ArchitectureCondensationEdge>,
    violations: Vec<ArchitectureViolation>,
    gaps: Vec<ArchitectureGap>,
}

impl<'de> Deserialize<'de> for ArchitectureDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ArchitectureDocumentWire::deserialize(deserializer)?;
        let document = Self {
            schema: wire.schema,
            projection_id: wire.projection_id,
            dependency_projection_id: wire.dependency_projection_id,
            dependency_policy: wire.dependency_policy,
            policy: wire.policy,
            source_coverage: wire.source_coverage,
            source_reasons: wire.source_reasons,
            coverage: wire.coverage,
            metrics: wire.metrics,
            components: wire.components,
            condensation_edges: wire.condensation_edges,
            violations: wire.violations,
            gaps: wire.gaps,
        };
        document.validate().map_err(D::Error::custom)?;
        Ok(document)
    }
}

#[derive(Debug, Clone)]
pub struct ArchitectureProjection {
    id: ProjectionId,
    dependency: Arc<DependencyProjection>,
    policy: ArchitecturePolicy,
    document: ArchitectureDocument,
}

impl ArchitectureProjection {
    pub fn id(&self) -> &ProjectionId {
        &self.id
    }
    pub fn dependency(&self) -> &Arc<DependencyProjection> {
        &self.dependency
    }
    pub fn policy(&self) -> &ArchitecturePolicy {
        &self.policy
    }
    pub fn document(&self) -> &ArchitectureDocument {
        &self.document
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchitectureBuildError {
    Invalid(String),
    Identity(String),
}

impl fmt::Display for ArchitectureBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "invalid architecture evidence: {detail}"),
            Self::Identity(detail) => write!(formatter, "architecture identity error: {detail}"),
        }
    }
}

impl std::error::Error for ArchitectureBuildError {}

#[derive(Debug)]
struct ComponentDraft {
    level: ArchitectureLevel,
    members: Vec<DependencyNodeKey>,
    cyclic: bool,
    layer: u32,
}

pub fn derive_architecture(
    dependency: Arc<DependencyProjection>,
    policy: ArchitecturePolicy,
) -> Result<ArchitectureProjection, ArchitectureBuildError> {
    policy.validate()?;
    let source = dependency.document();
    let mut classes = BTreeMap::new();
    for node in source.nodes() {
        classes.insert(node.key().clone(), node_class(node.kind()));
    }
    let structural = classes
        .iter()
        .filter_map(|(node, class)| match class {
            ArchitectureNodeClass::Structural { level } => Some((node.clone(), *level)),
            ArchitectureNodeClass::LocalApi | ArchitectureNodeClass::ExternalApi => None,
        })
        .collect::<BTreeMap<_, _>>();
    let mut adjacency = structural
        .keys()
        .cloned()
        .map(|node| (node, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut reverse = adjacency.clone();
    let mut api_uses = BTreeMap::<DependencyNodeKey, usize>::new();
    let mut api_users = BTreeMap::<DependencyNodeKey, usize>::new();
    let mut direct_edges =
        BTreeMap::<(DependencyNodeKey, DependencyNodeKey), Vec<DependencyEdgeKey>>::new();
    for edge in source.edges() {
        if let Some(level) = dependency_edge_level(edge.kind()) {
            let from_level = structural.get(edge.from());
            let to_level = structural.get(edge.to());
            if from_level != Some(&level) || to_level != Some(&level) {
                return Err(invalid(
                    "dependency edge endpoints do not match their architecture level",
                ));
            }
            adjacency
                .get_mut(edge.from())
                .expect("structural source")
                .insert(edge.to().clone());
            reverse
                .get_mut(edge.to())
                .expect("structural target")
                .insert(edge.from().clone());
            direct_edges
                .entry((edge.from().clone(), edge.to().clone()))
                .or_default()
                .push(edge.key().clone());
        } else if edge.kind() == DependencyEdgeKind::ApiUse {
            if !matches!(
                classes.get(edge.from()),
                Some(ArchitectureNodeClass::Structural {
                    level: ArchitectureLevel::File
                })
            ) || !matches!(
                classes.get(edge.to()),
                Some(ArchitectureNodeClass::LocalApi | ArchitectureNodeClass::ExternalApi)
            ) {
                return Err(invalid("API-use endpoints are not File -> API"));
            }
            *api_uses.entry(edge.from().clone()).or_default() += 1;
            *api_users.entry(edge.to().clone()).or_default() += 1;
        }
    }
    for edges in direct_edges.values_mut() {
        edges.sort();
        edges.dedup();
    }

    let source_coverage = source.coverage().status();
    let mut metrics = classes
        .iter()
        .map(|(node, class)| {
            let fan_out = adjacency.get(node).map_or(0, BTreeSet::len);
            let fan_in = reverse.get(node).map_or(0, BTreeSet::len);
            ArchitectureNodeMetrics {
                node: node.clone(),
                class: class.clone(),
                dependency_fan_in: fan_in,
                dependency_fan_out: fan_out,
                api_users: api_users.get(node).copied().unwrap_or(0),
                api_uses: api_uses.get(node).copied().unwrap_or(0),
                instability: ArchitectureRatio::new(fan_out, fan_in + fan_out),
                coverage: source_coverage,
            }
        })
        .collect::<Vec<_>>();
    metrics.sort_by(|left, right| left.node.cmp(&right.node));

    let mut drafts = Vec::new();
    for level in ArchitectureLevel::ALL {
        let nodes = structural
            .iter()
            .filter_map(|(node, actual)| (*actual == level).then_some(node.clone()))
            .collect::<Vec<_>>();
        for members in strongly_connected_components(&nodes, &adjacency, &reverse) {
            drafts.push(ComponentDraft {
                level,
                cyclic: members.len() > 1,
                members,
                layer: 0,
            });
        }
    }
    drafts.sort_by(|left, right| (left.level, &left.members).cmp(&(right.level, &right.members)));
    let mut component_by_node = BTreeMap::new();
    for (index, component) in drafts.iter().enumerate() {
        for member in &component.members {
            component_by_node.insert(member.clone(), index);
        }
    }
    let mut condensation_drafts =
        BTreeMap::<(usize, usize, ArchitectureLevel), BTreeSet<DependencyEdgeKey>>::new();
    for ((from, to), edges) in &direct_edges {
        let source_component = component_by_node[from];
        let target_component = component_by_node[to];
        if source_component != target_component {
            condensation_drafts
                .entry((source_component, target_component, structural[from]))
                .or_default()
                .extend(edges.iter().cloned());
        }
    }
    assign_layers(
        &mut drafts,
        condensation_drafts.keys().map(|(from, to, _)| (*from, *to)),
    )?;
    let mut components = Vec::with_capacity(drafts.len());
    let mut keys_by_index = Vec::with_capacity(drafts.len());
    for draft in &drafts {
        let mut component = ArchitectureComponent {
            key: ArchitectureComponentKey(String::new()),
            level: draft.level,
            members: draft.members.clone(),
            cyclic: draft.cyclic,
            layer: draft.layer,
            coverage: source_coverage,
        };
        component.key = make_component_key(dependency.id(), &component)?;
        keys_by_index.push(component.key.clone());
        components.push(component);
    }
    components.sort_by(|left, right| left.key.cmp(&right.key));
    let mut condensation_edges = condensation_drafts
        .into_iter()
        .map(|((from, to, level), source_edges)| {
            let mut edge = ArchitectureCondensationEdge {
                key: ArchitectureCondensationEdgeKey(String::new()),
                level,
                from: keys_by_index[from].clone(),
                to: keys_by_index[to].clone(),
                source_edges: source_edges.into_iter().collect(),
            };
            edge.key = make_condensation_edge_key(dependency.id(), &edge)?;
            Ok(edge)
        })
        .collect::<Result<Vec<_>, ArchitectureBuildError>>()?;
    condensation_edges.sort_by(|left, right| left.key.cmp(&right.key));

    let mut gap_kinds = source
        .gaps()
        .iter()
        .map(|gap| ArchitectureGapKind::SourceDependency {
            gap: gap.key().clone(),
        })
        .collect::<BTreeSet<_>>();
    for assignment in &policy.layer_assignments {
        if !structural.contains_key(&assignment.node) {
            gap_kinds.insert(ArchitectureGapKind::LayerAssignmentEndpointMissing {
                node: assignment.node.clone(),
            });
        }
    }
    let mut violation_kinds = BTreeSet::<(ArchitectureRuleKey, ArchitectureViolationKind)>::new();
    evaluate_rules(
        &policy,
        RuleEvaluationContext {
            source_coverage,
            structural: &structural,
            adjacency: &adjacency,
            direct_edges: &direct_edges,
            metrics: &metrics,
            components: &components,
        },
        &mut violation_kinds,
        &mut gap_kinds,
    )?;
    let mut violations = violation_kinds
        .into_iter()
        .map(|(rule, kind)| make_violation(&policy.id, rule, kind))
        .collect::<Result<Vec<_>, _>>()?;
    violations.sort_by(|left, right| left.key.cmp(&right.key));
    let mut gaps = gap_kinds
        .into_iter()
        .map(|kind| make_gap(&policy.id, kind))
        .collect::<Result<Vec<_>, _>>()?;
    gaps.sort_by(|left, right| left.key.cmp(&right.key));
    let mut source_reasons = source.coverage().reasons().to_vec();
    source_reasons.sort();
    source_reasons.dedup();
    let coverage = make_coverage(source_coverage, &source_reasons, &gaps);
    let payload = serde_json::to_vec(&(
        dependency.id(),
        &policy,
        &coverage,
        &metrics,
        &components,
        &condensation_edges,
        &violations,
        &gaps,
    ))
    .map_err(|error| ArchitectureBuildError::Identity(error.to_string()))?;
    let analysis = dependency.resolution().scope_graph().analysis();
    let id = analysis
        .derive_projection_id(
            ARCHITECTURE_SCHEMA,
            &payload,
            dependency.id().as_str().as_bytes(),
        )
        .map_err(|error| ArchitectureBuildError::Identity(error.to_string()))?;
    let document = ArchitectureDocument {
        schema: ARCHITECTURE_SCHEMA.into(),
        projection_id: id.clone(),
        dependency_projection_id: dependency.id().clone(),
        dependency_policy: dependency.policy().clone(),
        policy: policy.clone(),
        source_coverage,
        source_reasons,
        coverage,
        metrics,
        components,
        condensation_edges,
        violations,
        gaps,
    };
    document.validate()?;
    Ok(ArchitectureProjection {
        id,
        dependency,
        policy,
        document,
    })
}

struct RuleEvaluationContext<'a> {
    source_coverage: FactCoverage,
    structural: &'a BTreeMap<DependencyNodeKey, ArchitectureLevel>,
    adjacency: &'a BTreeMap<DependencyNodeKey, BTreeSet<DependencyNodeKey>>,
    direct_edges: &'a BTreeMap<(DependencyNodeKey, DependencyNodeKey), Vec<DependencyEdgeKey>>,
    metrics: &'a [ArchitectureNodeMetrics],
    components: &'a [ArchitectureComponent],
}

fn evaluate_rules(
    policy: &ArchitecturePolicy,
    context: RuleEvaluationContext<'_>,
    violations: &mut BTreeSet<(ArchitectureRuleKey, ArchitectureViolationKind)>,
    gaps: &mut BTreeSet<ArchitectureGapKind>,
) -> Result<(), ArchitectureBuildError> {
    let assignments = policy
        .layer_assignments
        .iter()
        .map(|assignment| (&assignment.node, assignment.layer))
        .collect::<BTreeMap<_, _>>();
    let metric_map = context
        .metrics
        .iter()
        .map(|metric| (&metric.node, metric))
        .collect::<BTreeMap<_, _>>();
    for rule in &policy.rules {
        match &rule.kind {
            ArchitectureRuleKind::ForbidDependency {
                from,
                to,
                transitive,
            } => {
                let missing = [from, to]
                    .into_iter()
                    .filter(|node| !context.structural.contains_key(*node))
                    .collect::<Vec<_>>();
                for node in missing {
                    gaps.insert(ArchitectureGapKind::RuleEndpointMissing {
                        rule: rule.key.clone(),
                        node: node.clone(),
                    });
                }
                if !context.structural.contains_key(from) || !context.structural.contains_key(to) {
                    continue;
                }
                if context.structural[from] != context.structural[to] {
                    return Err(invalid(
                        "forbidden dependency endpoints have different levels",
                    ));
                }
                let path = if *transitive {
                    find_path(from, to, context.adjacency)
                } else {
                    context.adjacency[from]
                        .contains(to)
                        .then(|| vec![from.clone(), to.clone()])
                };
                if let Some(path) = path {
                    let mut edges = Vec::new();
                    for pair in path.windows(2) {
                        edges.extend(
                            context.direct_edges[&(pair[0].clone(), pair[1].clone())]
                                .iter()
                                .cloned(),
                        );
                    }
                    edges.sort();
                    edges.dedup();
                    violations.insert((
                        rule.key.clone(),
                        ArchitectureViolationKind::ForbiddenDependency {
                            from: from.clone(),
                            to: to.clone(),
                            path,
                            edges,
                        },
                    ));
                }
            }
            ArchitectureRuleKind::ForbidCycles { level } => {
                for component in context
                    .components
                    .iter()
                    .filter(|component| component.level == *level && component.cyclic)
                {
                    violations.insert((
                        rule.key.clone(),
                        ArchitectureViolationKind::Cycle {
                            component: component.key.clone(),
                        },
                    ));
                }
            }
            ArchitectureRuleKind::RequireLayerDescent {
                level,
                require_total,
            } => {
                if *require_total {
                    for (node, _) in context
                        .structural
                        .iter()
                        .filter(|(_, actual)| **actual == *level)
                    {
                        if !assignments.contains_key(node) {
                            gaps.insert(ArchitectureGapKind::MissingLayerAssignment {
                                rule: rule.key.clone(),
                                node: node.clone(),
                            });
                        }
                    }
                }
                for ((from, to), edges) in context
                    .direct_edges
                    .iter()
                    .filter(|((from, _), _)| context.structural[from] == *level)
                {
                    let (Some(from_layer), Some(to_layer)) =
                        (assignments.get(from), assignments.get(to))
                    else {
                        continue;
                    };
                    if from_layer <= to_layer {
                        for edge in edges {
                            violations.insert((
                                rule.key.clone(),
                                ArchitectureViolationKind::LayerDirection {
                                    edge: edge.clone(),
                                    from: from.clone(),
                                    to: to.clone(),
                                    from_layer: *from_layer,
                                    to_layer: *to_layer,
                                },
                            ));
                        }
                    }
                }
            }
            ArchitectureRuleKind::RequireStableDependencies { level } => {
                if context.source_coverage != FactCoverage::Complete {
                    gaps.insert(ArchitectureGapKind::RuleRequiresCompleteTopology {
                        rule: rule.key.clone(),
                        actual: context.source_coverage,
                    });
                    continue;
                }
                for ((from, to), edges) in context
                    .direct_edges
                    .iter()
                    .filter(|((from, _), _)| context.structural[from] == *level)
                {
                    let (Some(from_ratio), Some(to_ratio)) = (
                        metric_map[from].instability.as_ref(),
                        metric_map[to].instability.as_ref(),
                    ) else {
                        continue;
                    };
                    if ratio_less(from_ratio, to_ratio) {
                        for edge in edges {
                            violations.insert((
                                rule.key.clone(),
                                ArchitectureViolationKind::StableDirection {
                                    edge: edge.clone(),
                                    from: from.clone(),
                                    to: to.clone(),
                                    from_instability: from_ratio.clone(),
                                    to_instability: to_ratio.clone(),
                                },
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn strongly_connected_components(
    nodes: &[DependencyNodeKey],
    adjacency: &BTreeMap<DependencyNodeKey, BTreeSet<DependencyNodeKey>>,
    reverse: &BTreeMap<DependencyNodeKey, BTreeSet<DependencyNodeKey>>,
) -> Vec<Vec<DependencyNodeKey>> {
    let allowed = nodes.iter().cloned().collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    for root in nodes {
        if seen.contains(root) {
            continue;
        }
        let mut stack = vec![(root.clone(), false)];
        while let Some((node, expanded)) = stack.pop() {
            if expanded {
                order.push(node);
                continue;
            }
            if !seen.insert(node.clone()) {
                continue;
            }
            stack.push((node.clone(), true));
            for target in adjacency[&node]
                .iter()
                .rev()
                .filter(|target| allowed.contains(*target))
            {
                if !seen.contains(target) {
                    stack.push((target.clone(), false));
                }
            }
        }
    }
    seen.clear();
    let mut components = Vec::new();
    for root in order.into_iter().rev() {
        if !seen.insert(root.clone()) {
            continue;
        }
        let mut members = Vec::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            members.push(node.clone());
            for source in reverse[&node]
                .iter()
                .rev()
                .filter(|source| allowed.contains(*source))
            {
                if seen.insert(source.clone()) {
                    stack.push(source.clone());
                }
            }
        }
        members.sort();
        components.push(members);
    }
    components.sort();
    components
}

fn assign_layers(
    components: &mut [ComponentDraft],
    edges: impl Iterator<Item = (usize, usize)>,
) -> Result<(), ArchitectureBuildError> {
    let mut outgoing = vec![BTreeSet::new(); components.len()];
    let mut incoming = vec![BTreeSet::new(); components.len()];
    for (from, to) in edges {
        outgoing[from].insert(to);
        incoming[to].insert(from);
    }
    let mut remaining = outgoing.iter().map(BTreeSet::len).collect::<Vec<_>>();
    let mut ready = remaining
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<BTreeSet<_>>();
    let mut processed = 0;
    while let Some(index) = ready.pop_first() {
        processed += 1;
        for dependent in incoming[index].iter().copied() {
            components[dependent].layer =
                components[dependent].layer.max(components[index].layer + 1);
            remaining[dependent] -= 1;
            if remaining[dependent] == 0 {
                ready.insert(dependent);
            }
        }
    }
    if processed != components.len() {
        Err(invalid("architecture condensation graph contains a cycle"))
    } else {
        Ok(())
    }
}

fn find_path(
    from: &DependencyNodeKey,
    to: &DependencyNodeKey,
    adjacency: &BTreeMap<DependencyNodeKey, BTreeSet<DependencyNodeKey>>,
) -> Option<Vec<DependencyNodeKey>> {
    if from == to {
        return Some(vec![from.clone()]);
    }
    let mut queue = VecDeque::from([from.clone()]);
    let mut predecessor = BTreeMap::<DependencyNodeKey, DependencyNodeKey>::new();
    let mut seen = BTreeSet::from([from.clone()]);
    while let Some(node) = queue.pop_front() {
        for target in &adjacency[&node] {
            if !seen.insert(target.clone()) {
                continue;
            }
            predecessor.insert(target.clone(), node.clone());
            if target == to {
                let mut path = vec![to.clone()];
                while path.last() != Some(from) {
                    path.push(predecessor[path.last().unwrap()].clone());
                }
                path.reverse();
                return Some(path);
            }
            queue.push_back(target.clone());
        }
    }
    None
}

fn ratio_less(left: &ArchitectureRatio, right: &ArchitectureRatio) -> bool {
    (left.numerator as u128) * (right.denominator as u128)
        < (right.numerator as u128) * (left.denominator as u128)
}

fn node_class(kind: &DependencyNodeKind) -> ArchitectureNodeClass {
    match kind {
        DependencyNodeKind::File { .. } => ArchitectureNodeClass::Structural {
            level: ArchitectureLevel::File,
        },
        DependencyNodeKind::Module { .. } => ArchitectureNodeClass::Structural {
            level: ArchitectureLevel::Module,
        },
        DependencyNodeKind::Package { .. } => ArchitectureNodeClass::Structural {
            level: ArchitectureLevel::Package,
        },
        DependencyNodeKind::BuildTarget { .. } => ArchitectureNodeClass::Structural {
            level: ArchitectureLevel::BuildTarget,
        },
        DependencyNodeKind::LocalApi { .. } => ArchitectureNodeClass::LocalApi,
        DependencyNodeKind::ExternalApi { .. } => ArchitectureNodeClass::ExternalApi,
    }
}

fn dependency_edge_level(kind: DependencyEdgeKind) -> Option<ArchitectureLevel> {
    match kind {
        DependencyEdgeKind::FileDependency => Some(ArchitectureLevel::File),
        DependencyEdgeKind::ModuleDependency => Some(ArchitectureLevel::Module),
        DependencyEdgeKind::PackageDependency => Some(ArchitectureLevel::Package),
        DependencyEdgeKind::BuildTargetDependency => Some(ArchitectureLevel::BuildTarget),
        DependencyEdgeKind::PackageContainsTarget
        | DependencyEdgeKind::TargetContainsModule
        | DependencyEdgeKind::ModuleContainsFile
        | DependencyEdgeKind::ApiUse => None,
    }
}

fn make_rule(
    name: String,
    kind: ArchitectureRuleKind,
) -> Result<ArchitectureRule, ArchitectureBuildError> {
    validate_text(&name)?;
    let key = make_rule_key(&name, &kind)?;
    Ok(ArchitectureRule { key, name, kind })
}

fn make_rule_key(
    name: &str,
    kind: &ArchitectureRuleKind,
) -> Result<ArchitectureRuleKey, ArchitectureBuildError> {
    let payload = serde_json::to_vec(&(name, kind))
        .map_err(|error| ArchitectureBuildError::Identity(error.to_string()))?;
    Ok(ArchitectureRuleKey(derive_id(
        RULE_DOMAIN,
        "arr1_",
        &[&payload],
    )))
}

fn make_policy_id(
    assignments: &[ArchitectureLayerAssignment],
    rules: &[ArchitectureRule],
) -> Result<ArchitecturePolicyId, ArchitectureBuildError> {
    let payload = serde_json::to_vec(&(ARCHITECTURE_POLICY_SCHEMA, assignments, rules))
        .map_err(|error| ArchitectureBuildError::Identity(error.to_string()))?;
    Ok(ArchitecturePolicyId(derive_id(
        POLICY_DOMAIN,
        "arp1_",
        &[&payload],
    )))
}

fn make_component_key(
    source: &ProjectionId,
    component: &ArchitectureComponent,
) -> Result<ArchitectureComponentKey, ArchitectureBuildError> {
    let payload = serde_json::to_vec(&(
        source,
        component.level,
        &component.members,
        component.cyclic,
        component.layer,
        component.coverage,
    ))
    .map_err(|error| ArchitectureBuildError::Identity(error.to_string()))?;
    Ok(ArchitectureComponentKey(derive_id(
        COMPONENT_DOMAIN,
        "arc1_",
        &[&payload],
    )))
}

fn make_condensation_edge_key(
    source: &ProjectionId,
    edge: &ArchitectureCondensationEdge,
) -> Result<ArchitectureCondensationEdgeKey, ArchitectureBuildError> {
    let payload =
        serde_json::to_vec(&(source, edge.level, &edge.from, &edge.to, &edge.source_edges))
            .map_err(|error| ArchitectureBuildError::Identity(error.to_string()))?;
    Ok(ArchitectureCondensationEdgeKey(derive_id(
        EDGE_DOMAIN,
        "are1_",
        &[&payload],
    )))
}

fn make_violation(
    policy: &ArchitecturePolicyId,
    rule: ArchitectureRuleKey,
    kind: ArchitectureViolationKind,
) -> Result<ArchitectureViolation, ArchitectureBuildError> {
    let key = make_violation_key(policy, &rule, &kind)?;
    Ok(ArchitectureViolation { key, rule, kind })
}

fn make_violation_key(
    policy: &ArchitecturePolicyId,
    rule: &ArchitectureRuleKey,
    kind: &ArchitectureViolationKind,
) -> Result<ArchitectureViolationKey, ArchitectureBuildError> {
    let payload = serde_json::to_vec(&(policy, rule, kind))
        .map_err(|error| ArchitectureBuildError::Identity(error.to_string()))?;
    Ok(ArchitectureViolationKey(derive_id(
        VIOLATION_DOMAIN,
        "arv1_",
        &[&payload],
    )))
}

fn make_gap(
    policy: &ArchitecturePolicyId,
    kind: ArchitectureGapKind,
) -> Result<ArchitectureGap, ArchitectureBuildError> {
    let key = make_gap_key(policy, &kind)?;
    Ok(ArchitectureGap { key, kind })
}

fn make_gap_key(
    policy: &ArchitecturePolicyId,
    kind: &ArchitectureGapKind,
) -> Result<ArchitectureGapKey, ArchitectureBuildError> {
    let payload = serde_json::to_vec(&(policy, kind))
        .map_err(|error| ArchitectureBuildError::Identity(error.to_string()))?;
    Ok(ArchitectureGapKey(derive_id(
        GAP_DOMAIN,
        "arg1_",
        &[&payload],
    )))
}

fn make_coverage(
    source: FactCoverage,
    source_reasons: &[String],
    gaps: &[ArchitectureGap],
) -> ArchitectureCoverageEvidence {
    let mut status = source;
    let mut reasons = source_reasons.to_vec();
    for gap in gaps {
        status = max_coverage(status, gap_coverage(gap));
        reasons.push(gap_reason(gap));
    }
    reasons.sort();
    reasons.dedup();
    ArchitectureCoverageEvidence { status, reasons }
}

fn gap_coverage(gap: &ArchitectureGap) -> FactCoverage {
    match &gap.kind {
        ArchitectureGapKind::SourceDependency { .. } => FactCoverage::Partial,
        ArchitectureGapKind::LayerAssignmentEndpointMissing { .. }
        | ArchitectureGapKind::RuleEndpointMissing { .. }
        | ArchitectureGapKind::MissingLayerAssignment { .. } => FactCoverage::Partial,
        ArchitectureGapKind::RuleRequiresCompleteTopology { actual, .. } => *actual,
    }
}

fn gap_reason(gap: &ArchitectureGap) -> String {
    match &gap.kind {
        ArchitectureGapKind::SourceDependency { gap } => {
            format!("source dependency gap {}", gap.as_str())
        }
        ArchitectureGapKind::LayerAssignmentEndpointMissing { node } => {
            format!("declared layer endpoint {} is absent", node.as_str())
        }
        ArchitectureGapKind::RuleEndpointMissing { rule, node } => format!(
            "rule {} endpoint {} is absent",
            rule.as_str(),
            node.as_str()
        ),
        ArchitectureGapKind::MissingLayerAssignment { rule, node } => {
            format!("rule {} has no layer for {}", rule.as_str(), node.as_str())
        }
        ArchitectureGapKind::RuleRequiresCompleteTopology { rule, actual } => format!(
            "rule {} requires complete topology but coverage is {}",
            rule.as_str(),
            coverage_label(*actual)
        ),
    }
}

fn max_coverage(left: FactCoverage, right: FactCoverage) -> FactCoverage {
    if coverage_severity(left) >= coverage_severity(right) {
        left
    } else {
        right
    }
}

fn coverage_severity(value: FactCoverage) -> u8 {
    match value {
        FactCoverage::Complete => 0,
        FactCoverage::Partial => 1,
        FactCoverage::Unsupported => 2,
        FactCoverage::Failed => 3,
    }
}

fn coverage_label(value: FactCoverage) -> &'static str {
    match value {
        FactCoverage::Complete => "complete",
        FactCoverage::Partial => "partial",
        FactCoverage::Unsupported => "unsupported",
        FactCoverage::Failed => "failed",
    }
}

fn validate_sorted<T>(
    label: &str,
    values: &[T],
    key: impl Fn(&T) -> &str,
) -> Result<(), ArchitectureBuildError> {
    if values.windows(2).any(|pair| key(&pair[0]) >= key(&pair[1])) {
        Err(invalid(format!("{label} are not canonical and distinct")))
    } else {
        Ok(())
    }
}

fn strictly_sorted<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn validate_text(value: &str) -> Result<(), ArchitectureBuildError> {
    if value.trim().is_empty() || value.trim() != value {
        Err(invalid("architecture text must be canonical and nonempty"))
    } else {
        Ok(())
    }
}

fn validate_digest(value: &str, prefix: &str) -> Result<(), ArchitectureBuildError> {
    let Some(digest) = value.strip_prefix(prefix) else {
        return Err(invalid(format!("identity must start with {prefix}")));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Err(invalid(
            "identity must contain a canonical 32-byte hexadecimal digest",
        ))
    } else {
        Ok(())
    }
}

fn invalid(detail: impl Into<String>) -> ArchitectureBuildError {
    ArchitectureBuildError::Invalid(detail.into())
}

fn derive_id(domain: &str, prefix: &str, parts: &[&[u8]]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(domain.len() as u64).to_le_bytes());
    hasher.update(domain.as_bytes());
    for part in parts {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    format!("{prefix}{}", hasher.finalize().to_hex())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::dependency::tests::{FixtureEndpoint, dependency_fixture};
    use crate::{DependencyNode, FactCoverageEvidence};

    fn complete_dependency() -> Arc<DependencyProjection> {
        Arc::new(dependency_fixture(
            FactCoverageEvidence::complete(),
            FixtureEndpoint::ProviderDeclaration,
            false,
        ))
    }

    fn empty_policy() -> ArchitecturePolicy {
        ArchitecturePolicy::new(vec![], vec![]).unwrap()
    }

    fn node_key(
        dependency: &DependencyProjection,
        predicate: impl Fn(&DependencyNodeKind) -> bool,
    ) -> DependencyNodeKey {
        dependency
            .document()
            .nodes()
            .iter()
            .find(|node| predicate(node.kind()))
            .map(DependencyNode::key)
            .unwrap()
            .clone()
    }

    fn package_key(dependency: &DependencyProjection, expected: &str) -> DependencyNodeKey {
        node_key(
            dependency,
            |kind| matches!(kind, DependencyNodeKind::Package { package_id } if package_id == expected),
        )
    }

    fn file_key(dependency: &DependencyProjection, expected: &str) -> DependencyNodeKey {
        node_key(
            dependency,
            |kind| matches!(kind, DependencyNodeKind::File { path } if path == Path::new(expected)),
        )
    }

    fn metric<'a>(
        projection: &'a ArchitectureProjection,
        node: &DependencyNodeKey,
    ) -> &'a ArchitectureNodeMetrics {
        projection
            .document()
            .metrics()
            .iter()
            .find(|metric| metric.node() == node)
            .unwrap()
    }

    #[test]
    fn exact_projection_has_pinned_dag_layers_fan_and_api_counts() {
        let dependency = complete_dependency();
        let projection = derive_architecture(Arc::clone(&dependency), empty_policy()).unwrap();
        let document = projection.document();
        assert_eq!(document.coverage().status(), FactCoverage::Complete);
        assert!(document.gaps().is_empty());
        assert!(document.violations().is_empty());
        assert_eq!(document.metrics().len(), 9);
        assert_eq!(document.components().len(), 8);
        assert_eq!(document.condensation_edges().len(), 4);
        for level in ArchitectureLevel::ALL {
            let components = document
                .components()
                .iter()
                .filter(|component| component.level() == level)
                .collect::<Vec<_>>();
            assert_eq!(components.len(), 2);
            assert_eq!(
                components
                    .iter()
                    .map(|component| component.layer())
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([0, 1])
            );
            assert!(components.iter().all(|component| !component.cyclic()));
        }

        let app = package_key(&dependency, "app-package");
        let dep = package_key(&dependency, "dep-package");
        let app_metric = metric(&projection, &app);
        assert_eq!(
            (
                app_metric.dependency_fan_in(),
                app_metric.dependency_fan_out()
            ),
            (0, 1)
        );
        assert_eq!(
            app_metric
                .instability()
                .map(|ratio| (ratio.numerator(), ratio.denominator())),
            Some((1, 1))
        );
        let dep_metric = metric(&projection, &dep);
        assert_eq!(
            (
                dep_metric.dependency_fan_in(),
                dep_metric.dependency_fan_out()
            ),
            (1, 0)
        );
        assert_eq!(
            dep_metric
                .instability()
                .map(|ratio| (ratio.numerator(), ratio.denominator())),
            Some((0, 1))
        );
        let consumer = file_key(&dependency, "consumer.resolutionrs");
        assert_eq!(metric(&projection, &consumer).api_uses(), 1);
        let api = node_key(&dependency, |kind| {
            matches!(kind, DependencyNodeKind::LocalApi { .. })
        });
        assert_eq!(metric(&projection, &api).api_users(), 1);
        assert!(metric(&projection, &api).instability().is_none());
    }

    #[test]
    fn declared_rules_emit_exact_forbidden_and_layer_violations() {
        let dependency = complete_dependency();
        let app = package_key(&dependency, "app-package");
        let dep = package_key(&dependency, "dep-package");
        let policy = ArchitecturePolicy::new(
            vec![
                ArchitectureLayerAssignment {
                    node: app.clone(),
                    layer: 0,
                },
                ArchitectureLayerAssignment {
                    node: dep.clone(),
                    layer: 1,
                },
            ],
            vec![
                ArchitectureRuleDraft {
                    name: "app-must-not-reach-dep".into(),
                    kind: ArchitectureRuleKind::ForbidDependency {
                        from: app.clone(),
                        to: dep.clone(),
                        transitive: true,
                    },
                },
                ArchitectureRuleDraft {
                    name: "package-layers-descend".into(),
                    kind: ArchitectureRuleKind::RequireLayerDescent {
                        level: ArchitectureLevel::Package,
                        require_total: true,
                    },
                },
                ArchitectureRuleDraft {
                    name: "dependencies-point-to-stability".into(),
                    kind: ArchitectureRuleKind::RequireStableDependencies {
                        level: ArchitectureLevel::Package,
                    },
                },
            ],
        )
        .unwrap();
        let projection = derive_architecture(dependency, policy).unwrap();
        assert_eq!(
            projection.document().coverage().status(),
            FactCoverage::Complete
        );
        assert!(projection.document().gaps().is_empty());
        assert_eq!(projection.document().violations().len(), 2);
        assert!(projection.document().violations().iter().any(|violation| matches!(
            violation.kind(),
            ArchitectureViolationKind::ForbiddenDependency { from, to, path, edges }
                if from == &app && to == &dep && path == &[app.clone(), dep.clone()] && edges.len() == 1
        )));
        assert!(
            projection
                .document()
                .violations()
                .iter()
                .any(|violation| matches!(
                    violation.kind(),
                    ArchitectureViolationKind::LayerDirection { from, to, from_layer, to_layer, .. }
                        if from == &app && to == &dep && *from_layer == 0 && *to_layer == 1
                ))
        );
        assert!(
            !projection
                .document()
                .violations()
                .iter()
                .any(|violation| matches!(
                    violation.kind(),
                    ArchitectureViolationKind::StableDirection { .. }
                ))
        );
    }

    #[test]
    fn partial_topology_propagates_source_gaps_and_blocks_stability_rules() {
        let dependency = Arc::new(dependency_fixture(
            FactCoverageEvidence::partial("architecture fixture exports are incomplete").unwrap(),
            FixtureEndpoint::AdapterOnly,
            false,
        ));
        let policy = ArchitecturePolicy::new(
            vec![],
            vec![ArchitectureRuleDraft {
                name: "complete-package-stability".into(),
                kind: ArchitectureRuleKind::RequireStableDependencies {
                    level: ArchitectureLevel::Package,
                },
            }],
        )
        .unwrap();
        let projection = derive_architecture(dependency, policy).unwrap();
        assert_eq!(
            projection.document().coverage().status(),
            FactCoverage::Partial
        );
        assert!(
            projection
                .document()
                .gaps()
                .iter()
                .any(|gap| matches!(gap.kind(), ArchitectureGapKind::SourceDependency { .. }))
        );
        assert!(projection.document().gaps().iter().any(|gap| matches!(
            gap.kind(),
            ArchitectureGapKind::RuleRequiresCompleteTopology { actual, .. }
                if *actual == FactCoverage::Partial
        )));
        assert!(
            !projection
                .document()
                .violations()
                .iter()
                .any(|violation| matches!(
                    violation.kind(),
                    ArchitectureViolationKind::StableDirection { .. }
                ))
        );
    }

    #[test]
    fn total_layer_rules_gap_on_unassigned_structural_nodes() {
        let dependency = complete_dependency();
        let app = package_key(&dependency, "app-package");
        let dep = package_key(&dependency, "dep-package");
        let policy = ArchitecturePolicy::new(
            vec![ArchitectureLayerAssignment {
                node: app,
                layer: 1,
            }],
            vec![ArchitectureRuleDraft {
                name: "all-packages-need-layers".into(),
                kind: ArchitectureRuleKind::RequireLayerDescent {
                    level: ArchitectureLevel::Package,
                    require_total: true,
                },
            }],
        )
        .unwrap();
        let projection = derive_architecture(dependency, policy).unwrap();
        assert_eq!(
            projection.document().coverage().status(),
            FactCoverage::Partial
        );
        assert!(projection.document().gaps().iter().any(|gap| matches!(
            gap.kind(),
            ArchitectureGapKind::MissingLayerAssignment { node, .. } if node == &dep
        )));
    }

    #[test]
    fn transitive_and_stability_rules_use_exact_paths_and_ratios() {
        let dependency = complete_dependency();
        let app = package_key(&dependency, "app-package");
        let dep = package_key(&dependency, "dep-package");
        let third = file_key(&dependency, "consumer.resolutionrs");
        let package_edge = dependency
            .document()
            .edges()
            .iter()
            .find(|edge| edge.kind() == DependencyEdgeKind::PackageDependency)
            .unwrap()
            .key()
            .clone();
        let file_edge = dependency
            .document()
            .edges()
            .iter()
            .find(|edge| edge.kind() == DependencyEdgeKind::FileDependency)
            .unwrap()
            .key()
            .clone();
        let structural = BTreeMap::from([
            (app.clone(), ArchitectureLevel::Package),
            (dep.clone(), ArchitectureLevel::Package),
            (third.clone(), ArchitectureLevel::Package),
        ]);
        let adjacency = BTreeMap::from([
            (app.clone(), BTreeSet::from([dep.clone()])),
            (dep.clone(), BTreeSet::from([third.clone()])),
            (third.clone(), BTreeSet::new()),
        ]);
        let direct_edges = BTreeMap::from([
            ((app.clone(), dep.clone()), vec![package_edge.clone()]),
            ((dep.clone(), third.clone()), vec![file_edge]),
        ]);
        let metric = |node, fan_in, fan_out| ArchitectureNodeMetrics {
            node,
            class: ArchitectureNodeClass::Structural {
                level: ArchitectureLevel::Package,
            },
            dependency_fan_in: fan_in,
            dependency_fan_out: fan_out,
            api_users: 0,
            api_uses: 0,
            instability: ArchitectureRatio::new(fan_out, fan_in + fan_out),
            coverage: FactCoverage::Complete,
        };
        let metrics = vec![
            metric(app.clone(), 2, 1),
            metric(dep.clone(), 1, 2),
            metric(third.clone(), 1, 0),
        ];
        let policy = ArchitecturePolicy::new(
            vec![],
            vec![
                ArchitectureRuleDraft {
                    name: "app-cannot-reach-third".into(),
                    kind: ArchitectureRuleKind::ForbidDependency {
                        from: app.clone(),
                        to: third.clone(),
                        transitive: true,
                    },
                },
                ArchitectureRuleDraft {
                    name: "stable-package-direction".into(),
                    kind: ArchitectureRuleKind::RequireStableDependencies {
                        level: ArchitectureLevel::Package,
                    },
                },
            ],
        )
        .unwrap();
        let mut violations = BTreeSet::new();
        let mut gaps = BTreeSet::new();
        evaluate_rules(
            &policy,
            RuleEvaluationContext {
                source_coverage: FactCoverage::Complete,
                structural: &structural,
                adjacency: &adjacency,
                direct_edges: &direct_edges,
                metrics: &metrics,
                components: &[],
            },
            &mut violations,
            &mut gaps,
        )
        .unwrap();
        assert!(gaps.is_empty());
        assert!(violations.iter().any(|(_, kind)| matches!(
            kind,
            ArchitectureViolationKind::ForbiddenDependency { path, edges, .. }
                if path == &[app.clone(), dep.clone(), third.clone()] && edges.len() == 2
        )));
        assert!(violations.iter().any(|(_, kind)| matches!(
            kind,
            ArchitectureViolationKind::StableDirection {
                edge,
                from_instability,
                to_instability,
                ..
            } if edge == &package_edge
                && (from_instability.numerator(), from_instability.denominator()) == (1, 3)
                && (to_instability.numerator(), to_instability.denominator()) == (2, 3)
        )));
    }

    #[test]
    fn scc_and_cycle_rule_are_deterministic_topology_not_default_defects() {
        let dependency = complete_dependency();
        let app = package_key(&dependency, "app-package");
        let dep = package_key(&dependency, "dep-package");
        let nodes = vec![app.clone(), dep.clone()];
        let adjacency = BTreeMap::from([
            (app.clone(), BTreeSet::from([dep.clone()])),
            (dep.clone(), BTreeSet::from([app.clone()])),
        ]);
        let reverse = adjacency.clone();
        assert_eq!(
            strongly_connected_components(&nodes, &adjacency, &reverse),
            vec![vec![app.clone(), dep.clone()]]
        );

        let policy = ArchitecturePolicy::new(
            vec![],
            vec![ArchitectureRuleDraft {
                name: "packages-must-be-acyclic".into(),
                kind: ArchitectureRuleKind::ForbidCycles {
                    level: ArchitectureLevel::Package,
                },
            }],
        )
        .unwrap();
        let mut component = ArchitectureComponent {
            key: ArchitectureComponentKey(String::new()),
            level: ArchitectureLevel::Package,
            members: vec![app.clone(), dep.clone()],
            cyclic: true,
            layer: 0,
            coverage: FactCoverage::Complete,
        };
        component.key = make_component_key(dependency.id(), &component).unwrap();
        let structural = BTreeMap::from([
            (app.clone(), ArchitectureLevel::Package),
            (dep.clone(), ArchitectureLevel::Package),
        ]);
        let metrics = nodes
            .iter()
            .map(|node| ArchitectureNodeMetrics {
                node: node.clone(),
                class: ArchitectureNodeClass::Structural {
                    level: ArchitectureLevel::Package,
                },
                dependency_fan_in: 1,
                dependency_fan_out: 1,
                api_users: 0,
                api_uses: 0,
                instability: ArchitectureRatio::new(1, 2),
                coverage: FactCoverage::Complete,
            })
            .collect::<Vec<_>>();
        let mut violations = BTreeSet::new();
        let mut gaps = BTreeSet::new();
        evaluate_rules(
            &policy,
            RuleEvaluationContext {
                source_coverage: FactCoverage::Complete,
                structural: &structural,
                adjacency: &adjacency,
                direct_edges: &BTreeMap::new(),
                metrics: &metrics,
                components: &[component],
            },
            &mut violations,
            &mut gaps,
        )
        .unwrap();
        assert!(gaps.is_empty());
        assert_eq!(violations.len(), 1);
        assert!(matches!(
            &violations.iter().next().unwrap().1,
            ArchitectureViolationKind::Cycle { .. }
        ));
        assert!(
            derive_architecture(dependency, empty_policy())
                .unwrap()
                .document()
                .violations()
                .is_empty()
        );
    }

    #[test]
    fn projection_is_deterministic_strict_and_tamper_rejecting() {
        let dependency = complete_dependency();
        let policy = empty_policy();
        let first = derive_architecture(Arc::clone(&dependency), policy.clone()).unwrap();
        let second = derive_architecture(dependency, policy).unwrap();
        assert_eq!(first.id(), second.id());
        let bytes = serde_json::to_vec(first.document()).unwrap();
        assert_eq!(bytes, serde_json::to_vec(second.document()).unwrap());
        let decoded: ArchitectureDocument = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(bytes, serde_json::to_vec(&decoded).unwrap());

        let mut tampered = serde_json::to_value(first.document()).unwrap();
        tampered["metrics"][0]["dependency_fan_out"] = serde_json::json!(99);
        assert!(serde_json::from_value::<ArchitectureDocument>(tampered).is_err());
    }

    #[test]
    fn exact_ratio_comparison_has_no_float_rounding_authority() {
        assert!(ratio_less(
            &ArchitectureRatio::new(1, 3).unwrap(),
            &ArchitectureRatio::new(2, 5).unwrap()
        ));
        assert!(!ratio_less(
            &ArchitectureRatio::new(2, 4).unwrap(),
            &ArchitectureRatio::new(1, 2).unwrap()
        ));
    }
}
