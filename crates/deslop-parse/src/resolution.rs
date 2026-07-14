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
    ScopeGraphProjection, TimingObservation, TraversalCandidate, VisibilityObservation,
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
    paths: Vec<ResolutionPath>,
    source_facts: Vec<ScopeFactKey>,
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

    pub fn paths(&self) -> &[ResolutionPath] {
        &self.paths
    }

    pub fn source_facts(&self) -> &[ScopeFactKey] {
        &self.source_facts
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
        }
        if self.coverage.status == FactCoverage::Complete
            && self
                .paths
                .iter()
                .any(|path| path.coverage.status != FactCoverage::Complete)
        {
            return Err(ResolutionProjectionError::Invalid(
                "complete resolution result contains an incomplete candidate path".into(),
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
        if self.authority != self.reference_evidence.authority {
            return Err(ResolutionProjectionError::Invalid(
                "resolution result authority differs from reference evidence".into(),
            ));
        }
        if self.coverage.status == FactCoverage::Complete && self.authority.is_none() {
            return Err(ResolutionProjectionError::Invalid(
                "complete resolution coverage requires explicit evidence authority".into(),
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
    paths: &'a [ResolutionPath],
    source_facts: &'a [ScopeFactKey],
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
            paths: &result.paths,
            source_facts: &result.source_facts,
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
        let mut references = BTreeSet::new();
        let mut keys = BTreeSet::new();
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
    resolution_policy: ResolutionPolicyId,
    results: Box<[ResolutionResultRecord]>,
    document: ResolutionDocument,
    owner: u64,
}

impl ResolutionProjection {
    pub fn build(
        scope_graph: Arc<ScopeGraphProjection>,
        resolution_policy: ResolutionPolicyId,
    ) -> Result<Self, ResolutionProjectionError> {
        let owner = NEXT_RESOLUTION_OWNER
            .fetch_update(AtomicOrdering::Relaxed, AtomicOrdering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| {
                ResolutionProjectionError::Invalid("resolution owner space exhausted".into())
            })?;
        let engine = ResolutionTraversalEngine::new(&scope_graph)?;
        let mut wires = Vec::new();
        for fact in scope_graph.facts() {
            if fact.data().kind() != ScopeFactKind::Reference {
                continue;
            }
            let traversal = engine.traverse_reference(fact.id())?;
            wires.push(build_result(&scope_graph, &traversal)?);
        }
        let mut result_identity = Vec::new();
        result_identity.extend_from_slice(scope_graph.id().as_str().as_bytes());
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
            resolution_policy,
            results,
            document,
            owner,
        })
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

fn build_result(
    graph: &ScopeGraphProjection,
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
        paths.push(import_path(
            graph,
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
        !traversal.deferred_imports().is_empty(),
        &paths,
    )?;
    let status = derive_status(coverage.status, &paths);
    let diagnostics = coverage.reasons.clone();
    ResolutionResult {
        key: ResolutionResultKey(String::new()),
        reference: reference.key().clone(),
        start_scope: start_scope.key().clone(),
        reference_evidence: reference.evidence().clone(),
        coverage,
        status,
        authority: reference.evidence().authority,
        paths,
        source_facts,
        dynamic_boundaries: dynamic_keys,
        diagnostics,
    }
    .finish()
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
        dynamic_boundaries: dynamic_boundaries.to_vec(),
        authorities: Vec::new(),
        coverage: ResolutionCoverageEvidence {
            status: FactCoverage::Complete,
            reasons: Vec::new(),
        },
    })
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
    let expected = derive_status(result.coverage.status, &result.paths);
    if result.status == ResolutionStatus::Conflict {
        if result.paths.iter().any(|path| {
            path.rejection_reasons
                .contains(&ResolutionRejectionReason::ProviderConflict)
        }) {
            return Ok(());
        }
        return Err(ResolutionProjectionError::Invalid(
            "conflict result has no provider-conflict path".into(),
        ));
    }
    if result.status != expected {
        return Err(ResolutionProjectionError::Invalid(format!(
            "resolution status {:?} contradicts coverage and viable endpoints; expected {:?}",
            result.status, expected
        )));
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
        BindingDraft, BindingForm, BindingTargetDraft, BuildContextId, DeclarationDraft,
        DeclarationModifier, DynamicBoundaryDraft, FactCoverageEvidence, ImportDraft, ImportForm,
        Mutability, NameNamespace, NamespacePolicy, ProjectAnalysis, ProjectSnapshotBuilder,
        ReferenceDraft, ReferenceRole, RepositoryId, ScopeDraft, ScopeFactPolicyId,
        ScopeGraphBuilder, ScopeKind, ShadowingDraft, VisibilityDraft, VisibilityKind,
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
        analysis
            .canonical_role_projection(Path::new("fixture.resolutionrs"))
            .unwrap()
            .facts()
            .iter()
            .find(|fact| fact.node() == node)
            .unwrap()
            .roles()
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
            FixtureMode::DeferredImport => "imported",
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
        if matches!(mode, FixtureMode::DeferredImport) {
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
    fn viable_explicit_shadowing_and_provider_conflict_are_not_order_fallbacks() {
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

        let mut conflict = result.clone();
        let index = conflict
            .paths
            .iter()
            .position(|path| path.viability == ResolutionPathViability::Viable)
            .unwrap();
        let mut path = conflict.paths[index].clone();
        path.viability = ResolutionPathViability::Rejected;
        path.rejection_reasons
            .push(ResolutionRejectionReason::ProviderConflict);
        path.checks.push(ResolutionCheck {
            kind: ResolutionCheckKind::AdapterIdentity,
            state: ResolutionCheckState::Rejected,
            detail: "authoritative providers disagree on the endpoint".into(),
            source_facts: vec![path.source_facts[0].clone()],
        });
        path.key = ResolutionPathKey(String::new());
        conflict.paths[index] = path.finish().unwrap();
        assert_eq!(
            derive_status(conflict.coverage.status, &conflict.paths),
            ResolutionStatus::Conflict
        );
        conflict.status = ResolutionStatus::Conflict;
        conflict.key = ResolutionResultKey(String::new());
        assert_eq!(
            conflict.finish().unwrap().status(),
            ResolutionStatus::Conflict
        );
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
