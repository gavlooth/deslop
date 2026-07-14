use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use deslop_lang::{
    CapabilityAuthority, CapabilitySupport, DuplicateDefinitionRule, ImportTraversalRule,
    LanguageResolutionRulePack, PrecedenceDimension, PrecedenceDirection, ResolutionInstruction,
    ResolutionRuleSectionKind, RuleNamespace,
};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    DynamicBoundaryTraversal, FactCoverage, NamespaceReachability, ProjectionId,
    ResolutionTraversal, ResolutionTraversalEngine, ResolutionTraversalError, RuleSectionGap,
    ScopeFactData, ScopeFactEvidence, ScopeFactId, ScopeFactKey, ScopeFactKind,
    ScopeGraphProjection, SemanticProvider, SemanticResolutionFact, SemanticResolutionFactDocument,
    SemanticResolutionFactError, SemanticResolutionFactKey, SemanticResolutionFacts,
    TimingObservation, TraversalCandidate, VisibilityObservation,
};

pub const RESOLUTION_SCHEMA: &str = "deslop.resolution/1";
pub const RESOLUTION_POLICY_SCHEMA: &str = "deslop.resolution-policy/1";

static NEXT_RESOLUTION_OWNER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ResolutionPolicyId(String);

impl ResolutionPolicyId {
    pub fn from_parts(parts: &[&[u8]]) -> Result<Self, ResolutionProjectionError> {
        derive_key(RESOLUTION_POLICY_SCHEMA, "rpol1_", parts).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ResolutionPolicyId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_key(&value, "rpol1_").map_err(D::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ResolutionPathKey(String);

impl ResolutionPathKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ResolutionPathKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_key(&value, "rp1_").map_err(D::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ResolutionResultKey(String);

impl ResolutionResultKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ResolutionResultKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_key(&value, "rr1_").map_err(D::Error::custom)?;
        Ok(Self(value))
    }
}

/// Dense identity for one result in a live projection. Wire consumers use [`ResolutionResultKey`].
///
/// ```compile_fail
/// fn assert_serializable<T: serde::Serialize>() {}
/// assert_serializable::<deslop_parse::ResolutionResultId>();
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolutionResultId {
    owner: u64,
    index: u32,
}

impl fmt::Debug for ResolutionResultId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolutionResultId")
            .field("owner", &self.owner)
            .field("index", &self.index)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionStatus {
    Unique,
    Ambiguous,
    Unresolved,
    Unknown,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionCoverageEvidence {
    status: FactCoverage,
    reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "key", rename_all = "kebab-case")]
pub enum ResolutionConclusionSource {
    Adapter,
    Semantic(SemanticResolutionFactKey),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionConclusion {
    source: ResolutionConclusionSource,
    authority: Option<CapabilityAuthority>,
    status: ResolutionStatus,
    endpoints: Vec<ResolutionEndpoint>,
    coverage: ResolutionCoverageEvidence,
}

impl ResolutionConclusion {
    pub fn source(&self) -> &ResolutionConclusionSource {
        &self.source
    }

    pub fn authority(&self) -> Option<CapabilityAuthority> {
        self.authority
    }

    pub fn status(&self) -> ResolutionStatus {
        self.status
    }

    pub fn endpoints(&self) -> &[ResolutionEndpoint] {
        &self.endpoints
    }

    pub fn coverage(&self) -> &ResolutionCoverageEvidence {
        &self.coverage
    }

    fn validate(&self) -> Result<(), ResolutionProjectionError> {
        self.coverage.validate()?;
        if self.endpoints.iter().collect::<BTreeSet<_>>().len() != self.endpoints.len() {
            return Err(ResolutionProjectionError::Invalid(
                "resolution conclusion contains duplicate endpoints".into(),
            ));
        }
        if self.coverage.status == FactCoverage::Complete
            && self.authority.is_none()
            && self.status != ResolutionStatus::Conflict
        {
            return Err(ResolutionProjectionError::Invalid(
                "complete resolution conclusion requires evidence authority".into(),
            ));
        }
        if self.authority == Some(CapabilityAuthority::RuntimeVerification) {
            return Err(ResolutionProjectionError::Invalid(
                "runtime verification cannot enter a static resolution conclusion".into(),
            ));
        }
        if self.coverage.status == FactCoverage::Complete
            && self.authority == Some(CapabilityAuthority::Syntax)
        {
            return Err(ResolutionProjectionError::Invalid(
                "syntax authority cannot assert a terminal binding conclusion".into(),
            ));
        }
        match (self.coverage.status, self.status, self.endpoints.len()) {
            (FactCoverage::Complete, ResolutionStatus::Unique, 1)
            | (FactCoverage::Complete, ResolutionStatus::Unresolved, 0) => Ok(()),
            (FactCoverage::Complete, ResolutionStatus::Ambiguous, count) if count > 1 => Ok(()),
            (FactCoverage::Complete, ResolutionStatus::Conflict, _)
                if self.source == ResolutionConclusionSource::Adapter =>
            {
                Ok(())
            }
            (FactCoverage::Complete, ResolutionStatus::Unknown | ResolutionStatus::Conflict, _) => {
                Err(ResolutionProjectionError::Invalid(
                    "one provider conclusion cannot be complete unknown/conflict evidence".into(),
                ))
            }
            (FactCoverage::Complete, _, _) => Err(ResolutionProjectionError::Invalid(
                "resolution conclusion endpoint cardinality contradicts its status".into(),
            )),
            (_, ResolutionStatus::Unknown, _) => Ok(()),
            (_, _, _) => Err(ResolutionProjectionError::Invalid(
                "incomplete resolution conclusion must remain unknown".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferredResolutionConclusion {
    authority: CapabilityAuthority,
    status: ResolutionStatus,
    endpoints: Vec<ResolutionEndpoint>,
    sources: Vec<ResolutionConclusionSource>,
}

impl PreferredResolutionConclusion {
    pub fn authority(&self) -> CapabilityAuthority {
        self.authority
    }

    pub fn status(&self) -> ResolutionStatus {
        self.status
    }

    pub fn endpoints(&self) -> &[ResolutionEndpoint] {
        &self.endpoints
    }

    pub fn sources(&self) -> &[ResolutionConclusionSource] {
        &self.sources
    }

    fn validate(&self) -> Result<(), ResolutionProjectionError> {
        if self.status == ResolutionStatus::Unknown || self.status == ResolutionStatus::Conflict {
            return Err(ResolutionProjectionError::Invalid(
                "preferred conclusion must be terminal and non-conflicting".into(),
            ));
        }
        if self.sources.is_empty()
            || self.sources.iter().collect::<BTreeSet<_>>().len() != self.sources.len()
        {
            return Err(ResolutionProjectionError::Invalid(
                "preferred conclusion requires distinct evidence sources".into(),
            ));
        }
        match (self.status, self.endpoints.len()) {
            (ResolutionStatus::Unique, 1) | (ResolutionStatus::Unresolved, 0) => Ok(()),
            (ResolutionStatus::Ambiguous, count) if count > 1 => Ok(()),
            _ => Err(ResolutionProjectionError::Invalid(
                "preferred conclusion endpoint cardinality contradicts its status".into(),
            )),
        }
    }
}

impl ResolutionCoverageEvidence {
    pub fn status(&self) -> FactCoverage {
        self.status
    }

    pub fn reasons(&self) -> &[String] {
        &self.reasons
    }

    fn validate(&self) -> Result<(), ResolutionProjectionError> {
        match (self.status, self.reasons.is_empty()) {
            (FactCoverage::Complete, true) => Ok(()),
            (FactCoverage::Complete, false) => Err(ResolutionProjectionError::Invalid(
                "complete resolution coverage cannot carry reasons".into(),
            )),
            (_, false) => {
                for reason in &self.reasons {
                    validate_text("resolution coverage reason", reason)?;
                }
                if self.reasons.iter().collect::<BTreeSet<_>>().len() != self.reasons.len() {
                    return Err(ResolutionProjectionError::Invalid(
                        "resolution coverage contains duplicate reasons".into(),
                    ));
                }
                Ok(())
            }
            (_, true) => Err(ResolutionProjectionError::Invalid(
                "incomplete resolution coverage requires an exact reason".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionPathEdgeKind {
    ReferenceScope,
    LexicalParent,
    Declares,
    Defines,
    Binds,
    ExplicitShadowing,
    ExplicitImport,
    AliasImport,
    SelectiveImport,
    GlobImport,
    Member,
    Module,
    Export,
    ReExport,
    Prelude,
    Package,
    ExternalProvider,
    DynamicBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionPathEdge {
    kind: ResolutionPathEdgeKind,
    from: ScopeFactKey,
    to: ScopeFactKey,
    source_fact: ScopeFactKey,
}

impl ResolutionPathEdge {
    pub fn kind(&self) -> ResolutionPathEdgeKind {
        self.kind
    }

    pub fn from(&self) -> &ScopeFactKey {
        &self.from
    }

    pub fn to(&self) -> &ScopeFactKey {
        &self.to
    }

    pub fn source_fact(&self) -> &ScopeFactKey {
        &self.source_fact
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "key", rename_all = "kebab-case")]
pub enum ResolutionEndpoint {
    Declaration(ScopeFactKey),
    Definition(ScopeFactKey),
    Module(ScopeFactKey),
    MergedDeclarations(Vec<ScopeFactKey>),
    External(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionCheckKind {
    Namespace,
    Visibility,
    Timing,
    Qualification,
    LookupPrecedence,
    DuplicateDefinition,
    Condition,
    BuildTarget,
    AdapterIdentity,
    Shadowing,
    ImportTarget,
    ExportSetCoverage,
    ProviderIdentity,
    ProviderArtifact,
    ProviderProjectModel,
    EvidenceAuthority,
    DynamicBoundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionCheckState {
    Passed,
    Rejected,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionCheck {
    kind: ResolutionCheckKind,
    state: ResolutionCheckState,
    detail: String,
    source_facts: Vec<ScopeFactKey>,
}

impl ResolutionCheck {
    pub fn kind(&self) -> ResolutionCheckKind {
        self.kind
    }

    pub fn state(&self) -> ResolutionCheckState {
        self.state
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    pub fn source_facts(&self) -> &[ScopeFactKey] {
        &self.source_facts
    }

    fn validate(&self) -> Result<(), ResolutionProjectionError> {
        validate_text("resolution check detail", &self.detail)?;
        validate_unique_keys("resolution check source facts", &self.source_facts)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionRejectionReason {
    Shadowed,
    WrongNamespace,
    NotVisible,
    DeclaredLater,
    InactiveCondition,
    WrongBuildTarget,
    ImportUnresolved,
    ExportIncomplete,
    OpaqueBoundary,
    ProviderConflict,
    DuplicateDefinition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionPathViability {
    Viable,
    Rejected,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionPrecedenceComponent {
    dimension: PrecedenceDimension,
    direction: PrecedenceDirection,
    value: u64,
}

impl ResolutionPrecedenceComponent {
    pub fn dimension(self) -> PrecedenceDimension {
        self.dimension
    }

    pub fn direction(self) -> PrecedenceDirection {
        self.direction
    }

    pub fn value(self) -> u64 {
        self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionPath {
    key: ResolutionPathKey,
    endpoint: Option<ResolutionEndpoint>,
    edges: Vec<ResolutionPathEdge>,
    precedence: Vec<ResolutionPrecedenceComponent>,
    viability: ResolutionPathViability,
    rejection_reasons: Vec<ResolutionRejectionReason>,
    checks: Vec<ResolutionCheck>,
    source_facts: Vec<ScopeFactKey>,
    source_provider_facts: Vec<SemanticResolutionFactKey>,
    dynamic_boundaries: Vec<ScopeFactKey>,
    authorities: Vec<CapabilityAuthority>,
    coverage: ResolutionCoverageEvidence,
}

impl ResolutionPath {
    pub fn key(&self) -> &ResolutionPathKey {
        &self.key
    }

    pub fn endpoint(&self) -> Option<&ResolutionEndpoint> {
        self.endpoint.as_ref()
    }

    pub fn edges(&self) -> &[ResolutionPathEdge] {
        &self.edges
    }

    pub fn precedence(&self) -> &[ResolutionPrecedenceComponent] {
        &self.precedence
    }

    pub fn viability(&self) -> ResolutionPathViability {
        self.viability
    }

    pub fn rejection_reasons(&self) -> &[ResolutionRejectionReason] {
        &self.rejection_reasons
    }

    pub fn checks(&self) -> &[ResolutionCheck] {
        &self.checks
    }

    pub fn source_facts(&self) -> &[ScopeFactKey] {
        &self.source_facts
    }

    pub fn source_provider_facts(&self) -> &[SemanticResolutionFactKey] {
        &self.source_provider_facts
    }

    pub fn dynamic_boundaries(&self) -> &[ScopeFactKey] {
        &self.dynamic_boundaries
    }

    pub fn authorities(&self) -> &[CapabilityAuthority] {
        &self.authorities
    }

    pub fn coverage(&self) -> &ResolutionCoverageEvidence {
        &self.coverage
    }

    fn finish(mut self) -> Result<Self, ResolutionProjectionError> {
        self.key = ResolutionPathKey(derive_serialized_key(
            RESOLUTION_SCHEMA,
            "rp1_",
            &ResolutionPathPayload::from(&self),
        )?);
        self.validate()?;
        Ok(self)
    }

    fn validate(&self) -> Result<(), ResolutionProjectionError> {
        validate_key(self.key.as_str(), "rp1_")?;
        if self.edges.is_empty() {
            return Err(ResolutionProjectionError::Invalid(
                "resolution path must retain at least one edge".into(),
            ));
        }
        if self.precedence.is_empty()
            && self.source_provider_facts.is_empty()
            && !self.checks.iter().any(|check| {
                check.kind == ResolutionCheckKind::LookupPrecedence
                    && check.state == ResolutionCheckState::Unknown
            })
        {
            return Err(ResolutionProjectionError::Invalid(
                "resolution path without precedence lacks an explicit unknown check".into(),
            ));
        }
        if self
            .precedence
            .iter()
            .map(|component| component.dimension)
            .collect::<BTreeSet<_>>()
            .len()
            != self.precedence.len()
        {
            return Err(ResolutionProjectionError::Invalid(
                "resolution path precedence contains duplicate dimensions".into(),
            ));
        }
        if self.source_facts.is_empty() {
            return Err(ResolutionProjectionError::Invalid(
                "resolution path has no source facts".into(),
            ));
        }
        validate_unique_keys("resolution path source facts", &self.source_facts)?;
        if self
            .source_provider_facts
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != self.source_provider_facts.len()
        {
            return Err(ResolutionProjectionError::Invalid(
                "resolution path contains duplicate provider facts".into(),
            ));
        }
        validate_unique_keys(
            "resolution path dynamic boundaries",
            &self.dynamic_boundaries,
        )?;
        self.coverage.validate()?;
        if self.authorities.iter().collect::<BTreeSet<_>>().len() != self.authorities.len() {
            return Err(ResolutionProjectionError::Invalid(
                "resolution path contains duplicate authorities".into(),
            ));
        }
        if self.coverage.status == FactCoverage::Complete && self.authorities.is_empty() {
            return Err(ResolutionProjectionError::Invalid(
                "complete resolution path coverage requires evidence authority".into(),
            ));
        }
        for check in &self.checks {
            check.validate()?;
            for key in check.source_facts() {
                if !self.source_facts.contains(key) {
                    return Err(ResolutionProjectionError::Invalid(
                        "resolution check references an unretained source fact".into(),
                    ));
                }
            }
        }
        for edge in &self.edges {
            for key in [&edge.from, &edge.to, &edge.source_fact] {
                if !self.source_facts.contains(key) {
                    return Err(ResolutionProjectionError::Invalid(
                        "resolution edge references an unretained source fact".into(),
                    ));
                }
            }
        }
        for key in &self.dynamic_boundaries {
            if !self.source_facts.contains(key) {
                return Err(ResolutionProjectionError::Invalid(
                    "resolution dynamic boundary is absent from source facts".into(),
                ));
            }
        }
        if let Some(ResolutionEndpoint::External(provider)) = &self.endpoint {
            validate_text("external resolution provider", provider)?;
        }
        if let Some(
            ResolutionEndpoint::Declaration(key)
            | ResolutionEndpoint::Definition(key)
            | ResolutionEndpoint::Module(key),
        ) = &self.endpoint
            && !self.source_facts.contains(key)
        {
            return Err(ResolutionProjectionError::Invalid(
                "resolution endpoint is absent from path source facts".into(),
            ));
        }
        if let Some(ResolutionEndpoint::MergedDeclarations(declarations)) = &self.endpoint {
            if declarations.len() < 2 {
                return Err(ResolutionProjectionError::Invalid(
                    "merged resolution endpoint requires at least two declarations".into(),
                ));
            }
            validate_unique_keys("merged resolution declarations", declarations)?;
            if declarations
                .iter()
                .any(|declaration| !self.source_facts.contains(declaration))
            {
                return Err(ResolutionProjectionError::Invalid(
                    "merged resolution endpoint omits a declaration source fact".into(),
                ));
            }
        }
        let rejected = self
            .checks
            .iter()
            .any(|check| check.state == ResolutionCheckState::Rejected);
        let unknown = self
            .checks
            .iter()
            .any(|check| check.state == ResolutionCheckState::Unknown);
        match self.viability {
            ResolutionPathViability::Viable if rejected || unknown => Err(
                ResolutionProjectionError::Invalid("viable path has a failed check".into()),
            ),
            ResolutionPathViability::Rejected if !rejected && self.rejection_reasons.is_empty() => {
                Err(ResolutionProjectionError::Invalid(
                    "rejected path has no rejection evidence".into(),
                ))
            }
            ResolutionPathViability::Unknown if !unknown => Err(
                ResolutionProjectionError::Invalid("unknown path has no unknown check".into()),
            ),
            _ => Ok(()),
        }?;
        if self.rejection_reasons.iter().collect::<BTreeSet<_>>().len()
            != self.rejection_reasons.len()
        {
            return Err(ResolutionProjectionError::Invalid(
                "resolution path contains duplicate rejection reasons".into(),
            ));
        }
        let expected = derive_serialized_key(
            RESOLUTION_SCHEMA,
            "rp1_",
            &ResolutionPathPayload::from(self),
        )?;
        if expected != self.key.0 {
            return Err(ResolutionProjectionError::Invalid(
                "resolution path key does not match its complete payload".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ResolutionPathPayload<'a> {
    endpoint: &'a Option<ResolutionEndpoint>,
    edges: &'a [ResolutionPathEdge],
    precedence: &'a [ResolutionPrecedenceComponent],
    viability: ResolutionPathViability,
    rejection_reasons: &'a [ResolutionRejectionReason],
    checks: &'a [ResolutionCheck],
    source_facts: &'a [ScopeFactKey],
    source_provider_facts: &'a [SemanticResolutionFactKey],
    dynamic_boundaries: &'a [ScopeFactKey],
    authorities: &'a [CapabilityAuthority],
    coverage: &'a ResolutionCoverageEvidence,
}

impl<'a> From<&'a ResolutionPath> for ResolutionPathPayload<'a> {
    fn from(path: &'a ResolutionPath) -> Self {
        Self {
            endpoint: &path.endpoint,
            edges: &path.edges,
            precedence: &path.precedence,
            viability: path.viability,
            rejection_reasons: &path.rejection_reasons,
            checks: &path.checks,
            source_facts: &path.source_facts,
            source_provider_facts: &path.source_provider_facts,
            dynamic_boundaries: &path.dynamic_boundaries,
            authorities: &path.authorities,
            coverage: &path.coverage,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionResult {
    key: ResolutionResultKey,
    reference: ScopeFactKey,
    start_scope: ScopeFactKey,
    reference_evidence: ScopeFactEvidence,
    coverage: ResolutionCoverageEvidence,
    status: ResolutionStatus,
    authority: Option<CapabilityAuthority>,
    conclusions: Vec<ResolutionConclusion>,
    preferred: Option<PreferredResolutionConclusion>,
    paths: Vec<ResolutionPath>,
    source_facts: Vec<ScopeFactKey>,
    source_provider_facts: Vec<SemanticResolutionFactKey>,
    dynamic_boundaries: Vec<ScopeFactKey>,
    diagnostics: Vec<String>,
}

impl ResolutionResult {
    pub fn key(&self) -> &ResolutionResultKey {
        &self.key
    }

    pub fn reference(&self) -> &ScopeFactKey {
        &self.reference
    }

    pub fn start_scope(&self) -> &ScopeFactKey {
        &self.start_scope
    }

    pub fn reference_evidence(&self) -> &ScopeFactEvidence {
        &self.reference_evidence
    }

    pub fn coverage(&self) -> &ResolutionCoverageEvidence {
        &self.coverage
    }

    pub fn status(&self) -> ResolutionStatus {
        self.status
    }

    pub fn authority(&self) -> Option<CapabilityAuthority> {
        self.authority
    }

    pub fn conclusions(&self) -> &[ResolutionConclusion] {
        &self.conclusions
    }

    pub fn preferred(&self) -> Option<&PreferredResolutionConclusion> {
        self.preferred.as_ref()
    }

    pub fn paths(&self) -> &[ResolutionPath] {
        &self.paths
    }

    pub fn source_facts(&self) -> &[ScopeFactKey] {
        &self.source_facts
    }

    pub fn source_provider_facts(&self) -> &[SemanticResolutionFactKey] {
        &self.source_provider_facts
    }

    pub fn dynamic_boundaries(&self) -> &[ScopeFactKey] {
        &self.dynamic_boundaries
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    fn finish(mut self) -> Result<Self, ResolutionProjectionError> {
        self.key = ResolutionResultKey(derive_serialized_key(
            RESOLUTION_SCHEMA,
            "rr1_",
            &ResolutionResultPayload::from(&self),
        )?);
        self.validate()?;
        Ok(self)
    }

    fn validate(&self) -> Result<(), ResolutionProjectionError> {
        validate_key(self.key.as_str(), "rr1_")?;
        self.coverage.validate()?;
        validate_unique_keys("resolution result source facts", &self.source_facts)?;
        if self
            .source_provider_facts
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != self.source_provider_facts.len()
        {
            return Err(ResolutionProjectionError::Invalid(
                "resolution result contains duplicate provider facts".into(),
            ));
        }
        validate_unique_keys(
            "resolution result dynamic boundaries",
            &self.dynamic_boundaries,
        )?;
        for diagnostic in &self.diagnostics {
            validate_text("resolution diagnostic", diagnostic)?;
        }
        if self.diagnostics.iter().collect::<BTreeSet<_>>().len() != self.diagnostics.len() {
            return Err(ResolutionProjectionError::Invalid(
                "resolution result contains duplicate diagnostics".into(),
            ));
        }
        let path_keys = self
            .paths
            .iter()
            .map(|path| path.key())
            .collect::<BTreeSet<_>>();
        if path_keys.len() != self.paths.len() {
            return Err(ResolutionProjectionError::Invalid(
                "resolution result contains duplicate paths".into(),
            ));
        }
        for path in &self.paths {
            path.validate()?;
            for key in path.source_facts() {
                if !self.source_facts.contains(key) {
                    return Err(ResolutionProjectionError::Invalid(
                        "resolution path source fact is absent from its result".into(),
                    ));
                }
            }
            for key in path.source_provider_facts() {
                if !self.source_provider_facts.contains(key) {
                    return Err(ResolutionProjectionError::Invalid(
                        "resolution path provider fact is absent from its result".into(),
                    ));
                }
            }
        }
        if self.coverage.status == FactCoverage::Complete
            && self.preferred.as_ref().is_some_and(|preferred| {
                self.paths.iter().any(|path| {
                    let supplies_preferred =
                        preferred.sources().iter().any(|source| match source {
                            ResolutionConclusionSource::Adapter => {
                                path.source_provider_facts().is_empty()
                            }
                            ResolutionConclusionSource::Semantic(key) => {
                                path.source_provider_facts().contains(key)
                            }
                        });
                    supplies_preferred && path.coverage.status != FactCoverage::Complete
                })
            })
        {
            return Err(ResolutionProjectionError::Invalid(
                "complete resolution result contains an incomplete candidate path for its preferred conclusion"
                    .into(),
            ));
        }
        if !self.source_facts.contains(&self.reference)
            || !self.source_facts.contains(&self.start_scope)
        {
            return Err(ResolutionProjectionError::Invalid(
                "resolution result omits its reference or starting scope source fact".into(),
            ));
        }
        if self.reference_evidence.capability != crate::AdapterCapability::NameResolution {
            return Err(ResolutionProjectionError::Invalid(
                "resolution result reference evidence is not name-resolution evidence".into(),
            ));
        }
        if self.coverage.status == FactCoverage::Complete
            && self.authority.is_none()
            && self.status != ResolutionStatus::Conflict
        {
            return Err(ResolutionProjectionError::Invalid(
                "complete resolution coverage requires explicit evidence authority".into(),
            ));
        }
        if self.conclusions.is_empty() {
            return Err(ResolutionProjectionError::Invalid(
                "resolution result contains no provider conclusions".into(),
            ));
        }
        let mut sources = BTreeSet::new();
        let mut adapter_conclusion = None;
        for conclusion in &self.conclusions {
            conclusion.validate()?;
            if !sources.insert(conclusion.source()) {
                return Err(ResolutionProjectionError::Invalid(
                    "resolution result contains duplicate conclusion sources".into(),
                ));
            }
            if conclusion.source() == &ResolutionConclusionSource::Adapter {
                adapter_conclusion = Some(conclusion);
            }
        }
        let adapter_conclusion = adapter_conclusion.ok_or_else(|| {
            ResolutionProjectionError::Invalid(
                "resolution result omits its adapter conclusion".into(),
            )
        })?;
        if adapter_conclusion.authority() != self.reference_evidence.authority {
            return Err(ResolutionProjectionError::Invalid(
                "adapter conclusion authority differs from reference evidence".into(),
            ));
        }
        if let Some(preferred) = &self.preferred {
            preferred.validate()?;
            if self.authority != Some(preferred.authority()) {
                return Err(ResolutionProjectionError::Invalid(
                    "resolution result authority differs from its preferred conclusion".into(),
                ));
            }
        } else if self.authority.is_some() {
            return Err(ResolutionProjectionError::Invalid(
                "resolution result without a preferred conclusion cannot claim authority".into(),
            ));
        }
        validate_status(self)?;
        let expected = derive_serialized_key(
            RESOLUTION_SCHEMA,
            "rr1_",
            &ResolutionResultPayload::from(self),
        )?;
        if expected != self.key.0 {
            return Err(ResolutionProjectionError::Invalid(
                "resolution result key does not match its complete payload".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ResolutionResultPayload<'a> {
    reference: &'a ScopeFactKey,
    start_scope: &'a ScopeFactKey,
    reference_evidence: &'a ScopeFactEvidence,
    coverage: &'a ResolutionCoverageEvidence,
    status: ResolutionStatus,
    authority: Option<CapabilityAuthority>,
    conclusions: &'a [ResolutionConclusion],
    preferred: &'a Option<PreferredResolutionConclusion>,
    paths: &'a [ResolutionPath],
    source_facts: &'a [ScopeFactKey],
    source_provider_facts: &'a [SemanticResolutionFactKey],
    dynamic_boundaries: &'a [ScopeFactKey],
    diagnostics: &'a [String],
}

impl<'a> From<&'a ResolutionResult> for ResolutionResultPayload<'a> {
    fn from(result: &'a ResolutionResult) -> Self {
        Self {
            reference: &result.reference,
            start_scope: &result.start_scope,
            reference_evidence: &result.reference_evidence,
            coverage: &result.coverage,
            status: result.status,
            authority: result.authority,
            conclusions: &result.conclusions,
            preferred: &result.preferred,
            paths: &result.paths,
            source_facts: &result.source_facts,
            source_provider_facts: &result.source_provider_facts,
            dynamic_boundaries: &result.dynamic_boundaries,
            diagnostics: &result.diagnostics,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolutionResultRecord {
    id: ResolutionResultId,
    wire: ResolutionResult,
}

impl ResolutionResultRecord {
    pub fn id(&self) -> ResolutionResultId {
        self.id
    }

    pub fn wire(&self) -> &ResolutionResult {
        &self.wire
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionDocument {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    scope_graph_id: ProjectionId,
    build_context: crate::BuildContextId,
    fact_policy: crate::ScopeFactPolicyId,
    resolution_policy: ResolutionPolicyId,
    semantic_facts: SemanticResolutionFactDocument,
    results: Vec<ResolutionResult>,
}

impl ResolutionDocument {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn projection_id(&self) -> &ProjectionId {
        &self.projection_id
    }

    pub fn analysis_id(&self) -> &str {
        &self.analysis_id
    }

    pub fn scope_graph_id(&self) -> &ProjectionId {
        &self.scope_graph_id
    }

    pub fn build_context(&self) -> &crate::BuildContextId {
        &self.build_context
    }

    pub fn fact_policy(&self) -> &crate::ScopeFactPolicyId {
        &self.fact_policy
    }

    pub fn resolution_policy(&self) -> &ResolutionPolicyId {
        &self.resolution_policy
    }

    pub fn semantic_facts(&self) -> &SemanticResolutionFactDocument {
        &self.semantic_facts
    }

    pub fn results(&self) -> &[ResolutionResult] {
        &self.results
    }

    fn validate(&self) -> Result<(), ResolutionProjectionError> {
        if self.schema != RESOLUTION_SCHEMA {
            return Err(ResolutionProjectionError::Invalid(format!(
                "unsupported resolution schema {}",
                self.schema
            )));
        }
        validate_text("resolution analysis identity", &self.analysis_id)?;
        self.semantic_facts
            .validate()
            .map_err(|error| ResolutionProjectionError::Invalid(error.to_string()))?;
        if self.semantic_facts.analysis_id() != self.analysis_id
            || self.semantic_facts.scope_graph_id() != &self.scope_graph_id
            || self.semantic_facts.build_context() != &self.build_context
        {
            return Err(ResolutionProjectionError::Invalid(
                "resolution semantic facts belong to another analysis, graph, or build context"
                    .into(),
            ));
        }
        let mut references = BTreeSet::new();
        let mut keys = BTreeSet::new();
        let semantic_facts = self
            .semantic_facts
            .facts()
            .iter()
            .map(|fact| (fact.key(), fact))
            .collect::<BTreeMap<_, _>>();
        let semantic_providers = self
            .semantic_facts
            .providers()
            .iter()
            .map(|provider| (provider.key(), provider))
            .collect::<BTreeMap<_, _>>();
        let mut used_semantic_facts = BTreeSet::new();
        for result in &self.results {
            result.validate()?;
            if !references.insert(result.reference()) {
                return Err(ResolutionProjectionError::Invalid(
                    "resolution document contains duplicate reference results".into(),
                ));
            }
            if !keys.insert(result.key()) {
                return Err(ResolutionProjectionError::Invalid(
                    "resolution document contains duplicate result keys".into(),
                ));
            }
            if result
                .conclusions()
                .iter()
                .filter(|conclusion| conclusion.source() == &ResolutionConclusionSource::Adapter)
                .count()
                != 1
            {
                return Err(ResolutionProjectionError::Invalid(
                    "resolution result must retain exactly one adapter conclusion".into(),
                ));
            }
            for key in result.source_provider_facts() {
                let fact = semantic_facts.get(key).ok_or_else(|| {
                    ResolutionProjectionError::Invalid(
                        "resolution result references an absent semantic fact".into(),
                    )
                })?;
                if fact.reference() != result.reference() {
                    return Err(ResolutionProjectionError::Invalid(
                        "semantic fact is attached to the wrong resolution reference".into(),
                    ));
                }
                used_semantic_facts.insert(key);
            }
            for conclusion in result.conclusions() {
                if let ResolutionConclusionSource::Semantic(key) = conclusion.source() {
                    if !result.source_provider_facts().contains(key) {
                        return Err(ResolutionProjectionError::Invalid(
                            "semantic conclusion omits its retained provider fact".into(),
                        ));
                    }
                    let fact = semantic_facts.get(key).ok_or_else(|| {
                        ResolutionProjectionError::Invalid(
                            "semantic conclusion references an absent fact".into(),
                        )
                    })?;
                    let provider = semantic_providers.get(fact.provider()).ok_or_else(|| {
                        ResolutionProjectionError::Invalid(
                            "semantic conclusion references an absent provider".into(),
                        )
                    })?;
                    let expected_coverage = semantic_conclusion_coverage(provider, fact)?;
                    let expected_status = if expected_coverage.status == FactCoverage::Complete {
                        fact.status()
                    } else {
                        ResolutionStatus::Unknown
                    };
                    if conclusion.authority() != Some(provider.authority())
                        || conclusion.status() != expected_status
                        || conclusion.endpoints() != fact.endpoints()
                        || conclusion.coverage() != &expected_coverage
                    {
                        return Err(ResolutionProjectionError::Invalid(
                            "semantic conclusion contradicts its pinned provider fact".into(),
                        ));
                    }
                }
            }
            let mut provider_paths =
                BTreeMap::<&SemanticResolutionFactKey, Vec<&ResolutionPath>>::new();
            for path in result.paths() {
                if path.source_provider_facts().is_empty() {
                    continue;
                }
                if path.source_provider_facts().len() != 1 {
                    return Err(ResolutionProjectionError::Invalid(
                        "one provider path must retain exactly one semantic fact".into(),
                    ));
                }
                let key = &path.source_provider_facts()[0];
                let fact = semantic_facts.get(key).ok_or_else(|| {
                    ResolutionProjectionError::Invalid(
                        "provider path references an absent semantic fact".into(),
                    )
                })?;
                let provider = semantic_providers.get(fact.provider()).ok_or_else(|| {
                    ResolutionProjectionError::Invalid(
                        "provider path references an absent semantic provider".into(),
                    )
                })?;
                let expected_coverage = semantic_conclusion_coverage(provider, fact)?;
                if path.authorities() != [provider.authority()]
                    || path.coverage() != &expected_coverage
                    || !path
                        .edges()
                        .iter()
                        .any(|edge| edge.kind() == ResolutionPathEdgeKind::ExternalProvider)
                {
                    return Err(ResolutionProjectionError::Invalid(
                        "provider path contradicts its pinned semantic fact".into(),
                    ));
                }
                if expected_coverage.status == FactCoverage::Complete {
                    if path.viability() == ResolutionPathViability::Unknown {
                        return Err(ResolutionProjectionError::Invalid(
                            "complete provider fact produced an unknown path".into(),
                        ));
                    }
                } else if path.viability() != ResolutionPathViability::Unknown {
                    return Err(ResolutionProjectionError::Invalid(
                        "incomplete provider fact produced a terminal path".into(),
                    ));
                }
                provider_paths.entry(key).or_default().push(path);
            }
            for key in result.source_provider_facts() {
                let fact = semantic_facts[key];
                let paths = provider_paths.get(key).ok_or_else(|| {
                    ResolutionProjectionError::Invalid(
                        "semantic fact has no retained provider path".into(),
                    )
                })?;
                let actual = paths
                    .iter()
                    .map(|path| path.endpoint().cloned())
                    .collect::<BTreeSet<_>>();
                let expected = if fact.endpoints().is_empty() {
                    BTreeSet::from([None])
                } else {
                    fact.endpoints().iter().cloned().map(Some).collect()
                };
                if actual != expected || paths.len() != expected.len() {
                    return Err(ResolutionProjectionError::Invalid(
                        "provider paths omit or invent semantic endpoints".into(),
                    ));
                }
            }
        }
        if used_semantic_facts.len() != semantic_facts.len() {
            return Err(ResolutionProjectionError::Invalid(
                "resolution document omits one or more semantic facts".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolutionDocumentWire {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    scope_graph_id: ProjectionId,
    build_context: crate::BuildContextId,
    fact_policy: crate::ScopeFactPolicyId,
    resolution_policy: ResolutionPolicyId,
    semantic_facts: SemanticResolutionFactDocument,
    results: Vec<ResolutionResult>,
}

impl<'de> Deserialize<'de> for ResolutionDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ResolutionDocumentWire::deserialize(deserializer)?;
        let document = Self {
            schema: wire.schema,
            projection_id: wire.projection_id,
            analysis_id: wire.analysis_id,
            scope_graph_id: wire.scope_graph_id,
            build_context: wire.build_context,
            fact_policy: wire.fact_policy,
            resolution_policy: wire.resolution_policy,
            semantic_facts: wire.semantic_facts,
            results: wire.results,
        };
        document.validate().map_err(D::Error::custom)?;
        Ok(document)
    }
}

#[derive(Debug, Clone)]
pub struct ResolutionProjection {
    id: ProjectionId,
    scope_graph: Arc<ScopeGraphProjection>,
    semantic_facts: Arc<SemanticResolutionFacts>,
    resolution_policy: ResolutionPolicyId,
    results: Box<[ResolutionResultRecord]>,
    document: ResolutionDocument,
    owner: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolutionInvalidationReason {
    PolicyOrBuildContextChanged,
    SourceFactChanged,
    ReachableScopeChanged,
    MatchingModuleAdded,
    SemanticFactChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionInvalidation {
    reference: ScopeFactKey,
    reasons: Vec<ResolutionInvalidationReason>,
}

impl ResolutionInvalidation {
    pub fn reference(&self) -> &ScopeFactKey {
        &self.reference
    }

    pub fn reasons(&self) -> &[ResolutionInvalidationReason] {
        &self.reasons
    }
}

#[derive(Debug, Clone)]
pub struct ResolutionProjectionUpdate {
    previous: Arc<ResolutionProjection>,
    current: Arc<ResolutionProjection>,
    reused: Vec<ResolutionResultKey>,
    rebuilt: Vec<ResolutionInvalidation>,
    added: Vec<ScopeFactKey>,
    removed: Vec<ScopeFactKey>,
}

impl ResolutionProjectionUpdate {
    pub fn previous(&self) -> &Arc<ResolutionProjection> {
        &self.previous
    }

    pub fn current(&self) -> &Arc<ResolutionProjection> {
        &self.current
    }

    pub fn into_current(self) -> Arc<ResolutionProjection> {
        self.current
    }

    pub fn reused_result_keys(&self) -> &[ResolutionResultKey] {
        &self.reused
    }

    pub fn rebuilt_results(&self) -> &[ResolutionInvalidation] {
        &self.rebuilt
    }

    pub fn added_references(&self) -> &[ScopeFactKey] {
        &self.added
    }

    pub fn removed_references(&self) -> &[ScopeFactKey] {
        &self.removed
    }
}

impl ResolutionProjection {
    pub fn build(
        scope_graph: Arc<ScopeGraphProjection>,
        resolution_policy: ResolutionPolicyId,
    ) -> Result<Self, ResolutionProjectionError> {
        let semantic_facts = Arc::new(SemanticResolutionFacts::empty(Arc::clone(&scope_graph))?);
        Self::build_with_semantic_facts(scope_graph, resolution_policy, semantic_facts)
    }

    pub fn build_with_semantic_facts(
        scope_graph: Arc<ScopeGraphProjection>,
        resolution_policy: ResolutionPolicyId,
        semantic_facts: Arc<SemanticResolutionFacts>,
    ) -> Result<Self, ResolutionProjectionError> {
        semantic_facts.validate_against(&scope_graph)?;
        let engine = ResolutionTraversalEngine::new(&scope_graph)?;
        let modules = ModuleStitchIndex::new(&scope_graph)?;
        let mut wires = Vec::new();
        for fact in scope_graph.facts() {
            if fact.data().kind() != ScopeFactKind::Reference {
                continue;
            }
            let traversal = engine.traverse_reference(fact.id())?;
            wires.push(build_result(
                &scope_graph,
                &modules,
                &semantic_facts,
                &traversal,
            )?);
        }
        Self::from_wires(scope_graph, resolution_policy, semantic_facts, wires)
    }

    fn from_wires(
        scope_graph: Arc<ScopeGraphProjection>,
        resolution_policy: ResolutionPolicyId,
        semantic_facts: Arc<SemanticResolutionFacts>,
        wires: Vec<ResolutionResult>,
    ) -> Result<Self, ResolutionProjectionError> {
        let owner = NEXT_RESOLUTION_OWNER
            .fetch_update(AtomicOrdering::Relaxed, AtomicOrdering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| {
                ResolutionProjectionError::Invalid("resolution owner space exhausted".into())
            })?;
        let mut result_identity = Vec::new();
        result_identity.extend_from_slice(scope_graph.id().as_str().as_bytes());
        result_identity.extend_from_slice(semantic_facts.id().as_str().as_bytes());
        for wire in &wires {
            result_identity.extend_from_slice(&(wire.key().as_str().len() as u64).to_le_bytes());
            result_identity.extend_from_slice(wire.key().as_str().as_bytes());
        }
        let id = scope_graph
            .analysis()
            .derive_projection_id(
                RESOLUTION_SCHEMA,
                resolution_policy.as_str().as_bytes(),
                &result_identity,
            )
            .map_err(|error| ResolutionProjectionError::Invalid(error.to_string()))?;
        let document = ResolutionDocument {
            schema: RESOLUTION_SCHEMA.into(),
            projection_id: id.clone(),
            analysis_id: scope_graph.analysis().id().as_str().into(),
            scope_graph_id: scope_graph.id().clone(),
            build_context: scope_graph.build_context().clone(),
            fact_policy: scope_graph.fact_policy().clone(),
            resolution_policy: resolution_policy.clone(),
            semantic_facts: semantic_facts.document().clone(),
            results: wires.clone(),
        };
        document.validate()?;
        let results = wires
            .into_iter()
            .enumerate()
            .map(|(index, wire)| {
                let index = u32::try_from(index).map_err(|_| {
                    ResolutionProjectionError::Invalid(
                        "resolution result count exceeds dense identity space".into(),
                    )
                })?;
                Ok::<_, ResolutionProjectionError>(ResolutionResultRecord {
                    id: ResolutionResultId { owner, index },
                    wire,
                })
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_boxed_slice();
        Ok(Self {
            id,
            scope_graph,
            semantic_facts,
            resolution_policy,
            results,
            document,
            owner,
        })
    }

    pub fn successor(
        self: &Arc<Self>,
        scope_graph: Arc<ScopeGraphProjection>,
        resolution_policy: ResolutionPolicyId,
    ) -> Result<ResolutionProjectionUpdate, ResolutionProjectionError> {
        if !self.semantic_facts.facts().is_empty() {
            return Err(ResolutionProjectionError::Invalid(
                "a resolution projection with semantic facts requires successor_with_semantic_facts"
                    .into(),
            ));
        }
        let semantic_facts = Arc::new(SemanticResolutionFacts::empty(Arc::clone(&scope_graph))?);
        build_resolution_successor(
            Arc::clone(self),
            scope_graph,
            resolution_policy,
            semantic_facts,
        )
    }

    pub fn successor_with_semantic_facts(
        self: &Arc<Self>,
        scope_graph: Arc<ScopeGraphProjection>,
        resolution_policy: ResolutionPolicyId,
        semantic_facts: Arc<SemanticResolutionFacts>,
    ) -> Result<ResolutionProjectionUpdate, ResolutionProjectionError> {
        semantic_facts.validate_against(&scope_graph)?;
        build_resolution_successor(
            Arc::clone(self),
            scope_graph,
            resolution_policy,
            semantic_facts,
        )
    }

    pub fn schema(&self) -> &'static str {
        RESOLUTION_SCHEMA
    }

    pub fn id(&self) -> &ProjectionId {
        &self.id
    }

    pub fn scope_graph(&self) -> &Arc<ScopeGraphProjection> {
        &self.scope_graph
    }

    pub fn resolution_policy(&self) -> &ResolutionPolicyId {
        &self.resolution_policy
    }

    pub fn semantic_facts(&self) -> &Arc<SemanticResolutionFacts> {
        &self.semantic_facts
    }

    pub fn results(&self) -> &[ResolutionResultRecord] {
        &self.results
    }

    pub fn document(&self) -> &ResolutionDocument {
        &self.document
    }

    pub fn result(
        &self,
        id: ResolutionResultId,
    ) -> Result<&ResolutionResultRecord, ResolutionProjectionError> {
        if id.owner != self.owner {
            return Err(ResolutionProjectionError::ForeignResult);
        }
        self.results.get(id.index as usize).ok_or_else(|| {
            ResolutionProjectionError::Invalid("resolution result index is out of range".into())
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionProjectionError {
    ForeignResult,
    MissingFact(String),
    Invalid(String),
    Traversal(String),
}

impl fmt::Display for ResolutionProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignResult => {
                write!(formatter, "resolution result belongs to another projection")
            }
            Self::MissingFact(key) => write!(formatter, "resolution source fact {key} is missing"),
            Self::Invalid(message) => write!(formatter, "invalid resolution projection: {message}"),
            Self::Traversal(message) => write!(formatter, "resolution traversal failed: {message}"),
        }
    }
}

impl Error for ResolutionProjectionError {}

impl From<ResolutionTraversalError> for ResolutionProjectionError {
    fn from(error: ResolutionTraversalError) -> Self {
        Self::Traversal(error.to_string())
    }
}

impl From<SemanticResolutionFactError> for ResolutionProjectionError {
    fn from(error: SemanticResolutionFactError) -> Self {
        Self::Invalid(error.to_string())
    }
}

#[derive(Debug, Clone)]
struct ModuleRecord {
    key: ScopeFactKey,
    package_id: String,
    target_id: String,
    module_path: Vec<String>,
    file_scopes: Vec<ScopeFactKey>,
    export_coverage: crate::FactCoverageEvidence,
}

#[derive(Debug, Clone)]
struct ExportRecord {
    key: ScopeFactKey,
    scope: ScopeFactKey,
    local_target: Option<ScopeFactKey>,
    local_name: Option<String>,
    exported_name: String,
    reexport_segments: Vec<String>,
    visibility: crate::Visibility,
    conditions: Vec<String>,
}

#[derive(Debug)]
struct ModuleStitchIndex {
    modules: Vec<ModuleRecord>,
    exports: Vec<ExportRecord>,
    scope_parents: BTreeMap<ScopeFactKey, Option<ScopeFactKey>>,
    scope_kinds: BTreeMap<ScopeFactKey, crate::ScopeKind>,
}

#[derive(Debug, Clone)]
struct ModuleCandidate {
    module: ModuleRecord,
    importer: ModuleRecord,
    target_matches: bool,
}

#[derive(Debug, Clone)]
struct ExportHop {
    candidate: ModuleCandidate,
    export: ExportRecord,
}

#[derive(Debug, Clone)]
struct ExportChain {
    hops: Vec<ExportHop>,
    endpoint: ResolutionEndpoint,
}

#[derive(Debug, Default)]
struct ExportResolution {
    chains: Vec<ExportChain>,
    rejected_reexports: Vec<RejectedReexport>,
    incomplete: bool,
    observed_facts: Vec<ScopeFactKey>,
}

#[derive(Debug, Clone)]
struct RejectedReexport {
    hops: Vec<ExportHop>,
    candidate: ModuleCandidate,
}

impl ModuleStitchIndex {
    fn new(graph: &ScopeGraphProjection) -> Result<Self, ResolutionProjectionError> {
        let mut modules = Vec::new();
        let mut exports = Vec::new();
        let mut scope_parents = BTreeMap::new();
        let mut scope_kinds = BTreeMap::new();
        for fact in graph.facts() {
            match fact.data() {
                ScopeFactData::Scope {
                    scope_kind, parent, ..
                } => {
                    scope_parents.insert(fact.key().clone(), parent.clone());
                    scope_kinds.insert(fact.key().clone(), scope_kind.clone());
                }
                ScopeFactData::BuildModule {
                    package_id,
                    target_id,
                    module_path,
                    file_scopes,
                    export_coverage,
                    ..
                } => modules.push(ModuleRecord {
                    key: fact.key().clone(),
                    package_id: package_id.clone(),
                    target_id: target_id.clone(),
                    module_path: module_path.clone(),
                    file_scopes: file_scopes.clone(),
                    export_coverage: export_coverage.clone(),
                }),
                ScopeFactData::Export {
                    scope,
                    local_target,
                    local_name,
                    exported_name,
                    reexport_segments,
                    visibility,
                    conditions,
                } => exports.push(ExportRecord {
                    key: fact.key().clone(),
                    scope: scope.clone(),
                    local_target: local_target.clone(),
                    local_name: local_name.clone(),
                    exported_name: exported_name.clone(),
                    reexport_segments: reexport_segments.clone(),
                    visibility: visibility.clone(),
                    conditions: conditions.clone(),
                }),
                _ => {}
            }
        }
        let index = Self {
            modules,
            exports,
            scope_parents,
            scope_kinds,
        };
        for module in &index.modules {
            for scope in &module.file_scopes {
                if index.scope_kinds.get(scope) != Some(&crate::ScopeKind::File) {
                    return Err(ResolutionProjectionError::Invalid(
                        "module index contains a non-file constituent scope".into(),
                    ));
                }
            }
        }
        Ok(index)
    }

    fn file_scope(&self, scope: &ScopeFactKey) -> Option<ScopeFactKey> {
        let mut current = Some(scope);
        let mut seen = BTreeSet::new();
        while let Some(key) = current {
            if !seen.insert(key) {
                return None;
            }
            if self.scope_kinds.get(key) == Some(&crate::ScopeKind::File) {
                return Some(key.clone());
            }
            current = self.scope_parents.get(key).and_then(Option::as_ref);
        }
        None
    }

    fn modules_for_scope(&self, scope: &ScopeFactKey) -> Vec<&ModuleRecord> {
        let Some(file_scope) = self.file_scope(scope) else {
            return Vec::new();
        };
        self.modules
            .iter()
            .filter(|module| module.file_scopes.contains(&file_scope))
            .collect()
    }

    fn candidates(&self, scope: &ScopeFactKey, segments: &[String]) -> Vec<ModuleCandidate> {
        let importers = self.modules_for_scope(scope);
        importers
            .into_iter()
            .flat_map(|importer| self.candidates_from(importer, segments))
            .collect()
    }

    fn candidates_from(
        &self,
        importer: &ModuleRecord,
        segments: &[String],
    ) -> Vec<ModuleCandidate> {
        let explicit_package = segments.first().filter(|package| {
            self.modules.iter().any(|module| {
                &module.package_id == *package
                    && module.module_path.as_slice() == segments.get(1..).unwrap_or_default()
            })
        });
        let mut candidates = Vec::new();
        for module in &self.modules {
            let path_matches = if let Some(package) = explicit_package {
                &module.package_id == package
                    && module.module_path.as_slice() == segments.get(1..).unwrap_or_default()
            } else {
                module.package_id == importer.package_id && module.module_path == segments
            };
            if !path_matches {
                continue;
            }
            let target_matches = explicit_package.is_some()
                && module.package_id != importer.package_id
                || module.target_id == importer.target_id;
            candidates.push(ModuleCandidate {
                module: module.clone(),
                importer: importer.clone(),
                target_matches,
            });
        }
        candidates
    }

    fn exports_for_module<'a>(
        &'a self,
        module: &ModuleRecord,
        exported_name: &str,
    ) -> Vec<&'a ExportRecord> {
        self.exports
            .iter()
            .filter(|export| export.exported_name == exported_name)
            .filter(|export| {
                self.file_scope(&export.scope)
                    .is_some_and(|scope| module.file_scopes.contains(&scope))
            })
            .collect()
    }

    fn all_exports_for_module<'a>(&'a self, module: &ModuleRecord) -> Vec<&'a ExportRecord> {
        self.exports
            .iter()
            .filter(|export| {
                self.file_scope(&export.scope)
                    .is_some_and(|scope| module.file_scopes.contains(&scope))
            })
            .collect()
    }
}

fn build_resolution_successor(
    previous: Arc<ResolutionProjection>,
    scope_graph: Arc<ScopeGraphProjection>,
    resolution_policy: ResolutionPolicyId,
    semantic_facts: Arc<SemanticResolutionFacts>,
) -> Result<ResolutionProjectionUpdate, ResolutionProjectionError> {
    if previous.scope_graph.analysis().snapshot().repository()
        != scope_graph.analysis().snapshot().repository()
    {
        return Err(ResolutionProjectionError::Invalid(
            "resolution successor belongs to a different repository".into(),
        ));
    }
    let previous_facts = previous
        .scope_graph
        .facts()
        .iter()
        .map(|fact| (fact.key().clone(), fact))
        .collect::<BTreeMap<_, _>>();
    let current_facts = scope_graph
        .facts()
        .iter()
        .map(|fact| (fact.key().clone(), fact))
        .collect::<BTreeMap<_, _>>();
    let added_facts = current_facts
        .keys()
        .filter(|key| !previous_facts.contains_key(*key))
        .filter_map(|key| current_facts.get(key).copied())
        .collect::<Vec<_>>();
    let removed_fact_keys = previous_facts
        .keys()
        .filter(|key| !current_facts.contains_key(*key))
        .cloned()
        .collect::<BTreeSet<_>>();
    let modules = ModuleStitchIndex::new(&scope_graph)?;
    let mut impact_scopes = BTreeSet::new();
    for fact in &added_facts {
        for scope in fact_impact_scopes(&scope_graph, &modules, fact)? {
            impact_scopes.insert(scope);
        }
    }
    let previous_results = previous
        .results()
        .iter()
        .map(|result| (result.wire().reference().clone(), result.wire()))
        .collect::<BTreeMap<_, _>>();
    let current_references = scope_graph
        .facts()
        .iter()
        .filter(|fact| fact.data().kind() == ScopeFactKind::Reference)
        .collect::<Vec<_>>();
    let current_reference_keys = current_references
        .iter()
        .map(|fact| fact.key().clone())
        .collect::<BTreeSet<_>>();
    let removed = previous_results
        .keys()
        .filter(|key| !current_reference_keys.contains(*key))
        .cloned()
        .collect::<Vec<_>>();
    let global_change = previous.resolution_policy != resolution_policy
        || previous.scope_graph.build_context() != scope_graph.build_context()
        || previous.scope_graph.fact_policy() != scope_graph.fact_policy();
    let engine = ResolutionTraversalEngine::new(&scope_graph)?;
    let mut wires = Vec::with_capacity(current_references.len());
    let mut reused = Vec::new();
    let mut rebuilt = Vec::new();
    let mut added = Vec::new();
    for reference in current_references {
        let Some(old) = previous_results.get(reference.key()).copied() else {
            let traversal = engine.traverse_reference(reference.id())?;
            wires.push(build_result(
                &scope_graph,
                &modules,
                &semantic_facts,
                &traversal,
            )?);
            added.push(reference.key().clone());
            continue;
        };
        let mut reasons = BTreeSet::new();
        if global_change {
            reasons.insert(ResolutionInvalidationReason::PolicyOrBuildContextChanged);
        }
        if old
            .source_facts()
            .iter()
            .any(|key| removed_fact_keys.contains(key) || !current_facts.contains_key(key))
        {
            reasons.insert(ResolutionInvalidationReason::SourceFactChanged);
        }
        let result_scopes = old
            .source_facts()
            .iter()
            .filter(|key| {
                current_facts
                    .get(*key)
                    .is_some_and(|fact| fact.data().kind() == ScopeFactKind::Scope)
            })
            .collect::<BTreeSet<_>>();
        if impact_scopes
            .iter()
            .any(|scope| result_scopes.contains(scope))
        {
            reasons.insert(ResolutionInvalidationReason::ReachableScopeChanged);
        }
        if added_facts.iter().any(|fact| {
            matches!(fact.data(), ScopeFactData::BuildModule { .. })
                && result_has_matching_import(old, &current_facts, fact)
        }) {
            reasons.insert(ResolutionInvalidationReason::MatchingModuleAdded);
        }
        let current_provider_facts = semantic_facts
            .facts_for_reference(reference.key())
            .map(|fact| fact.key().clone())
            .collect::<BTreeSet<_>>();
        let previous_provider_facts = old
            .source_provider_facts()
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if current_provider_facts != previous_provider_facts {
            reasons.insert(ResolutionInvalidationReason::SemanticFactChanged);
        }
        if reasons.is_empty() {
            wires.push(old.clone());
            reused.push(old.key().clone());
        } else {
            let traversal = engine.traverse_reference(reference.id())?;
            wires.push(build_result(
                &scope_graph,
                &modules,
                &semantic_facts,
                &traversal,
            )?);
            rebuilt.push(ResolutionInvalidation {
                reference: reference.key().clone(),
                reasons: reasons.into_iter().collect(),
            });
        }
    }
    let current = Arc::new(ResolutionProjection::from_wires(
        scope_graph,
        resolution_policy,
        semantic_facts,
        wires,
    )?);
    Ok(ResolutionProjectionUpdate {
        previous,
        current,
        reused,
        rebuilt,
        added,
        removed,
    })
}

fn fact_impact_scopes(
    graph: &ScopeGraphProjection,
    modules: &ModuleStitchIndex,
    fact: &crate::ScopeFactRecord,
) -> Result<Vec<ScopeFactKey>, ResolutionProjectionError> {
    let direct = match fact.data() {
        ScopeFactData::Scope { .. } => vec![fact.key().clone()],
        ScopeFactData::Declaration { scope, .. }
        | ScopeFactData::Import { scope, .. }
        | ScopeFactData::Export { scope, .. } => vec![scope.clone()],
        ScopeFactData::Definition { declaration, .. } => declaration_scopes(graph, declaration)?,
        ScopeFactData::Binding { target, .. } => match target {
            crate::BindingTarget::Declaration(declaration) => {
                declaration_scopes(graph, declaration)?
            }
            crate::BindingTarget::Definition(definition) => {
                let ScopeFactData::Definition { declaration, .. } =
                    fact_by_key(graph, definition)?.data()
                else {
                    return Err(ResolutionProjectionError::Invalid(
                        "binding target is not a definition fact".into(),
                    ));
                };
                declaration_scopes(graph, declaration)?
            }
        },
        ScopeFactData::BuildModule { file_scopes, .. }
        | ScopeFactData::DynamicBoundary {
            scopes: file_scopes,
            ..
        } => file_scopes.clone(),
        ScopeFactData::Shadowing {
            shadowing_declaration,
            shadowed_declaration,
            ..
        } => {
            let mut scopes = declaration_scopes(graph, shadowing_declaration)?;
            for scope in declaration_scopes(graph, shadowed_declaration)? {
                push_unique(&mut scopes, scope);
            }
            scopes
        }
        ScopeFactData::Reference { .. } => Vec::new(),
    };
    let mut expanded = Vec::new();
    for scope in direct {
        let mut current = Some(scope);
        let mut seen = BTreeSet::new();
        while let Some(key) = current {
            if !seen.insert(key.clone()) {
                return Err(ResolutionProjectionError::Invalid(
                    "scope impact relation contains a cycle".into(),
                ));
            }
            push_unique(&mut expanded, key.clone());
            current = modules.scope_parents.get(&key).cloned().flatten();
        }
    }
    Ok(expanded)
}

fn declaration_scopes(
    graph: &ScopeGraphProjection,
    declaration: &ScopeFactKey,
) -> Result<Vec<ScopeFactKey>, ResolutionProjectionError> {
    let ScopeFactData::Declaration { scope, .. } = fact_by_key(graph, declaration)?.data() else {
        return Err(ResolutionProjectionError::Invalid(
            "declaration dependency is not a declaration fact".into(),
        ));
    };
    Ok(vec![scope.clone()])
}

fn result_has_matching_import(
    result: &ResolutionResult,
    current_facts: &BTreeMap<ScopeFactKey, &crate::ScopeFactRecord>,
    module: &crate::ScopeFactRecord,
) -> bool {
    let ScopeFactData::BuildModule {
        package_id,
        module_path,
        ..
    } = module.data()
    else {
        return false;
    };
    result.source_facts().iter().any(|key| {
        matches!(
            current_facts.get(key).map(|fact| fact.data()),
            Some(ScopeFactData::Import { module_segments, .. })
                if module_segments == module_path
                    || module_segments.first() == Some(package_id)
                        && module_segments.get(1..).unwrap_or_default() == module_path
        )
    })
}

fn build_result(
    graph: &ScopeGraphProjection,
    modules: &ModuleStitchIndex,
    semantic_facts: &SemanticResolutionFacts,
    traversal: &ResolutionTraversal,
) -> Result<ResolutionResult, ResolutionProjectionError> {
    let reference = graph
        .fact(traversal.reference())
        .map_err(|error| ResolutionProjectionError::Invalid(error.to_string()))?;
    let start_scope = graph
        .fact(traversal.start_scope())
        .map_err(|error| ResolutionProjectionError::Invalid(error.to_string()))?;
    let dynamic_keys = traversal
        .dynamic_boundaries()
        .iter()
        .map(|boundary| fact_key(graph, boundary.fact()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut paths = Vec::new();
    for candidate in traversal.candidates() {
        let endpoints = if candidate.definitions().is_empty() {
            vec![ResolutionEndpoint::Declaration(fact_key(
                graph,
                candidate.declaration(),
            )?)]
        } else {
            candidate
                .definitions()
                .iter()
                .map(|definition| fact_key(graph, *definition).map(ResolutionEndpoint::Definition))
                .collect::<Result<Vec<_>, _>>()?
        };
        for endpoint in endpoints {
            paths.push(candidate_path(
                graph,
                traversal,
                candidate,
                endpoint,
                &dynamic_keys,
            )?);
        }
    }
    for deferred in traversal.deferred_imports() {
        paths.extend(stitch_import_paths(
            graph,
            modules,
            traversal,
            deferred.import(),
            deferred.lexical_distance(),
            deferred.rule(),
            deferred.rule_declared(),
            deferred.conditions(),
            &dynamic_keys,
        )?);
    }
    apply_duplicate_rules(
        graph,
        reference.evidence().adapter.resolution_rules(),
        &mut paths,
    )?;
    apply_explicit_shadowing(&mut paths)?;
    apply_precedence(&mut paths)?;
    for path in &mut paths {
        path.authorities = path_authorities(graph, &path.source_facts)?;
        path.coverage = derive_path_coverage(graph, path, traversal.rule_gaps())?;
        path.key = ResolutionPathKey(String::new());
        *path = path.clone().finish()?;
    }

    let mut source_facts = vec![reference.key().clone()];
    for step in traversal.scopes() {
        push_unique(&mut source_facts, fact_key(graph, step.scope())?);
    }
    for path in &paths {
        for key in &path.source_facts {
            push_unique(&mut source_facts, key.clone());
        }
    }
    for key in &dynamic_keys {
        push_unique(&mut source_facts, key.clone());
    }
    let coverage = derive_coverage(
        graph,
        &source_facts,
        traversal.rule_gaps(),
        traversal.dynamic_boundaries(),
        paths.iter().any(|path| {
            path.viability == ResolutionPathViability::Unknown
                && path.rejection_reasons.iter().any(|reason| {
                    matches!(
                        reason,
                        ResolutionRejectionReason::ImportUnresolved
                            | ResolutionRejectionReason::ExportIncomplete
                    )
                })
        }),
        &paths,
    )?;
    let status = derive_status(coverage.status, &paths);
    let diagnostics = coverage.reasons.clone();
    let endpoints = paths
        .iter()
        .filter(|path| path.viability == ResolutionPathViability::Viable)
        .filter_map(|path| path.endpoint.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let adapter_conclusion = ResolutionConclusion {
        source: ResolutionConclusionSource::Adapter,
        authority: reference.evidence().authority,
        status,
        endpoints: endpoints.clone(),
        coverage: coverage.clone(),
    };
    let preferred = (coverage.status == FactCoverage::Complete
        && !matches!(
            status,
            ResolutionStatus::Unknown | ResolutionStatus::Conflict
        ))
    .then(|| PreferredResolutionConclusion {
        authority: reference
            .evidence()
            .authority
            .expect("complete adapter resolution evidence has authority"),
        status,
        endpoints,
        sources: vec![ResolutionConclusionSource::Adapter],
    });
    join_semantic_facts(
        semantic_facts,
        ResolutionResult {
            key: ResolutionResultKey(String::new()),
            reference: reference.key().clone(),
            start_scope: start_scope.key().clone(),
            reference_evidence: reference.evidence().clone(),
            coverage,
            status,
            authority: preferred
                .as_ref()
                .map(PreferredResolutionConclusion::authority),
            conclusions: vec![adapter_conclusion],
            preferred,
            paths,
            source_facts,
            source_provider_facts: Vec::new(),
            dynamic_boundaries: dynamic_keys,
            diagnostics,
        },
    )
}

fn join_semantic_facts(
    semantic_facts: &SemanticResolutionFacts,
    mut result: ResolutionResult,
) -> Result<ResolutionResult, ResolutionProjectionError> {
    for fact in semantic_facts.facts_for_reference(&result.reference) {
        let provider = semantic_facts.provider(fact.provider()).ok_or_else(|| {
            ResolutionProjectionError::Invalid(
                "semantic resolution fact references an absent provider".into(),
            )
        })?;
        let coverage = semantic_conclusion_coverage(provider, fact)?;
        let status = if coverage.status == FactCoverage::Complete {
            fact.status()
        } else {
            ResolutionStatus::Unknown
        };
        let conclusion = ResolutionConclusion {
            source: ResolutionConclusionSource::Semantic(fact.key().clone()),
            authority: Some(provider.authority()),
            status,
            endpoints: fact.endpoints().to_vec(),
            coverage: coverage.clone(),
        };
        conclusion.validate()?;
        let endpoints = if fact.endpoints().is_empty() {
            vec![None]
        } else {
            fact.endpoints().iter().cloned().map(Some).collect()
        };
        for endpoint in endpoints {
            let path = semantic_provider_path(&result, provider, fact, endpoint, coverage.clone())?;
            for key in path.source_facts() {
                push_unique(&mut result.source_facts, key.clone());
            }
            result.paths.push(path);
        }
        push_unique(&mut result.source_provider_facts, fact.key().clone());
        result.conclusions.push(conclusion);
        for diagnostic in fact.diagnostics() {
            push_unique(&mut result.diagnostics, diagnostic.clone());
        }
        for reason in coverage.reasons() {
            push_unique(&mut result.diagnostics, reason.clone());
        }
    }

    let (status, preferred, coverage) = derive_conclusion_join(&result.conclusions)?;
    let conflict_sources = conflicting_conclusion_sources(&result.conclusions, preferred.as_ref());
    if !conflict_sources.is_empty() {
        push_unique(
            &mut result.diagnostics,
            "static resolution providers disagree; all conclusions are retained".into(),
        );
        for path in &mut result.paths {
            let belongs = if path.source_provider_facts().is_empty() {
                conflict_sources.contains(&ResolutionConclusionSource::Adapter)
            } else {
                path.source_provider_facts().iter().any(|key| {
                    conflict_sources.contains(&ResolutionConclusionSource::Semantic(key.clone()))
                })
            };
            if !belongs {
                continue;
            }
            push_unique(
                &mut path.rejection_reasons,
                ResolutionRejectionReason::ProviderConflict,
            );
            if !path.checks.iter().any(|check| {
                check.kind == ResolutionCheckKind::EvidenceAuthority
                    && check.state == ResolutionCheckState::Rejected
            }) {
                path.checks.push(ResolutionCheck {
                    kind: ResolutionCheckKind::EvidenceAuthority,
                    state: ResolutionCheckState::Rejected,
                    detail: "static provider conclusion disagrees with retained evidence".into(),
                    source_facts: vec![result.reference.clone()],
                });
            }
            if path.viability == ResolutionPathViability::Viable {
                path.viability = ResolutionPathViability::Rejected;
            }
            path.key = ResolutionPathKey(String::new());
            *path = path.clone().finish()?;
        }
    }
    result.status = status;
    result.authority = preferred
        .as_ref()
        .map(PreferredResolutionConclusion::authority);
    result.preferred = preferred;
    result.coverage = coverage;
    result.key = ResolutionResultKey(String::new());
    result.finish()
}

fn semantic_conclusion_coverage(
    provider: &SemanticProvider,
    fact: &SemanticResolutionFact,
) -> Result<ResolutionCoverageEvidence, ResolutionProjectionError> {
    let mut status = FactCoverage::Complete;
    let mut reasons = Vec::new();
    for (label, evidence) in [
        ("provider project model", provider.project_model_coverage()),
        ("provider result", fact.coverage()),
    ] {
        if evidence.status == FactCoverage::Complete {
            continue;
        }
        status = combine_coverage(status, evidence.status);
        push_unique(
            &mut reasons,
            format!(
                "{label} is {:?}: {}",
                evidence.status,
                evidence.reason.as_deref().unwrap_or("no reason retained")
            ),
        );
    }
    let coverage = ResolutionCoverageEvidence { status, reasons };
    coverage.validate()?;
    Ok(coverage)
}

fn semantic_provider_path(
    result: &ResolutionResult,
    provider: &SemanticProvider,
    fact: &SemanticResolutionFact,
    endpoint: Option<ResolutionEndpoint>,
    coverage: ResolutionCoverageEvidence,
) -> Result<ResolutionPath, ResolutionProjectionError> {
    let mut source_facts = vec![result.reference.clone(), result.start_scope.clone()];
    if let Some(endpoint) = &endpoint {
        match endpoint {
            ResolutionEndpoint::Declaration(key)
            | ResolutionEndpoint::Definition(key)
            | ResolutionEndpoint::Module(key) => push_unique(&mut source_facts, key.clone()),
            ResolutionEndpoint::MergedDeclarations(keys) => {
                for key in keys {
                    push_unique(&mut source_facts, key.clone());
                }
            }
            ResolutionEndpoint::External(_) => {}
        }
    }
    let complete = coverage.status == FactCoverage::Complete;
    let mut checks = vec![
        ResolutionCheck {
            kind: ResolutionCheckKind::ProviderIdentity,
            state: ResolutionCheckState::Passed,
            detail: format!(
                "pinned {:?} provider {} {}",
                provider.kind(),
                provider.name(),
                provider.version()
            ),
            source_facts: vec![result.reference.clone()],
        },
        ResolutionCheck {
            kind: ResolutionCheckKind::ProviderArtifact,
            state: if fact.coverage().status == FactCoverage::Complete {
                ResolutionCheckState::Passed
            } else {
                ResolutionCheckState::Unknown
            },
            detail: format!(
                "provider result artifact {} is retained with {:?} coverage",
                fact.result_artifact().as_str(),
                fact.coverage().status
            ),
            source_facts: vec![result.reference.clone()],
        },
        ResolutionCheck {
            kind: ResolutionCheckKind::ProviderProjectModel,
            state: if provider.project_model_coverage().status == FactCoverage::Complete {
                ResolutionCheckState::Passed
            } else {
                ResolutionCheckState::Unknown
            },
            detail: format!(
                "provider project model is {:?}",
                provider.project_model_coverage().status
            ),
            source_facts: vec![result.reference.clone()],
        },
        ResolutionCheck {
            kind: ResolutionCheckKind::EvidenceAuthority,
            state: ResolutionCheckState::Passed,
            detail: format!(
                "provider contributes distinct {} static evidence",
                provider.authority().as_str()
            ),
            source_facts: vec![result.reference.clone()],
        },
        ResolutionCheck {
            kind: ResolutionCheckKind::BuildTarget,
            state: ResolutionCheckState::Passed,
            detail: "provider fact is bound to the exact resolution build context".into(),
            source_facts: vec![result.reference.clone()],
        },
    ];
    if !complete
        && checks
            .iter()
            .all(|check| check.state != ResolutionCheckState::Unknown)
    {
        checks.push(ResolutionCheck {
            kind: ResolutionCheckKind::ProviderArtifact,
            state: ResolutionCheckState::Unknown,
            detail: "provider evidence coverage is incomplete".into(),
            source_facts: vec![result.reference.clone()],
        });
    }
    ResolutionPath {
        key: ResolutionPathKey(String::new()),
        endpoint,
        edges: vec![ResolutionPathEdge {
            kind: ResolutionPathEdgeKind::ExternalProvider,
            from: result.start_scope.clone(),
            to: result.reference.clone(),
            source_fact: result.reference.clone(),
        }],
        precedence: Vec::new(),
        viability: if complete {
            ResolutionPathViability::Viable
        } else {
            ResolutionPathViability::Unknown
        },
        rejection_reasons: Vec::new(),
        checks,
        source_facts,
        source_provider_facts: vec![fact.key().clone()],
        dynamic_boundaries: Vec::new(),
        authorities: vec![provider.authority()],
        coverage,
    }
    .finish()
}

fn derive_conclusion_join(
    conclusions: &[ResolutionConclusion],
) -> Result<
    (
        ResolutionStatus,
        Option<PreferredResolutionConclusion>,
        ResolutionCoverageEvidence,
    ),
    ResolutionProjectionError,
> {
    for conclusion in conclusions {
        conclusion.validate()?;
    }
    let terminal = conclusions
        .iter()
        .filter(|conclusion| conclusion.coverage.status == FactCoverage::Complete)
        .collect::<Vec<_>>();
    if terminal.is_empty() {
        let mut status = FactCoverage::Complete;
        let mut reasons = Vec::new();
        for conclusion in conclusions {
            status = combine_coverage(status, conclusion.coverage.status);
            for reason in conclusion.coverage.reasons() {
                push_unique(&mut reasons, reason.clone());
            }
        }
        if status == FactCoverage::Complete {
            status = FactCoverage::Partial;
            reasons.push("no provider supplied a complete static conclusion".into());
        }
        let coverage = ResolutionCoverageEvidence { status, reasons };
        coverage.validate()?;
        return Ok((ResolutionStatus::Unknown, None, coverage));
    }

    let signatures = terminal
        .iter()
        .map(|conclusion| (conclusion.status, conclusion.endpoints.clone()))
        .collect::<BTreeSet<_>>();
    let conflict = signatures.len() > 1
        || terminal
            .iter()
            .any(|conclusion| conclusion.status == ResolutionStatus::Conflict);
    let max_authority = terminal
        .iter()
        .filter(|conclusion| conclusion.status != ResolutionStatus::Conflict)
        .filter_map(|conclusion| conclusion.authority)
        .max();
    let preferred = max_authority.and_then(|authority| {
        let highest = terminal
            .iter()
            .filter(|conclusion| {
                conclusion.authority == Some(authority)
                    && conclusion.status != ResolutionStatus::Conflict
            })
            .collect::<Vec<_>>();
        let highest_signatures = highest
            .iter()
            .map(|conclusion| (conclusion.status, conclusion.endpoints.clone()))
            .collect::<BTreeSet<_>>();
        if highest_signatures.len() != 1 {
            return None;
        }
        let (status, endpoints) = highest_signatures.into_iter().next().unwrap();
        let mut sources = highest
            .into_iter()
            .map(|conclusion| conclusion.source.clone())
            .collect::<Vec<_>>();
        sources.sort();
        Some(PreferredResolutionConclusion {
            authority,
            status,
            endpoints,
            sources,
        })
    });
    let status = if conflict {
        ResolutionStatus::Conflict
    } else {
        preferred.as_ref().map_or(
            ResolutionStatus::Unknown,
            PreferredResolutionConclusion::status,
        )
    };
    let coverage = ResolutionCoverageEvidence {
        status: FactCoverage::Complete,
        reasons: Vec::new(),
    };
    coverage.validate()?;
    Ok((status, preferred, coverage))
}

fn conflicting_conclusion_sources(
    conclusions: &[ResolutionConclusion],
    preferred: Option<&PreferredResolutionConclusion>,
) -> BTreeSet<ResolutionConclusionSource> {
    let terminal = conclusions
        .iter()
        .filter(|conclusion| conclusion.coverage.status == FactCoverage::Complete)
        .collect::<Vec<_>>();
    let signatures = terminal
        .iter()
        .map(|conclusion| (conclusion.status, conclusion.endpoints.clone()))
        .collect::<BTreeSet<_>>();
    let conflict = signatures.len() > 1
        || terminal
            .iter()
            .any(|conclusion| conclusion.status == ResolutionStatus::Conflict);
    if !conflict {
        return BTreeSet::new();
    }
    let preferred_signature =
        preferred.map(|preferred| (preferred.status, preferred.endpoints.clone()));
    terminal
        .into_iter()
        .filter(|conclusion| {
            preferred_signature.as_ref().is_none_or(|signature| {
                &(conclusion.status, conclusion.endpoints.clone()) != signature
            })
        })
        .map(|conclusion| conclusion.source.clone())
        .collect()
}

fn candidate_path(
    graph: &ScopeGraphProjection,
    traversal: &ResolutionTraversal,
    candidate: &TraversalCandidate,
    endpoint: ResolutionEndpoint,
    dynamic_boundaries: &[ScopeFactKey],
) -> Result<ResolutionPath, ResolutionProjectionError> {
    let reference = fact_key(graph, traversal.reference())?;
    let declaration = fact_key(graph, candidate.declaration())?;
    let mut edges = lexical_edges(graph, traversal, candidate.lexical_distance())?;
    let declaration_scope = fact_key(
        graph,
        traversal.scopes()[candidate.lexical_distance() as usize].scope(),
    )?;
    edges.push(ResolutionPathEdge {
        kind: ResolutionPathEdgeKind::Declares,
        from: declaration_scope,
        to: declaration.clone(),
        source_fact: declaration.clone(),
    });
    let mut source_facts = vec![reference];
    for step in traversal
        .scopes()
        .iter()
        .take(candidate.lexical_distance() as usize + 1)
    {
        push_unique(&mut source_facts, fact_key(graph, step.scope())?);
    }
    for edge in &edges {
        push_unique(&mut source_facts, edge.source_fact.clone());
    }
    for binding in candidate.bindings() {
        let binding = fact_key(graph, *binding)?;
        edges.push(ResolutionPathEdge {
            kind: ResolutionPathEdgeKind::Binds,
            from: binding.clone(),
            to: declaration.clone(),
            source_fact: binding.clone(),
        });
        push_unique(&mut source_facts, binding);
    }
    if let ResolutionEndpoint::Definition(definition) = &endpoint {
        edges.push(ResolutionPathEdge {
            kind: ResolutionPathEdgeKind::Defines,
            from: declaration.clone(),
            to: definition.clone(),
            source_fact: definition.clone(),
        });
        push_unique(&mut source_facts, definition.clone());
    }
    for shadowing in candidate.shadowed_by() {
        let fact = fact_key(graph, shadowing.fact())?;
        let shadowing_declaration = fact_key(graph, shadowing.declaration())?;
        edges.push(ResolutionPathEdge {
            kind: ResolutionPathEdgeKind::ExplicitShadowing,
            from: declaration.clone(),
            to: shadowing_declaration.clone(),
            source_fact: fact.clone(),
        });
        push_unique(&mut source_facts, fact);
        push_unique(&mut source_facts, shadowing_declaration);
    }
    for boundary in dynamic_boundaries {
        push_unique(&mut source_facts, boundary.clone());
    }

    let mut checks = Vec::new();
    let mut rejections = Vec::new();
    checks.push(namespace_check(
        candidate.namespace(),
        &declaration,
        &mut rejections,
    ));
    checks.push(visibility_check(
        candidate.visibility(),
        &declaration,
        &mut rejections,
    ));
    for timing in candidate.timing() {
        checks.push(timing_check(*timing, graph, &mut rejections)?);
    }
    if !traversal.remaining_segments().is_empty() {
        checks.push(ResolutionCheck {
            kind: ResolutionCheckKind::Qualification,
            state: ResolutionCheckState::Unknown,
            detail: format!(
                "qualified tail awaits member traversal: {}",
                traversal.remaining_segments().join(".")
            ),
            source_facts: vec![fact_key(graph, traversal.reference())?],
        });
    }
    checks.push(ResolutionCheck {
        kind: ResolutionCheckKind::AdapterIdentity,
        state: if candidate.adapter_schema_matches() {
            ResolutionCheckState::Passed
        } else {
            rejections.push(ResolutionRejectionReason::ProviderConflict);
            ResolutionCheckState::Rejected
        },
        detail: if candidate.adapter_schema_matches() {
            "candidate and reference use the same adapter schema".into()
        } else {
            "candidate and reference adapter schemas differ".into()
        },
        source_facts: vec![declaration.clone()],
    });
    let precedence = candidate
        .precedence()
        .iter()
        .map(|component| ResolutionPrecedenceComponent {
            dimension: component.dimension(),
            direction: component.direction(),
            value: component.value(),
        })
        .collect::<Vec<_>>();
    if precedence.is_empty() {
        checks.push(ResolutionCheck {
            kind: ResolutionCheckKind::LookupPrecedence,
            state: ResolutionCheckState::Unknown,
            detail: "adapter provides no lookup-precedence relation".into(),
            source_facts: vec![declaration.clone()],
        });
    }
    let viability = viability_from_checks(&checks);
    Ok(ResolutionPath {
        key: ResolutionPathKey(String::new()),
        endpoint: Some(endpoint),
        edges,
        precedence,
        viability,
        rejection_reasons: deduplicate(rejections),
        checks,
        source_facts,
        source_provider_facts: Vec::new(),
        dynamic_boundaries: dynamic_boundaries.to_vec(),
        authorities: Vec::new(),
        coverage: ResolutionCoverageEvidence {
            status: FactCoverage::Complete,
            reasons: Vec::new(),
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn import_path(
    graph: &ScopeGraphProjection,
    traversal: &ResolutionTraversal,
    import: ScopeFactId,
    lexical_distance: u32,
    rule: ImportTraversalRule,
    rule_declared: bool,
    conditions: &[String],
    dynamic_boundaries: &[ScopeFactKey],
) -> Result<ResolutionPath, ResolutionProjectionError> {
    let import_key = fact_key(graph, import)?;
    let import_fact = graph
        .fact(import)
        .map_err(|error| ResolutionProjectionError::Invalid(error.to_string()))?;
    let mut edges = lexical_edges(graph, traversal, lexical_distance)?;
    let scope = fact_key(graph, traversal.scopes()[lexical_distance as usize].scope())?;
    edges.push(ResolutionPathEdge {
        kind: match rule {
            ImportTraversalRule::Alias => ResolutionPathEdgeKind::AliasImport,
            ImportTraversalRule::Selective => ResolutionPathEdgeKind::SelectiveImport,
            ImportTraversalRule::Glob => ResolutionPathEdgeKind::GlobImport,
            _ => ResolutionPathEdgeKind::ExplicitImport,
        },
        from: scope,
        to: import_key.clone(),
        source_fact: import_key.clone(),
    });
    let mut source_facts = vec![fact_key(graph, traversal.reference())?];
    for step in traversal
        .scopes()
        .iter()
        .take(lexical_distance as usize + 1)
    {
        push_unique(&mut source_facts, fact_key(graph, step.scope())?);
    }
    for edge in &edges {
        push_unique(&mut source_facts, edge.source_fact.clone());
    }
    for boundary in dynamic_boundaries {
        push_unique(&mut source_facts, boundary.clone());
    }
    let mut checks = vec![ResolutionCheck {
        kind: ResolutionCheckKind::ImportTarget,
        state: ResolutionCheckState::Unknown,
        detail: if rule_declared {
            "declared import traversal awaits exact module/export resolution".into()
        } else {
            "import traversal rule is unavailable".into()
        },
        source_facts: vec![import_key.clone()],
    }];
    if !conditions.is_empty() {
        checks.push(ResolutionCheck {
            kind: ResolutionCheckKind::Condition,
            state: ResolutionCheckState::Unknown,
            detail: format!(
                "import conditions are unevaluated: {}",
                conditions.join(", ")
            ),
            source_facts: vec![import_key.clone()],
        });
    }
    let pack = import_fact.evidence().adapter.resolution_rules();
    let terms = pack
        .section(deslop_lang::ResolutionRuleSectionKind::Precedence)
        .instructions()
        .first();
    let precedence = match terms {
        Some(deslop_lang::ResolutionInstruction::Precedence { terms }) => terms
            .iter()
            .map(|term| ResolutionPrecedenceComponent {
                dimension: term.dimension(),
                direction: term.direction(),
                value: match term.dimension() {
                    PrecedenceDimension::RuleStep => 1,
                    PrecedenceDimension::LexicalDistance => u64::from(lexical_distance),
                    PrecedenceDimension::Namespace => 0,
                    PrecedenceDimension::ImportSpecificity => import_specificity(rule),
                    PrecedenceDimension::SourceOrder | PrecedenceDimension::AdapterOrder => {
                        import_fact.evidence().source_order
                    }
                },
            })
            .collect(),
        _ => Vec::new(),
    };
    if precedence.is_empty() {
        checks.push(ResolutionCheck {
            kind: ResolutionCheckKind::LookupPrecedence,
            state: ResolutionCheckState::Unknown,
            detail: "adapter provides no lookup-precedence relation".into(),
            source_facts: vec![import_key.clone()],
        });
    }
    Ok(ResolutionPath {
        key: ResolutionPathKey(String::new()),
        endpoint: None,
        edges,
        precedence,
        viability: ResolutionPathViability::Unknown,
        rejection_reasons: vec![ResolutionRejectionReason::ImportUnresolved],
        checks,
        source_facts,
        source_provider_facts: Vec::new(),
        dynamic_boundaries: dynamic_boundaries.to_vec(),
        authorities: Vec::new(),
        coverage: ResolutionCoverageEvidence {
            status: FactCoverage::Complete,
            reasons: Vec::new(),
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn stitch_import_paths(
    graph: &ScopeGraphProjection,
    modules: &ModuleStitchIndex,
    traversal: &ResolutionTraversal,
    import: ScopeFactId,
    lexical_distance: u32,
    rule: ImportTraversalRule,
    rule_declared: bool,
    conditions: &[String],
    dynamic_boundaries: &[ScopeFactKey],
) -> Result<Vec<ResolutionPath>, ResolutionProjectionError> {
    let base = import_path(
        graph,
        traversal,
        import,
        lexical_distance,
        rule,
        rule_declared,
        conditions,
        dynamic_boundaries,
    )?;
    if !rule_declared {
        return Ok(vec![base]);
    }
    let import_fact = graph
        .fact(import)
        .map_err(|error| ResolutionProjectionError::Invalid(error.to_string()))?;
    let ScopeFactData::Import {
        scope,
        module_segments,
        ..
    } = import_fact.data()
    else {
        return Err(ResolutionProjectionError::Invalid(
            "deferred import is not an import fact".into(),
        ));
    };
    let candidates = modules.candidates(scope, module_segments);
    if candidates.is_empty() {
        return Ok(vec![base]);
    }

    let mut paths = Vec::new();
    for candidate in candidates {
        let mut module_path = base.clone();
        attach_module_candidate(&mut module_path, import_fact.key(), &candidate);
        match rule {
            ImportTraversalRule::Alias | ImportTraversalRule::Explicit => {
                module_path.endpoint =
                    Some(ResolutionEndpoint::Module(candidate.module.key.clone()));
                set_import_target_check(
                    &mut module_path,
                    ResolutionCheckState::Passed,
                    "import source module is mapped in the exact build context",
                    vec![import_fact.key().clone(), candidate.module.key.clone()],
                );
                module_path.viability = viability_from_checks(&module_path.checks);
                paths.push(module_path);
            }
            ImportTraversalRule::Selective | ImportTraversalRule::Glob => {
                attach_export_coverage_check(&mut module_path, &candidate.module);
                let exports =
                    modules.exports_for_module(&candidate.module, traversal.lookup_root());
                if exports.is_empty() {
                    let complete = candidate.module.export_coverage.status
                        == FactCoverage::Complete
                        && fact_is_complete(graph, &candidate.module.key)?
                        && modules
                            .all_exports_for_module(&candidate.module)
                            .into_iter()
                            .all(|export| fact_is_complete(graph, &export.key).unwrap_or(false));
                    if complete {
                        push_unique(
                            &mut module_path.rejection_reasons,
                            ResolutionRejectionReason::ImportUnresolved,
                        );
                        set_import_target_check(
                            &mut module_path,
                            ResolutionCheckState::Rejected,
                            "complete source module export set does not contain the imported name",
                            vec![import_fact.key().clone(), candidate.module.key.clone()],
                        );
                    } else {
                        push_unique(
                            &mut module_path.rejection_reasons,
                            ResolutionRejectionReason::ExportIncomplete,
                        );
                        set_import_target_check(
                            &mut module_path,
                            ResolutionCheckState::Unknown,
                            "source module export set is incomplete",
                            vec![import_fact.key().clone(), candidate.module.key.clone()],
                        );
                    }
                    module_path.viability = viability_from_checks(&module_path.checks);
                    paths.push(module_path);
                    continue;
                }
                let export_set = modules.all_exports_for_module(&candidate.module);
                for export in exports {
                    let mut export_path = module_path.clone();
                    if rule == ImportTraversalRule::Glob {
                        for member in &export_set {
                            push_unique(&mut export_path.source_facts, member.key.clone());
                        }
                    }
                    let resolved = resolve_export_chains(
                        graph,
                        modules,
                        &candidate,
                        export,
                        &mut BTreeSet::new(),
                    )?;
                    if resolved.chains.is_empty() {
                        for key in resolved.observed_facts {
                            push_unique(&mut export_path.source_facts, key);
                        }
                        push_unique(
                            &mut export_path.rejection_reasons,
                            ResolutionRejectionReason::ExportIncomplete,
                        );
                        attach_export_edge(&mut export_path, &candidate.module, export);
                        set_import_target_check(
                            &mut export_path,
                            ResolutionCheckState::Unknown,
                            "export or re-export graph has no exact acyclic target",
                            vec![import_fact.key().clone(), export.key.clone()],
                        );
                        export_path.viability = viability_from_checks(&export_path.checks);
                        paths.push(export_path);
                        continue;
                    }
                    if resolved.incomplete {
                        let mut incomplete_path = export_path.clone();
                        attach_export_edge(&mut incomplete_path, &candidate.module, export);
                        for key in &resolved.observed_facts {
                            push_unique(&mut incomplete_path.source_facts, key.clone());
                        }
                        push_unique(
                            &mut incomplete_path.rejection_reasons,
                            ResolutionRejectionReason::ExportIncomplete,
                        );
                        set_import_target_check(
                            &mut incomplete_path,
                            ResolutionCheckState::Unknown,
                            "an alternate export or re-export branch is incomplete",
                            vec![import_fact.key().clone(), export.key.clone()],
                        );
                        incomplete_path.viability = viability_from_checks(&incomplete_path.checks);
                        paths.push(incomplete_path);
                    }
                    for rejected in resolved.rejected_reexports {
                        let mut rejected_path = export_path.clone();
                        attach_rejected_reexport(&mut rejected_path, import_fact.key(), &rejected);
                        paths.push(rejected_path);
                    }
                    for chain in resolved.chains {
                        let mut target_path = export_path.clone();
                        attach_export_chain(graph, traversal, &mut target_path, &chain)?;
                        paths.push(target_path);
                    }
                }
            }
            _ => paths.push(base.clone()),
        }
    }
    Ok(paths)
}

fn attach_module_candidate(
    path: &mut ResolutionPath,
    import: &ScopeFactKey,
    candidate: &ModuleCandidate,
) {
    path.rejection_reasons
        .retain(|reason| *reason != ResolutionRejectionReason::ImportUnresolved);
    push_unique(&mut path.source_facts, candidate.module.key.clone());
    for scope in &candidate.module.file_scopes {
        push_unique(&mut path.source_facts, scope.clone());
    }
    if candidate.module.package_id != candidate.importer.package_id {
        path.edges.push(ResolutionPathEdge {
            kind: ResolutionPathEdgeKind::Package,
            from: import.clone(),
            to: candidate.module.key.clone(),
            source_fact: candidate.module.key.clone(),
        });
    }
    path.edges.push(ResolutionPathEdge {
        kind: ResolutionPathEdgeKind::Module,
        from: import.clone(),
        to: candidate.module.key.clone(),
        source_fact: candidate.module.key.clone(),
    });
    let (state, detail) = if candidate.target_matches {
        (
            ResolutionCheckState::Passed,
            "module belongs to the importer's exact build target",
        )
    } else {
        push_unique(
            &mut path.rejection_reasons,
            ResolutionRejectionReason::WrongBuildTarget,
        );
        (
            ResolutionCheckState::Rejected,
            "module path exists only in a different build target",
        )
    };
    path.checks.push(ResolutionCheck {
        kind: ResolutionCheckKind::BuildTarget,
        state,
        detail: detail.into(),
        source_facts: vec![import.clone(), candidate.module.key.clone()],
    });
}

fn attach_export_edge(path: &mut ResolutionPath, module: &ModuleRecord, export: &ExportRecord) {
    push_unique(&mut path.source_facts, export.key.clone());
    path.edges.push(ResolutionPathEdge {
        kind: ResolutionPathEdgeKind::Export,
        from: module.key.clone(),
        to: export.key.clone(),
        source_fact: export.key.clone(),
    });
}

fn resolve_export_chains(
    graph: &ScopeGraphProjection,
    modules: &ModuleStitchIndex,
    candidate: &ModuleCandidate,
    export: &ExportRecord,
    visiting: &mut BTreeSet<(ScopeFactKey, ScopeFactKey)>,
) -> Result<ExportResolution, ResolutionProjectionError> {
    let visit = (candidate.module.key.clone(), export.key.clone());
    if !visiting.insert(visit.clone()) {
        return Ok(ExportResolution {
            incomplete: true,
            observed_facts: vec![candidate.module.key.clone(), export.key.clone()],
            ..ExportResolution::default()
        });
    }
    let hop = ExportHop {
        candidate: candidate.clone(),
        export: export.clone(),
    };
    let targets = export_targets(graph, modules, &candidate.module, export)?;
    let chains = targets
        .into_iter()
        .map(|endpoint| ExportChain {
            hops: vec![hop.clone()],
            endpoint,
        })
        .collect::<Vec<_>>();
    let mut resolved = ExportResolution {
        chains,
        rejected_reexports: Vec::new(),
        incomplete: false,
        observed_facts: vec![candidate.module.key.clone(), export.key.clone()],
    };
    for scope in &candidate.module.file_scopes {
        push_unique(&mut resolved.observed_facts, scope.clone());
    }
    if !export.reexport_segments.is_empty() {
        if !reexport_rule_declared(graph, &export.key)? || export.reexport_segments.len() < 2 {
            resolved.incomplete = true;
        } else {
            let split = export.reexport_segments.len() - 1;
            let module_segments = &export.reexport_segments[..split];
            let exported_name = &export.reexport_segments[split];
            let next_candidates = modules.candidates_from(&candidate.module, module_segments);
            if next_candidates.is_empty() {
                resolved.incomplete = true;
            }
            for next in next_candidates {
                push_unique(&mut resolved.observed_facts, next.module.key.clone());
                for scope in &next.module.file_scopes {
                    push_unique(&mut resolved.observed_facts, scope.clone());
                }
                if !next.target_matches {
                    resolved.rejected_reexports.push(RejectedReexport {
                        hops: vec![hop.clone()],
                        candidate: next,
                    });
                    continue;
                }
                let next_exports = modules.exports_for_module(&next.module, exported_name);
                if next_exports.is_empty() {
                    resolved.incomplete = true;
                }
                for next_export in next_exports {
                    let tail = resolve_export_chains(graph, modules, &next, next_export, visiting)?;
                    resolved.incomplete |= tail.incomplete;
                    for key in tail.observed_facts {
                        push_unique(&mut resolved.observed_facts, key);
                    }
                    for mut rejected in tail.rejected_reexports {
                        rejected.hops.insert(0, hop.clone());
                        resolved.rejected_reexports.push(rejected);
                    }
                    for mut chain in tail.chains {
                        chain.hops.insert(0, hop.clone());
                        resolved.chains.push(chain);
                    }
                }
            }
        }
    } else if resolved.chains.is_empty() {
        resolved.incomplete = true;
    }
    visiting.remove(&visit);
    Ok(resolved)
}

fn reexport_rule_declared(
    graph: &ScopeGraphProjection,
    export: &ScopeFactKey,
) -> Result<bool, ResolutionProjectionError> {
    Ok(fact_by_key(graph, export)?
        .evidence()
        .adapter
        .resolution_rules()
        .section(ResolutionRuleSectionKind::ImportsExports)
        .instructions()
        .iter()
        .any(|instruction| {
            matches!(
                instruction,
                ResolutionInstruction::ImportTraversal {
                    rule: ImportTraversalRule::ReExport
                }
            )
        }))
}

fn attach_export_chain(
    graph: &ScopeGraphProjection,
    traversal: &ResolutionTraversal,
    path: &mut ResolutionPath,
    chain: &ExportChain,
) -> Result<(), ResolutionProjectionError> {
    let Some(first) = chain.hops.first() else {
        return Err(ResolutionProjectionError::Invalid(
            "resolved export chain contains no hops".into(),
        ));
    };
    attach_export_edge(path, &first.candidate.module, &first.export);
    attach_export_visibility(path, &first.candidate, &first.export);
    attach_export_conditions(path, &first.export);
    let mut previous_export = first.export.key.clone();
    for hop in chain.hops.iter().skip(1) {
        push_unique(&mut path.source_facts, hop.candidate.module.key.clone());
        for scope in &hop.candidate.module.file_scopes {
            push_unique(&mut path.source_facts, scope.clone());
        }
        if hop.candidate.module.package_id != hop.candidate.importer.package_id {
            path.edges.push(ResolutionPathEdge {
                kind: ResolutionPathEdgeKind::Package,
                from: previous_export.clone(),
                to: hop.candidate.module.key.clone(),
                source_fact: hop.candidate.module.key.clone(),
            });
        }
        push_unique(&mut path.source_facts, hop.export.key.clone());
        path.edges.push(ResolutionPathEdge {
            kind: ResolutionPathEdgeKind::ReExport,
            from: previous_export.clone(),
            to: hop.candidate.module.key.clone(),
            source_fact: hop.export.key.clone(),
        });
        attach_export_edge(path, &hop.candidate.module, &hop.export);
        attach_reexport_target_check(path, &previous_export, &hop.candidate);
        attach_export_coverage_check(path, &hop.candidate.module);
        attach_export_visibility(path, &hop.candidate, &hop.export);
        attach_export_conditions(path, &hop.export);
        previous_export = hop.export.key.clone();
    }
    let endpoint_key = match &chain.endpoint {
        ResolutionEndpoint::Declaration(key) | ResolutionEndpoint::Definition(key) => key.clone(),
        _ => {
            return Err(ResolutionProjectionError::Invalid(
                "module export target is not a declaration or definition".into(),
            ));
        }
    };
    push_unique(&mut path.source_facts, endpoint_key.clone());
    path.edges.push(ResolutionPathEdge {
        kind: ResolutionPathEdgeKind::Export,
        from: previous_export.clone(),
        to: endpoint_key.clone(),
        source_fact: previous_export.clone(),
    });
    path.endpoint = Some(chain.endpoint.clone());
    set_import_target_check(
        path,
        ResolutionCheckState::Passed,
        "imported name reaches an exact declared export target",
        vec![previous_export.clone(), endpoint_key.clone()],
    );
    let namespace = endpoint_namespace(graph, &endpoint_key)?;
    let reference = graph
        .fact(traversal.reference())
        .map_err(|error| ResolutionProjectionError::Invalid(error.to_string()))?;
    let ScopeFactData::Reference {
        namespace: requested,
        ..
    } = reference.data()
    else {
        unreachable!("resolution traversal references only reference facts")
    };
    let namespace_matches = namespace.as_ref().is_some_and(|actual| actual == requested);
    if !namespace_matches {
        push_unique(
            &mut path.rejection_reasons,
            ResolutionRejectionReason::WrongNamespace,
        );
    }
    path.checks.push(ResolutionCheck {
        kind: ResolutionCheckKind::Namespace,
        state: if namespace_matches {
            ResolutionCheckState::Passed
        } else {
            ResolutionCheckState::Rejected
        },
        detail: if namespace_matches {
            "export target matches the requested namespace".into()
        } else {
            "export target does not match the requested namespace".into()
        },
        source_facts: vec![previous_export, endpoint_key],
    });
    path.viability = viability_from_checks(&path.checks);
    Ok(())
}

fn attach_rejected_reexport(
    path: &mut ResolutionPath,
    import: &ScopeFactKey,
    rejected: &RejectedReexport,
) {
    let Some(first) = rejected.hops.first() else {
        return;
    };
    attach_export_edge(path, &first.candidate.module, &first.export);
    attach_export_visibility(path, &first.candidate, &first.export);
    attach_export_conditions(path, &first.export);
    let mut previous_export = first.export.key.clone();
    for hop in rejected.hops.iter().skip(1) {
        push_unique(&mut path.source_facts, hop.candidate.module.key.clone());
        for scope in &hop.candidate.module.file_scopes {
            push_unique(&mut path.source_facts, scope.clone());
        }
        push_unique(&mut path.source_facts, hop.export.key.clone());
        path.edges.push(ResolutionPathEdge {
            kind: ResolutionPathEdgeKind::ReExport,
            from: previous_export.clone(),
            to: hop.candidate.module.key.clone(),
            source_fact: hop.export.key.clone(),
        });
        attach_export_edge(path, &hop.candidate.module, &hop.export);
        attach_reexport_target_check(path, &previous_export, &hop.candidate);
        attach_export_coverage_check(path, &hop.candidate.module);
        attach_export_visibility(path, &hop.candidate, &hop.export);
        attach_export_conditions(path, &hop.export);
        previous_export = hop.export.key.clone();
    }
    push_unique(
        &mut path.source_facts,
        rejected.candidate.module.key.clone(),
    );
    for scope in &rejected.candidate.module.file_scopes {
        push_unique(&mut path.source_facts, scope.clone());
    }
    path.edges.push(ResolutionPathEdge {
        kind: ResolutionPathEdgeKind::ReExport,
        from: previous_export.clone(),
        to: rejected.candidate.module.key.clone(),
        source_fact: rejected.candidate.module.key.clone(),
    });
    attach_reexport_target_check(path, &previous_export, &rejected.candidate);
    push_unique(
        &mut path.rejection_reasons,
        ResolutionRejectionReason::ImportUnresolved,
    );
    set_import_target_check(
        path,
        ResolutionCheckState::Rejected,
        "re-export module is outside the exact build target",
        vec![import.clone(), rejected.candidate.module.key.clone()],
    );
    path.viability = viability_from_checks(&path.checks);
}

fn attach_reexport_target_check(
    path: &mut ResolutionPath,
    source_export: &ScopeFactKey,
    candidate: &ModuleCandidate,
) {
    let (state, detail) = if candidate.target_matches {
        (
            ResolutionCheckState::Passed,
            "re-export source belongs to the exact build target",
        )
    } else {
        push_unique(
            &mut path.rejection_reasons,
            ResolutionRejectionReason::WrongBuildTarget,
        );
        (
            ResolutionCheckState::Rejected,
            "re-export source belongs to a different build target",
        )
    };
    path.checks.push(ResolutionCheck {
        kind: ResolutionCheckKind::BuildTarget,
        state,
        detail: detail.into(),
        source_facts: vec![source_export.clone(), candidate.module.key.clone()],
    });
}

fn attach_export_conditions(path: &mut ResolutionPath, export: &ExportRecord) {
    if !export.conditions.is_empty() {
        path.checks.push(ResolutionCheck {
            kind: ResolutionCheckKind::Condition,
            state: ResolutionCheckState::Unknown,
            detail: format!(
                "export conditions are unevaluated: {}",
                export.conditions.join(", ")
            ),
            source_facts: vec![export.key.clone()],
        });
    }
}

fn attach_export_coverage_check(path: &mut ResolutionPath, module: &ModuleRecord) {
    let (state, detail) = if module.export_coverage.status == FactCoverage::Complete {
        (
            ResolutionCheckState::Passed,
            "module export set is complete".to_string(),
        )
    } else {
        (
            ResolutionCheckState::Unknown,
            format!(
                "module export set is {:?}: {}",
                module.export_coverage.status,
                module
                    .export_coverage
                    .reason
                    .as_deref()
                    .unwrap_or("no exact reason retained")
            ),
        )
    };
    path.checks.push(ResolutionCheck {
        kind: ResolutionCheckKind::ExportSetCoverage,
        state,
        detail,
        source_facts: vec![module.key.clone()],
    });
}

fn attach_export_visibility(
    path: &mut ResolutionPath,
    candidate: &ModuleCandidate,
    export: &ExportRecord,
) {
    let (state, detail) = match export.visibility.kind {
        crate::VisibilityKind::Public => {
            (ResolutionCheckState::Passed, "export is public".to_string())
        }
        crate::VisibilityKind::Package
            if candidate.module.package_id == candidate.importer.package_id =>
        {
            (
                ResolutionCheckState::Passed,
                "export is visible within the package".to_string(),
            )
        }
        crate::VisibilityKind::Module if candidate.module.key == candidate.importer.key => (
            ResolutionCheckState::Passed,
            "export is visible within the module".to_string(),
        ),
        crate::VisibilityKind::AdapterDefined => (
            ResolutionCheckState::Unknown,
            "export visibility requires an adapter rule".to_string(),
        ),
        _ => {
            push_unique(
                &mut path.rejection_reasons,
                ResolutionRejectionReason::NotVisible,
            );
            (
                ResolutionCheckState::Rejected,
                "export is outside its visibility boundary".to_string(),
            )
        }
    };
    path.checks.push(ResolutionCheck {
        kind: ResolutionCheckKind::Visibility,
        state,
        detail,
        source_facts: vec![export.key.clone()],
    });
}

fn set_import_target_check(
    path: &mut ResolutionPath,
    state: ResolutionCheckState,
    detail: &str,
    source_facts: Vec<ScopeFactKey>,
) {
    path.checks
        .retain(|check| check.kind != ResolutionCheckKind::ImportTarget);
    path.checks.push(ResolutionCheck {
        kind: ResolutionCheckKind::ImportTarget,
        state,
        detail: detail.into(),
        source_facts,
    });
}

fn fact_is_complete(
    graph: &ScopeGraphProjection,
    key: &ScopeFactKey,
) -> Result<bool, ResolutionProjectionError> {
    Ok(fact_by_key(graph, key)?.evidence().coverage.status == FactCoverage::Complete)
}

fn fact_by_key<'a>(
    graph: &'a ScopeGraphProjection,
    key: &ScopeFactKey,
) -> Result<&'a crate::ScopeFactRecord, ResolutionProjectionError> {
    graph
        .facts()
        .iter()
        .find(|fact| fact.key() == key)
        .ok_or_else(|| ResolutionProjectionError::MissingFact(key.as_str().into()))
}

fn export_targets(
    graph: &ScopeGraphProjection,
    modules: &ModuleStitchIndex,
    module: &ModuleRecord,
    export: &ExportRecord,
) -> Result<Vec<ResolutionEndpoint>, ResolutionProjectionError> {
    if let Some(target) = &export.local_target {
        return Ok(vec![endpoint_for_fact(graph, target)?]);
    }
    let Some(local_name) = &export.local_name else {
        return Ok(Vec::new());
    };
    graph
        .facts()
        .iter()
        .filter_map(|fact| match fact.data() {
            ScopeFactData::Declaration {
                lookup_key, scope, ..
            } if lookup_key == local_name
                && modules
                    .file_scope(scope)
                    .is_some_and(|file| module.file_scopes.contains(&file)) =>
            {
                Some(Ok(ResolutionEndpoint::Declaration(fact.key().clone())))
            }
            _ => None,
        })
        .collect()
}

fn endpoint_for_fact(
    graph: &ScopeGraphProjection,
    key: &ScopeFactKey,
) -> Result<ResolutionEndpoint, ResolutionProjectionError> {
    match fact_by_key(graph, key)?.data().kind() {
        ScopeFactKind::Declaration => Ok(ResolutionEndpoint::Declaration(key.clone())),
        ScopeFactKind::Definition => Ok(ResolutionEndpoint::Definition(key.clone())),
        actual => Err(ResolutionProjectionError::Invalid(format!(
            "export target has unsupported fact kind {actual:?}"
        ))),
    }
}

fn endpoint_namespace(
    graph: &ScopeGraphProjection,
    key: &ScopeFactKey,
) -> Result<Option<crate::NameNamespace>, ResolutionProjectionError> {
    match fact_by_key(graph, key)?.data() {
        ScopeFactData::Declaration { namespace, .. } => Ok(Some(namespace.clone())),
        ScopeFactData::Definition { declaration, .. } => {
            match fact_by_key(graph, declaration)?.data() {
                ScopeFactData::Declaration { namespace, .. } => Ok(Some(namespace.clone())),
                _ => Err(ResolutionProjectionError::Invalid(
                    "definition links a non-declaration fact".into(),
                )),
            }
        }
        _ => Ok(None),
    }
}

fn lexical_edges(
    graph: &ScopeGraphProjection,
    traversal: &ResolutionTraversal,
    lexical_distance: u32,
) -> Result<Vec<ResolutionPathEdge>, ResolutionProjectionError> {
    let reference = fact_key(graph, traversal.reference())?;
    let start = fact_key(graph, traversal.start_scope())?;
    let mut edges = vec![ResolutionPathEdge {
        kind: ResolutionPathEdgeKind::ReferenceScope,
        from: reference.clone(),
        to: start,
        source_fact: reference,
    }];
    for pair in traversal
        .scopes()
        .windows(2)
        .take(lexical_distance as usize)
    {
        let child = fact_key(graph, pair[0].scope())?;
        let parent = fact_key(graph, pair[1].scope())?;
        edges.push(ResolutionPathEdge {
            kind: ResolutionPathEdgeKind::LexicalParent,
            from: child.clone(),
            to: parent,
            source_fact: child,
        });
    }
    Ok(edges)
}

fn namespace_check(
    observation: NamespaceReachability,
    declaration: &ScopeFactKey,
    rejections: &mut Vec<ResolutionRejectionReason>,
) -> ResolutionCheck {
    let (state, detail) = match observation {
        NamespaceReachability::Exact => (ResolutionCheckState::Passed, "exact namespace".into()),
        NamespaceReachability::Unified => (
            ResolutionCheckState::Passed,
            "pack-declared unified namespace".into(),
        ),
        NamespaceReachability::Transition => (
            ResolutionCheckState::Passed,
            "pack-declared namespace transition".into(),
        ),
        NamespaceReachability::Unreachable => {
            rejections.push(ResolutionRejectionReason::WrongNamespace);
            (
                ResolutionCheckState::Rejected,
                "namespace is unreachable".into(),
            )
        }
        NamespaceReachability::RuleUnavailable(support) => (
            ResolutionCheckState::Unknown,
            format!("namespace rules are {support:?}"),
        ),
    };
    ResolutionCheck {
        kind: ResolutionCheckKind::Namespace,
        state,
        detail,
        source_facts: vec![declaration.clone()],
    }
}

fn visibility_check(
    observation: VisibilityObservation,
    declaration: &ScopeFactKey,
    rejections: &mut Vec<ResolutionRejectionReason>,
) -> ResolutionCheck {
    let (state, detail) = match observation {
        VisibilityObservation::Visible => (ResolutionCheckState::Passed, "publicly visible".into()),
        VisibilityObservation::WithinBoundary => (
            ResolutionCheckState::Passed,
            "reference is within visibility boundary".into(),
        ),
        VisibilityObservation::OutsideBoundary => {
            rejections.push(ResolutionRejectionReason::NotVisible);
            (
                ResolutionCheckState::Rejected,
                "reference is outside visibility boundary".into(),
            )
        }
        VisibilityObservation::RuleRequired => (
            ResolutionCheckState::Unknown,
            "visibility requires an adapter rule".into(),
        ),
        VisibilityObservation::RuleUnavailable(support) => (
            ResolutionCheckState::Unknown,
            format!("visibility rules are {support:?}"),
        ),
    };
    ResolutionCheck {
        kind: ResolutionCheckKind::Visibility,
        state,
        detail,
        source_facts: vec![declaration.clone()],
    }
}

fn timing_check(
    observation: TimingObservation,
    graph: &ScopeGraphProjection,
    rejections: &mut Vec<ResolutionRejectionReason>,
) -> Result<ResolutionCheck, ResolutionProjectionError> {
    let (state, detail, source_facts) = match observation {
        TimingObservation::VisibleAtReference { binding } => (
            ResolutionCheckState::Passed,
            "binding is visible at the reference".into(),
            vec![fact_key(graph, binding)?],
        ),
        TimingObservation::DeclaredAfterReference { binding } => {
            rejections.push(ResolutionRejectionReason::DeclaredLater);
            (
                ResolutionCheckState::Rejected,
                "binding is declared after the reference".into(),
                vec![fact_key(graph, binding)?],
            )
        }
        TimingObservation::AdapterRuleRequired { binding } => (
            ResolutionCheckState::Unknown,
            "binding timing requires an adapter rule".into(),
            vec![fact_key(graph, binding)?],
        ),
        TimingObservation::Unspecified => (
            ResolutionCheckState::Unknown,
            "declaration has no binding-timing fact".into(),
            Vec::new(),
        ),
        TimingObservation::RuleUnavailable(support) => (
            ResolutionCheckState::Unknown,
            format!("binding timing rules are {support:?}"),
            Vec::new(),
        ),
    };
    Ok(ResolutionCheck {
        kind: ResolutionCheckKind::Timing,
        state,
        detail,
        source_facts,
    })
}

fn viability_from_checks(checks: &[ResolutionCheck]) -> ResolutionPathViability {
    if checks
        .iter()
        .any(|check| check.state == ResolutionCheckState::Rejected)
    {
        ResolutionPathViability::Rejected
    } else if checks
        .iter()
        .any(|check| check.state == ResolutionCheckState::Unknown)
    {
        ResolutionPathViability::Unknown
    } else {
        ResolutionPathViability::Viable
    }
}

fn apply_duplicate_rules(
    graph: &ScopeGraphProjection,
    pack: &LanguageResolutionRulePack,
    paths: &mut [ResolutionPath],
) -> Result<(), ResolutionProjectionError> {
    let facts = graph
        .facts()
        .iter()
        .map(|fact| (fact.key(), fact))
        .collect::<BTreeMap<_, _>>();
    let mut groups: BTreeMap<(ScopeFactKey, RuleNamespace), BTreeSet<ScopeFactKey>> =
        BTreeMap::new();
    for path in paths.iter() {
        let Some(declaration) = path_declaration(path) else {
            continue;
        };
        let fact = facts
            .get(declaration)
            .ok_or_else(|| ResolutionProjectionError::MissingFact(declaration.as_str().into()))?;
        let ScopeFactData::Declaration {
            scope, namespace, ..
        } = fact.data()
        else {
            return Err(ResolutionProjectionError::Invalid(
                "declares edge does not target a declaration fact".into(),
            ));
        };
        groups
            .entry((scope.clone(), duplicate_rule_namespace(namespace)))
            .or_default()
            .insert(declaration.clone());
    }

    for ((_, namespace), declarations) in groups {
        if declarations.len() < 2 {
            continue;
        }
        let Some(rule) = duplicate_rule(pack, &namespace) else {
            for path in paths
                .iter_mut()
                .filter(|path| path_declaration(path).is_some_and(|key| declarations.contains(key)))
            {
                path.checks.push(ResolutionCheck {
                    kind: ResolutionCheckKind::DuplicateDefinition,
                    state: ResolutionCheckState::Unknown,
                    detail: "adapter provides no duplicate-definition rule for this namespace"
                        .into(),
                    source_facts: declarations.iter().cloned().collect(),
                });
                for declaration in &declarations {
                    push_unique(&mut path.source_facts, declaration.clone());
                }
                path.viability = viability_from_checks(&path.checks);
            }
            continue;
        };
        let declarations = declarations.into_iter().collect::<Vec<_>>();
        match rule {
            DuplicateDefinitionRule::Ambiguous => {}
            DuplicateDefinitionRule::MergeDeclarations => {
                for path in paths.iter_mut().filter(|path| {
                    path_declaration(path).is_some_and(|key| declarations.contains(key))
                }) {
                    for declaration in &declarations {
                        push_unique(&mut path.source_facts, declaration.clone());
                    }
                    path.endpoint =
                        Some(ResolutionEndpoint::MergedDeclarations(declarations.clone()));
                    path.checks.push(ResolutionCheck {
                        kind: ResolutionCheckKind::DuplicateDefinition,
                        state: ResolutionCheckState::Passed,
                        detail: "adapter merges same-scope declarations".into(),
                        source_facts: declarations.clone(),
                    });
                }
            }
            DuplicateDefinitionRule::AdapterRejects => {
                for path in paths.iter_mut().filter(|path| {
                    path_declaration(path).is_some_and(|key| declarations.contains(key))
                }) {
                    reject_duplicate_path(
                        path,
                        &declarations,
                        ResolutionRejectionReason::DuplicateDefinition,
                        "adapter rejects duplicate same-scope definitions",
                    );
                }
            }
            DuplicateDefinitionRule::LatestVisible => {
                let latest_order = declarations
                    .iter()
                    .map(|declaration| {
                        facts
                            .get(declaration)
                            .map(|fact| (declaration, fact.evidence().source_order))
                            .ok_or_else(|| {
                                ResolutionProjectionError::MissingFact(declaration.as_str().into())
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(|(_, source_order)| source_order)
                    .max()
                    .expect("duplicate group is non-empty");
                for path in paths.iter_mut().filter(|path| {
                    path_declaration(path).is_some_and(|key| {
                        declarations.contains(key)
                            && facts
                                .get(key)
                                .is_some_and(|fact| fact.evidence().source_order < latest_order)
                    })
                }) {
                    reject_duplicate_path(
                        path,
                        &declarations,
                        ResolutionRejectionReason::Shadowed,
                        "adapter selects the latest visible same-scope declaration",
                    );
                }
            }
        }
    }
    Ok(())
}

fn path_declaration(path: &ResolutionPath) -> Option<&ScopeFactKey> {
    path.edges
        .iter()
        .find(|edge| edge.kind == ResolutionPathEdgeKind::Declares)
        .map(|edge| &edge.to)
}

fn reject_duplicate_path(
    path: &mut ResolutionPath,
    declarations: &[ScopeFactKey],
    reason: ResolutionRejectionReason,
    detail: &str,
) {
    for declaration in declarations {
        push_unique(&mut path.source_facts, declaration.clone());
    }
    push_unique(&mut path.rejection_reasons, reason);
    path.checks.push(ResolutionCheck {
        kind: ResolutionCheckKind::DuplicateDefinition,
        state: ResolutionCheckState::Rejected,
        detail: detail.into(),
        source_facts: declarations.to_vec(),
    });
    path.viability = ResolutionPathViability::Rejected;
}

fn duplicate_rule(
    pack: &LanguageResolutionRulePack,
    namespace: &RuleNamespace,
) -> Option<DuplicateDefinitionRule> {
    pack.section(ResolutionRuleSectionKind::ShadowingDuplicates)
        .instructions()
        .iter()
        .find_map(|instruction| match instruction {
            ResolutionInstruction::DuplicateDefinitions {
                namespace: candidate,
                rule,
            } if candidate == namespace => Some(*rule),
            _ => None,
        })
}

fn duplicate_rule_namespace(namespace: &crate::NameNamespace) -> RuleNamespace {
    match namespace {
        crate::NameNamespace::Value => RuleNamespace::Value,
        crate::NameNamespace::Type => RuleNamespace::Type,
        crate::NameNamespace::Module => RuleNamespace::Module,
        crate::NameNamespace::Macro => RuleNamespace::Macro,
        crate::NameNamespace::Label => RuleNamespace::Label,
        crate::NameNamespace::Member => RuleNamespace::Member,
        crate::NameNamespace::AdapterDefined { schema, name } => RuleNamespace::AdapterDefined {
            schema: schema.clone(),
            name: name.clone(),
        },
    }
}

fn apply_precedence(paths: &mut [ResolutionPath]) -> Result<(), ResolutionProjectionError> {
    let viable = paths
        .iter()
        .enumerate()
        .filter(|(_, path)| path.viability == ResolutionPathViability::Viable)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let Some(&first) = viable.first() else {
        return Ok(());
    };
    let mut maxima = vec![first];
    for &index in &viable[1..] {
        match compare_precedence(&paths[index].precedence, &paths[maxima[0]].precedence)? {
            Ordering::Greater => maxima = vec![index],
            Ordering::Equal => maxima.push(index),
            Ordering::Less => {}
        }
    }
    let maxima = maxima.into_iter().collect::<BTreeSet<_>>();
    for index in viable {
        if maxima.contains(&index) {
            continue;
        }
        let path = &mut paths[index];
        path.viability = ResolutionPathViability::Rejected;
        path.rejection_reasons
            .push(ResolutionRejectionReason::Shadowed);
        path.checks.push(ResolutionCheck {
            kind: ResolutionCheckKind::Shadowing,
            state: ResolutionCheckState::Rejected,
            detail: "candidate has lower declared lookup precedence".into(),
            source_facts: Vec::new(),
        });
    }
    Ok(())
}

fn apply_explicit_shadowing(paths: &mut [ResolutionPath]) -> Result<(), ResolutionProjectionError> {
    let viable_declarations = paths
        .iter()
        .filter(|path| path.viability == ResolutionPathViability::Viable)
        .filter_map(|path| {
            path.edges
                .iter()
                .find(|edge| edge.kind == ResolutionPathEdgeKind::Declares)
                .map(|edge| edge.to.clone())
        })
        .collect::<BTreeSet<_>>();
    for path in paths {
        if path.viability != ResolutionPathViability::Viable {
            continue;
        }
        let shadowing = path.edges.iter().find(|edge| {
            edge.kind == ResolutionPathEdgeKind::ExplicitShadowing
                && viable_declarations.contains(&edge.to)
        });
        let Some(shadowing) = shadowing else {
            continue;
        };
        path.viability = ResolutionPathViability::Rejected;
        path.rejection_reasons
            .push(ResolutionRejectionReason::Shadowed);
        path.checks.push(ResolutionCheck {
            kind: ResolutionCheckKind::Shadowing,
            state: ResolutionCheckState::Rejected,
            detail: "a viable declaration has an explicit adapter shadowing relation".into(),
            source_facts: vec![shadowing.source_fact.clone()],
        });
    }
    Ok(())
}

fn compare_precedence(
    left: &[ResolutionPrecedenceComponent],
    right: &[ResolutionPrecedenceComponent],
) -> Result<Ordering, ResolutionProjectionError> {
    if left.len() != right.len() {
        return Err(ResolutionProjectionError::Invalid(
            "candidate precedence keys have different arity".into(),
        ));
    }
    for (left, right) in left.iter().zip(right) {
        if left.dimension != right.dimension || left.direction != right.direction {
            return Err(ResolutionProjectionError::Invalid(
                "candidate precedence keys use different relations".into(),
            ));
        }
        let ordering = left.value.cmp(&right.value);
        if ordering == Ordering::Equal {
            continue;
        }
        return Ok(match left.direction {
            PrecedenceDirection::LowerFirst => ordering.reverse(),
            PrecedenceDirection::HigherFirst => ordering,
        });
    }
    Ok(Ordering::Equal)
}

fn path_authorities(
    graph: &ScopeGraphProjection,
    source_facts: &[ScopeFactKey],
) -> Result<Vec<CapabilityAuthority>, ResolutionProjectionError> {
    let facts = graph
        .facts()
        .iter()
        .map(|fact| (fact.key(), fact))
        .collect::<BTreeMap<_, _>>();
    let mut authorities = Vec::new();
    for key in source_facts {
        let fact = facts
            .get(key)
            .ok_or_else(|| ResolutionProjectionError::MissingFact(key.as_str().into()))?;
        if let Some(authority) = fact.evidence().authority {
            push_unique(&mut authorities, authority);
        }
    }
    Ok(authorities)
}

fn derive_path_coverage(
    graph: &ScopeGraphProjection,
    path: &ResolutionPath,
    rule_gaps: &[RuleSectionGap],
) -> Result<ResolutionCoverageEvidence, ResolutionProjectionError> {
    let facts = graph
        .facts()
        .iter()
        .map(|fact| (fact.key(), fact))
        .collect::<BTreeMap<_, _>>();
    let mut status = FactCoverage::Complete;
    let mut reasons = Vec::new();
    for key in &path.source_facts {
        let fact = facts
            .get(key)
            .ok_or_else(|| ResolutionProjectionError::MissingFact(key.as_str().into()))?;
        let evidence = &fact.evidence().coverage;
        if evidence.status != FactCoverage::Complete {
            status = combine_coverage(status, evidence.status);
            push_unique(
                &mut reasons,
                format!(
                    "source fact {} is {:?}: {}",
                    key.as_str(),
                    evidence.status,
                    evidence.reason.as_deref().unwrap_or("no reason retained")
                ),
            );
        }
    }
    for gap in rule_gaps {
        let gap_status = match gap.support() {
            CapabilitySupport::Unknown => FactCoverage::Partial,
            CapabilitySupport::Unsupported => FactCoverage::Unsupported,
            CapabilitySupport::Provided => continue,
        };
        status = combine_coverage(status, gap_status);
        push_unique(
            &mut reasons,
            format!(
                "resolution rule section {:?} is {:?}",
                gap.section(),
                gap.support()
            ),
        );
    }
    for key in &path.dynamic_boundaries {
        let fact = facts
            .get(key)
            .ok_or_else(|| ResolutionProjectionError::MissingFact(key.as_str().into()))?;
        let ScopeFactData::DynamicBoundary {
            construct_kind,
            reason,
            ..
        } = fact.data()
        else {
            return Err(ResolutionProjectionError::Invalid(
                "resolution path dynamic key is not a dynamic-boundary fact".into(),
            ));
        };
        status = combine_coverage(status, FactCoverage::Partial);
        push_unique(
            &mut reasons,
            format!("dynamic boundary {construct_kind}: {reason}"),
        );
    }
    if path
        .checks
        .iter()
        .any(|check| check.state == ResolutionCheckState::Unknown)
    {
        status = combine_coverage(status, FactCoverage::Partial);
        push_unique(
            &mut reasons,
            "candidate path contains an unknown check".into(),
        );
    }
    if path
        .rejection_reasons
        .contains(&ResolutionRejectionReason::DuplicateDefinition)
    {
        status = combine_coverage(status, FactCoverage::Failed);
        push_unique(
            &mut reasons,
            "adapter rejected duplicate same-scope definitions".into(),
        );
    }
    let coverage = ResolutionCoverageEvidence { status, reasons };
    coverage.validate()?;
    Ok(coverage)
}

fn derive_coverage(
    graph: &ScopeGraphProjection,
    source_facts: &[ScopeFactKey],
    rule_gaps: &[RuleSectionGap],
    dynamic_boundaries: &[DynamicBoundaryTraversal],
    has_deferred_imports: bool,
    paths: &[ResolutionPath],
) -> Result<ResolutionCoverageEvidence, ResolutionProjectionError> {
    let by_key = graph
        .facts()
        .iter()
        .map(|fact| (fact.key(), fact))
        .collect::<BTreeMap<_, _>>();
    let mut status = FactCoverage::Complete;
    let mut reasons = Vec::new();
    for key in source_facts {
        let fact = by_key
            .get(key)
            .ok_or_else(|| ResolutionProjectionError::MissingFact(key.as_str().into()))?;
        let evidence = &fact.evidence().coverage;
        if evidence.status != FactCoverage::Complete {
            status = combine_coverage(status, evidence.status);
            push_unique(
                &mut reasons,
                format!(
                    "source fact {} is {:?}: {}",
                    key.as_str(),
                    evidence.status,
                    evidence.reason.as_deref().unwrap_or("no reason retained")
                ),
            );
        }
    }
    for gap in rule_gaps {
        let gap_status = match gap.support() {
            CapabilitySupport::Unknown => FactCoverage::Partial,
            CapabilitySupport::Unsupported => FactCoverage::Unsupported,
            CapabilitySupport::Provided => continue,
        };
        status = combine_coverage(status, gap_status);
        push_unique(
            &mut reasons,
            format!(
                "resolution rule section {:?} is {:?}",
                gap.section(),
                gap.support()
            ),
        );
    }
    for boundary in dynamic_boundaries {
        status = combine_coverage(status, FactCoverage::Partial);
        push_unique(
            &mut reasons,
            format!(
                "dynamic boundary {}: {}",
                boundary.construct_kind(),
                boundary.reason()
            ),
        );
    }
    if has_deferred_imports {
        status = combine_coverage(status, FactCoverage::Partial);
        push_unique(
            &mut reasons,
            "reachable import traversal has no exact module/export endpoint".into(),
        );
    }
    if paths
        .iter()
        .any(|path| path.viability == ResolutionPathViability::Unknown)
    {
        status = combine_coverage(status, FactCoverage::Partial);
        push_unique(
            &mut reasons,
            "one or more candidate paths have an unknown check".into(),
        );
    }
    if paths.iter().any(|path| {
        path.rejection_reasons
            .contains(&ResolutionRejectionReason::DuplicateDefinition)
    }) {
        status = combine_coverage(status, FactCoverage::Failed);
        push_unique(
            &mut reasons,
            "adapter rejected duplicate same-scope definitions".into(),
        );
    }
    let coverage = ResolutionCoverageEvidence { status, reasons };
    coverage.validate()?;
    Ok(coverage)
}

fn derive_status(status: FactCoverage, paths: &[ResolutionPath]) -> ResolutionStatus {
    if paths.iter().any(|path| {
        path.rejection_reasons
            .contains(&ResolutionRejectionReason::ProviderConflict)
    }) {
        return ResolutionStatus::Conflict;
    }
    if status != FactCoverage::Complete {
        return ResolutionStatus::Unknown;
    }
    let endpoints = paths
        .iter()
        .filter(|path| path.viability == ResolutionPathViability::Viable)
        .filter_map(|path| path.endpoint.as_ref())
        .collect::<BTreeSet<_>>();
    match endpoints.len() {
        0 => ResolutionStatus::Unresolved,
        1 => ResolutionStatus::Unique,
        _ => ResolutionStatus::Ambiguous,
    }
}

fn validate_status(result: &ResolutionResult) -> Result<(), ResolutionProjectionError> {
    let (expected_status, expected_preferred, expected_coverage) =
        derive_conclusion_join(&result.conclusions)?;
    if result.status != expected_status {
        return Err(ResolutionProjectionError::Invalid(
            "resolution status contradicts its retained provider conclusions".into(),
        ));
    }
    if result.preferred != expected_preferred
        || result.authority
            != expected_preferred
                .as_ref()
                .map(PreferredResolutionConclusion::authority)
    {
        return Err(ResolutionProjectionError::Invalid(
            "preferred resolution conclusion contradicts retained provider conclusions".into(),
        ));
    }
    if result.coverage != expected_coverage {
        return Err(ResolutionProjectionError::Invalid(
            "resolution coverage contradicts retained provider conclusions".into(),
        ));
    }
    if result.status == ResolutionStatus::Conflict
        && !result.paths.iter().any(|path| {
            path.rejection_reasons
                .contains(&ResolutionRejectionReason::ProviderConflict)
        })
    {
        return Err(ResolutionProjectionError::Invalid(
            "conflict result has no provider-conflict path".into(),
        ));
    }
    Ok(())
}

const fn combine_coverage(left: FactCoverage, right: FactCoverage) -> FactCoverage {
    use FactCoverage as F;
    match (left, right) {
        (F::Failed, _) | (_, F::Failed) => F::Failed,
        (F::Partial, _) | (_, F::Partial) => F::Partial,
        (F::Unsupported, _) | (_, F::Unsupported) => F::Unsupported,
        _ => F::Complete,
    }
}

const fn import_specificity(rule: ImportTraversalRule) -> u64 {
    match rule {
        ImportTraversalRule::Alias => 0,
        ImportTraversalRule::Selective => 1,
        ImportTraversalRule::Explicit => 2,
        ImportTraversalRule::Prelude => 3,
        ImportTraversalRule::Glob => 4,
        ImportTraversalRule::Export => 5,
        ImportTraversalRule::ReExport => 6,
    }
}

fn fact_key(
    graph: &ScopeGraphProjection,
    id: ScopeFactId,
) -> Result<ScopeFactKey, ResolutionProjectionError> {
    graph
        .fact(id)
        .map(|fact| fact.key().clone())
        .map_err(|error| ResolutionProjectionError::Invalid(error.to_string()))
}

fn deduplicate<T: Clone + Ord>(values: Vec<T>) -> Vec<T> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert((*value).clone()))
        .collect()
}

fn push_unique<T: PartialEq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn validate_unique_keys(
    label: &str,
    keys: &[ScopeFactKey],
) -> Result<(), ResolutionProjectionError> {
    if keys.iter().collect::<BTreeSet<_>>().len() != keys.len() {
        return Err(ResolutionProjectionError::Invalid(format!(
            "{label} contain duplicates"
        )));
    }
    Ok(())
}

fn validate_text(label: &str, value: &str) -> Result<(), ResolutionProjectionError> {
    if value.trim().is_empty() {
        return Err(ResolutionProjectionError::Invalid(format!(
            "{label} must not be empty"
        )));
    }
    Ok(())
}

fn derive_serialized_key(
    domain: &str,
    prefix: &str,
    payload: &impl Serialize,
) -> Result<String, ResolutionProjectionError> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|error| ResolutionProjectionError::Invalid(error.to_string()))?;
    derive_key(domain, prefix, &[&bytes])
}

fn derive_key(
    domain: &str,
    prefix: &str,
    parts: &[&[u8]],
) -> Result<String, ResolutionProjectionError> {
    validate_text("resolution identity domain", domain)?;
    let mut hasher = blake3::Hasher::new_derive_key(domain);
    for part in parts {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    Ok(format!("{prefix}{}", hasher.finalize().to_hex()))
}

fn validate_key(value: &str, prefix: &str) -> Result<(), ResolutionProjectionError> {
    let Some(hex) = value.strip_prefix(prefix) else {
        return Err(ResolutionProjectionError::Invalid(format!(
            "resolution identity must start with {prefix}"
        )));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ResolutionProjectionError::Invalid(
            "resolution identity must contain a 32-byte hexadecimal digest".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use deslop_core::Lang;
    use deslop_lang::{
        AdapterCapability, CanonicalRoleSet, CapabilityDeclaration, DuplicateDefinitionRule,
        ExtractionFactKind, GENERIC_PACK, GrammarDescriptor, LangPack, LanguageResolutionRulePack,
        RUST_PACK, Registry, ResolutionInstruction, ResolutionRuleSection,
        ResolutionRuleSectionKind, ResolutionSyntaxSelector, RuleNamespace,
    };
    use tree_sitter::Node;

    use crate::{
        BindingDraft, BindingForm, BindingTargetDraft, BuildContextId, BuildModuleDraft,
        DeclarationDraft, DeclarationModifier, DynamicBoundaryDraft, ExportDraft,
        FactCoverageEvidence, ImportDraft, ImportForm, Mutability, NameNamespace, NamespacePolicy,
        ProjectAnalysis, ProjectSnapshotBuilder, ReferenceDraft, ReferenceRole, RepositoryId,
        ScopeDraft, ScopeFactPolicyId, ScopeGraphBuilder, ScopeKind, SemanticArtifactId,
        SemanticProviderDraft, SemanticProviderKind, SemanticResolutionFactBuilder,
        SemanticResolutionFactDocument, SemanticResolutionFactDraft, ShadowingDraft,
        VisibilityDraft, VisibilityKind,
    };

    use super::*;

    struct CompleteResolutionPack {
        duplicate_rule: DuplicateDefinitionRule,
    }

    static COMPLETE_RESOLUTION_PACK: CompleteResolutionPack = CompleteResolutionPack {
        duplicate_rule: DuplicateDefinitionRule::Ambiguous,
    };
    static MERGING_RESOLUTION_PACK: CompleteResolutionPack = CompleteResolutionPack {
        duplicate_rule: DuplicateDefinitionRule::MergeDeclarations,
    };
    static LATEST_RESOLUTION_PACK: CompleteResolutionPack = CompleteResolutionPack {
        duplicate_rule: DuplicateDefinitionRule::LatestVisible,
    };
    static REJECTING_RESOLUTION_PACK: CompleteResolutionPack = CompleteResolutionPack {
        duplicate_rule: DuplicateDefinitionRule::AdapterRejects,
    };

    impl LangPack for CompleteResolutionPack {
        fn name(&self) -> &'static str {
            "complete-resolution-test"
        }

        fn capability_manifest(&self) -> deslop_lang::LanguageAdapterCapabilityManifest {
            let mut manifest = RUST_PACK.capability_manifest();
            for capability in [
                AdapterCapability::LexicalScopes,
                AdapterCapability::NameResolution,
                AdapterCapability::ImportsExports,
                AdapterCapability::DependencyGraph,
            ] {
                manifest = manifest
                    .with_declaration(CapabilityDeclaration::provided(
                        capability,
                        CapabilityAuthority::Adapter,
                    ))
                    .unwrap();
            }
            manifest
        }

        fn query_pack(&self) -> deslop_lang::LanguageQueryPack {
            RUST_PACK.query_pack()
        }

        fn lexical_policy(&self) -> deslop_lang::LanguageLexicalPolicy {
            RUST_PACK.lexical_policy()
        }

        fn construct_policy(&self) -> deslop_lang::LanguageConstructPolicy {
            RUST_PACK.construct_policy()
        }

        fn resolution_rule_pack(&self) -> LanguageResolutionRulePack {
            let source = RUST_PACK.resolution_rule_pack();
            let mut sections = source.sections().to_vec();
            sections[ResolutionRuleSectionKind::ALL
                .iter()
                .position(|kind| *kind == ResolutionRuleSectionKind::Extraction)
                .unwrap()] = ResolutionRuleSection::provided(
                ResolutionRuleSectionKind::Extraction,
                [
                    ExtractionFactKind::Declaration,
                    ExtractionFactKind::Definition,
                    ExtractionFactKind::Binding,
                    ExtractionFactKind::Reference,
                    ExtractionFactKind::Import,
                    ExtractionFactKind::Export,
                ]
                .into_iter()
                .map(|fact_kind| ResolutionInstruction::ExtractFact {
                    selector: ResolutionSyntaxSelector::new("identifier", None, None).unwrap(),
                    name_field: None,
                    namespace: matches!(
                        fact_kind,
                        ExtractionFactKind::Declaration | ExtractionFactKind::Reference
                    )
                    .then_some(RuleNamespace::Value),
                    fact_kind,
                })
                .collect(),
            )
            .unwrap();
            let imports_index = ResolutionRuleSectionKind::ALL
                .iter()
                .position(|kind| *kind == ResolutionRuleSectionKind::ImportsExports)
                .unwrap();
            sections[imports_index] = ResolutionRuleSection::provided(
                ResolutionRuleSectionKind::ImportsExports,
                [
                    ImportTraversalRule::Explicit,
                    ImportTraversalRule::Selective,
                    ImportTraversalRule::Alias,
                    ImportTraversalRule::Glob,
                    ImportTraversalRule::Prelude,
                    ImportTraversalRule::Export,
                    ImportTraversalRule::ReExport,
                ]
                .into_iter()
                .map(|rule| ResolutionInstruction::ImportTraversal { rule })
                .collect(),
            )
            .unwrap();
            let duplicate_index = ResolutionRuleSectionKind::ALL
                .iter()
                .position(|kind| *kind == ResolutionRuleSectionKind::ShadowingDuplicates)
                .unwrap();
            sections[duplicate_index] = ResolutionRuleSection::provided(
                ResolutionRuleSectionKind::ShadowingDuplicates,
                sections[duplicate_index]
                    .instructions()
                    .iter()
                    .cloned()
                    .map(|instruction| match instruction {
                        ResolutionInstruction::DuplicateDefinitions { namespace, .. } => {
                            ResolutionInstruction::DuplicateDefinitions {
                                namespace,
                                rule: self.duplicate_rule,
                            }
                        }
                        other => other,
                    })
                    .collect(),
            )
            .unwrap();
            let precedence_index = ResolutionRuleSectionKind::ALL
                .iter()
                .position(|kind| *kind == ResolutionRuleSectionKind::Precedence)
                .unwrap();
            sections[precedence_index] = ResolutionRuleSection::provided(
                ResolutionRuleSectionKind::Precedence,
                vec![ResolutionInstruction::Precedence {
                    terms: vec![
                        deslop_lang::PrecedenceTerm::new(
                            PrecedenceDimension::RuleStep,
                            PrecedenceDirection::LowerFirst,
                        ),
                        deslop_lang::PrecedenceTerm::new(
                            PrecedenceDimension::LexicalDistance,
                            PrecedenceDirection::LowerFirst,
                        ),
                        deslop_lang::PrecedenceTerm::new(
                            PrecedenceDimension::Namespace,
                            PrecedenceDirection::LowerFirst,
                        ),
                    ],
                }],
            )
            .unwrap();
            LanguageResolutionRulePack::new(
                self.adapter_schema(),
                source.dialects().to_vec(),
                sections,
            )
            .unwrap()
        }

        fn canonical_roles(&self, node: Node<'_>, text: &str) -> CanonicalRoleSet {
            RUST_PACK.canonical_roles(node, text)
        }

        fn lang(&self) -> Lang {
            Lang::Rust
        }

        fn extensions(&self) -> &'static [&'static str] {
            &["resolutionrs"]
        }

        fn grammar(&self) -> Option<tree_sitter::Language> {
            RUST_PACK.grammar()
        }

        fn grammar_for_path(&self, path: &Path) -> Option<tree_sitter::Language> {
            RUST_PACK.grammar_for_path(path)
        }

        fn grammar_descriptor_for_path(&self, _path: &Path) -> Option<GrammarDescriptor> {
            RUST_PACK.grammar_descriptor_for_path(Path::new("fixture.rs"))
        }

        fn line_comments(&self) -> &'static [&'static str] {
            RUST_PACK.line_comments()
        }

        fn metrics_regions(&self) -> &'static [&'static str] {
            RUST_PACK.metrics_regions()
        }

        fn metrics_branches(&self) -> &'static [&'static str] {
            RUST_PACK.metrics_branches()
        }

        fn metrics_nesting(&self) -> &'static [&'static str] {
            RUST_PACK.metrics_nesting()
        }

        fn metrics_flow_breaks(&self) -> &'static [&'static str] {
            RUST_PACK.metrics_flow_breaks()
        }

        fn halstead_operator_tokens(&self) -> &'static [&'static str] {
            RUST_PACK.halstead_operator_tokens()
        }

        fn enclosing_region(&self, node: Node<'_>, text: &str) -> Option<deslop_lang::RegionSpan> {
            RUST_PACK.enclosing_region(node, text)
        }
    }

    const SOURCE: &str = r#"fn outer() {
    let target = 1;
    {
        let target = 2;
        target;
    }
}
fn sibling() {
    let target = 3;
}
"#;

    #[derive(Clone, Copy)]
    enum FixtureMode {
        Unique,
        Ambiguous,
        OrderedDuplicate,
        Rejected,
        Dynamic,
        DeferredImport,
        DeferredMappedImport,
        Qualified,
        ExplicitShadowing,
        Missing,
    }

    struct Fixture {
        graph: Arc<ScopeGraphProjection>,
        inner: Option<ScopeFactKey>,
        outer: Option<ScopeFactKey>,
        sibling: ScopeFactKey,
    }

    fn analysis(pack: &'static dyn LangPack) -> Arc<ProjectAnalysis> {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::new(&GENERIC_PACK);
        registry.register(pack);
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("resolution-result-test-repository").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay("fixture.resolutionrs", SOURCE.as_bytes().to_vec())
        .unwrap()
        .build()
        .unwrap();
        ProjectAnalysis::build(snapshot).unwrap()
    }

    fn nodes_by_text(analysis: &ProjectAnalysis, text: &str) -> Vec<crate::NodeId> {
        analysis
            .node_ids()
            .filter(|id| analysis.node(*id).unwrap().text() == text)
            .collect()
    }

    fn nodes_by_kind(analysis: &ProjectAnalysis, kind: &str) -> Vec<crate::NodeId> {
        analysis
            .node_ids()
            .filter(|id| analysis.node(*id).unwrap().raw_kind() == kind)
            .collect()
    }

    fn roles(analysis: &Arc<ProjectAnalysis>, node: crate::NodeId) -> CanonicalRoleSet {
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

    fn module_analysis(peer_source: &str) -> Arc<ProjectAnalysis> {
        let root = tempfile::tempdir().unwrap();
        let mut registry = Registry::new(&GENERIC_PACK);
        registry.register(&COMPLETE_RESOLUTION_PACK);
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("module-stitch-test-repository").unwrap(),
        )
        .unwrap()
        .with_registry(registry)
        .with_overlay(
            "importer.resolutionrs",
            b"fn consume() { imported; alias; through; globbed; looped; }\n".to_vec(),
        )
        .unwrap()
        .with_overlay(
            "source.resolutionrs",
            b"fn source() {} fn through() {} fn globbed() {}\n".to_vec(),
        )
        .unwrap()
        .with_overlay("middle.resolutionrs", b"fn middle() {}\n".to_vec())
        .unwrap()
        .with_overlay("cycle.resolutionrs", b"fn cycle() {}\n".to_vec())
        .unwrap()
        .with_overlay("wrong.resolutionrs", b"fn wrong() {}\n".to_vec())
        .unwrap()
        .with_overlay("peer.resolutionrs", peer_source.as_bytes().to_vec())
        .unwrap()
        .build()
        .unwrap();
        ProjectAnalysis::build(snapshot).unwrap()
    }

    fn node_by_path_kind(analysis: &ProjectAnalysis, path: &str, kind: &str) -> crate::NodeId {
        analysis
            .node_ids()
            .find(|id| {
                let node = analysis.node(*id).unwrap();
                node.path() == Path::new(path) && node.raw_kind() == kind
            })
            .unwrap_or_else(|| panic!("missing {kind} in {path}"))
    }

    fn node_by_path_text(analysis: &ProjectAnalysis, path: &str, value: &str) -> crate::NodeId {
        analysis
            .node_ids()
            .find(|id| {
                let node = analysis.node(*id).unwrap();
                node.path() == Path::new(path) && node.text() == value
            })
            .unwrap_or_else(|| panic!("missing {value} in {path}"))
    }

    fn nodes_by_path_text(
        analysis: &ProjectAnalysis,
        path: &str,
        value: &str,
    ) -> Vec<crate::NodeId> {
        analysis
            .node_ids()
            .filter(|id| {
                let node = analysis.node(*id).unwrap();
                node.path() == Path::new(path) && node.text() == value
            })
            .collect()
    }

    fn module_fixture() -> Arc<ScopeGraphProjection> {
        module_fixture_with_peer_and_export("fn peer_before() {}\n", true)
    }

    fn module_fixture_with_peer(peer_source: &str) -> Arc<ScopeGraphProjection> {
        module_fixture_with_peer_and_export(peer_source, true)
    }

    fn module_fixture_with_peer_and_export(
        peer_source: &str,
        include_through_export: bool,
    ) -> Arc<ScopeGraphProjection> {
        module_fixture_with_peer_export_coverage(peer_source, include_through_export, true)
    }

    fn module_fixture_with_peer_export_coverage(
        peer_source: &str,
        include_through_export: bool,
        complete_exports: bool,
    ) -> Arc<ScopeGraphProjection> {
        let analysis = module_analysis(peer_source);
        let complete = FactCoverageEvidence::complete();
        let export_coverage = if complete_exports {
            complete.clone()
        } else {
            FactCoverageEvidence::partial("module export extraction is incomplete").unwrap()
        };
        let namespaces =
            NamespacePolicy::new(vec![NameNamespace::Value, NameNamespace::Module], vec![])
                .unwrap();
        let importer_root = node_by_path_kind(&analysis, "importer.resolutionrs", "source_file");
        let source_root = node_by_path_kind(&analysis, "source.resolutionrs", "source_file");
        let middle_root = node_by_path_kind(&analysis, "middle.resolutionrs", "source_file");
        let cycle_root = node_by_path_kind(&analysis, "cycle.resolutionrs", "source_file");
        let wrong_root = node_by_path_kind(&analysis, "wrong.resolutionrs", "source_file");
        let peer_root = node_by_path_kind(&analysis, "peer.resolutionrs", "source_file");
        let imported = node_by_path_text(&analysis, "importer.resolutionrs", "imported");
        let alias = node_by_path_text(&analysis, "importer.resolutionrs", "alias");
        let through_reference = node_by_path_text(&analysis, "importer.resolutionrs", "through");
        let globbed_reference = node_by_path_text(&analysis, "importer.resolutionrs", "globbed");
        let looped_reference = node_by_path_text(&analysis, "importer.resolutionrs", "looped");
        let source = node_by_path_text(&analysis, "source.resolutionrs", "source");
        let through = node_by_path_text(&analysis, "source.resolutionrs", "through");
        let globbed = node_by_path_text(&analysis, "source.resolutionrs", "globbed");
        let mut builder = ScopeGraphBuilder::new(
            Arc::clone(&analysis),
            BuildContextId::from_parts(&[b"module-stitch-context"]).unwrap(),
            ScopeFactPolicyId::from_parts(&[b"module-stitch-facts/1"]).unwrap(),
        )
        .unwrap();
        let mut add_file = |node| {
            builder
                .add_scope(
                    node,
                    roles(&analysis, node),
                    complete.clone(),
                    ScopeDraft {
                        kind: ScopeKind::File,
                        parent: None,
                        namespace_policy: namespaces.clone(),
                    },
                )
                .unwrap()
        };
        let importer_scope = add_file(importer_root);
        let source_scope = add_file(source_root);
        let middle_scope = add_file(middle_root);
        let cycle_scope = add_file(cycle_root);
        let wrong_scope = add_file(wrong_root);
        let peer_scope = add_file(peer_root);
        let independent = nodes_by_path_text(&analysis, "peer.resolutionrs", "independent");
        if independent.len() >= 2 {
            let declaration = builder
                .add_declaration(
                    independent[0],
                    roles(&analysis, independent[0]),
                    complete.clone(),
                    DeclarationDraft {
                        original_name: "independent".into(),
                        lookup_key: "independent".into(),
                        namespace: NameNamespace::Value,
                        scope: peer_scope,
                        visibility: VisibilityDraft {
                            kind: VisibilityKind::Scope,
                            boundary: Some(peer_scope),
                            adapter_rule: None,
                        },
                        modifiers: vec![],
                    },
                )
                .unwrap();
            builder
                .add_binding(
                    independent[0],
                    roles(&analysis, independent[0]),
                    complete.clone(),
                    BindingDraft {
                        target: BindingTargetDraft::Declaration(declaration),
                        form: BindingForm::Declaration,
                        timing: crate::BindingTiming::AtDeclaration,
                        mutability: Mutability::Immutable,
                    },
                )
                .unwrap();
            let reference = *independent.last().unwrap();
            builder
                .add_reference(
                    reference,
                    roles(&analysis, reference),
                    complete.clone(),
                    ReferenceDraft {
                        original_spelling: "independent".into(),
                        segments: vec!["independent".into()],
                        namespace: NameNamespace::Value,
                        scope: peer_scope,
                        role: ReferenceRole::Read,
                    },
                )
                .unwrap();
        }
        let target = builder
            .add_declaration(
                source,
                roles(&analysis, source),
                complete.clone(),
                DeclarationDraft {
                    original_name: "imported".into(),
                    lookup_key: "imported".into(),
                    namespace: NameNamespace::Value,
                    scope: source_scope,
                    visibility: VisibilityDraft {
                        kind: VisibilityKind::Public,
                        boundary: None,
                        adapter_rule: None,
                    },
                    modifiers: vec![],
                },
            )
            .unwrap();
        builder
            .add_export(
                source,
                roles(&analysis, source),
                complete.clone(),
                ExportDraft {
                    scope: source_scope,
                    local_target: Some(target),
                    local_name: Some("imported".into()),
                    exported_name: "imported".into(),
                    reexport_segments: vec![],
                    visibility: VisibilityDraft {
                        kind: VisibilityKind::Public,
                        boundary: None,
                        adapter_rule: None,
                    },
                    conditions: vec![],
                },
            )
            .unwrap();
        let through_target = builder
            .add_declaration(
                through,
                roles(&analysis, through),
                complete.clone(),
                DeclarationDraft {
                    original_name: "through".into(),
                    lookup_key: "through".into(),
                    namespace: NameNamespace::Value,
                    scope: source_scope,
                    visibility: VisibilityDraft {
                        kind: VisibilityKind::Public,
                        boundary: None,
                        adapter_rule: None,
                    },
                    modifiers: vec![],
                },
            )
            .unwrap();
        let globbed_target = builder
            .add_declaration(
                globbed,
                roles(&analysis, globbed),
                complete.clone(),
                DeclarationDraft {
                    original_name: "globbed".into(),
                    lookup_key: "globbed".into(),
                    namespace: NameNamespace::Value,
                    scope: source_scope,
                    visibility: VisibilityDraft {
                        kind: VisibilityKind::Public,
                        boundary: None,
                        adapter_rule: None,
                    },
                    modifiers: vec![],
                },
            )
            .unwrap();
        for (node, target, name) in [
            (through, through_target, "through"),
            (globbed, globbed_target, "globbed"),
        ] {
            if name == "through" && !include_through_export {
                continue;
            }
            builder
                .add_export(
                    node,
                    roles(&analysis, node),
                    complete.clone(),
                    ExportDraft {
                        scope: source_scope,
                        local_target: Some(target),
                        local_name: Some(name.into()),
                        exported_name: name.into(),
                        reexport_segments: vec![],
                        visibility: VisibilityDraft {
                            kind: VisibilityKind::Public,
                            boundary: None,
                            adapter_rule: None,
                        },
                        conditions: vec![],
                    },
                )
                .unwrap();
        }
        builder
            .add_export(
                middle_root,
                roles(&analysis, middle_root),
                complete.clone(),
                ExportDraft {
                    scope: middle_scope,
                    local_target: None,
                    local_name: None,
                    exported_name: "through".into(),
                    reexport_segments: vec!["dep".into(), "through".into()],
                    visibility: VisibilityDraft {
                        kind: VisibilityKind::Public,
                        boundary: None,
                        adapter_rule: None,
                    },
                    conditions: vec![],
                },
            )
            .unwrap();
        builder
            .add_export(
                middle_root,
                roles(&analysis, middle_root),
                complete.clone(),
                ExportDraft {
                    scope: middle_scope,
                    local_target: None,
                    local_name: None,
                    exported_name: "looped".into(),
                    reexport_segments: vec!["cycle".into(), "looped".into()],
                    visibility: VisibilityDraft {
                        kind: VisibilityKind::Public,
                        boundary: None,
                        adapter_rule: None,
                    },
                    conditions: vec![],
                },
            )
            .unwrap();
        builder
            .add_export(
                cycle_root,
                roles(&analysis, cycle_root),
                complete.clone(),
                ExportDraft {
                    scope: cycle_scope,
                    local_target: None,
                    local_name: None,
                    exported_name: "looped".into(),
                    reexport_segments: vec!["facade".into(), "looped".into()],
                    visibility: VisibilityDraft {
                        kind: VisibilityKind::Public,
                        boundary: None,
                        adapter_rule: None,
                    },
                    conditions: vec![],
                },
            )
            .unwrap();
        for (node, scope, target_id, path) in [
            (importer_root, importer_scope, "lib", vec!["app".into()]),
            (source_root, source_scope, "lib", vec!["dep".into()]),
            (middle_root, middle_scope, "lib", vec!["facade".into()]),
            (cycle_root, cycle_scope, "lib", vec!["cycle".into()]),
            (wrong_root, wrong_scope, "test", vec!["dep".into()]),
        ] {
            builder
                .add_build_module(
                    node,
                    roles(&analysis, node),
                    complete.clone(),
                    BuildModuleDraft {
                        package_id: "workspace".into(),
                        target_id: target_id.into(),
                        source_root: "src".into(),
                        module_path: path,
                        file_scopes: vec![scope],
                        export_coverage: export_coverage.clone(),
                    },
                )
                .unwrap();
        }
        builder
            .add_import(
                importer_root,
                roles(&analysis, importer_root),
                complete.clone(),
                ImportDraft {
                    scope: importer_scope,
                    module_segments: vec!["dep".into()],
                    form: ImportForm::Selective,
                    alias: None,
                    selected_names: vec!["imported".into()],
                    conditions: vec![],
                },
            )
            .unwrap();
        builder
            .add_import(
                importer_root,
                roles(&analysis, importer_root),
                complete.clone(),
                ImportDraft {
                    scope: importer_scope,
                    module_segments: vec!["dep".into()],
                    form: ImportForm::Module,
                    alias: Some("alias".into()),
                    selected_names: vec![],
                    conditions: vec![],
                },
            )
            .unwrap();
        for (module, form, selected) in [
            ("facade", ImportForm::Selective, vec!["through".into()]),
            ("dep", ImportForm::Glob, Vec::new()),
            ("facade", ImportForm::Selective, vec!["looped".into()]),
        ] {
            builder
                .add_import(
                    importer_root,
                    roles(&analysis, importer_root),
                    complete.clone(),
                    ImportDraft {
                        scope: importer_scope,
                        module_segments: vec![module.into()],
                        form,
                        alias: None,
                        selected_names: selected,
                        conditions: vec![],
                    },
                )
                .unwrap();
        }
        for (node, name) in [
            (imported, "imported"),
            (alias, "alias"),
            (through_reference, "through"),
            (globbed_reference, "globbed"),
            (looped_reference, "looped"),
        ] {
            builder
                .add_reference(
                    node,
                    roles(&analysis, node),
                    complete.clone(),
                    ReferenceDraft {
                        original_spelling: name.into(),
                        segments: vec![name.into()],
                        namespace: NameNamespace::Value,
                        scope: importer_scope,
                        role: ReferenceRole::Read,
                    },
                )
                .unwrap();
        }
        Arc::new(builder.build().unwrap())
    }

    fn visibility(boundary: ScopeFactId) -> VisibilityDraft {
        VisibilityDraft {
            kind: VisibilityKind::Scope,
            boundary: Some(boundary),
            adapter_rule: None,
        }
    }

    fn fixture(mode: FixtureMode, partial_reference: bool) -> Fixture {
        fixture_with_pack(mode, partial_reference, &COMPLETE_RESOLUTION_PACK)
    }

    fn fixture_with_pack(
        mode: FixtureMode,
        partial_reference: bool,
        pack: &'static dyn LangPack,
    ) -> Fixture {
        let analysis = analysis(pack);
        let targets = nodes_by_text(&analysis, "target");
        let functions = nodes_by_kind(&analysis, "function_item");
        let blocks = nodes_by_kind(&analysis, "block");
        let root_node = nodes_by_kind(&analysis, "source_file")[0];
        assert_eq!(targets.len(), 4);
        assert_eq!(functions.len(), 2);
        let complete = FactCoverageEvidence::complete();
        let reference_coverage = if partial_reference {
            FactCoverageEvidence::partial("reference extraction is intentionally partial").unwrap()
        } else {
            complete.clone()
        };
        let namespaces = NamespacePolicy::new(
            vec![
                NameNamespace::Value,
                NameNamespace::Type,
                NameNamespace::Module,
                NameNamespace::Macro,
                NameNamespace::Label,
                NameNamespace::Member,
            ],
            vec![],
        )
        .unwrap();
        let mut builder = ScopeGraphBuilder::new(
            Arc::clone(&analysis),
            BuildContextId::from_parts(&[b"resolution-test-target"]).unwrap(),
            ScopeFactPolicyId::from_parts(&[b"complete-resolution-fixture/1"]).unwrap(),
        )
        .unwrap();
        let root = builder
            .add_scope(
                root_node,
                roles(&analysis, root_node),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::File,
                    parent: None,
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let outer_scope = builder
            .add_scope(
                functions[0],
                roles(&analysis, functions[0]),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::Callable,
                    parent: Some(root),
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let inner_scope = builder
            .add_scope(
                blocks[1],
                roles(&analysis, blocks[1]),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::Block,
                    parent: Some(outer_scope),
                    namespace_policy: namespaces.clone(),
                },
            )
            .unwrap();
        let sibling_scope = builder
            .add_scope(
                functions[1],
                roles(&analysis, functions[1]),
                complete.clone(),
                ScopeDraft {
                    kind: ScopeKind::Callable,
                    parent: Some(root),
                    namespace_policy: namespaces,
                },
            )
            .unwrap();

        let lookup = match mode {
            FixtureMode::Missing => "missing",
            FixtureMode::DeferredImport | FixtureMode::DeferredMappedImport => "imported",
            _ => "target",
        };
        let mut outer_id = None;
        let mut inner_id = None;
        if matches!(
            mode,
            FixtureMode::Unique
                | FixtureMode::Ambiguous
                | FixtureMode::OrderedDuplicate
                | FixtureMode::Dynamic
                | FixtureMode::Qualified
                | FixtureMode::ExplicitShadowing
        ) {
            let outer = builder
                .add_declaration(
                    targets[0],
                    roles(&analysis, targets[0]),
                    complete.clone(),
                    DeclarationDraft {
                        original_name: "target".into(),
                        lookup_key: "target".into(),
                        namespace: NameNamespace::Value,
                        scope: outer_scope,
                        visibility: visibility(outer_scope),
                        modifiers: vec![],
                    },
                )
                .unwrap();
            builder
                .add_binding(
                    targets[0],
                    roles(&analysis, targets[0]),
                    complete.clone(),
                    BindingDraft {
                        target: BindingTargetDraft::Declaration(outer),
                        form: BindingForm::Declaration,
                        timing: crate::BindingTiming::AtDeclaration,
                        mutability: Mutability::Immutable,
                    },
                )
                .unwrap();
            let inner = builder
                .add_declaration(
                    targets[1],
                    roles(&analysis, targets[1]),
                    complete.clone(),
                    DeclarationDraft {
                        original_name: "target".into(),
                        lookup_key: "target".into(),
                        namespace: NameNamespace::Value,
                        scope: inner_scope,
                        visibility: visibility(inner_scope),
                        modifiers: vec![],
                    },
                )
                .unwrap();
            builder
                .add_binding(
                    targets[1],
                    roles(&analysis, targets[1]),
                    complete.clone(),
                    BindingDraft {
                        target: BindingTargetDraft::Declaration(inner),
                        form: BindingForm::Declaration,
                        timing: crate::BindingTiming::AtDeclaration,
                        mutability: Mutability::Immutable,
                    },
                )
                .unwrap();
            if matches!(mode, FixtureMode::Ambiguous | FixtureMode::OrderedDuplicate) {
                let peer_node = if matches!(mode, FixtureMode::OrderedDuplicate) {
                    targets[0]
                } else {
                    targets[1]
                };
                let peer = builder
                    .add_declaration(
                        peer_node,
                        roles(&analysis, peer_node),
                        complete.clone(),
                        DeclarationDraft {
                            original_name: "target".into(),
                            lookup_key: "target".into(),
                            namespace: NameNamespace::Value,
                            scope: inner_scope,
                            visibility: visibility(inner_scope),
                            modifiers: vec![DeclarationModifier::Static],
                        },
                    )
                    .unwrap();
                builder
                    .add_binding(
                        peer_node,
                        roles(&analysis, peer_node),
                        complete.clone(),
                        BindingDraft {
                            target: BindingTargetDraft::Declaration(peer),
                            form: BindingForm::Declaration,
                            timing: crate::BindingTiming::AtDeclaration,
                            mutability: Mutability::Immutable,
                        },
                    )
                    .unwrap();
            }
            outer_id = Some(outer);
            inner_id = Some(inner);
        }
        if matches!(mode, FixtureMode::ExplicitShadowing) {
            builder
                .add_shadowing(
                    targets[1],
                    roles(&analysis, targets[1]),
                    complete.clone(),
                    ShadowingDraft {
                        shadowing_declaration: inner_id.unwrap(),
                        shadowed_declaration: outer_id.unwrap(),
                        namespace: NameNamespace::Value,
                        adapter_rule: "test-explicit-shadowing/1".into(),
                    },
                )
                .unwrap();
        }
        if matches!(mode, FixtureMode::Rejected) {
            let later = builder
                .add_declaration(
                    targets[3],
                    roles(&analysis, targets[3]),
                    complete.clone(),
                    DeclarationDraft {
                        original_name: "target".into(),
                        lookup_key: "target".into(),
                        namespace: NameNamespace::Value,
                        scope: inner_scope,
                        visibility: visibility(inner_scope),
                        modifiers: vec![],
                    },
                )
                .unwrap();
            builder
                .add_binding(
                    targets[3],
                    roles(&analysis, targets[3]),
                    complete.clone(),
                    BindingDraft {
                        target: BindingTargetDraft::Declaration(later),
                        form: BindingForm::Declaration,
                        timing: crate::BindingTiming::AfterInitializer,
                        mutability: Mutability::Immutable,
                    },
                )
                .unwrap();
            let wrong_namespace = builder
                .add_declaration(
                    targets[0],
                    roles(&analysis, targets[0]),
                    complete.clone(),
                    DeclarationDraft {
                        original_name: "target".into(),
                        lookup_key: "target".into(),
                        namespace: NameNamespace::Type,
                        scope: inner_scope,
                        visibility: visibility(inner_scope),
                        modifiers: vec![],
                    },
                )
                .unwrap();
            builder
                .add_binding(
                    targets[0],
                    roles(&analysis, targets[0]),
                    complete.clone(),
                    BindingDraft {
                        target: BindingTargetDraft::Declaration(wrong_namespace),
                        form: BindingForm::Declaration,
                        timing: crate::BindingTiming::AtDeclaration,
                        mutability: Mutability::Immutable,
                    },
                )
                .unwrap();
            let invisible = builder
                .add_declaration(
                    targets[0],
                    roles(&analysis, targets[0]),
                    complete.clone(),
                    DeclarationDraft {
                        original_name: "target".into(),
                        lookup_key: "target".into(),
                        namespace: NameNamespace::Value,
                        scope: inner_scope,
                        visibility: visibility(sibling_scope),
                        modifiers: vec![],
                    },
                )
                .unwrap();
            builder
                .add_binding(
                    targets[0],
                    roles(&analysis, targets[0]),
                    complete.clone(),
                    BindingDraft {
                        target: BindingTargetDraft::Declaration(invisible),
                        form: BindingForm::Declaration,
                        timing: crate::BindingTiming::AtDeclaration,
                        mutability: Mutability::Immutable,
                    },
                )
                .unwrap();
        }
        builder
            .add_reference(
                targets[2],
                roles(&analysis, targets[2]),
                reference_coverage,
                ReferenceDraft {
                    original_spelling: if matches!(mode, FixtureMode::Qualified) {
                        "target.member".into()
                    } else {
                        lookup.into()
                    },
                    segments: if matches!(mode, FixtureMode::Qualified) {
                        vec!["target".into(), "member".into()]
                    } else {
                        vec![lookup.into()]
                    },
                    namespace: NameNamespace::Value,
                    scope: inner_scope,
                    role: ReferenceRole::Read,
                },
            )
            .unwrap();
        if matches!(mode, FixtureMode::Dynamic) {
            builder
                .add_dynamic_boundary(
                    targets[2],
                    roles(&analysis, targets[2]),
                    complete.clone(),
                    DynamicBoundaryDraft {
                        construct_kind: "macro-invocation".into(),
                        scopes: vec![inner_scope],
                        namespaces: vec![NameNamespace::Value],
                        reason: "macro expansion is unavailable".into(),
                    },
                )
                .unwrap();
        }
        if matches!(
            mode,
            FixtureMode::DeferredImport | FixtureMode::DeferredMappedImport
        ) {
            builder
                .add_import(
                    root_node,
                    roles(&analysis, root_node),
                    complete.clone(),
                    ImportDraft {
                        scope: root,
                        module_segments: vec!["crate".into(), "dependency".into()],
                        form: ImportForm::Module,
                        alias: Some("imported".into()),
                        selected_names: vec![],
                        conditions: vec!["default-target".into()],
                    },
                )
                .unwrap();
        }
        if matches!(mode, FixtureMode::DeferredMappedImport) {
            builder
                .add_build_module(
                    root_node,
                    roles(&analysis, root_node),
                    complete.clone(),
                    BuildModuleDraft {
                        package_id: "workspace".into(),
                        target_id: "lib".into(),
                        source_root: "src".into(),
                        module_path: vec!["crate".into(), "dependency".into()],
                        file_scopes: vec![root],
                        export_coverage: complete.clone(),
                    },
                )
                .unwrap();
        }
        let sibling = builder
            .add_declaration(
                targets[3],
                roles(&analysis, targets[3]),
                complete.clone(),
                DeclarationDraft {
                    original_name: lookup.into(),
                    lookup_key: lookup.into(),
                    namespace: NameNamespace::Value,
                    scope: sibling_scope,
                    visibility: visibility(sibling_scope),
                    modifiers: vec![],
                },
            )
            .unwrap();
        let projection = Arc::new(builder.build().unwrap());
        let sibling_key = projection.fact(sibling).unwrap().key().clone();
        Fixture {
            inner: inner_id.map(|id| projection.fact(id).unwrap().key().clone()),
            outer: outer_id.map(|id| projection.fact(id).unwrap().key().clone()),
            sibling: sibling_key,
            graph: projection,
        }
    }

    fn policy(label: &[u8]) -> ResolutionPolicyId {
        ResolutionPolicyId::from_parts(&[label]).unwrap()
    }

    fn semantic_artifact(label: &[u8]) -> SemanticArtifactId {
        SemanticArtifactId::from_parts(&[label]).unwrap()
    }

    fn module_reference(graph: &ScopeGraphProjection, spelling: &str) -> ScopeFactKey {
        graph
            .facts()
            .iter()
            .find_map(|fact| match fact.data() {
                ScopeFactData::Reference {
                    original_spelling, ..
                } if original_spelling == spelling => Some(fact.key().clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing module reference {spelling}"))
    }

    fn module_declaration(graph: &ScopeGraphProjection, name: &str) -> ScopeFactKey {
        graph
            .facts()
            .iter()
            .find_map(|fact| match fact.data() {
                ScopeFactData::Declaration { original_name, .. } if original_name == name => {
                    Some(fact.key().clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing module declaration {name}"))
    }

    fn add_semantic_provider(
        builder: &mut SemanticResolutionFactBuilder,
        kind: SemanticProviderKind,
        label: &str,
        coverage: FactCoverageEvidence,
    ) -> crate::SemanticProviderKey {
        builder
            .add_provider(SemanticProviderDraft {
                kind,
                name: label.into(),
                version: "1.0.0".into(),
                executable_artifact: semantic_artifact(format!("{label}-executable").as_bytes()),
                configuration_artifact: semantic_artifact(
                    format!("{label}-configuration").as_bytes(),
                ),
                project_model_artifact: (coverage.status == FactCoverage::Complete)
                    .then(|| semantic_artifact(format!("{label}-project-model").as_bytes())),
                project_model_coverage: coverage,
            })
            .unwrap()
    }

    fn add_semantic_fact(
        builder: &mut SemanticResolutionFactBuilder,
        provider: crate::SemanticProviderKey,
        reference: ScopeFactKey,
        label: &str,
        status: ResolutionStatus,
        endpoints: Vec<ResolutionEndpoint>,
        coverage: FactCoverageEvidence,
    ) -> SemanticResolutionFactKey {
        builder
            .add_fact(SemanticResolutionFactDraft {
                provider,
                reference,
                result_artifact: semantic_artifact(format!("{label}-result").as_bytes()),
                status,
                endpoints,
                coverage,
                diagnostics: vec![format!("{label} retained diagnostic")],
            })
            .unwrap()
    }

    #[test]
    fn complete_unique_retains_lower_precedence_and_excludes_unrelated_names() {
        let fixture = fixture(FixtureMode::Unique, false);
        let projection =
            ResolutionProjection::build(Arc::clone(&fixture.graph), policy(b"base")).unwrap();
        let result = projection.results()[0].wire();
        assert_eq!(result.coverage().status(), FactCoverage::Complete);
        assert_eq!(result.status(), ResolutionStatus::Unique);
        assert_eq!(result.paths().len(), 2);
        assert!(result.paths().iter().all(|path| {
            path.coverage().status() == FactCoverage::Complete
                && path.authorities() == [CapabilityAuthority::Adapter]
        }));
        let viable = result
            .paths()
            .iter()
            .filter(|path| path.viability() == ResolutionPathViability::Viable)
            .collect::<Vec<_>>();
        assert_eq!(viable.len(), 1);
        assert!(matches!(
            viable[0].endpoint(),
            Some(ResolutionEndpoint::Declaration(key)) if Some(key) == fixture.inner.as_ref()
        ));
        let outer = result
            .paths()
            .iter()
            .find(|path| matches!(
                path.endpoint(),
                Some(ResolutionEndpoint::Declaration(key)) if Some(key) == fixture.outer.as_ref()
            ))
            .unwrap();
        assert_eq!(outer.viability(), ResolutionPathViability::Rejected);
        assert!(
            outer
                .rejection_reasons()
                .contains(&ResolutionRejectionReason::Shadowed)
        );
        assert!(
            result
                .paths()
                .iter()
                .all(|path| !path.source_facts().contains(&fixture.sibling))
        );
        assert!(Arc::ptr_eq(projection.scope_graph(), &fixture.graph));
    }

    #[test]
    fn semantic_fact_document_is_strict_pinned_and_build_context_bound() {
        let graph = module_fixture();
        let reference = module_reference(&graph, "imported");
        let endpoint = module_declaration(&graph, "imported");
        let mut builder = SemanticResolutionFactBuilder::new(Arc::clone(&graph));
        let provider = add_semantic_provider(
            &mut builder,
            SemanticProviderKind::LanguageServer,
            "rust-analyzer",
            FactCoverageEvidence::complete(),
        );
        add_semantic_fact(
            &mut builder,
            provider.clone(),
            reference.clone(),
            "rust-analyzer-imported",
            ResolutionStatus::Unique,
            vec![ResolutionEndpoint::Declaration(endpoint)],
            FactCoverageEvidence::complete(),
        );
        assert!(
            builder
                .add_fact(SemanticResolutionFactDraft {
                    provider,
                    reference,
                    result_artifact: semantic_artifact(b"duplicate-result"),
                    status: ResolutionStatus::Unresolved,
                    endpoints: Vec::new(),
                    coverage: FactCoverageEvidence::complete(),
                    diagnostics: Vec::new(),
                })
                .unwrap_err()
                .to_string()
                .contains("already emitted")
        );
        let facts = Arc::new(builder.finish().unwrap());
        let json = serde_json::to_value(facts.document()).unwrap();
        let decoded: SemanticResolutionFactDocument = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), json);

        let mut unknown = json.clone();
        unknown["winner"] = serde_json::json!(true);
        assert!(serde_json::from_value::<SemanticResolutionFactDocument>(unknown).is_err());

        let mut forged = json;
        forged["facts"][0]["status"] = serde_json::json!("ambiguous");
        assert!(serde_json::from_value::<SemanticResolutionFactDocument>(forged).is_err());

        let projection = ResolutionProjection::build_with_semantic_facts(
            Arc::clone(&graph),
            policy(b"strict-semantic-resolution"),
            Arc::clone(&facts),
        )
        .unwrap();
        let resolution_json = serde_json::to_value(projection.document()).unwrap();
        let decoded: ResolutionDocument = serde_json::from_value(resolution_json.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), resolution_json);
        let mut forged_conclusion = resolution_json;
        let result = forged_conclusion["results"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|result| result["reference"] == facts.facts()[0].reference().as_str())
            .unwrap();
        let conclusion = result["conclusions"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|conclusion| conclusion["source"]["kind"] == "semantic")
            .unwrap();
        conclusion["authority"] = serde_json::json!("runtime-verification");
        assert!(serde_json::from_value::<ResolutionDocument>(forged_conclusion).is_err());

        let foreign_graph = module_fixture_with_peer("fn changed_peer() {}\n");
        let error = ResolutionProjection::build_with_semantic_facts(
            foreign_graph,
            policy(b"foreign-semantic-facts"),
            facts,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("another analysis, scope graph, or build context")
        );

        let mut incomplete_builder = SemanticResolutionFactBuilder::new(Arc::clone(&graph));
        let incomplete = add_semantic_provider(
            &mut incomplete_builder,
            SemanticProviderKind::LanguageServer,
            "incomplete-server",
            FactCoverageEvidence::partial("workspace model is incomplete").unwrap(),
        );
        let error = incomplete_builder
            .add_fact(SemanticResolutionFactDraft {
                provider: incomplete,
                reference: module_reference(&graph, "imported"),
                result_artifact: semantic_artifact(b"incomplete-terminal-result"),
                status: ResolutionStatus::Unresolved,
                endpoints: Vec::new(),
                coverage: FactCoverageEvidence::complete(),
                diagnostics: Vec::new(),
            })
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("complete provider project-model coverage")
        );
    }

    #[test]
    fn complete_language_server_agreement_outranks_adapter_without_erasing_it() {
        let graph = module_fixture();
        let reference = module_reference(&graph, "imported");
        let endpoint = module_declaration(&graph, "imported");
        let mut builder = SemanticResolutionFactBuilder::new(Arc::clone(&graph));
        let provider = add_semantic_provider(
            &mut builder,
            SemanticProviderKind::LanguageServer,
            "rust-analyzer",
            FactCoverageEvidence::complete(),
        );
        let semantic_key = add_semantic_fact(
            &mut builder,
            provider,
            reference.clone(),
            "rust-analyzer-agreement",
            ResolutionStatus::Unique,
            vec![ResolutionEndpoint::Declaration(endpoint.clone())],
            FactCoverageEvidence::complete(),
        );
        let projection = ResolutionProjection::build_with_semantic_facts(
            Arc::clone(&graph),
            policy(b"language-server-agreement"),
            Arc::new(builder.finish().unwrap()),
        )
        .unwrap();
        let result = projection
            .results()
            .iter()
            .map(ResolutionResultRecord::wire)
            .find(|result| result.reference() == &reference)
            .unwrap();
        assert_eq!(result.status(), ResolutionStatus::Unique);
        assert_eq!(
            result.authority(),
            Some(CapabilityAuthority::LanguageServer)
        );
        assert_eq!(result.conclusions().len(), 2);
        assert_eq!(
            result.preferred().unwrap().sources(),
            [ResolutionConclusionSource::Semantic(semantic_key.clone())]
        );
        assert_eq!(
            result.preferred().unwrap().endpoints(),
            [ResolutionEndpoint::Declaration(endpoint)]
        );
        assert!(result.conclusions().iter().any(|conclusion| {
            conclusion.source() == &ResolutionConclusionSource::Adapter
                && conclusion.authority() == Some(CapabilityAuthority::Adapter)
        }));
        assert!(result.paths().iter().any(|path| {
            path.source_provider_facts() == [semantic_key.clone()]
                && path.authorities() == [CapabilityAuthority::LanguageServer]
                && path
                    .edges()
                    .iter()
                    .any(|edge| edge.kind() == ResolutionPathEdgeKind::ExternalProvider)
        }));
    }

    #[test]
    fn compiler_preference_retains_lower_disagreement_and_reports_conflict() {
        let graph = module_fixture();
        let reference = module_reference(&graph, "imported");
        let adapter_endpoint = module_declaration(&graph, "imported");
        let provider_endpoint = module_declaration(&graph, "through");
        assert_ne!(adapter_endpoint, provider_endpoint);
        let mut builder = SemanticResolutionFactBuilder::new(Arc::clone(&graph));
        let language_server = add_semantic_provider(
            &mut builder,
            SemanticProviderKind::LanguageServer,
            "rust-analyzer",
            FactCoverageEvidence::complete(),
        );
        let compiler = add_semantic_provider(
            &mut builder,
            SemanticProviderKind::Compiler,
            "rustc",
            FactCoverageEvidence::complete(),
        );
        let lsp_key = add_semantic_fact(
            &mut builder,
            language_server,
            reference.clone(),
            "lsp-through",
            ResolutionStatus::Unique,
            vec![ResolutionEndpoint::Declaration(provider_endpoint.clone())],
            FactCoverageEvidence::complete(),
        );
        let compiler_key = add_semantic_fact(
            &mut builder,
            compiler,
            reference.clone(),
            "compiler-through",
            ResolutionStatus::Unique,
            vec![ResolutionEndpoint::Declaration(provider_endpoint.clone())],
            FactCoverageEvidence::complete(),
        );
        let projection = ResolutionProjection::build_with_semantic_facts(
            Arc::clone(&graph),
            policy(b"provider-precedence"),
            Arc::new(builder.finish().unwrap()),
        )
        .unwrap();
        let result = projection
            .results()
            .iter()
            .map(ResolutionResultRecord::wire)
            .find(|result| result.reference() == &reference)
            .unwrap();
        assert_eq!(result.status(), ResolutionStatus::Conflict);
        assert_eq!(result.coverage().status(), FactCoverage::Complete);
        assert_eq!(result.authority(), Some(CapabilityAuthority::Compiler));
        let preferred = result.preferred().unwrap();
        assert_eq!(
            preferred.sources(),
            [ResolutionConclusionSource::Semantic(compiler_key.clone())]
        );
        assert_eq!(
            preferred.endpoints(),
            [ResolutionEndpoint::Declaration(provider_endpoint)]
        );
        assert!(result.paths().iter().any(|path| {
            path.source_provider_facts().is_empty()
                && path
                    .rejection_reasons()
                    .contains(&ResolutionRejectionReason::ProviderConflict)
        }));
        assert!(result.paths().iter().any(|path| {
            path.source_provider_facts() == [lsp_key.clone()]
                && !path
                    .rejection_reasons()
                    .contains(&ResolutionRejectionReason::ProviderConflict)
        }));
        assert!(result.paths().iter().any(|path| {
            path.source_provider_facts() == [compiler_key.clone()]
                && path.viability() == ResolutionPathViability::Viable
        }));
    }

    #[test]
    fn equal_compiler_disagreement_has_no_order_winner() {
        let graph = module_fixture();
        let reference = module_reference(&graph, "imported");
        let left_endpoint = module_declaration(&graph, "imported");
        let right_endpoint = module_declaration(&graph, "through");
        let build = |reverse: bool| {
            let mut builder = SemanticResolutionFactBuilder::new(Arc::clone(&graph));
            let labels = if reverse {
                ["compiler-b", "compiler-a"]
            } else {
                ["compiler-a", "compiler-b"]
            };
            for label in labels {
                let provider = add_semantic_provider(
                    &mut builder,
                    SemanticProviderKind::Compiler,
                    label,
                    FactCoverageEvidence::complete(),
                );
                let endpoint = if label == "compiler-a" {
                    left_endpoint.clone()
                } else {
                    right_endpoint.clone()
                };
                add_semantic_fact(
                    &mut builder,
                    provider,
                    reference.clone(),
                    label,
                    ResolutionStatus::Unique,
                    vec![ResolutionEndpoint::Declaration(endpoint)],
                    FactCoverageEvidence::complete(),
                );
            }
            ResolutionProjection::build_with_semantic_facts(
                Arc::clone(&graph),
                policy(b"equal-compiler-conflict"),
                Arc::new(builder.finish().unwrap()),
            )
            .unwrap()
        };
        let forward = build(false);
        let reverse = build(true);
        assert_eq!(
            serde_json::to_value(forward.document()).unwrap(),
            serde_json::to_value(reverse.document()).unwrap()
        );
        let result = forward
            .results()
            .iter()
            .map(ResolutionResultRecord::wire)
            .find(|result| result.reference() == &reference)
            .unwrap();
        assert_eq!(result.status(), ResolutionStatus::Conflict);
        assert_eq!(result.authority(), None);
        assert!(result.preferred().is_none());
        assert_eq!(
            result
                .conclusions()
                .iter()
                .filter(|conclusion| conclusion.authority() == Some(CapabilityAuthority::Compiler))
                .count(),
            2
        );
    }

    #[test]
    fn incomplete_lsp_fact_is_retained_without_authorizing_or_conflicting() {
        let graph = module_fixture();
        let reference = module_reference(&graph, "imported");
        let possible_endpoint = module_declaration(&graph, "through");
        let mut builder = SemanticResolutionFactBuilder::new(Arc::clone(&graph));
        let provider = add_semantic_provider(
            &mut builder,
            SemanticProviderKind::LanguageServer,
            "partial-server",
            FactCoverageEvidence::partial("workspace model omitted a target").unwrap(),
        );
        let key = add_semantic_fact(
            &mut builder,
            provider,
            reference.clone(),
            "partial-server-imported",
            ResolutionStatus::Unknown,
            vec![ResolutionEndpoint::Declaration(possible_endpoint)],
            FactCoverageEvidence::partial("server result is not exhaustive").unwrap(),
        );
        let projection = ResolutionProjection::build_with_semantic_facts(
            Arc::clone(&graph),
            policy(b"incomplete-language-server"),
            Arc::new(builder.finish().unwrap()),
        )
        .unwrap();
        let result = projection
            .results()
            .iter()
            .map(ResolutionResultRecord::wire)
            .find(|result| result.reference() == &reference)
            .unwrap();
        assert_eq!(result.status(), ResolutionStatus::Unique);
        assert_eq!(result.authority(), Some(CapabilityAuthority::Adapter));
        assert_eq!(
            result.preferred().unwrap().sources(),
            [ResolutionConclusionSource::Adapter]
        );
        assert!(result.paths().iter().any(|path| {
            path.source_provider_facts() == [key.clone()]
                && path.viability() == ResolutionPathViability::Unknown
                && path.coverage().status() == FactCoverage::Partial
                && !path
                    .rejection_reasons()
                    .contains(&ResolutionRejectionReason::ProviderConflict)
        }));
    }

    #[test]
    fn compiler_can_retain_a_positive_external_endpoint_but_disagreement_stays_conflict() {
        let graph = module_fixture();
        let reference = module_reference(&graph, "imported");
        let mut builder = SemanticResolutionFactBuilder::new(Arc::clone(&graph));
        let provider = add_semantic_provider(
            &mut builder,
            SemanticProviderKind::Compiler,
            "rustc",
            FactCoverageEvidence::complete(),
        );
        let key = add_semantic_fact(
            &mut builder,
            provider,
            reference.clone(),
            "compiler-external",
            ResolutionStatus::Unique,
            vec![ResolutionEndpoint::External(
                "registry://example/dependency::imported".into(),
            )],
            FactCoverageEvidence::complete(),
        );
        let projection = ResolutionProjection::build_with_semantic_facts(
            Arc::clone(&graph),
            policy(b"positive-external-provider"),
            Arc::new(builder.finish().unwrap()),
        )
        .unwrap();
        let result = projection
            .results()
            .iter()
            .map(ResolutionResultRecord::wire)
            .find(|result| result.reference() == &reference)
            .unwrap();
        assert_eq!(result.status(), ResolutionStatus::Conflict);
        assert_eq!(result.authority(), Some(CapabilityAuthority::Compiler));
        assert_eq!(
            result.preferred().unwrap().endpoints(),
            [ResolutionEndpoint::External(
                "registry://example/dependency::imported".into()
            )]
        );
        assert!(result.paths().iter().any(|path| {
            path.source_provider_facts() == [key.clone()]
                && matches!(path.endpoint(), Some(ResolutionEndpoint::External(symbol)) if symbol == "registry://example/dependency::imported")
        }));
    }

    #[test]
    fn declared_modules_stitch_alias_and_selective_exports_without_target_fallback() {
        let graph = module_fixture();
        let projection =
            ResolutionProjection::build(Arc::clone(&graph), policy(b"modules")).unwrap();
        assert_eq!(projection.results().len(), 5);
        let result_for = |spelling: &str| {
            projection
                .results()
                .iter()
                .map(ResolutionResultRecord::wire)
                .find(|result| {
                    matches!(
                        fact_by_key(&graph, result.reference()).unwrap().data(),
                        ScopeFactData::Reference { original_spelling, .. }
                            if original_spelling == spelling
                    )
                })
                .unwrap()
        };

        let selective = result_for("imported");
        assert_eq!(selective.status(), ResolutionStatus::Unique);
        assert_eq!(selective.coverage().status(), FactCoverage::Complete);
        assert!(selective.paths().iter().any(|path| {
            path.viability() == ResolutionPathViability::Viable
                && matches!(path.endpoint(), Some(ResolutionEndpoint::Declaration(_)))
                && [
                    ResolutionPathEdgeKind::SelectiveImport,
                    ResolutionPathEdgeKind::Module,
                    ResolutionPathEdgeKind::Export,
                ]
                .into_iter()
                .all(|kind| path.edges().iter().any(|edge| edge.kind() == kind))
        }));
        assert!(selective.paths().iter().any(|path| {
            path.rejection_reasons()
                .contains(&ResolutionRejectionReason::WrongBuildTarget)
                && path.checks().iter().any(|check| {
                    check.kind() == ResolutionCheckKind::BuildTarget
                        && check.state() == ResolutionCheckState::Rejected
                })
        }));

        let alias = result_for("alias");
        assert_eq!(alias.status(), ResolutionStatus::Unique);
        assert_eq!(alias.coverage().status(), FactCoverage::Complete);
        assert!(alias.paths().iter().any(|path| {
            path.viability() == ResolutionPathViability::Viable
                && matches!(path.endpoint(), Some(ResolutionEndpoint::Module(_)))
                && path
                    .edges()
                    .iter()
                    .any(|edge| edge.kind() == ResolutionPathEdgeKind::AliasImport)
        }));

        let reexported = result_for("through");
        assert_eq!(
            reexported.status(),
            ResolutionStatus::Unique,
            "{:?}",
            reexported
                .paths()
                .iter()
                .map(|path| (
                    path.viability(),
                    path.rejection_reasons(),
                    path.checks()
                        .iter()
                        .filter(|check| check.state() == ResolutionCheckState::Unknown)
                        .map(ResolutionCheck::detail)
                        .collect::<Vec<_>>()
                ))
                .collect::<Vec<_>>()
        );
        assert!(reexported.paths().iter().any(|path| {
            path.viability() == ResolutionPathViability::Viable
                && path
                    .edges()
                    .iter()
                    .any(|edge| edge.kind() == ResolutionPathEdgeKind::ReExport)
        }));

        let globbed = result_for("globbed");
        assert_eq!(globbed.status(), ResolutionStatus::Unique);
        assert!(globbed.paths().iter().any(|path| {
            path.viability() == ResolutionPathViability::Viable
                && path
                    .edges()
                    .iter()
                    .any(|edge| edge.kind() == ResolutionPathEdgeKind::GlobImport)
        }));

        let looped = result_for("looped");
        assert_eq!(looped.status(), ResolutionStatus::Unknown);
        assert_eq!(looped.coverage().status(), FactCoverage::Partial);
        assert!(looped.paths().iter().any(|path| {
            path.rejection_reasons()
                .contains(&ResolutionRejectionReason::ExportIncomplete)
                && path.viability() == ResolutionPathViability::Unknown
        }));
    }

    #[test]
    fn incomplete_export_set_never_authorizes_a_terminal_import_result() {
        let graph = module_fixture_with_peer_export_coverage("fn peer_before() {}\n", true, false);
        let projection =
            ResolutionProjection::build(Arc::clone(&graph), policy(b"partial-exports")).unwrap();
        let imported = projection
            .results()
            .iter()
            .map(ResolutionResultRecord::wire)
            .find(|result| {
                matches!(
                    fact_by_key(&graph, result.reference()).unwrap().data(),
                    ScopeFactData::Reference { original_spelling, .. }
                        if original_spelling == "imported"
                )
            })
            .unwrap();
        assert_eq!(imported.status(), ResolutionStatus::Unknown);
        assert_eq!(imported.coverage().status(), FactCoverage::Partial);
        assert!(imported.paths().iter().any(|path| {
            path.checks().iter().any(|check| {
                check.kind() == ResolutionCheckKind::ExportSetCoverage
                    && check.state() == ResolutionCheckState::Unknown
            })
        }));
    }

    #[test]
    fn incremental_module_successor_reuses_unrelated_results_and_matches_clean_build() {
        let resolution_policy = policy(b"module-successor");
        let previous_graph = module_fixture_with_peer("fn peer_before() {}\n");
        let previous = Arc::new(
            ResolutionProjection::build(previous_graph, resolution_policy.clone()).unwrap(),
        );
        let current_graph = module_fixture_with_peer("fn peer_after() { changed(); }\n");
        let clean =
            ResolutionProjection::build(Arc::clone(&current_graph), resolution_policy.clone())
                .unwrap();
        let update = previous
            .successor(current_graph, resolution_policy)
            .unwrap();

        assert_eq!(update.reused_result_keys().len(), 5);
        assert!(update.rebuilt_results().is_empty());
        assert!(update.added_references().is_empty());
        assert!(update.removed_references().is_empty());
        assert_eq!(
            serde_json::to_value(update.current().document()).unwrap(),
            serde_json::to_value(clean.document()).unwrap()
        );
        assert_eq!(
            update
                .previous()
                .results()
                .iter()
                .map(|result| result.wire().key())
                .collect::<Vec<_>>(),
            update
                .current()
                .results()
                .iter()
                .map(|result| result.wire().key())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn semantic_successor_invalidates_exact_artifact_dependents_and_matches_clean_build() {
        let graph = module_fixture();
        let resolution_policy = policy(b"semantic-successor");
        let imported = module_reference(&graph, "imported");
        let through = module_reference(&graph, "through");
        let imported_endpoint = module_declaration(&graph, "imported");
        let through_endpoint = module_declaration(&graph, "through");
        let facts = |configuration: &[u8], imported_result: &[u8]| {
            let mut builder = SemanticResolutionFactBuilder::new(Arc::clone(&graph));
            let provider = builder
                .add_provider(SemanticProviderDraft {
                    kind: SemanticProviderKind::Compiler,
                    name: "rustc".into(),
                    version: "1.90.0".into(),
                    executable_artifact: semantic_artifact(b"rustc-executable-stable"),
                    configuration_artifact: semantic_artifact(configuration),
                    project_model_artifact: Some(semantic_artifact(b"cargo-model-stable")),
                    project_model_coverage: FactCoverageEvidence::complete(),
                })
                .unwrap();
            builder
                .add_fact(SemanticResolutionFactDraft {
                    provider: provider.clone(),
                    reference: imported.clone(),
                    result_artifact: semantic_artifact(imported_result),
                    status: ResolutionStatus::Unique,
                    endpoints: vec![ResolutionEndpoint::Declaration(imported_endpoint.clone())],
                    coverage: FactCoverageEvidence::complete(),
                    diagnostics: vec!["compiler imported result".into()],
                })
                .unwrap();
            builder
                .add_fact(SemanticResolutionFactDraft {
                    provider,
                    reference: through.clone(),
                    result_artifact: semantic_artifact(b"through-result-stable"),
                    status: ResolutionStatus::Unique,
                    endpoints: vec![ResolutionEndpoint::Declaration(through_endpoint.clone())],
                    coverage: FactCoverageEvidence::complete(),
                    diagnostics: vec!["compiler through result".into()],
                })
                .unwrap();
            Arc::new(builder.finish().unwrap())
        };

        let previous = Arc::new(
            ResolutionProjection::build_with_semantic_facts(
                Arc::clone(&graph),
                resolution_policy.clone(),
                facts(b"compiler-config-v1", b"imported-result-v1"),
            )
            .unwrap(),
        );
        assert!(
            previous
                .successor(Arc::clone(&graph), resolution_policy.clone())
                .unwrap_err()
                .to_string()
                .contains("successor_with_semantic_facts")
        );

        let result_changed_facts = facts(b"compiler-config-v1", b"imported-result-v2");
        let clean_result_change = ResolutionProjection::build_with_semantic_facts(
            Arc::clone(&graph),
            resolution_policy.clone(),
            Arc::clone(&result_changed_facts),
        )
        .unwrap();
        let result_update = previous
            .successor_with_semantic_facts(
                Arc::clone(&graph),
                resolution_policy.clone(),
                result_changed_facts,
            )
            .unwrap();
        assert_eq!(result_update.reused_result_keys().len(), 4);
        assert_eq!(result_update.rebuilt_results().len(), 1);
        assert_eq!(result_update.rebuilt_results()[0].reference(), &imported);
        assert_eq!(
            result_update.rebuilt_results()[0].reasons(),
            [ResolutionInvalidationReason::SemanticFactChanged]
        );
        assert_eq!(
            serde_json::to_value(result_update.current().document()).unwrap(),
            serde_json::to_value(clean_result_change.document()).unwrap()
        );

        let configuration_changed_facts = facts(b"compiler-config-v2", b"imported-result-v2");
        let clean_configuration_change = ResolutionProjection::build_with_semantic_facts(
            Arc::clone(&graph),
            resolution_policy.clone(),
            Arc::clone(&configuration_changed_facts),
        )
        .unwrap();
        let configuration_update = result_update
            .current()
            .successor_with_semantic_facts(
                Arc::clone(&graph),
                resolution_policy,
                configuration_changed_facts,
            )
            .unwrap();
        assert_eq!(configuration_update.reused_result_keys().len(), 3);
        assert_eq!(configuration_update.rebuilt_results().len(), 2);
        assert!(
            configuration_update
                .rebuilt_results()
                .iter()
                .all(|invalidation| {
                    [imported.clone(), through.clone()].contains(invalidation.reference())
                        && invalidation.reasons()
                            == [ResolutionInvalidationReason::SemanticFactChanged]
                })
        );
        assert_eq!(
            serde_json::to_value(configuration_update.current().document()).unwrap(),
            serde_json::to_value(clean_configuration_change.document()).unwrap()
        );
    }

    #[test]
    fn export_addition_invalidates_reverse_dependents_but_reuses_independent_result() {
        let resolution_policy = policy(b"export-successor");
        let peer = "fn peer() { let independent = 1; independent; }\n";
        let previous_graph = module_fixture_with_peer_and_export(peer, false);
        let previous = Arc::new(
            ResolutionProjection::build(previous_graph, resolution_policy.clone()).unwrap(),
        );
        let current_graph = module_fixture_with_peer_and_export(peer, true);
        let clean =
            ResolutionProjection::build(Arc::clone(&current_graph), resolution_policy.clone())
                .unwrap();
        let update = previous
            .successor(current_graph, resolution_policy)
            .unwrap();

        assert_eq!(update.reused_result_keys().len(), 1);
        assert_eq!(update.rebuilt_results().len(), 5);
        assert!(update.rebuilt_results().iter().all(|invalidation| {
            invalidation
                .reasons()
                .contains(&ResolutionInvalidationReason::ReachableScopeChanged)
        }));
        assert_eq!(
            serde_json::to_value(update.current().document()).unwrap(),
            serde_json::to_value(clean.document()).unwrap()
        );
    }

    #[test]
    fn equal_maximum_endpoints_are_ambiguous_and_order_cannot_pick_a_winner() {
        let fixture = fixture(FixtureMode::Ambiguous, false);
        let projection = ResolutionProjection::build(fixture.graph, policy(b"ambiguous")).unwrap();
        let result = projection.results()[0].wire();
        assert_eq!(result.status(), ResolutionStatus::Ambiguous);
        assert_eq!(
            result
                .paths()
                .iter()
                .filter(|path| path.viability() == ResolutionPathViability::Viable)
                .count(),
            2
        );

        let mut reversed = result.clone();
        reversed.paths.reverse();
        reversed.key = ResolutionResultKey(String::new());
        let reversed = reversed.finish().unwrap();
        assert_eq!(reversed.status(), ResolutionStatus::Ambiguous);
    }

    #[test]
    fn duplicate_definition_rules_remain_language_specific() {
        let merged = fixture_with_pack(FixtureMode::Ambiguous, false, &MERGING_RESOLUTION_PACK);
        let merged = ResolutionProjection::build(merged.graph, policy(b"merged")).unwrap();
        let merged = merged.results()[0].wire();
        assert_eq!(merged.status(), ResolutionStatus::Unique);
        let merged_endpoints = merged
            .paths()
            .iter()
            .filter(|path| path.viability() == ResolutionPathViability::Viable)
            .filter_map(ResolutionPath::endpoint)
            .collect::<BTreeSet<_>>();
        assert_eq!(merged_endpoints.len(), 1);
        assert!(matches!(
            merged_endpoints.into_iter().next(),
            Some(ResolutionEndpoint::MergedDeclarations(declarations)) if declarations.len() == 2
        ));

        let latest = fixture_with_pack(
            FixtureMode::OrderedDuplicate,
            false,
            &LATEST_RESOLUTION_PACK,
        );
        let latest = ResolutionProjection::build(latest.graph, policy(b"latest")).unwrap();
        let latest = latest.results()[0].wire();
        assert_eq!(latest.status(), ResolutionStatus::Unique);
        assert_eq!(
            latest
                .paths()
                .iter()
                .filter(|path| path.viability() == ResolutionPathViability::Viable)
                .count(),
            1
        );
        assert!(
            latest
                .paths()
                .iter()
                .any(|path| path.checks().iter().any(|check| {
                    check.kind() == ResolutionCheckKind::DuplicateDefinition
                        && check.detail().contains("latest visible")
                }))
        );

        let rejected = fixture_with_pack(FixtureMode::Ambiguous, false, &REJECTING_RESOLUTION_PACK);
        let rejected = ResolutionProjection::build(rejected.graph, policy(b"reject")).unwrap();
        let rejected = rejected.results()[0].wire();
        assert_eq!(rejected.coverage().status(), FactCoverage::Failed);
        assert_eq!(rejected.status(), ResolutionStatus::Unknown);
        assert!(rejected.paths().iter().any(|path| {
            path.rejection_reasons()
                .contains(&ResolutionRejectionReason::DuplicateDefinition)
                && path.coverage().status() == FactCoverage::Failed
        }));
    }

    #[test]
    fn multiple_paths_to_one_endpoint_remain_unique_and_are_all_retained() {
        let fixture = fixture(FixtureMode::Unique, false);
        let projection = ResolutionProjection::build(fixture.graph, policy(b"converged")).unwrap();
        let mut result = projection.results()[0].wire().clone();
        let mut alternate = result
            .paths
            .iter()
            .find(|path| path.viability == ResolutionPathViability::Viable)
            .unwrap()
            .clone();
        alternate.checks.push(ResolutionCheck {
            kind: ResolutionCheckKind::Shadowing,
            state: ResolutionCheckState::Passed,
            detail: "alternate declared path reaches the same endpoint".into(),
            source_facts: Vec::new(),
        });
        alternate.key = ResolutionPathKey(String::new());
        result.paths.push(alternate.finish().unwrap());
        result.key = ResolutionResultKey(String::new());
        let result = result.finish().unwrap();
        assert_eq!(result.status(), ResolutionStatus::Unique);
        assert_eq!(
            result
                .paths()
                .iter()
                .filter(|path| path.viability() == ResolutionPathViability::Viable)
                .count(),
            2
        );
    }

    #[test]
    fn zero_candidates_require_complete_coverage_to_be_unresolved() {
        let complete = fixture(FixtureMode::Missing, false);
        let complete =
            ResolutionProjection::build(complete.graph, policy(b"missing-complete")).unwrap();
        let complete = complete.results()[0].wire();
        assert!(complete.paths().is_empty());
        assert_eq!(complete.coverage().status(), FactCoverage::Complete);
        assert_eq!(complete.status(), ResolutionStatus::Unresolved);

        let partial = fixture(FixtureMode::Missing, true);
        let partial =
            ResolutionProjection::build(partial.graph, policy(b"missing-partial")).unwrap();
        let partial = partial.results()[0].wire();
        assert!(partial.paths().is_empty());
        assert_eq!(partial.coverage().status(), FactCoverage::Partial);
        assert_eq!(partial.status(), ResolutionStatus::Unknown);
        assert!(!partial.coverage().reasons().is_empty());
    }

    #[test]
    fn rejected_attempts_retain_exact_namespace_visibility_and_timing_reasons() {
        let fixture = fixture(FixtureMode::Rejected, false);
        let projection = ResolutionProjection::build(fixture.graph, policy(b"rejected")).unwrap();
        let result = projection.results()[0].wire();
        assert_eq!(result.coverage().status(), FactCoverage::Complete);
        assert_eq!(result.status(), ResolutionStatus::Unresolved);
        assert_eq!(result.paths().len(), 3);
        let reasons = result
            .paths()
            .iter()
            .flat_map(|path| path.rejection_reasons().iter().copied())
            .collect::<BTreeSet<_>>();
        assert!(reasons.contains(&ResolutionRejectionReason::WrongNamespace));
        assert!(reasons.contains(&ResolutionRejectionReason::NotVisible));
        assert!(reasons.contains(&ResolutionRejectionReason::DeclaredLater));
        assert!(
            result
                .paths()
                .iter()
                .all(|path| path.viability() == ResolutionPathViability::Rejected)
        );
    }

    #[test]
    fn dynamic_and_deferred_import_near_cases_are_unknown_not_terminal() {
        let dynamic = fixture(FixtureMode::Dynamic, false);
        let dynamic = ResolutionProjection::build(dynamic.graph, policy(b"dynamic")).unwrap();
        let dynamic = dynamic.results()[0].wire();
        assert_eq!(dynamic.status(), ResolutionStatus::Unknown);
        assert_eq!(dynamic.coverage().status(), FactCoverage::Partial);
        assert_eq!(dynamic.dynamic_boundaries().len(), 1);
        assert!(
            dynamic
                .paths()
                .iter()
                .all(|path| path.coverage().status() == FactCoverage::Partial)
        );
        assert!(
            dynamic
                .coverage()
                .reasons()
                .iter()
                .any(|reason| reason.contains("macro expansion is unavailable"))
        );

        let import = fixture(FixtureMode::DeferredImport, false);
        let import = ResolutionProjection::build(import.graph, policy(b"import")).unwrap();
        let import = import.results()[0].wire();
        assert_eq!(import.status(), ResolutionStatus::Unknown);
        assert_eq!(import.coverage().status(), FactCoverage::Partial);
        assert_eq!(import.paths().len(), 1);
        assert_eq!(
            import.paths()[0].viability(),
            ResolutionPathViability::Unknown
        );
        assert_eq!(import.paths()[0].coverage().status(), FactCoverage::Partial);
        assert!(
            import.paths()[0]
                .rejection_reasons()
                .contains(&ResolutionRejectionReason::ImportUnresolved)
        );
        assert!(import.paths()[0].checks().iter().any(|check| {
            check.kind() == ResolutionCheckKind::Condition
                && check.state() == ResolutionCheckState::Unknown
        }));
    }

    #[test]
    fn newly_declared_matching_module_invalidates_a_formerly_unresolved_import() {
        let resolution_policy = policy(b"new-module");
        let previous_graph = fixture(FixtureMode::DeferredImport, false).graph;
        let previous = Arc::new(
            ResolutionProjection::build(previous_graph, resolution_policy.clone()).unwrap(),
        );
        let current_graph = fixture(FixtureMode::DeferredMappedImport, false).graph;
        let clean =
            ResolutionProjection::build(Arc::clone(&current_graph), resolution_policy.clone())
                .unwrap();
        let update = previous
            .successor(current_graph, resolution_policy)
            .unwrap();

        assert!(update.reused_result_keys().is_empty());
        assert_eq!(update.rebuilt_results().len(), 1);
        assert!(
            update.rebuilt_results()[0]
                .reasons()
                .contains(&ResolutionInvalidationReason::MatchingModuleAdded)
        );
        assert_eq!(
            serde_json::to_value(update.current().document()).unwrap(),
            serde_json::to_value(clean.document()).unwrap()
        );
    }

    #[test]
    fn unresolved_qualification_tail_cannot_promote_a_root_candidate_to_unique() {
        let fixture = fixture(FixtureMode::Qualified, false);
        let projection = ResolutionProjection::build(fixture.graph, policy(b"qualified")).unwrap();
        let result = projection.results()[0].wire();
        assert_eq!(result.status(), ResolutionStatus::Unknown);
        assert_eq!(result.coverage().status(), FactCoverage::Partial);
        assert!(result.paths().iter().any(|path| {
            path.checks().iter().any(|check| {
                check.kind() == ResolutionCheckKind::Qualification
                    && check.state() == ResolutionCheckState::Unknown
                    && check.detail().contains("member")
            })
        }));
    }

    #[test]
    fn viable_explicit_shadowing_is_not_an_order_fallback() {
        let fixture = fixture(FixtureMode::ExplicitShadowing, false);
        let projection = ResolutionProjection::build(fixture.graph, policy(b"shadowing")).unwrap();
        let result = projection.results()[0].wire();
        let outer = result
            .paths()
            .iter()
            .find(|path| matches!(
                path.endpoint(),
                Some(ResolutionEndpoint::Declaration(key)) if Some(key) == fixture.outer.as_ref()
            ))
            .unwrap();
        assert!(outer.checks().iter().any(|check| {
            check.kind() == ResolutionCheckKind::Shadowing
                && check.state() == ResolutionCheckState::Rejected
                && check.detail().contains("explicit adapter shadowing")
        }));
    }

    #[test]
    fn strict_document_round_trip_rejects_status_keys_and_unknown_fields() {
        let fixture = fixture(FixtureMode::Unique, false);
        let projection = ResolutionProjection::build(fixture.graph, policy(b"wire")).unwrap();
        let json = serde_json::to_value(projection.document()).unwrap();
        let decoded: ResolutionDocument = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), json);

        let mut wrong_status = json.clone();
        wrong_status["results"][0]["status"] = serde_json::json!("unresolved");
        assert!(serde_json::from_value::<ResolutionDocument>(wrong_status).is_err());
        let mut corrupt_key = json.clone();
        corrupt_key["results"][0]["paths"][0]["key"] =
            serde_json::json!(format!("rp1_{}", "0".repeat(64)));
        assert!(serde_json::from_value::<ResolutionDocument>(corrupt_key).is_err());
        let mut unknown = json;
        unknown["winner"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ResolutionDocument>(unknown).is_err());
    }

    #[test]
    fn complete_results_reject_incomplete_paths_and_noncanonical_keys() {
        let fixture = fixture(FixtureMode::Unique, false);
        let projection =
            ResolutionProjection::build(fixture.graph, policy(b"strict-coverage")).unwrap();
        let mut result = projection.results()[0].wire().clone();
        result.paths[0].coverage = ResolutionCoverageEvidence {
            status: FactCoverage::Partial,
            reasons: vec!["candidate path crosses an unproven boundary".into()],
        };
        result.paths[0].key = ResolutionPathKey(String::new());
        result.paths[0] = result.paths[0].clone().finish().unwrap();
        result.key = ResolutionResultKey(String::new());
        assert!(matches!(
            result.finish(),
            Err(ResolutionProjectionError::Invalid(message))
                if message.contains("incomplete candidate path")
        ));

        let uppercase = format!("rpol1_{}", "A".repeat(64));
        assert!(
            serde_json::from_value::<ResolutionPolicyId>(serde_json::json!(uppercase)).is_err()
        );
    }

    #[test]
    fn policy_changes_projection_identity_and_dense_ids_are_owner_checked() {
        let first_fixture = fixture(FixtureMode::Unique, false);
        let first = ResolutionProjection::build(Arc::clone(&first_fixture.graph), policy(b"first"))
            .unwrap();
        let second = ResolutionProjection::build(first_fixture.graph, policy(b"second")).unwrap();
        assert_ne!(first.id(), second.id());
        assert_eq!(
            second.result(first.results()[0].id()).unwrap_err(),
            ResolutionProjectionError::ForeignResult
        );
    }
}
