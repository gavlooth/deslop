use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use deslop_lang::CapabilityAuthority;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::resolution::{ResolutionEndpoint, ResolutionStatus};
use crate::{
    BuildContextId, FactCoverage, FactCoverageEvidence, ProjectionId, ScopeFactKey, ScopeFactKind,
    ScopeGraphProjection,
};

pub const SEMANTIC_RESOLUTION_FACT_SCHEMA: &str = "deslop.semantic-resolution-facts/1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SemanticArtifactId(String);

impl SemanticArtifactId {
    pub fn from_parts(parts: &[&[u8]]) -> Result<Self, SemanticResolutionFactError> {
        derive_key("deslop.semantic-artifact/1", "sa1_", parts).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SemanticArtifactId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_key(&value, "sa1_").map_err(D::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SemanticProviderKey(String);

impl SemanticProviderKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SemanticProviderKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_key(&value, "sp1_").map_err(D::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SemanticResolutionFactKey(String);

impl SemanticResolutionFactKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SemanticResolutionFactKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_key(&value, "srf1_").map_err(D::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticProviderKind {
    LanguageServer,
    Compiler,
}

impl SemanticProviderKind {
    pub const fn authority(self) -> CapabilityAuthority {
        match self {
            Self::LanguageServer => CapabilityAuthority::LanguageServer,
            Self::Compiler => CapabilityAuthority::Compiler,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticProviderDraft {
    pub kind: SemanticProviderKind,
    pub name: String,
    pub version: String,
    pub executable_artifact: SemanticArtifactId,
    pub configuration_artifact: SemanticArtifactId,
    pub project_model_artifact: Option<SemanticArtifactId>,
    pub project_model_coverage: FactCoverageEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticProvider {
    key: SemanticProviderKey,
    kind: SemanticProviderKind,
    name: String,
    version: String,
    executable_artifact: SemanticArtifactId,
    configuration_artifact: SemanticArtifactId,
    project_model_artifact: Option<SemanticArtifactId>,
    project_model_coverage: FactCoverageEvidence,
}

impl SemanticProvider {
    pub fn key(&self) -> &SemanticProviderKey {
        &self.key
    }

    pub fn kind(&self) -> SemanticProviderKind {
        self.kind
    }

    pub fn authority(&self) -> CapabilityAuthority {
        self.kind.authority()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn executable_artifact(&self) -> &SemanticArtifactId {
        &self.executable_artifact
    }

    pub fn configuration_artifact(&self) -> &SemanticArtifactId {
        &self.configuration_artifact
    }

    pub fn project_model_artifact(&self) -> Option<&SemanticArtifactId> {
        self.project_model_artifact.as_ref()
    }

    pub fn project_model_coverage(&self) -> &FactCoverageEvidence {
        &self.project_model_coverage
    }

    fn from_draft(draft: SemanticProviderDraft) -> Result<Self, SemanticResolutionFactError> {
        let mut provider = Self {
            key: SemanticProviderKey(String::new()),
            kind: draft.kind,
            name: draft.name,
            version: draft.version,
            executable_artifact: draft.executable_artifact,
            configuration_artifact: draft.configuration_artifact,
            project_model_artifact: draft.project_model_artifact,
            project_model_coverage: draft.project_model_coverage,
        };
        provider.key = SemanticProviderKey(derive_serialized_key(
            "deslop.semantic-provider/1",
            "sp1_",
            &SemanticProviderPayload::from(&provider),
        )?);
        provider.validate()?;
        Ok(provider)
    }

    pub(crate) fn validate(&self) -> Result<(), SemanticResolutionFactError> {
        validate_key(self.key.as_str(), "sp1_")?;
        validate_text("semantic provider name", &self.name)?;
        validate_text("semantic provider version", &self.version)?;
        validate_coverage(&self.project_model_coverage)?;
        if self.project_model_coverage.status == FactCoverage::Complete
            && self.project_model_artifact.is_none()
        {
            return Err(SemanticResolutionFactError::Invalid(
                "complete provider project-model coverage requires an exact artifact".into(),
            ));
        }
        let expected = derive_serialized_key(
            "deslop.semantic-provider/1",
            "sp1_",
            &SemanticProviderPayload::from(self),
        )?;
        if expected != self.key.0 {
            return Err(SemanticResolutionFactError::Invalid(
                "semantic provider key does not bind its complete payload".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct SemanticProviderPayload<'a> {
    kind: SemanticProviderKind,
    name: &'a str,
    version: &'a str,
    executable_artifact: &'a SemanticArtifactId,
    configuration_artifact: &'a SemanticArtifactId,
    project_model_artifact: &'a Option<SemanticArtifactId>,
    project_model_coverage: &'a FactCoverageEvidence,
}

impl<'a> From<&'a SemanticProvider> for SemanticProviderPayload<'a> {
    fn from(provider: &'a SemanticProvider) -> Self {
        Self {
            kind: provider.kind,
            name: &provider.name,
            version: &provider.version,
            executable_artifact: &provider.executable_artifact,
            configuration_artifact: &provider.configuration_artifact,
            project_model_artifact: &provider.project_model_artifact,
            project_model_coverage: &provider.project_model_coverage,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticResolutionFactDraft {
    pub provider: SemanticProviderKey,
    pub reference: ScopeFactKey,
    pub result_artifact: SemanticArtifactId,
    pub status: ResolutionStatus,
    pub endpoints: Vec<ResolutionEndpoint>,
    pub coverage: FactCoverageEvidence,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticResolutionFact {
    key: SemanticResolutionFactKey,
    provider: SemanticProviderKey,
    reference: ScopeFactKey,
    result_artifact: SemanticArtifactId,
    status: ResolutionStatus,
    endpoints: Vec<ResolutionEndpoint>,
    coverage: FactCoverageEvidence,
    diagnostics: Vec<String>,
}

impl SemanticResolutionFact {
    pub fn key(&self) -> &SemanticResolutionFactKey {
        &self.key
    }

    pub fn provider(&self) -> &SemanticProviderKey {
        &self.provider
    }

    pub fn reference(&self) -> &ScopeFactKey {
        &self.reference
    }

    pub fn result_artifact(&self) -> &SemanticArtifactId {
        &self.result_artifact
    }

    pub fn status(&self) -> ResolutionStatus {
        self.status
    }

    pub fn endpoints(&self) -> &[ResolutionEndpoint] {
        &self.endpoints
    }

    pub fn coverage(&self) -> &FactCoverageEvidence {
        &self.coverage
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    fn from_draft(draft: SemanticResolutionFactDraft) -> Result<Self, SemanticResolutionFactError> {
        let mut endpoints = draft.endpoints;
        endpoints.sort();
        let mut diagnostics = draft.diagnostics;
        diagnostics.sort();
        let mut fact = Self {
            key: SemanticResolutionFactKey(String::new()),
            provider: draft.provider,
            reference: draft.reference,
            result_artifact: draft.result_artifact,
            status: draft.status,
            endpoints,
            coverage: draft.coverage,
            diagnostics,
        };
        fact.key = SemanticResolutionFactKey(derive_serialized_key(
            SEMANTIC_RESOLUTION_FACT_SCHEMA,
            "srf1_",
            &SemanticResolutionFactPayload::from(&fact),
        )?);
        fact.validate()?;
        Ok(fact)
    }

    pub(crate) fn validate(&self) -> Result<(), SemanticResolutionFactError> {
        validate_key(self.key.as_str(), "srf1_")?;
        validate_coverage(&self.coverage)?;
        if self.endpoints.iter().collect::<BTreeSet<_>>().len() != self.endpoints.len() {
            return Err(SemanticResolutionFactError::Invalid(
                "semantic resolution fact contains duplicate endpoints".into(),
            ));
        }
        for endpoint in &self.endpoints {
            if let ResolutionEndpoint::External(symbol) = endpoint {
                validate_text("external semantic endpoint", symbol)?;
            }
        }
        for diagnostic in &self.diagnostics {
            validate_text("semantic resolution diagnostic", diagnostic)?;
        }
        if self.diagnostics.iter().collect::<BTreeSet<_>>().len() != self.diagnostics.len() {
            return Err(SemanticResolutionFactError::Invalid(
                "semantic resolution fact contains duplicate diagnostics".into(),
            ));
        }
        match (self.coverage.status, self.status, self.endpoints.len()) {
            (FactCoverage::Complete, ResolutionStatus::Unique, 1)
            | (FactCoverage::Complete, ResolutionStatus::Unresolved, 0) => {}
            (FactCoverage::Complete, ResolutionStatus::Ambiguous, count) if count > 1 => {}
            (FactCoverage::Complete, ResolutionStatus::Unknown | ResolutionStatus::Conflict, _) => {
                return Err(SemanticResolutionFactError::Invalid(
                    "complete provider fact cannot claim unknown or conflict".into(),
                ));
            }
            (FactCoverage::Complete, _, _) => {
                return Err(SemanticResolutionFactError::Invalid(
                    "terminal provider fact endpoint cardinality contradicts its status".into(),
                ));
            }
            (_, ResolutionStatus::Unknown, _) => {}
            (_, _, _) => {
                return Err(SemanticResolutionFactError::Invalid(
                    "incomplete provider fact must remain unknown".into(),
                ));
            }
        }
        let expected = derive_serialized_key(
            SEMANTIC_RESOLUTION_FACT_SCHEMA,
            "srf1_",
            &SemanticResolutionFactPayload::from(self),
        )?;
        if expected != self.key.0 {
            return Err(SemanticResolutionFactError::Invalid(
                "semantic resolution fact key does not bind its complete payload".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct SemanticResolutionFactPayload<'a> {
    provider: &'a SemanticProviderKey,
    reference: &'a ScopeFactKey,
    result_artifact: &'a SemanticArtifactId,
    status: ResolutionStatus,
    endpoints: &'a [ResolutionEndpoint],
    coverage: &'a FactCoverageEvidence,
    diagnostics: &'a [String],
}

impl<'a> From<&'a SemanticResolutionFact> for SemanticResolutionFactPayload<'a> {
    fn from(fact: &'a SemanticResolutionFact) -> Self {
        Self {
            provider: &fact.provider,
            reference: &fact.reference,
            result_artifact: &fact.result_artifact,
            status: fact.status,
            endpoints: &fact.endpoints,
            coverage: &fact.coverage,
            diagnostics: &fact.diagnostics,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticResolutionFactDocument {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    scope_graph_id: ProjectionId,
    build_context: BuildContextId,
    providers: Vec<SemanticProvider>,
    facts: Vec<SemanticResolutionFact>,
}

impl SemanticResolutionFactDocument {
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

    pub fn build_context(&self) -> &BuildContextId {
        &self.build_context
    }

    pub fn providers(&self) -> &[SemanticProvider] {
        &self.providers
    }

    pub fn facts(&self) -> &[SemanticResolutionFact] {
        &self.facts
    }

    pub(crate) fn validate(&self) -> Result<(), SemanticResolutionFactError> {
        if self.schema != SEMANTIC_RESOLUTION_FACT_SCHEMA {
            return Err(SemanticResolutionFactError::Invalid(format!(
                "unsupported semantic resolution fact schema {}",
                self.schema
            )));
        }
        validate_text("semantic fact analysis identity", &self.analysis_id)?;
        let mut providers = BTreeMap::new();
        for provider in &self.providers {
            provider.validate()?;
            if providers.insert(provider.key(), provider).is_some() {
                return Err(SemanticResolutionFactError::Invalid(
                    "semantic fact document contains duplicate providers".into(),
                ));
            }
        }
        let mut keys = BTreeSet::new();
        let mut queries = BTreeSet::new();
        for fact in &self.facts {
            fact.validate()?;
            if !providers.contains_key(fact.provider()) {
                return Err(SemanticResolutionFactError::Invalid(
                    "semantic fact references an absent provider".into(),
                ));
            }
            if !keys.insert(fact.key()) {
                return Err(SemanticResolutionFactError::Invalid(
                    "semantic fact document contains duplicate fact keys".into(),
                ));
            }
            if !queries.insert((fact.provider(), fact.reference())) {
                return Err(SemanticResolutionFactError::Invalid(
                    "semantic provider emitted duplicate facts for one reference".into(),
                ));
            }
            let provider = providers[fact.provider()];
            if fact.coverage.status == FactCoverage::Complete
                && provider.project_model_coverage.status != FactCoverage::Complete
            {
                return Err(SemanticResolutionFactError::Invalid(
                    "complete semantic fact requires complete provider project-model coverage"
                        .into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticResolutionFactDocumentWire {
    schema: String,
    projection_id: ProjectionId,
    analysis_id: String,
    scope_graph_id: ProjectionId,
    build_context: BuildContextId,
    providers: Vec<SemanticProvider>,
    facts: Vec<SemanticResolutionFact>,
}

impl<'de> Deserialize<'de> for SemanticResolutionFactDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SemanticResolutionFactDocumentWire::deserialize(deserializer)?;
        let document = Self {
            schema: wire.schema,
            projection_id: wire.projection_id,
            analysis_id: wire.analysis_id,
            scope_graph_id: wire.scope_graph_id,
            build_context: wire.build_context,
            providers: wire.providers,
            facts: wire.facts,
        };
        document.validate().map_err(D::Error::custom)?;
        Ok(document)
    }
}

#[derive(Debug, Clone)]
pub struct SemanticResolutionFacts {
    scope_graph: Arc<ScopeGraphProjection>,
    document: SemanticResolutionFactDocument,
}

impl SemanticResolutionFacts {
    pub fn empty(
        scope_graph: Arc<ScopeGraphProjection>,
    ) -> Result<Self, SemanticResolutionFactError> {
        SemanticResolutionFactBuilder::new(scope_graph).finish()
    }

    pub fn scope_graph(&self) -> &Arc<ScopeGraphProjection> {
        &self.scope_graph
    }

    pub fn document(&self) -> &SemanticResolutionFactDocument {
        &self.document
    }

    pub fn id(&self) -> &ProjectionId {
        self.document.projection_id()
    }

    pub fn providers(&self) -> &[SemanticProvider] {
        self.document.providers()
    }

    pub fn facts(&self) -> &[SemanticResolutionFact] {
        self.document.facts()
    }

    pub fn provider(&self, key: &SemanticProviderKey) -> Option<&SemanticProvider> {
        self.providers()
            .iter()
            .find(|provider| provider.key() == key)
    }

    pub fn facts_for_reference(
        &self,
        reference: &ScopeFactKey,
    ) -> impl Iterator<Item = &SemanticResolutionFact> {
        self.facts()
            .iter()
            .filter(move |fact| fact.reference() == reference)
    }

    pub(crate) fn validate_against(
        &self,
        scope_graph: &ScopeGraphProjection,
    ) -> Result<(), SemanticResolutionFactError> {
        self.document.validate()?;
        if self.document.analysis_id() != scope_graph.analysis().id().as_str()
            || self.document.scope_graph_id() != scope_graph.id()
            || self.document.build_context() != scope_graph.build_context()
        {
            return Err(SemanticResolutionFactError::Invalid(
                "semantic facts belong to another analysis, scope graph, or build context".into(),
            ));
        }
        let facts = scope_graph
            .facts()
            .iter()
            .map(|fact| (fact.key(), fact.data().kind()))
            .collect::<BTreeMap<_, _>>();
        for fact in self.facts() {
            if facts.get(fact.reference()) != Some(&ScopeFactKind::Reference) {
                return Err(SemanticResolutionFactError::Invalid(
                    "semantic fact reference is absent or not a reference fact".into(),
                ));
            }
            for endpoint in fact.endpoints() {
                validate_endpoint(endpoint, &facts)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct SemanticResolutionFactBuilder {
    scope_graph: Arc<ScopeGraphProjection>,
    providers: Vec<SemanticProvider>,
    facts: Vec<SemanticResolutionFact>,
}

impl SemanticResolutionFactBuilder {
    pub fn new(scope_graph: Arc<ScopeGraphProjection>) -> Self {
        Self {
            scope_graph,
            providers: Vec::new(),
            facts: Vec::new(),
        }
    }

    pub fn add_provider(
        &mut self,
        draft: SemanticProviderDraft,
    ) -> Result<SemanticProviderKey, SemanticResolutionFactError> {
        let provider = SemanticProvider::from_draft(draft)?;
        if self
            .providers
            .iter()
            .any(|existing| existing.key() == provider.key())
        {
            return Err(SemanticResolutionFactError::Invalid(
                "semantic provider was added twice".into(),
            ));
        }
        let key = provider.key().clone();
        self.providers.push(provider);
        Ok(key)
    }

    pub fn add_fact(
        &mut self,
        draft: SemanticResolutionFactDraft,
    ) -> Result<SemanticResolutionFactKey, SemanticResolutionFactError> {
        let provider = self
            .providers
            .iter()
            .find(|provider| provider.key() == &draft.provider)
            .ok_or_else(|| {
                SemanticResolutionFactError::Invalid(
                    "semantic fact references a provider not owned by this builder".into(),
                )
            })?;
        if draft.coverage.status == FactCoverage::Complete
            && provider.project_model_coverage.status != FactCoverage::Complete
        {
            return Err(SemanticResolutionFactError::Invalid(
                "complete semantic fact requires complete provider project-model coverage".into(),
            ));
        }
        let fact = SemanticResolutionFact::from_draft(draft)?;
        if self.facts.iter().any(|existing| {
            existing.provider() == fact.provider() && existing.reference() == fact.reference()
        }) {
            return Err(SemanticResolutionFactError::Invalid(
                "semantic provider already emitted a fact for this reference".into(),
            ));
        }
        let graph_facts = self
            .scope_graph
            .facts()
            .iter()
            .map(|record| (record.key(), record.data().kind()))
            .collect::<BTreeMap<_, _>>();
        if graph_facts.get(fact.reference()) != Some(&ScopeFactKind::Reference) {
            return Err(SemanticResolutionFactError::Invalid(
                "semantic fact reference is absent or not a reference fact".into(),
            ));
        }
        for endpoint in fact.endpoints() {
            validate_endpoint(endpoint, &graph_facts)?;
        }
        let key = fact.key().clone();
        self.facts.push(fact);
        Ok(key)
    }

    pub fn finish(mut self) -> Result<SemanticResolutionFacts, SemanticResolutionFactError> {
        self.providers
            .sort_by(|left, right| left.key().cmp(right.key()));
        self.facts
            .sort_by(|left, right| left.key().cmp(right.key()));
        let payload = serde_json::to_vec(&(&self.providers, &self.facts))
            .map_err(|error| SemanticResolutionFactError::Invalid(error.to_string()))?;
        let id = self
            .scope_graph
            .analysis()
            .derive_projection_id(
                SEMANTIC_RESOLUTION_FACT_SCHEMA,
                self.scope_graph.build_context().as_str().as_bytes(),
                &payload,
            )
            .map_err(|error| SemanticResolutionFactError::Invalid(error.to_string()))?;
        let document = SemanticResolutionFactDocument {
            schema: SEMANTIC_RESOLUTION_FACT_SCHEMA.into(),
            projection_id: id,
            analysis_id: self.scope_graph.analysis().id().as_str().into(),
            scope_graph_id: self.scope_graph.id().clone(),
            build_context: self.scope_graph.build_context().clone(),
            providers: self.providers,
            facts: self.facts,
        };
        document.validate()?;
        let facts = SemanticResolutionFacts {
            scope_graph: Arc::clone(&self.scope_graph),
            document,
        };
        facts.validate_against(&self.scope_graph)?;
        Ok(facts)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticResolutionFactError {
    Invalid(String),
}

impl fmt::Display for SemanticResolutionFactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => {
                write!(formatter, "invalid semantic resolution facts: {message}")
            }
        }
    }
}

impl Error for SemanticResolutionFactError {}

fn validate_endpoint(
    endpoint: &ResolutionEndpoint,
    facts: &BTreeMap<&ScopeFactKey, ScopeFactKind>,
) -> Result<(), SemanticResolutionFactError> {
    let require = |key: &ScopeFactKey, expected: ScopeFactKind| {
        if facts.get(key) == Some(&expected) {
            Ok(())
        } else {
            Err(SemanticResolutionFactError::Invalid(format!(
                "semantic endpoint {} is absent or not {expected:?}",
                key.as_str()
            )))
        }
    };
    match endpoint {
        ResolutionEndpoint::Declaration(key) => require(key, ScopeFactKind::Declaration),
        ResolutionEndpoint::Definition(key) => require(key, ScopeFactKind::Definition),
        ResolutionEndpoint::Module(key) => require(key, ScopeFactKind::BuildModule),
        ResolutionEndpoint::MergedDeclarations(keys) => {
            if keys.len() < 2 || keys.iter().collect::<BTreeSet<_>>().len() != keys.len() {
                return Err(SemanticResolutionFactError::Invalid(
                    "merged semantic endpoint requires distinct declarations".into(),
                ));
            }
            for key in keys {
                require(key, ScopeFactKind::Declaration)?;
            }
            Ok(())
        }
        ResolutionEndpoint::External(symbol) => validate_text("external semantic endpoint", symbol),
    }
}

fn validate_coverage(coverage: &FactCoverageEvidence) -> Result<(), SemanticResolutionFactError> {
    match (coverage.status, coverage.reason.as_deref()) {
        (FactCoverage::Complete, None) => Ok(()),
        (FactCoverage::Complete, Some(_)) => Err(SemanticResolutionFactError::Invalid(
            "complete semantic coverage cannot carry an incompleteness reason".into(),
        )),
        (_, Some(reason)) => validate_text("semantic coverage reason", reason),
        (_, None) => Err(SemanticResolutionFactError::Invalid(
            "incomplete semantic coverage requires an exact reason".into(),
        )),
    }
}

fn validate_text(label: &str, value: &str) -> Result<(), SemanticResolutionFactError> {
    if value.trim().is_empty() {
        return Err(SemanticResolutionFactError::Invalid(format!(
            "{label} must not be empty"
        )));
    }
    Ok(())
}

fn derive_serialized_key(
    domain: &str,
    prefix: &str,
    payload: &impl Serialize,
) -> Result<String, SemanticResolutionFactError> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|error| SemanticResolutionFactError::Invalid(error.to_string()))?;
    derive_key(domain, prefix, &[&bytes])
}

fn derive_key(
    domain: &str,
    prefix: &str,
    parts: &[&[u8]],
) -> Result<String, SemanticResolutionFactError> {
    validate_text("semantic identity domain", domain)?;
    let mut hasher = blake3::Hasher::new_derive_key(domain);
    for part in parts {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    Ok(format!("{prefix}{}", hasher.finalize().to_hex()))
}

fn validate_key(value: &str, prefix: &str) -> Result<(), SemanticResolutionFactError> {
    let Some(hex) = value.strip_prefix(prefix) else {
        return Err(SemanticResolutionFactError::Invalid(format!(
            "semantic identity must start with {prefix}"
        )));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(SemanticResolutionFactError::Invalid(
            "semantic identity must contain a lowercase 32-byte hexadecimal digest".into(),
        ));
    }
    Ok(())
}
