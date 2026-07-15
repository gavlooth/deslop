use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    AdapterCapability, CapabilityAuthority, CapabilitySupport, ControlEdgeKey,
    ControlEdgePrecision, ControlRegionResidualKey, DataFlowAccessKey, DataFlowEffectKey,
    FactCoverage, NonStructuredControlClassification, NonStructuredControlFactKey,
    ProgramDependenceGapKey, ProgramDependenceGapKind, ProgramDependenceProjection,
    ResolutionResultKey, ResolutionStatus, StructuredControlRegionKind, SystemDependenceGapKey,
    SystemDependenceGapKind, SystemDependenceProjection,
};

pub const GRAPH_RECIPE_ELIGIBILITY_SCHEMA: &str = "deslop.graph-recipe-eligibility/1";
const GRAPH_RECIPE_ELIGIBILITY_ID_DOMAIN: &str = "deslop.graph-recipe-eligibility-id/1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct GraphEligibilityDecisionId(String);

impl GraphEligibilityDecisionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for GraphEligibilityDecisionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_digest_id(&value, "gre1_").map_err(serde::de::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GraphEvidenceLayer {
    ScopeGraph,
    Resolution,
    ControlFlow,
    ControlRegions,
    NonStructuredControl,
    DataFlow,
    ProgramDependence,
    SystemDependence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphRecipeRequirement {
    consumer: String,
    layers: Vec<GraphEvidenceLayer>,
}

impl GraphRecipeRequirement {
    pub fn new(
        consumer: impl Into<String>,
        mut layers: Vec<GraphEvidenceLayer>,
    ) -> Result<Self, GraphEligibilityError> {
        layers.sort();
        layers.dedup();
        let requirement = Self {
            consumer: consumer.into(),
            layers,
        };
        requirement.validate()?;
        Ok(requirement)
    }

    pub fn local_recipe(consumer: impl Into<String>) -> Self {
        Self::new(
            consumer,
            vec![
                GraphEvidenceLayer::ControlFlow,
                GraphEvidenceLayer::ControlRegions,
                GraphEvidenceLayer::NonStructuredControl,
                GraphEvidenceLayer::DataFlow,
                GraphEvidenceLayer::ProgramDependence,
            ],
        )
        .expect("the built-in local graph requirement is valid")
    }

    pub fn interprocedural_recipe(consumer: impl Into<String>) -> Self {
        Self::new(
            consumer,
            vec![
                GraphEvidenceLayer::ControlFlow,
                GraphEvidenceLayer::ControlRegions,
                GraphEvidenceLayer::NonStructuredControl,
                GraphEvidenceLayer::DataFlow,
                GraphEvidenceLayer::ProgramDependence,
                GraphEvidenceLayer::SystemDependence,
            ],
        )
        .expect("the built-in interprocedural graph requirement is valid")
    }

    pub fn consumer(&self) -> &str {
        &self.consumer
    }

    pub fn layers(&self) -> &[GraphEvidenceLayer] {
        &self.layers
    }

    fn requires(&self, layer: GraphEvidenceLayer) -> bool {
        self.layers.binary_search(&layer).is_ok()
    }

    fn validate(&self) -> Result<(), GraphEligibilityError> {
        if self.consumer.trim().is_empty() || self.consumer.trim() != self.consumer {
            return Err(GraphEligibilityError::InvalidRequirement(
                "graph-recipe consumer must be canonical nonempty text".into(),
            ));
        }
        if self.layers.is_empty() || self.layers.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(GraphEligibilityError::InvalidRequirement(
                "graph-recipe layers must be canonical and distinct".into(),
            ));
        }
        for (layer, prerequisites) in [
            (
                GraphEvidenceLayer::Resolution,
                &[GraphEvidenceLayer::ScopeGraph][..],
            ),
            (
                GraphEvidenceLayer::ControlRegions,
                &[GraphEvidenceLayer::ControlFlow][..],
            ),
            (
                GraphEvidenceLayer::NonStructuredControl,
                &[
                    GraphEvidenceLayer::ControlFlow,
                    GraphEvidenceLayer::ControlRegions,
                ][..],
            ),
            (
                GraphEvidenceLayer::DataFlow,
                &[
                    GraphEvidenceLayer::ControlFlow,
                    GraphEvidenceLayer::ControlRegions,
                ][..],
            ),
            (
                GraphEvidenceLayer::ProgramDependence,
                &[
                    GraphEvidenceLayer::ControlFlow,
                    GraphEvidenceLayer::ControlRegions,
                    GraphEvidenceLayer::NonStructuredControl,
                    GraphEvidenceLayer::DataFlow,
                ][..],
            ),
            (
                GraphEvidenceLayer::SystemDependence,
                &[
                    GraphEvidenceLayer::ControlFlow,
                    GraphEvidenceLayer::ControlRegions,
                    GraphEvidenceLayer::NonStructuredControl,
                    GraphEvidenceLayer::DataFlow,
                    GraphEvidenceLayer::ProgramDependence,
                ][..],
            ),
        ] {
            if self.requires(layer)
                && prerequisites
                    .iter()
                    .any(|prerequisite| !self.requires(*prerequisite))
            {
                return Err(GraphEligibilityError::InvalidRequirement(format!(
                    "graph-recipe layer {layer:?} omits a prerequisite"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphRecipeRequirementWire {
    consumer: String,
    layers: Vec<GraphEvidenceLayer>,
}

impl<'de> Deserialize<'de> for GraphRecipeRequirement {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = GraphRecipeRequirementWire::deserialize(deserializer)?;
        let requirement = Self {
            consumer: wire.consumer,
            layers: wire.layers,
        };
        requirement.validate().map_err(serde::de::Error::custom)?;
        Ok(requirement)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "block",
    content = "evidence",
    rename_all = "kebab-case"
)]
pub enum GraphEligibilityBlock {
    MissingLayer {
        layer: GraphEvidenceLayer,
    },
    SourceMismatch {
        layer: GraphEvidenceLayer,
        expected_projection: String,
        actual_projection: String,
    },
    IncompleteCoverage {
        layer: GraphEvidenceLayer,
        graph: String,
        status: FactCoverage,
        reasons: Vec<String>,
    },
    CapabilityUnavailable {
        layer: GraphEvidenceLayer,
        graph: String,
        capability: AdapterCapability,
        support: CapabilitySupport,
        authority: Option<CapabilityAuthority>,
    },
    NonUniqueResolution {
        graph: String,
        result: ResolutionResultKey,
        status: ResolutionStatus,
    },
    ConservativeControlEdge {
        graph: String,
        edge: ControlEdgeKey,
        precision: ControlEdgePrecision,
    },
    ControlRegionResidual {
        graph: String,
        residual: ControlRegionResidualKey,
        kind: StructuredControlRegionKind,
        reason: String,
    },
    NonStructuredFact {
        graph: String,
        fact: NonStructuredControlFactKey,
        classification: NonStructuredControlClassification,
    },
    DataFlowAccessUncertainty {
        graph: String,
        access: DataFlowAccessKey,
        reason: String,
    },
    DataFlowEffectUncertainty {
        graph: String,
        effect: DataFlowEffectKey,
        reason: String,
    },
    ProgramDependenceGap {
        graph: String,
        gap: ProgramDependenceGapKey,
        kind: ProgramDependenceGapKind,
    },
    SystemDependenceCallUncertainty {
        call: DataFlowAccessKey,
        reason: String,
    },
    SystemDependenceGap {
        gap: SystemDependenceGapKey,
        kind: SystemDependenceGapKind,
    },
}

impl GraphEligibilityBlock {
    fn layer(&self) -> GraphEvidenceLayer {
        match self {
            Self::MissingLayer { layer }
            | Self::SourceMismatch { layer, .. }
            | Self::IncompleteCoverage { layer, .. }
            | Self::CapabilityUnavailable { layer, .. } => *layer,
            Self::NonUniqueResolution { .. } => GraphEvidenceLayer::Resolution,
            Self::ConservativeControlEdge { .. } => GraphEvidenceLayer::ControlFlow,
            Self::ControlRegionResidual { .. } => GraphEvidenceLayer::ControlRegions,
            Self::NonStructuredFact { .. } => GraphEvidenceLayer::NonStructuredControl,
            Self::DataFlowAccessUncertainty { .. } | Self::DataFlowEffectUncertainty { .. } => {
                GraphEvidenceLayer::DataFlow
            }
            Self::ProgramDependenceGap { .. } => GraphEvidenceLayer::ProgramDependence,
            Self::SystemDependenceCallUncertainty { .. } | Self::SystemDependenceGap { .. } => {
                GraphEvidenceLayer::SystemDependence
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphEligibilityDecision {
    schema: String,
    decision_id: GraphEligibilityDecisionId,
    consumer: String,
    required_layers: Vec<GraphEvidenceLayer>,
    eligible: bool,
    blocks: Vec<GraphEligibilityBlock>,
}

impl GraphEligibilityDecision {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn id(&self) -> &GraphEligibilityDecisionId {
        &self.decision_id
    }

    pub fn consumer(&self) -> &str {
        &self.consumer
    }

    pub fn required_layers(&self) -> &[GraphEvidenceLayer] {
        &self.required_layers
    }

    pub fn eligible(&self) -> bool {
        self.eligible
    }

    pub fn blocks(&self) -> &[GraphEligibilityBlock] {
        &self.blocks
    }

    fn validate(&self) -> Result<(), GraphEligibilityError> {
        if self.schema != GRAPH_RECIPE_ELIGIBILITY_SCHEMA {
            return Err(GraphEligibilityError::InvalidDecision(
                "unsupported graph-recipe eligibility schema".into(),
            ));
        }
        let requirement = GraphRecipeRequirement {
            consumer: self.consumer.clone(),
            layers: self.required_layers.clone(),
        };
        requirement.validate()?;
        if self.blocks.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(GraphEligibilityError::InvalidDecision(
                "graph-recipe eligibility blocks must be canonical and distinct".into(),
            ));
        }
        if self.eligible != self.blocks.is_empty() {
            return Err(GraphEligibilityError::InvalidDecision(
                "graph-recipe eligibility contradicts its blocks".into(),
            ));
        }
        for block in &self.blocks {
            if !requirement.requires(block.layer()) {
                return Err(GraphEligibilityError::InvalidDecision(
                    "graph-recipe block names an unrequired evidence layer".into(),
                ));
            }
            match block {
                GraphEligibilityBlock::IncompleteCoverage {
                    graph,
                    status,
                    reasons,
                    ..
                } if *status == FactCoverage::Complete
                    || !is_canonical_text(graph)
                    || reasons.is_empty()
                    || reasons.iter().any(|reason| !is_canonical_text(reason))
                    || reasons.windows(2).any(|pair| pair[0] >= pair[1]) =>
                {
                    return Err(GraphEligibilityError::InvalidDecision(
                        "incomplete coverage block has invalid status/reasons".into(),
                    ));
                }
                GraphEligibilityBlock::CapabilityUnavailable {
                    graph, authority, ..
                } if !is_canonical_text(graph) || authority.is_some() => {
                    return Err(GraphEligibilityError::InvalidDecision(
                        "capability block contains usable or contradictory authority".into(),
                    ));
                }
                GraphEligibilityBlock::ConservativeControlEdge {
                    precision: ControlEdgePrecision::Exact,
                    ..
                } => {
                    return Err(GraphEligibilityError::InvalidDecision(
                        "conservative-edge block contains Exact precision".into(),
                    ));
                }
                GraphEligibilityBlock::ControlRegionResidual { reason, .. }
                | GraphEligibilityBlock::DataFlowAccessUncertainty { reason, .. }
                | GraphEligibilityBlock::DataFlowEffectUncertainty { reason, .. }
                | GraphEligibilityBlock::SystemDependenceCallUncertainty { reason, .. }
                    if !is_canonical_text(reason) =>
                {
                    return Err(GraphEligibilityError::InvalidDecision(
                        "graph-recipe block reason must be canonical nonempty text".into(),
                    ));
                }
                GraphEligibilityBlock::SourceMismatch {
                    expected_projection,
                    actual_projection,
                    ..
                } if !is_canonical_text(expected_projection)
                    || !is_canonical_text(actual_projection)
                    || expected_projection == actual_projection =>
                {
                    return Err(GraphEligibilityError::InvalidDecision(
                        "source-mismatch block has invalid projection identities".into(),
                    ));
                }
                GraphEligibilityBlock::ConservativeControlEdge { graph, .. }
                | GraphEligibilityBlock::ControlRegionResidual { graph, .. }
                | GraphEligibilityBlock::NonStructuredFact { graph, .. }
                | GraphEligibilityBlock::DataFlowAccessUncertainty { graph, .. }
                | GraphEligibilityBlock::DataFlowEffectUncertainty { graph, .. }
                | GraphEligibilityBlock::ProgramDependenceGap { graph, .. }
                | GraphEligibilityBlock::NonUniqueResolution { graph, .. }
                    if !is_canonical_text(graph) =>
                {
                    return Err(GraphEligibilityError::InvalidDecision(
                        "graph-recipe block graph must be canonical nonempty text".into(),
                    ));
                }
                _ => {}
            }
        }
        let expected = derive_decision_id(
            &self.consumer,
            &self.required_layers,
            self.eligible,
            &self.blocks,
        )?;
        if self.decision_id != expected {
            return Err(GraphEligibilityError::InvalidDecision(
                "graph-recipe eligibility identity does not match its payload".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphEligibilityDecisionWire {
    schema: String,
    decision_id: GraphEligibilityDecisionId,
    consumer: String,
    required_layers: Vec<GraphEvidenceLayer>,
    eligible: bool,
    blocks: Vec<GraphEligibilityBlock>,
}

impl<'de> Deserialize<'de> for GraphEligibilityDecision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = GraphEligibilityDecisionWire::deserialize(deserializer)?;
        let decision = Self {
            schema: wire.schema,
            decision_id: wire.decision_id,
            consumer: wire.consumer,
            required_layers: wire.required_layers,
            eligible: wire.eligible,
            blocks: wire.blocks,
        };
        decision.validate().map_err(serde::de::Error::custom)?;
        Ok(decision)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphEligibilityError {
    InvalidRequirement(String),
    InvalidDecision(String),
}

impl fmt::Display for GraphEligibilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequirement(detail) => {
                write!(formatter, "invalid graph-recipe requirement: {detail}")
            }
            Self::InvalidDecision(detail) => {
                write!(
                    formatter,
                    "invalid graph-recipe eligibility decision: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for GraphEligibilityError {}

pub fn evaluate_graph_recipe_eligibility(
    program_dependence: &ProgramDependenceProjection,
    system_dependence: Option<&SystemDependenceProjection>,
    requirement: &GraphRecipeRequirement,
) -> Result<GraphEligibilityDecision, GraphEligibilityError> {
    requirement.validate()?;
    let data_flow = program_dependence.data_flow();
    let control_regions = data_flow.control_regions();
    let control_flow = control_regions.control_flow();
    let non_structured = program_dependence.non_structured_control();
    let resolution = data_flow.resolution();
    let scope_graph = resolution.scope_graph();
    let mut blocks = Vec::new();

    if requirement.requires(GraphEvidenceLayer::ScopeGraph) {
        for fact in scope_graph.facts() {
            let reasons = fact
                .evidence()
                .coverage
                .reason
                .iter()
                .cloned()
                .collect::<Vec<_>>();
            push_coverage(
                &mut blocks,
                GraphEvidenceLayer::ScopeGraph,
                fact.key().as_str(),
                fact.evidence().coverage.status,
                &reasons,
            );
            push_capability(
                &mut blocks,
                GraphEvidenceLayer::ScopeGraph,
                fact.key().as_str(),
                fact.evidence().capability,
                fact.evidence().capability_support,
                fact.evidence().authority,
            );
        }
    }

    if requirement.requires(GraphEvidenceLayer::Resolution) {
        for result in resolution.results() {
            let wire = result.wire();
            push_coverage(
                &mut blocks,
                GraphEvidenceLayer::Resolution,
                wire.key().as_str(),
                wire.coverage().status(),
                wire.coverage().reasons(),
            );
            push_capability(
                &mut blocks,
                GraphEvidenceLayer::Resolution,
                wire.key().as_str(),
                AdapterCapability::NameResolution,
                wire.reference_evidence().capability_support,
                wire.authority(),
            );
            if wire.status() != ResolutionStatus::Unique {
                blocks.push(GraphEligibilityBlock::NonUniqueResolution {
                    graph: resolution.id().as_str().into(),
                    result: wire.key().clone(),
                    status: wire.status(),
                });
            }
        }
    }

    if requirement.requires(GraphEvidenceLayer::ControlFlow) {
        for graph in control_flow.document().graphs() {
            push_coverage(
                &mut blocks,
                GraphEvidenceLayer::ControlFlow,
                graph.key().as_str(),
                graph.coverage().status(),
                graph.coverage().reasons(),
            );
            push_capability(
                &mut blocks,
                GraphEvidenceLayer::ControlFlow,
                graph.key().as_str(),
                AdapterCapability::ControlFlow,
                graph.capability_support(),
                graph.authority(),
            );
            for edge in graph.edges() {
                if !matches!(edge.precision(), ControlEdgePrecision::Exact) {
                    blocks.push(GraphEligibilityBlock::ConservativeControlEdge {
                        graph: graph.key().as_str().into(),
                        edge: edge.key().clone(),
                        precision: edge.precision().clone(),
                    });
                }
            }
        }
    }

    if requirement.requires(GraphEvidenceLayer::ControlRegions) {
        for graph in control_regions.document().graphs() {
            push_coverage(
                &mut blocks,
                GraphEvidenceLayer::ControlRegions,
                graph.key().as_str(),
                graph.coverage().status(),
                graph.coverage().reasons(),
            );
            for residual in graph.residuals() {
                blocks.push(GraphEligibilityBlock::ControlRegionResidual {
                    graph: graph.key().as_str().into(),
                    residual: residual.key().clone(),
                    kind: residual.kind(),
                    reason: residual.reason().into(),
                });
            }
        }
    }

    if requirement.requires(GraphEvidenceLayer::NonStructuredControl) {
        for graph in non_structured.document().graphs() {
            push_coverage(
                &mut blocks,
                GraphEvidenceLayer::NonStructuredControl,
                graph.key().as_str(),
                graph.coverage().status(),
                graph.coverage().reasons(),
            );
            for fact in graph.facts() {
                blocks.push(GraphEligibilityBlock::NonStructuredFact {
                    graph: graph.key().as_str().into(),
                    fact: fact.key().clone(),
                    classification: fact.classification(),
                });
            }
        }
    }

    if requirement.requires(GraphEvidenceLayer::DataFlow) {
        for graph in data_flow.document().graphs() {
            push_coverage(
                &mut blocks,
                GraphEvidenceLayer::DataFlow,
                graph.key().as_str(),
                graph.coverage().status(),
                graph.coverage().reasons(),
            );
            push_capability(
                &mut blocks,
                GraphEvidenceLayer::DataFlow,
                graph.key().as_str(),
                AdapterCapability::DefUse,
                graph.coverage().def_use_support(),
                graph.coverage().def_use_authority(),
            );
            push_capability(
                &mut blocks,
                GraphEvidenceLayer::DataFlow,
                graph.key().as_str(),
                AdapterCapability::Effects,
                graph.coverage().effects_support(),
                graph.coverage().effects_authority(),
            );
            for access in graph.accesses() {
                if let Some(reason) = access.uncertainty() {
                    blocks.push(GraphEligibilityBlock::DataFlowAccessUncertainty {
                        graph: graph.key().as_str().into(),
                        access: access.key().clone(),
                        reason: reason.into(),
                    });
                }
            }
            for effect in graph.effects() {
                if let Some(reason) = effect.uncertainty() {
                    blocks.push(GraphEligibilityBlock::DataFlowEffectUncertainty {
                        graph: graph.key().as_str().into(),
                        effect: effect.key().clone(),
                        reason: reason.into(),
                    });
                }
            }
        }
    }

    if requirement.requires(GraphEvidenceLayer::ProgramDependence) {
        for graph in program_dependence.document().graphs() {
            push_coverage(
                &mut blocks,
                GraphEvidenceLayer::ProgramDependence,
                graph.key().as_str(),
                graph.coverage().status(),
                graph.coverage().reasons(),
            );
            push_capability(
                &mut blocks,
                GraphEvidenceLayer::ProgramDependence,
                graph.key().as_str(),
                AdapterCapability::LocalPdg,
                graph.coverage().local_pdg_support(),
                graph.coverage().local_pdg_authority(),
            );
            for gap in graph.gaps() {
                blocks.push(GraphEligibilityBlock::ProgramDependenceGap {
                    graph: graph.key().as_str().into(),
                    gap: gap.key().clone(),
                    kind: gap.kind().clone(),
                });
            }
        }
    }

    if requirement.requires(GraphEvidenceLayer::SystemDependence) {
        match system_dependence {
            None => blocks.push(GraphEligibilityBlock::MissingLayer {
                layer: GraphEvidenceLayer::SystemDependence,
            }),
            Some(system) if system.program_dependence().id() != program_dependence.id() => {
                blocks.push(GraphEligibilityBlock::SourceMismatch {
                    layer: GraphEvidenceLayer::SystemDependence,
                    expected_projection: program_dependence.id().as_str().into(),
                    actual_projection: system.program_dependence().id().as_str().into(),
                });
            }
            Some(system) => {
                push_coverage(
                    &mut blocks,
                    GraphEvidenceLayer::SystemDependence,
                    system.id().as_str(),
                    system.document().coverage().status(),
                    system.document().coverage().reasons(),
                );
                for evidence in system.document().capabilities() {
                    push_capability(
                        &mut blocks,
                        GraphEvidenceLayer::SystemDependence,
                        evidence.graph().as_str(),
                        AdapterCapability::CallGraph,
                        evidence.call_graph_support(),
                        evidence.call_graph_authority(),
                    );
                    push_capability(
                        &mut blocks,
                        GraphEvidenceLayer::SystemDependence,
                        evidence.graph().as_str(),
                        AdapterCapability::Sdg,
                        evidence.sdg_support(),
                        evidence.sdg_authority(),
                    );
                }
                for call in system.document().calls() {
                    if let Some(reason) = call.uncertainty() {
                        blocks.push(GraphEligibilityBlock::SystemDependenceCallUncertainty {
                            call: call.call().clone(),
                            reason: reason.into(),
                        });
                    }
                }
                for gap in system.document().gaps() {
                    blocks.push(GraphEligibilityBlock::SystemDependenceGap {
                        gap: gap.key().clone(),
                        kind: gap.kind().clone(),
                    });
                }
            }
        }
    }

    blocks.sort();
    blocks.dedup();
    let eligible = blocks.is_empty();
    let decision = GraphEligibilityDecision {
        schema: GRAPH_RECIPE_ELIGIBILITY_SCHEMA.into(),
        decision_id: derive_decision_id(
            &requirement.consumer,
            &requirement.layers,
            eligible,
            &blocks,
        )?,
        consumer: requirement.consumer.clone(),
        required_layers: requirement.layers.clone(),
        eligible,
        blocks,
    };
    decision.validate()?;
    Ok(decision)
}

/// Evaluate graph evidence for one exact local program-dependence graph.
///
/// This target-scoped form prevents an unrelated callable or file from granting or
/// denying authority for a candidate. System-dependence requirements remain
/// projection-wide and must use [`evaluate_graph_recipe_eligibility`].
pub fn evaluate_program_graph_recipe_eligibility(
    program_dependence: &ProgramDependenceProjection,
    graph: &crate::ProgramDependenceGraph,
    requirement: &GraphRecipeRequirement,
) -> Result<GraphEligibilityDecision, GraphEligibilityError> {
    requirement.validate()?;
    if requirement.requires(GraphEvidenceLayer::SystemDependence) {
        return Err(GraphEligibilityError::InvalidRequirement(
            "target-scoped eligibility cannot authorize system-dependence evidence".into(),
        ));
    }
    let retained = program_dependence
        .document()
        .graphs()
        .iter()
        .find(|candidate| candidate.key() == graph.key())
        .ok_or_else(|| {
            GraphEligibilityError::InvalidDecision(
                "target graph is foreign to the program-dependence projection".into(),
            )
        })?;
    let data_flow = program_dependence.data_flow();
    let control_regions = data_flow.control_regions();
    let control_flow = control_regions.control_flow();
    let non_structured = program_dependence.non_structured_control();
    if requirement.requires(GraphEvidenceLayer::ScopeGraph)
        || requirement.requires(GraphEvidenceLayer::Resolution)
    {
        return Err(GraphEligibilityError::InvalidRequirement(
            "target-scoped eligibility cannot authorize project scope or resolution evidence"
                .into(),
        ));
    }
    let flow_graph = control_flow
        .document()
        .graphs()
        .iter()
        .find(|candidate| candidate.key() == retained.control_flow_graph())
        .ok_or_else(|| GraphEligibilityError::InvalidDecision("target CFG is missing".into()))?;
    let region_graph = control_regions
        .document()
        .graphs()
        .iter()
        .find(|candidate| candidate.key() == retained.control_region_graph())
        .ok_or_else(|| {
            GraphEligibilityError::InvalidDecision("target control-region graph is missing".into())
        })?;
    let non_structured_graph = non_structured
        .document()
        .graphs()
        .iter()
        .find(|candidate| candidate.key() == retained.non_structured_control_graph())
        .ok_or_else(|| {
            GraphEligibilityError::InvalidDecision(
                "target non-structured-control graph is missing".into(),
            )
        })?;
    let data_graph = data_flow
        .document()
        .graphs()
        .iter()
        .find(|candidate| candidate.key() == retained.data_flow_graph())
        .ok_or_else(|| {
            GraphEligibilityError::InvalidDecision("target data-flow graph is missing".into())
        })?;
    let mut blocks = Vec::new();

    if requirement.requires(GraphEvidenceLayer::ControlFlow) {
        push_coverage(
            &mut blocks,
            GraphEvidenceLayer::ControlFlow,
            flow_graph.key().as_str(),
            flow_graph.coverage().status(),
            flow_graph.coverage().reasons(),
        );
        push_capability(
            &mut blocks,
            GraphEvidenceLayer::ControlFlow,
            flow_graph.key().as_str(),
            AdapterCapability::ControlFlow,
            flow_graph.capability_support(),
            flow_graph.authority(),
        );
        for edge in flow_graph.edges() {
            if !matches!(edge.precision(), ControlEdgePrecision::Exact) {
                blocks.push(GraphEligibilityBlock::ConservativeControlEdge {
                    graph: flow_graph.key().as_str().into(),
                    edge: edge.key().clone(),
                    precision: edge.precision().clone(),
                });
            }
        }
    }
    if requirement.requires(GraphEvidenceLayer::ControlRegions) {
        push_coverage(
            &mut blocks,
            GraphEvidenceLayer::ControlRegions,
            region_graph.key().as_str(),
            region_graph.coverage().status(),
            region_graph.coverage().reasons(),
        );
        for residual in region_graph.residuals() {
            blocks.push(GraphEligibilityBlock::ControlRegionResidual {
                graph: region_graph.key().as_str().into(),
                residual: residual.key().clone(),
                kind: residual.kind(),
                reason: residual.reason().into(),
            });
        }
    }
    if requirement.requires(GraphEvidenceLayer::NonStructuredControl) {
        push_coverage(
            &mut blocks,
            GraphEvidenceLayer::NonStructuredControl,
            non_structured_graph.key().as_str(),
            non_structured_graph.coverage().status(),
            non_structured_graph.coverage().reasons(),
        );
        for fact in non_structured_graph.facts() {
            blocks.push(GraphEligibilityBlock::NonStructuredFact {
                graph: non_structured_graph.key().as_str().into(),
                fact: fact.key().clone(),
                classification: fact.classification(),
            });
        }
    }
    if requirement.requires(GraphEvidenceLayer::DataFlow) {
        push_coverage(
            &mut blocks,
            GraphEvidenceLayer::DataFlow,
            data_graph.key().as_str(),
            data_graph.coverage().status(),
            data_graph.coverage().reasons(),
        );
        push_capability(
            &mut blocks,
            GraphEvidenceLayer::DataFlow,
            data_graph.key().as_str(),
            AdapterCapability::DefUse,
            data_graph.coverage().def_use_support(),
            data_graph.coverage().def_use_authority(),
        );
        push_capability(
            &mut blocks,
            GraphEvidenceLayer::DataFlow,
            data_graph.key().as_str(),
            AdapterCapability::Effects,
            data_graph.coverage().effects_support(),
            data_graph.coverage().effects_authority(),
        );
        for access in data_graph.accesses() {
            if let Some(reason) = access.uncertainty() {
                blocks.push(GraphEligibilityBlock::DataFlowAccessUncertainty {
                    graph: data_graph.key().as_str().into(),
                    access: access.key().clone(),
                    reason: reason.into(),
                });
            }
        }
        for effect in data_graph.effects() {
            if let Some(reason) = effect.uncertainty() {
                blocks.push(GraphEligibilityBlock::DataFlowEffectUncertainty {
                    graph: data_graph.key().as_str().into(),
                    effect: effect.key().clone(),
                    reason: reason.into(),
                });
            }
        }
    }
    if requirement.requires(GraphEvidenceLayer::ProgramDependence) {
        push_coverage(
            &mut blocks,
            GraphEvidenceLayer::ProgramDependence,
            retained.key().as_str(),
            retained.coverage().status(),
            retained.coverage().reasons(),
        );
        push_capability(
            &mut blocks,
            GraphEvidenceLayer::ProgramDependence,
            retained.key().as_str(),
            AdapterCapability::LocalPdg,
            retained.coverage().local_pdg_support(),
            retained.coverage().local_pdg_authority(),
        );
        for gap in retained.gaps() {
            blocks.push(GraphEligibilityBlock::ProgramDependenceGap {
                graph: retained.key().as_str().into(),
                gap: gap.key().clone(),
                kind: gap.kind().clone(),
            });
        }
    }

    blocks.sort();
    blocks.dedup();
    let eligible = blocks.is_empty();
    let decision = GraphEligibilityDecision {
        schema: GRAPH_RECIPE_ELIGIBILITY_SCHEMA.into(),
        decision_id: derive_decision_id(
            &requirement.consumer,
            &requirement.layers,
            eligible,
            &blocks,
        )?,
        consumer: requirement.consumer.clone(),
        required_layers: requirement.layers.clone(),
        eligible,
        blocks,
    };
    decision.validate()?;
    Ok(decision)
}

fn is_canonical_text(value: &str) -> bool {
    !value.trim().is_empty() && value.trim() == value
}

fn derive_decision_id(
    consumer: &str,
    required_layers: &[GraphEvidenceLayer],
    eligible: bool,
    blocks: &[GraphEligibilityBlock],
) -> Result<GraphEligibilityDecisionId, GraphEligibilityError> {
    let payload = serde_json::to_vec(&(
        GRAPH_RECIPE_ELIGIBILITY_SCHEMA,
        consumer,
        required_layers,
        eligible,
        blocks,
    ))
    .map_err(|error| GraphEligibilityError::InvalidDecision(error.to_string()))?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(GRAPH_RECIPE_ELIGIBILITY_ID_DOMAIN.len() as u64).to_le_bytes());
    hasher.update(GRAPH_RECIPE_ELIGIBILITY_ID_DOMAIN.as_bytes());
    hasher.update(&(payload.len() as u64).to_le_bytes());
    hasher.update(&payload);
    Ok(GraphEligibilityDecisionId(format!(
        "gre1_{}",
        hasher.finalize().to_hex()
    )))
}

fn validate_digest_id(value: &str, prefix: &str) -> Result<(), GraphEligibilityError> {
    if !value.strip_prefix(prefix).is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(GraphEligibilityError::InvalidDecision(format!(
            "identity must be canonical lowercase {prefix}<64-hex>"
        )));
    }
    Ok(())
}

fn push_coverage(
    blocks: &mut Vec<GraphEligibilityBlock>,
    layer: GraphEvidenceLayer,
    graph: &str,
    status: FactCoverage,
    reasons: &[String],
) {
    if status != FactCoverage::Complete {
        let mut reasons = reasons.to_vec();
        reasons.sort();
        reasons.dedup();
        blocks.push(GraphEligibilityBlock::IncompleteCoverage {
            layer,
            graph: graph.into(),
            status,
            reasons,
        });
    }
}

fn push_capability(
    blocks: &mut Vec<GraphEligibilityBlock>,
    layer: GraphEvidenceLayer,
    graph: &str,
    capability: AdapterCapability,
    support: CapabilitySupport,
    authority: Option<CapabilityAuthority>,
) {
    if support != CapabilitySupport::Provided || authority.is_none() {
        blocks.push(GraphEligibilityBlock::CapabilityUnavailable {
            layer,
            graph: graph.into(),
            capability,
            support,
            authority,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use super::*;

    fn fixture() -> crate::system_dependence::tests::SystemDependenceTestFixture {
        crate::system_dependence::tests::system_dependence_fixture()
    }

    fn family_counts(blocks: &[GraphEligibilityBlock]) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for block in blocks {
            let family = match block {
                GraphEligibilityBlock::MissingLayer { .. } => "missing-layer",
                GraphEligibilityBlock::SourceMismatch { .. } => "source-mismatch",
                GraphEligibilityBlock::IncompleteCoverage { .. } => "coverage",
                GraphEligibilityBlock::CapabilityUnavailable { .. } => "capability",
                GraphEligibilityBlock::NonUniqueResolution { .. } => "resolution-status",
                GraphEligibilityBlock::ConservativeControlEdge { .. } => "control-edge",
                GraphEligibilityBlock::ControlRegionResidual { .. } => "region-residual",
                GraphEligibilityBlock::NonStructuredFact { .. } => "non-structured",
                GraphEligibilityBlock::DataFlowAccessUncertainty { .. } => "access",
                GraphEligibilityBlock::DataFlowEffectUncertainty { .. } => "effect",
                GraphEligibilityBlock::ProgramDependenceGap { .. } => "pdg-gap",
                GraphEligibilityBlock::SystemDependenceCallUncertainty { .. } => "call",
                GraphEligibilityBlock::SystemDependenceGap { .. } => "sdg-gap",
            };
            *counts.entry(family).or_insert(0) += 1;
        }
        counts
    }

    fn roles(
        analysis: &Arc<crate::ProjectAnalysis>,
        node: crate::NodeId,
    ) -> deslop_lang::CanonicalRoleSet {
        let path = analysis.node(node).unwrap().path().to_path_buf();
        analysis
            .canonical_role_projection(&path)
            .unwrap()
            .facts()
            .iter()
            .find(|fact| fact.node() == node)
            .unwrap()
            .roles()
    }

    fn incomplete_projection_chain()
    -> (Arc<ProgramDependenceProjection>, SystemDependenceProjection) {
        let root = tempfile::tempdir().unwrap();
        let snapshot = crate::ProjectSnapshotBuilder::new(
            root.path(),
            crate::RepositoryId::explicit("graph-eligibility-incomplete-test").unwrap(),
        )
        .unwrap()
        .with_registry(deslop_lang::Registry::default())
        .with_overlay(
            "flow.rs",
            b"fn run(x: bool) { if x { loop {} } else { println!(\"x\"); } }\n".to_vec(),
        )
        .unwrap()
        .build()
        .unwrap();
        let analysis = crate::ProjectAnalysis::build(snapshot).unwrap();
        let lowered = crate::lower_control_flow(
            Arc::clone(&analysis),
            crate::ControlFlowPolicyId::from_parts(&[b"graph-eligibility-incomplete-cfg/1"])
                .unwrap(),
        )
        .unwrap();
        let flow = Arc::new(lowered.projection().unwrap().clone());
        let regions = Arc::new(
            crate::derive_control_regions(
                Arc::clone(&flow),
                crate::ControlRegionPolicyId::from_parts(&[
                    b"graph-eligibility-incomplete-regions/1",
                ])
                .unwrap(),
            )
            .unwrap(),
        );
        let flow_graph = &flow.document().graphs()[0];
        let owner = analysis
            .node_ids()
            .find(|node| analysis.node_key(*node).unwrap() == flow_graph.owner())
            .unwrap();
        let call_node = analysis
            .node_ids()
            .find(|node| {
                let view = analysis.node(*node).unwrap();
                view.raw_kind() == "identifier" && view.text() == "println"
            })
            .unwrap();
        let incomplete = crate::FactCoverageEvidence::partial(
            "production adapter does not provide this scope fact",
        )
        .unwrap();
        let mut scope = crate::ScopeGraphBuilder::new(
            Arc::clone(&analysis),
            crate::BuildContextId::from_parts(&[b"graph-eligibility-incomplete-target"]).unwrap(),
            crate::ScopeFactPolicyId::from_parts(&[b"graph-eligibility-incomplete-scope/1"])
                .unwrap(),
        )
        .unwrap();
        let callable_scope = scope
            .add_scope(
                owner,
                roles(&analysis, owner),
                incomplete.clone(),
                crate::ScopeDraft {
                    kind: crate::ScopeKind::Callable,
                    parent: None,
                    namespace_policy: crate::NamespacePolicy::new(
                        vec![crate::NameNamespace::Value],
                        vec![],
                    )
                    .unwrap(),
                },
            )
            .unwrap();
        let reference = scope
            .add_reference(
                call_node,
                roles(&analysis, call_node),
                incomplete,
                crate::ReferenceDraft {
                    original_spelling: "println".into(),
                    segments: vec!["println".into()],
                    namespace: crate::NameNamespace::Value,
                    scope: callable_scope,
                    role: crate::ReferenceRole::Call,
                },
            )
            .unwrap();
        let scope = Arc::new(scope.build().unwrap());
        let reference = scope.fact(reference).unwrap().key().clone();
        let resolution = Arc::new(
            crate::ResolutionProjection::build(
                scope,
                crate::ResolutionPolicyId::from_parts(&[
                    b"graph-eligibility-incomplete-resolution/1",
                ])
                .unwrap(),
            )
            .unwrap(),
        );
        let access_point = flow_graph
            .points()
            .iter()
            .find(|point| point.source().is_some())
            .unwrap()
            .key()
            .clone();
        let mut data_flow = crate::DataFlowBuilder::new(
            Arc::clone(&regions),
            resolution,
            crate::DataFlowPolicyId::from_parts(&[b"graph-eligibility-incomplete-data/1"]).unwrap(),
        )
        .unwrap();
        data_flow
            .add_graph(crate::DataFlowGraphDraft {
                control_flow_graph: flow_graph.key().clone(),
                definitions: vec![],
                accesses: vec![crate::DataFlowAccessDraft {
                    point: access_point,
                    reference,
                    kind: crate::DataFlowAccessKind::Call,
                    ordinal: 0,
                }],
                boundaries: vec![],
                effects: flow_graph
                    .points()
                    .iter()
                    .enumerate()
                    .map(|(index, point)| crate::DataFlowEffectDraft {
                        point: point.key().clone(),
                        effects: vec![],
                        uncertainty: (index == 0).then(|| "effect identity is unavailable".into()),
                    })
                    .collect(),
            })
            .unwrap();
        let data_flow = Arc::new(data_flow.build().unwrap());
        let call = data_flow.document().graphs()[0].accesses()[0].key().clone();
        let non_structured = Arc::new(
            crate::derive_non_structured_control_regions(
                Arc::clone(&regions),
                crate::NonStructuredControlPolicyId::from_parts(&[
                    b"graph-eligibility-incomplete-non-structured/1",
                ])
                .unwrap(),
            )
            .unwrap(),
        );
        let pdg = Arc::new(
            crate::derive_program_dependence(
                data_flow,
                non_structured,
                crate::ProgramDependencePolicyId::from_parts(&[
                    b"graph-eligibility-incomplete-pdg/1",
                ])
                .unwrap(),
            )
            .unwrap(),
        );
        let pdg_graph = &pdg.document().graphs()[0];
        let mut system = crate::SystemDependenceBuilder::new(
            Arc::clone(&pdg),
            crate::SystemDependencePolicyId::from_parts(&[
                b"graph-eligibility-incomplete-system/1",
            ])
            .unwrap(),
        );
        system
            .add_summary(crate::CallableSummaryDraft {
                program_dependence_graph: pdg_graph.key().clone(),
                formal_inputs: vec![],
                outputs: vec![],
                globals: vec![],
            })
            .unwrap();
        system.add_call_site(crate::CallSiteDraft {
            caller: pdg_graph.key().clone(),
            call,
            parameter_bindings: vec![],
            output_bindings: vec![],
        });
        (pdg, system.build().unwrap())
    }

    #[test]
    fn m4_dod_complete_local_and_interprocedural_evidence_is_eligible() {
        let fixture = fixture();
        let local = evaluate_graph_recipe_eligibility(
            &fixture.pdg,
            None,
            &GraphRecipeRequirement::local_recipe("local-recipe"),
        )
        .unwrap();
        assert!(local.eligible());
        assert!(local.blocks().is_empty());

        let interprocedural = evaluate_graph_recipe_eligibility(
            &fixture.pdg,
            Some(&fixture.complete),
            &GraphRecipeRequirement::interprocedural_recipe("interprocedural-recipe"),
        )
        .unwrap();
        assert!(interprocedural.eligible());
        assert!(interprocedural.blocks().is_empty());
        assert_ne!(local.id(), interprocedural.id());
        let bytes = serde_json::to_vec(&interprocedural).unwrap();
        let decoded: GraphEligibilityDecision = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, interprocedural);
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
    }

    #[test]
    fn m4_dod_missing_and_partial_system_dependence_propagate_exact_blocks() {
        let fixture = fixture();
        let requirement = GraphRecipeRequirement::interprocedural_recipe("interprocedural-recipe");
        let missing = evaluate_graph_recipe_eligibility(&fixture.pdg, None, &requirement).unwrap();
        assert_eq!(
            missing.blocks(),
            &[GraphEligibilityBlock::MissingLayer {
                layer: GraphEvidenceLayer::SystemDependence,
            }]
        );

        let missing_parameter = evaluate_graph_recipe_eligibility(
            &fixture.pdg,
            Some(&fixture.missing_parameter),
            &requirement,
        )
        .unwrap();
        assert!(!missing_parameter.eligible());
        assert_eq!(
            family_counts(missing_parameter.blocks()),
            BTreeMap::from([("coverage", 1), ("sdg-gap", 1)])
        );
        assert!(missing_parameter.blocks().iter().any(|block| matches!(
            block,
            GraphEligibilityBlock::SystemDependenceGap {
                kind: SystemDependenceGapKind::MissingParameterBinding { .. },
                ..
            }
        )));

        let missing_output = evaluate_graph_recipe_eligibility(
            &fixture.pdg,
            Some(&fixture.missing_output),
            &requirement,
        )
        .unwrap();
        assert!(!missing_output.eligible());
        assert_eq!(
            family_counts(missing_output.blocks()),
            BTreeMap::from([("coverage", 1), ("sdg-gap", 2)])
        );
        assert_eq!(
            missing_output
                .blocks()
                .iter()
                .filter(|block| matches!(
                    block,
                    GraphEligibilityBlock::SystemDependenceGap {
                        kind: SystemDependenceGapKind::MissingOutputBinding { .. },
                        ..
                    }
                ))
                .count(),
            2
        );
    }

    #[test]
    fn m4_dod_ambiguous_capture_and_nontermination_propagate_every_upstream_block() {
        let pdg = crate::data_flow::tests::ambiguous_capture_pdg_fixture();
        let decision = evaluate_graph_recipe_eligibility(
            &pdg,
            None,
            &GraphRecipeRequirement::local_recipe("local-recipe"),
        )
        .unwrap();
        assert!(!decision.eligible());
        assert_eq!(
            family_counts(decision.blocks()),
            BTreeMap::from([
                ("access", 1),
                ("coverage", 4),
                ("non-structured", 1),
                ("pdg-gap", 3),
            ])
        );
        assert!(decision.blocks().iter().any(|block| matches!(
            block,
            GraphEligibilityBlock::DataFlowAccessUncertainty { .. }
        )));
        assert!(decision.blocks().iter().any(|block| matches!(
            block,
            GraphEligibilityBlock::NonStructuredFact {
                classification: NonStructuredControlClassification::NonTerminatingCycle,
                ..
            }
        )));
        assert_eq!(
            decision
                .blocks()
                .iter()
                .filter(|block| matches!(block, GraphEligibilityBlock::ProgramDependenceGap { .. }))
                .count(),
            3
        );
    }

    #[test]
    fn m4_dod_conservative_residual_capability_effect_and_call_facts_all_propagate() {
        let (pdg, system) = incomplete_projection_chain();
        let decision = evaluate_graph_recipe_eligibility(
            &pdg,
            Some(&system),
            &GraphRecipeRequirement::interprocedural_recipe("incomplete-recipe"),
        )
        .unwrap();
        assert!(!decision.eligible());
        let counts = family_counts(decision.blocks());
        assert_eq!(
            counts,
            BTreeMap::from([
                ("access", 1),
                ("call", 1),
                ("capability", 5),
                ("control-edge", 1),
                ("coverage", 6),
                ("effect", 1),
                ("non-structured", 3),
                ("pdg-gap", 4),
                ("region-residual", 1),
                ("sdg-gap", 1),
            ])
        );
    }

    #[test]
    fn m4_dod_foreign_system_dependence_and_invalid_requirements_fail_closed() {
        let fixture = fixture();
        let foreign = crate::data_flow::tests::ambiguous_capture_pdg_fixture();
        let decision = evaluate_graph_recipe_eligibility(
            &foreign,
            Some(&fixture.complete),
            &GraphRecipeRequirement::interprocedural_recipe("interprocedural-recipe"),
        )
        .unwrap();
        assert_eq!(
            decision
                .blocks()
                .iter()
                .filter(|block| matches!(block, GraphEligibilityBlock::SourceMismatch { .. }))
                .count(),
            1
        );
        assert!(
            GraphRecipeRequirement::new("invalid", vec![GraphEvidenceLayer::ProgramDependence])
                .is_err()
        );
        let local = GraphRecipeRequirement::local_recipe("local");
        let bytes = serde_json::to_vec(&local).unwrap();
        assert_eq!(
            serde_json::from_slice::<GraphRecipeRequirement>(&bytes).unwrap(),
            local
        );
        let mut unclosed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        unclosed["layers"] = serde_json::json!(["program-dependence"]);
        assert!(serde_json::from_value::<GraphRecipeRequirement>(unclosed).is_err());
    }

    #[test]
    fn m4_dod_decision_wire_rejects_schema_state_order_and_layer_corruption() {
        let fixture = fixture();
        let requirement = GraphRecipeRequirement::interprocedural_recipe("interprocedural-recipe");
        let valid = evaluate_graph_recipe_eligibility(
            &fixture.pdg,
            Some(&fixture.missing_output),
            &requirement,
        )
        .unwrap();
        let value = serde_json::to_value(&valid).unwrap();

        let mut wrong_schema = value.clone();
        wrong_schema["schema"] = "deslop.graph-recipe-eligibility/999".into();
        assert!(serde_json::from_value::<GraphEligibilityDecision>(wrong_schema).is_err());

        let mut malformed_id = value.clone();
        malformed_id["decision_id"] = "gre1_bad".into();
        assert!(serde_json::from_value::<GraphEligibilityDecision>(malformed_id).is_err());

        let mut stale_id = value.clone();
        stale_id["consumer"] = "changed-consumer".into();
        assert!(serde_json::from_value::<GraphEligibilityDecision>(stale_id).is_err());

        let mut contradictory = value.clone();
        contradictory["eligible"] = true.into();
        assert!(serde_json::from_value::<GraphEligibilityDecision>(contradictory).is_err());

        let mut unknown = value.clone();
        unknown["untrusted"] = true.into();
        assert!(serde_json::from_value::<GraphEligibilityDecision>(unknown).is_err());

        let mut noncanonical = value.clone();
        noncanonical["blocks"].as_array_mut().unwrap().reverse();
        assert!(serde_json::from_value::<GraphEligibilityDecision>(noncanonical).is_err());

        let missing = evaluate_graph_recipe_eligibility(&fixture.pdg, None, &requirement).unwrap();
        let mut unrequired_layer = serde_json::to_value(missing).unwrap();
        unrequired_layer["required_layers"] =
            serde_json::to_value(GraphRecipeRequirement::local_recipe("local").layers()).unwrap();
        assert!(serde_json::from_value::<GraphEligibilityDecision>(unrequired_layer).is_err());
    }

    #[test]
    fn m4_dod_frozen_gold_and_complete_eligibility_close_the_milestone() {
        assert_eq!(crate::program_dependence::tests::m4_gold_vector_count(), 50);
        let fixture = fixture();
        let decision = evaluate_graph_recipe_eligibility(
            &fixture.pdg,
            Some(&fixture.complete),
            &GraphRecipeRequirement::interprocedural_recipe("m4-definition-of-done"),
        )
        .unwrap();
        assert!(decision.eligible());
        assert!(decision.blocks().is_empty());
    }
}
