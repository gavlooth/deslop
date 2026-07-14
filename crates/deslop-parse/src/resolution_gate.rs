use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

use serde::Serialize;

use crate::{
    AdapterCapability, BuildContextId, CapabilityAuthority, CapabilitySupport, FactCoverage,
    ProjectionId, ResolutionEndpoint, ResolutionPathViability, ResolutionProjection,
    ResolutionResult, ResolutionResultKey, ResolutionStatus,
};

pub const RESOLUTION_CONSUMER_GATE_SCHEMA: &str = "deslop.resolution-consumer-gate/1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionCapabilityRequirement {
    capability: AdapterCapability,
    minimum_authority: CapabilityAuthority,
}

impl ResolutionCapabilityRequirement {
    pub fn new(
        capability: AdapterCapability,
        minimum_authority: CapabilityAuthority,
    ) -> Result<Self, ResolutionGateError> {
        require_static_semantic_authority(minimum_authority)?;
        Ok(Self {
            capability,
            minimum_authority,
        })
    }

    pub fn capability(&self) -> AdapterCapability {
        self.capability
    }

    pub fn minimum_authority(&self) -> CapabilityAuthority {
        self.minimum_authority
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionConsumerRequirement {
    consumer: String,
    capabilities: Vec<ResolutionCapabilityRequirement>,
}

impl ResolutionConsumerRequirement {
    pub fn unique_binding(
        consumer: impl Into<String>,
        mut capabilities: Vec<ResolutionCapabilityRequirement>,
    ) -> Result<Self, ResolutionGateError> {
        let consumer = consumer.into();
        validate_text("resolution consumer", &consumer)?;
        if capabilities.is_empty() {
            return Err(ResolutionGateError::InvalidRequirement(
                "semantic consumer requires at least NameResolution capability".into(),
            ));
        }
        capabilities.sort();
        if capabilities
            .windows(2)
            .any(|pair| pair[0].capability == pair[1].capability)
        {
            return Err(ResolutionGateError::InvalidRequirement(
                "semantic consumer contains duplicate capability requirements".into(),
            ));
        }
        if !capabilities
            .iter()
            .any(|requirement| requirement.capability == AdapterCapability::NameResolution)
        {
            return Err(ResolutionGateError::InvalidRequirement(
                "semantic unique-binding consumer must require NameResolution".into(),
            ));
        }
        Ok(Self {
            consumer,
            capabilities,
        })
    }

    pub fn consumer(&self) -> &str {
        &self.consumer
    }

    pub fn capabilities(&self) -> &[ResolutionCapabilityRequirement] {
        &self.capabilities
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionDependencyEvidence {
    projection_id: ProjectionId,
    result_key: ResolutionResultKey,
    coverage: FactCoverage,
    reasons: Vec<String>,
}

impl ResolutionDependencyEvidence {
    pub fn from_projection(
        projection: &ResolutionProjection,
        result: &ResolutionResult,
    ) -> Result<Self, ResolutionGateError> {
        require_result(projection, result)?;
        let mut coverage = result.coverage().status();
        let mut reasons = result.coverage().reasons().to_vec();
        if !result.dynamic_boundaries().is_empty() {
            coverage = FactCoverage::Partial;
            reasons.push(format!(
                "{} dynamic boundary fact(s) make reverse dependencies incomplete",
                result.dynamic_boundaries().len()
            ));
        }
        reasons.sort();
        reasons.dedup();
        if coverage == FactCoverage::Complete && !reasons.is_empty() {
            return Err(ResolutionGateError::InvalidEvidence(
                "complete reverse-dependency evidence cannot carry reasons".into(),
            ));
        }
        if coverage != FactCoverage::Complete && reasons.is_empty() {
            reasons.push("resolution coverage does not prove complete reverse dependencies".into());
        }
        Ok(Self {
            projection_id: projection.id().clone(),
            result_key: result.key().clone(),
            coverage,
            reasons,
        })
    }

    pub fn downgrade(mut self, reason: impl Into<String>) -> Result<Self, ResolutionGateError> {
        let reason = reason.into();
        validate_text("reverse-dependency downgrade reason", &reason)?;
        self.coverage = FactCoverage::Partial;
        self.reasons.push(reason);
        self.reasons.sort();
        self.reasons.dedup();
        Ok(self)
    }

    pub fn projection_id(&self) -> &ProjectionId {
        &self.projection_id
    }

    pub fn result_key(&self) -> &ResolutionResultKey {
        &self.result_key
    }

    pub fn coverage(&self) -> FactCoverage {
        self.coverage
    }

    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ResolutionEligibilityBlock {
    StatusNotUnique {
        actual: ResolutionStatus,
    },
    ResultCoverageIncomplete {
        actual: FactCoverage,
        reasons: Vec<String>,
    },
    DynamicBoundary {
        count: usize,
    },
    MissingAuthority,
    NonStaticAuthority {
        capability: AdapterCapability,
        actual: CapabilityAuthority,
    },
    AuthorityInsufficient {
        capability: AdapterCapability,
        required: CapabilityAuthority,
        actual: CapabilityAuthority,
    },
    CapabilityUnavailable {
        capability: AdapterCapability,
        support: CapabilitySupport,
    },
    CapabilityAuthorityMissing {
        capability: AdapterCapability,
    },
    ReverseDependenciesIncomplete {
        actual: FactCoverage,
        reasons: Vec<String>,
    },
    ViableEndpointCardinality {
        actual: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionEligibilityDecision {
    schema: String,
    analysis_id: String,
    projection_id: ProjectionId,
    scope_graph_id: ProjectionId,
    build_context: BuildContextId,
    result_key: ResolutionResultKey,
    requirement: ResolutionConsumerRequirement,
    dependency_evidence: ResolutionDependencyEvidence,
    eligible: bool,
    endpoint: Option<ResolutionEndpoint>,
    blocks: Vec<ResolutionEligibilityBlock>,
}

impl ResolutionEligibilityDecision {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn analysis_id(&self) -> &str {
        &self.analysis_id
    }

    pub fn projection_id(&self) -> &ProjectionId {
        &self.projection_id
    }

    pub fn scope_graph_id(&self) -> &ProjectionId {
        &self.scope_graph_id
    }

    pub fn build_context(&self) -> &BuildContextId {
        &self.build_context
    }

    pub fn result_key(&self) -> &ResolutionResultKey {
        &self.result_key
    }

    pub fn requirement(&self) -> &ResolutionConsumerRequirement {
        &self.requirement
    }

    pub fn dependency_evidence(&self) -> &ResolutionDependencyEvidence {
        &self.dependency_evidence
    }

    pub fn eligible(&self) -> bool {
        self.eligible
    }

    pub fn endpoint(&self) -> Option<&ResolutionEndpoint> {
        self.endpoint.as_ref()
    }

    pub fn blocks(&self) -> &[ResolutionEligibilityBlock] {
        &self.blocks
    }
}

pub fn evaluate_unique_binding(
    projection: &ResolutionProjection,
    result: &ResolutionResult,
    requirement: &ResolutionConsumerRequirement,
    dependency_evidence: &ResolutionDependencyEvidence,
) -> Result<ResolutionEligibilityDecision, ResolutionGateError> {
    require_result(projection, result)?;
    if dependency_evidence.projection_id != *projection.id()
        || dependency_evidence.result_key != *result.key()
    {
        return Err(ResolutionGateError::ForeignEvidence);
    }

    let mut blocks = Vec::new();
    if result.status() != ResolutionStatus::Unique {
        blocks.push(ResolutionEligibilityBlock::StatusNotUnique {
            actual: result.status(),
        });
    }
    if result.coverage().status() != FactCoverage::Complete {
        blocks.push(ResolutionEligibilityBlock::ResultCoverageIncomplete {
            actual: result.coverage().status(),
            reasons: result.coverage().reasons().to_vec(),
        });
    }
    if !result.dynamic_boundaries().is_empty() {
        blocks.push(ResolutionEligibilityBlock::DynamicBoundary {
            count: result.dynamic_boundaries().len(),
        });
    }

    let manifest = result.reference_evidence().adapter.capabilities();
    for capability in requirement.capabilities() {
        let declaration = manifest.declaration(capability.capability);
        if declaration.support() != CapabilitySupport::Provided {
            blocks.push(ResolutionEligibilityBlock::CapabilityUnavailable {
                capability: capability.capability,
                support: declaration.support(),
            });
            continue;
        }
        let actual_authority = if capability.capability == AdapterCapability::NameResolution {
            result.authority()
        } else {
            declaration.authority()
        };
        let Some(actual_authority) = actual_authority else {
            if capability.capability == AdapterCapability::NameResolution {
                blocks.push(ResolutionEligibilityBlock::MissingAuthority);
            } else {
                blocks.push(ResolutionEligibilityBlock::CapabilityAuthorityMissing {
                    capability: capability.capability,
                });
            }
            continue;
        };
        if static_authority_rank(actual_authority).is_none() {
            blocks.push(ResolutionEligibilityBlock::NonStaticAuthority {
                capability: capability.capability,
                actual: actual_authority,
            });
        } else if static_authority_rank(actual_authority)
            < static_authority_rank(capability.minimum_authority)
        {
            blocks.push(ResolutionEligibilityBlock::AuthorityInsufficient {
                capability: capability.capability,
                required: capability.minimum_authority,
                actual: actual_authority,
            });
        }
    }

    if dependency_evidence.coverage != FactCoverage::Complete {
        blocks.push(ResolutionEligibilityBlock::ReverseDependenciesIncomplete {
            actual: dependency_evidence.coverage,
            reasons: dependency_evidence.reasons.clone(),
        });
    }

    let endpoints = result
        .paths()
        .iter()
        .filter(|path| path.viability() == ResolutionPathViability::Viable)
        .filter_map(|path| path.endpoint().cloned())
        .collect::<BTreeSet<_>>();
    if endpoints.len() != 1 {
        blocks.push(ResolutionEligibilityBlock::ViableEndpointCardinality {
            actual: endpoints.len(),
        });
    }
    blocks.sort();
    blocks.dedup();
    let eligible = blocks.is_empty();
    let endpoint = eligible.then(|| {
        endpoints
            .into_iter()
            .next()
            .expect("eligible unique binding has exactly one endpoint")
    });
    let document = projection.document();
    Ok(ResolutionEligibilityDecision {
        schema: RESOLUTION_CONSUMER_GATE_SCHEMA.into(),
        analysis_id: document.analysis_id().into(),
        projection_id: projection.id().clone(),
        scope_graph_id: document.scope_graph_id().clone(),
        build_context: document.build_context().clone(),
        result_key: result.key().clone(),
        requirement: requirement.clone(),
        dependency_evidence: dependency_evidence.clone(),
        eligible,
        endpoint,
        blocks,
    })
}

fn require_result(
    projection: &ResolutionProjection,
    result: &ResolutionResult,
) -> Result<(), ResolutionGateError> {
    if projection
        .results()
        .iter()
        .any(|record| record.wire().key() == result.key() && record.wire() == result)
    {
        Ok(())
    } else {
        Err(ResolutionGateError::ForeignResult)
    }
}

fn require_static_semantic_authority(
    authority: CapabilityAuthority,
) -> Result<(), ResolutionGateError> {
    match authority {
        CapabilityAuthority::Adapter
        | CapabilityAuthority::LanguageServer
        | CapabilityAuthority::Compiler => Ok(()),
        CapabilityAuthority::Syntax => Err(ResolutionGateError::InvalidRequirement(
            "Syntax authority cannot authorize a semantic unique-binding consumer".into(),
        )),
        CapabilityAuthority::RuntimeVerification => Err(ResolutionGateError::InvalidRequirement(
            "RuntimeVerification is orthogonal to static resolution authority".into(),
        )),
    }
}

fn static_authority_rank(authority: CapabilityAuthority) -> Option<u8> {
    match authority {
        CapabilityAuthority::Syntax => Some(0),
        CapabilityAuthority::Adapter => Some(1),
        CapabilityAuthority::LanguageServer => Some(2),
        CapabilityAuthority::Compiler => Some(3),
        CapabilityAuthority::RuntimeVerification => None,
    }
}

fn validate_text(label: &str, value: &str) -> Result<(), ResolutionGateError> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        Err(ResolutionGateError::InvalidRequirement(format!(
            "{label} must be nonempty control-free text"
        )))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionGateError {
    InvalidRequirement(String),
    InvalidEvidence(String),
    ForeignResult,
    ForeignEvidence,
}

impl fmt::Display for ResolutionGateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequirement(message) => {
                write!(
                    formatter,
                    "invalid resolution consumer requirement: {message}"
                )
            }
            Self::InvalidEvidence(message) => {
                write!(
                    formatter,
                    "invalid resolution dependency evidence: {message}"
                )
            }
            Self::ForeignResult => {
                write!(formatter, "resolution result belongs to another projection")
            }
            Self::ForeignEvidence => write!(
                formatter,
                "resolution dependency evidence belongs to another projection or result"
            ),
        }
    }
}

impl Error for ResolutionGateError {}
