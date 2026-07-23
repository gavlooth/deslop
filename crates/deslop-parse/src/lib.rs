use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use deslop_core::{AnalysisDiagnostic, AnalysisProvenance, Lang, Span};
use deslop_lang::{LangPack, Registry, detect_lang};
use tree_sitter::{Parser, Tree};

pub use deslop_lang::{
    AdapterCapability, CANONICAL_ROLE_SCHEMA, CanonicalRole, CanonicalRoleSet, CapabilityAuthority,
    CapabilityDeclaration, CapabilitySupport, ConstructHandling, ConstructPolicyKind,
    ConstructPolicySection, ConstructRule, ControlAbruptForm, ControlEvaluationOrder,
    ControlExceptionalForm, ControlFlowAction, ControlFlowOwnerRule, ControlFlowOwnerRuleKind,
    ControlFlowRule, ControlFlowSyntaxSelector, ControlLoopForm, ControlSuspensionForm,
    DeclarationTimingRule, DialectDeclaration, DialectPolicy, DuplicateDefinitionRule,
    ExtractionFactKind, IdentifierCasePolicy, ImportTraversalRule,
    LANGUAGE_ADAPTER_CAPABILITY_SCHEMA, LANGUAGE_CONSTRUCT_POLICY_SCHEMA,
    LANGUAGE_CONTROL_FLOW_RULE_SCHEMA, LANGUAGE_LEXICAL_POLICY_SCHEMA, LANGUAGE_QUERY_PACK_SCHEMA,
    LANGUAGE_RESOLUTION_RULE_SCHEMA, LanguageAdapterCapabilityManifest, LanguageConstructPolicy,
    LanguageControlFlowRulePack, LanguageLexicalPolicy, LanguageQueryPack,
    LanguageResolutionRulePack, LexicalClassification, LexicalOperatorClass, LexicalRule,
    LexicalTokenClass, ModulePrerequisite, ParseRecoveryHandling, ParseRecoveryPolicy,
    PrecedenceDimension, PrecedenceDirection, PrecedenceTerm, QualificationRootRule,
    QueryCaptureDeclaration, QueryFamily, QueryFamilyDeclaration, RegionSpan,
    ResolutionInstruction, ResolutionRuleSection, ResolutionRuleSectionKind,
    ResolutionSyntaxSelector, RuleNamespace, RuleScopeKind, ScopeParentRule, SemanticTier,
};

// M1.3 owns the raw arena internally; M1.4 adds owner-validated public node views.
mod adapter;
mod aggregation;
mod architecture;
#[allow(dead_code)]
mod arena;
mod clone_candidate_index;
mod containment;
mod contract_history;
mod control_flow;
mod control_regions;
mod cycle_seam;
mod data_flow;
mod dependency;
mod graph_eligibility;
mod identity;
mod incremental;
mod instrumentation;
mod module_restructure;
mod non_structured_control;
mod planner;
mod program_dependence;
mod project_cache;
mod project_invalidation;
mod project_runtime;
mod project_session;
mod query;
mod resolution;
mod resolution_gate;
mod resolution_traversal;
mod scope_graph;
mod semantic_resolution;
mod snapshot;
mod subtree_fingerprint;
mod system_dependence;

pub use adapter::{
    CANONICAL_ROLE_PROJECTION_SCHEMA, CONSTRUCT_POLICY_PROJECTION_SCHEMA, CanonicalNodeRoles,
    CanonicalRoleProjection, CanonicalRoleProjectionError, ConstructPolicyFact,
    ConstructPolicyFactKind, ConstructPolicyProjection, ConstructPolicyProjectionError,
    DialectPolicyFact, LEXICAL_TOKEN_PROJECTION_SCHEMA, LexicalTokenFact, LexicalTokenProjection,
    LexicalTokenProjectionError, RawSyntaxFact, SyntaxAdapterFacts, SyntaxAdapterFactsError,
};
pub use aggregation::{
    InclusiveSyntaxPolicy, SyntaxAggregateLookupError, SyntaxAggregateOwner,
    SyntaxAggregateProjection, SyntaxAggregates, SyntaxAggregationError,
    SyntaxAggregationInstrumentation, SyntaxNodeAggregate,
};
pub use architecture::{
    ARCHITECTURE_POLICY_SCHEMA, ARCHITECTURE_SCHEMA, ArchitectureBuildError, ArchitectureComponent,
    ArchitectureComponentKey, ArchitectureCondensationEdge, ArchitectureCondensationEdgeKey,
    ArchitectureCoverageEvidence, ArchitectureDocument, ArchitectureGap, ArchitectureGapKey,
    ArchitectureGapKind, ArchitectureLayerAssignment, ArchitectureLevel, ArchitectureNodeClass,
    ArchitectureNodeMetrics, ArchitecturePolicy, ArchitecturePolicyId, ArchitectureProjection,
    ArchitectureRatio, ArchitectureRule, ArchitectureRuleDraft, ArchitectureRuleKey,
    ArchitectureRuleKind, ArchitectureViolation, ArchitectureViolationKey,
    ArchitectureViolationKind, derive_architecture,
};
pub use arena::{SourcePoint, SyntaxSpan};
pub use contract_history::{
    AdmissionGuardFacts, CONTRACT_CHANGE_HISTORY_SCHEMA, CONTRACT_SNAPSHOT_SCHEMA,
    ContractChangeHistory, ContractFunction, ContractHistoryBuildError, ContractSnapshot,
    FileContracts, RevisionContracts,
};
pub use control_flow::{
    CONTROL_FLOW_POLICY_SCHEMA, CONTROL_FLOW_SCHEMA, ControlAbruptKind, ControlBranchKind,
    ControlEdge, ControlEdgeDraft, ControlEdgeKey, ControlEdgeKind, ControlEdgePrecision,
    ControlExceptionalKind, ControlExitOutcome, ControlFlowBuildError, ControlFlowBuilder,
    ControlFlowCoverageEvidence, ControlFlowDocument, ControlFlowGraph, ControlFlowGraphDraft,
    ControlFlowGraphKey, ControlFlowLoweringGap, ControlFlowLoweringResult, ControlFlowOwnerKind,
    ControlFlowPolicyId, ControlFlowProjection, ControlLoopKind, ControlPoint, ControlPointDraft,
    ControlPointKey, ControlPointKind, ControlSuspensionKind, ControlSyntheticPointKind,
    lower_control_flow,
};
pub use control_regions::{
    CONTROL_REGION_POLICY_SCHEMA, CONTROL_REGION_SCHEMA, ControlPointRelations,
    ControlRegionBuildError, ControlRegionCoverageEvidence, ControlRegionDocument,
    ControlRegionGraph, ControlRegionGraphKey, ControlRegionKey, ControlRegionPointKey,
    ControlRegionPolicyId, ControlRegionProjection, ControlRegionResidual,
    ControlRegionResidualKey, StructuredControlRegion, StructuredControlRegionKind,
    derive_control_regions,
};
pub use cycle_seam::{
    CYCLE_SEAM_POLICY_SCHEMA, CYCLE_SEAM_SCHEMA, CycleSeamAction, CycleSeamBuildError,
    CycleSeamCandidate, CycleSeamCandidateKey, CycleSeamCost, CycleSeamCoverageEvidence,
    CycleSeamDisposition, CycleSeamDocument, CycleSeamGap, CycleSeamGapKey, CycleSeamGapKind,
    CycleSeamPolicyId, CycleSeamProjection, derive_cycle_seams,
};
pub use data_flow::{
    DATA_FLOW_POLICY_SCHEMA, DATA_FLOW_SCHEMA, DataFlowAccess, DataFlowAccessDraft,
    DataFlowAccessKey, DataFlowAccessKind, DataFlowBoundary, DataFlowBoundaryDraft,
    DataFlowBoundaryKey, DataFlowBoundaryKind, DataFlowBuildError, DataFlowBuilder,
    DataFlowCoverageEvidence, DataFlowDefinition, DataFlowDefinitionDraft, DataFlowDefinitionKey,
    DataFlowDocument, DataFlowEffect, DataFlowEffectDraft, DataFlowEffectKey, DataFlowEffectKind,
    DataFlowGraph, DataFlowGraphDraft, DataFlowGraphKey, DataFlowPointFacts, DataFlowPointKey,
    DataFlowPolicyId, DataFlowProjection, DataFlowSymbol, DataFlowSymbolKey,
};
pub use dependency::{
    DEPENDENCY_POLICY_SCHEMA, DEPENDENCY_SCHEMA, DependencyBuildError, DependencyCoverageEvidence,
    DependencyDocument, DependencyEdge, DependencyEdgeKey, DependencyEdgeKind, DependencyEvidence,
    DependencyGap, DependencyGapKey, DependencyGapKind, DependencyNode, DependencyNodeKey,
    DependencyNodeKind, DependencyPolicyId, DependencyProjection, derive_dependencies,
};
pub use graph_eligibility::{
    GRAPH_RECIPE_ELIGIBILITY_SCHEMA, GraphEligibilityBlock, GraphEligibilityDecision,
    GraphEligibilityDecisionId, GraphEligibilityError, GraphEvidenceLayer, GraphRecipeRequirement,
    evaluate_graph_recipe_eligibility, evaluate_program_graph_recipe_eligibility,
};
pub use identity::{
    NODE_BASELINE_SCHEMA, NODE_KEY_SCHEMA, NodeAnchor, NodeBaselineFingerprint, NodeId, NodeKey,
    NodeKeyLookupError, NodeLookupError,
};
pub use incremental::{
    FileAnalysisChange, FileAnalysisChangeKind, FileRebuildReason, FileSourceEdits,
    NodeExpiryReason, NodeReanchor, NodeReanchorEvidence, ProjectAnalysisUpdate,
    ProjectAnalysisUpdateError, SourceEdit, SourceEditEvidence, SourceReplacement,
};
pub use instrumentation::{
    AnalysisMemoryInstrumentation, AnalysisStructureInstrumentation, ParseOwnershipInstrumentation,
    ProjectAnalysisInstrumentation, ProjectAnalysisUpdateInstrumentation,
    SyntaxPointContextInstrumentation, SyntaxQueryInstrumentation,
    SyntaxQueryResultsInstrumentation,
};
pub use module_restructure::{
    MODULE_CHANGE_HISTORY_SCHEMA, MODULE_RESTRUCTURE_POLICY_SCHEMA, MODULE_RESTRUCTURE_SCHEMA,
    ModuleChangeHistory, ModuleChangeHistoryId, ModuleChangeObservation,
    ModuleChangeObservationDraft, ModuleChangeObservationKey, ModuleHistoryStatus, ModuleProfile,
    ModuleProfileKey, ModuleRatio, ModuleRestructureBuildError, ModuleRestructureCandidate,
    ModuleRestructureCandidateKey, ModuleRestructureCoverageEvidence, ModuleRestructureDisposition,
    ModuleRestructureDocument, ModuleRestructureGap, ModuleRestructureGapKey,
    ModuleRestructureGapKind, ModuleRestructureKind, ModuleRestructurePolicyId,
    ModuleRestructureProjection, ModuleRestructureScore, derive_module_restructure,
};
pub use non_structured_control::{
    NON_STRUCTURED_CONTROL_POLICY_SCHEMA, NON_STRUCTURED_CONTROL_SCHEMA,
    NonStructuredControlBuildError, NonStructuredControlClassification,
    NonStructuredControlCoverageEvidence, NonStructuredControlDocument, NonStructuredControlFact,
    NonStructuredControlFactKey, NonStructuredControlFactSource, NonStructuredControlGraph,
    NonStructuredControlGraphKey, NonStructuredControlPolicyId, NonStructuredControlProjection,
    derive_non_structured_control_regions,
};
pub use planner::{
    ProjectSnapshotPlanner, ProjectSnapshotRequest, RepositorySpec, RootSpec, SnapshotBuild,
    SnapshotPresentationMap,
};
pub use program_dependence::{
    PROGRAM_DEPENDENCE_POLICY_SCHEMA, PROGRAM_DEPENDENCE_SCHEMA, ProgramDependenceBuildError,
    ProgramDependenceCoverageEvidence, ProgramDependenceDocument, ProgramDependenceEdge,
    ProgramDependenceEdgeKey, ProgramDependenceEdgeKind, ProgramDependenceGap,
    ProgramDependenceGapKey, ProgramDependenceGapKind, ProgramDependenceGraph,
    ProgramDependenceGraphKey, ProgramDependenceNode, ProgramDependenceNodeKey,
    ProgramDependencePolicyId, ProgramDependenceProjection, derive_program_dependence,
};
pub use project_cache::{
    ARTIFACT_CACHE_KEY_SCHEMA, ARTIFACT_CACHE_RECORD_SCHEMA, ArtifactCacheKey, ArtifactCacheKeyId,
    ArtifactKind, CacheLookup, CacheSemanticVersions, CacheStatistics, PersistentArtifactCache,
    ProjectCacheError,
};
pub use project_invalidation::{
    INVALIDATION_PLAN_SCHEMA, InvalidationDependencyEvidence, InvalidationReason,
    InvalidationScope, ProjectInvalidationPlan, ProjectionDependencyIndex, ProjectionInvalidation,
    ProjectionKind,
};
pub use project_runtime::{
    ANALYSIS_BUDGET_SCHEMA, AnalysisBudget, AnalysisBudgetError, AnalysisContinuation,
    AnalysisWorkCost, BudgetExhaustionReason, BudgetStatus, BudgetedAnalysis,
    DETERMINISTIC_COMMIT_SCHEMA, DeterministicCommitBatch, DeterministicCommitEntry,
    DeterministicRegionExecutor, RegionExecutionError, RegionWorkItem,
};
pub use project_session::{
    PROJECT_SESSION_MANIFEST_SCHEMA, ProjectSessionCapture, ProjectSessionError, ProjectSessionId,
    ProjectSessionStore, RestoredProjectSession, capture_snapshot_from_environment,
    project_file_semantic_versions, project_session_semantic_versions,
};
pub use query::{
    CompiledQueryFamily, LANGUAGE_QUERY_PROJECTION_SCHEMA, LanguageQueryProjection,
    LanguageQueryProjectionError, OwnedSyntaxCapture, OwnedSyntaxMatch, SyntaxCaptureQuantifier,
    SyntaxQuery, SyntaxQueryCompileErrorKind, SyntaxQueryError, SyntaxQueryId, SyntaxQueryPattern,
    SyntaxQueryPredicate, SyntaxQueryPredicateArgument, SyntaxQueryProperty,
    SyntaxQueryPropertyPredicate,
};
pub use resolution::{
    PreferredResolutionConclusion, RESOLUTION_POLICY_SCHEMA, RESOLUTION_SCHEMA, ResolutionCheck,
    ResolutionCheckKind, ResolutionCheckState, ResolutionConclusion, ResolutionConclusionSource,
    ResolutionCoverageEvidence, ResolutionDocument, ResolutionEndpoint, ResolutionInvalidation,
    ResolutionInvalidationReason, ResolutionPath, ResolutionPathEdge, ResolutionPathEdgeKind,
    ResolutionPathKey, ResolutionPathViability, ResolutionPolicyId, ResolutionPrecedenceComponent,
    ResolutionProjection, ResolutionProjectionError, ResolutionProjectionUpdate,
    ResolutionRejectionReason, ResolutionResult, ResolutionResultId, ResolutionResultKey,
    ResolutionResultRecord, ResolutionStatus,
};
pub use resolution_gate::{
    RESOLUTION_CONSUMER_GATE_SCHEMA, ResolutionCapabilityRequirement,
    ResolutionConsumerRequirement, ResolutionDependencyEvidence, ResolutionEligibilityBlock,
    ResolutionEligibilityDecision, ResolutionGateError, evaluate_unique_binding,
};
pub use resolution_traversal::{
    DeferredImportTraversal, DynamicBoundaryTraversal, ExplicitShadowing, LexicalScopeStep,
    NamespaceReachability, PrecedenceComponent, ResolutionTraversal, ResolutionTraversalEngine,
    ResolutionTraversalError, RuleSectionGap, TimingObservation, TraversalCandidate,
    VisibilityObservation,
};
pub use scope_graph::{
    BUILD_CONTEXT_SCHEMA, BindingDraft, BindingForm, BindingTarget, BindingTargetDraft,
    BindingTiming, BuildContextId, BuildModuleDraft, DeclarationDraft, DeclarationModifier,
    DefinitionDraft, DynamicBoundaryDraft, ExportDraft, FactCoverage, FactCoverageEvidence,
    ImportDraft, ImportForm, Mutability, NameNamespace, NamespacePolicy, ReferenceDraft,
    ReferenceRole, SCOPE_FACT_POLICY_SCHEMA, SCOPE_GRAPH_SCHEMA, ScopeDraft, ScopeFactData,
    ScopeFactEvidence, ScopeFactId, ScopeFactKey, ScopeFactKind, ScopeFactPolicyId,
    ScopeFactRecord, ScopeFactWire, ScopeGraphBuildError, ScopeGraphBuilder, ScopeGraphDocument,
    ScopeGraphProjection, ScopeKind, ShadowingDraft, SymbolKind, Visibility, VisibilityDraft,
    VisibilityKind,
};
pub use semantic_resolution::{
    SEMANTIC_RESOLUTION_FACT_SCHEMA, SemanticArtifactId, SemanticProvider, SemanticProviderDraft,
    SemanticProviderKey, SemanticProviderKind, SemanticResolutionFact,
    SemanticResolutionFactBuilder, SemanticResolutionFactDocument, SemanticResolutionFactDraft,
    SemanticResolutionFactError, SemanticResolutionFactKey, SemanticResolutionFacts,
};

pub use clone_candidate_index::{
    AbstractionReadiness, CLONE_CANDIDATE_INDEX_SCHEMA, CLONE_CLASS_SCHEMA,
    CLONE_GRAPH_CONTEXT_SCHEMA, CLONE_REPETITION_CLASSIFICATION_SCHEMA, CloneCandidateEntry,
    CloneCandidateEntryId, CloneCandidateIndex, CloneCandidateIndexError, CloneCandidateIndexId,
    CloneClass, CloneClassId, CloneGraphContext, CloneGraphContextId, CloneMatchKind,
    CloneMemberRepetitionEvidence, CloneMemberRepetitionRole, ClonePairVerification,
    CloneRepetitionClassification, CloneRepetitionClassificationId, CloneRepetitionKind,
    classify_clone_repetition,
};
pub use snapshot::{
    DiscoveryPolicy, ExactZeroWidthNodes, ExclusiveSyntaxKind, ExclusiveSyntaxLookupError,
    ExclusiveSyntaxOwner, ExclusiveSyntaxRegion, ExclusiveSyntaxRegions, FileParseCount,
    FileRevisionKey, GrammarSelection, LanguageAdapterIdentity, NodeChildren,
    NodeExclusiveSyntaxRegions, NodeIds, NodeRangeLookupError, NodeView, ParseLedger, ParsedFile,
    ProjectAnalysis, ProjectAnalysisId, ProjectSnapshot, ProjectSnapshotBuilder, ProjectSnapshotId,
    ProjectionId, RepositoryId, ScopeEntry, ScopeEntryKind, ScopeSpec, SnapshotEntry,
    SnapshotEntryKind, SourceRevision, SourceStore, StoredSource, SyntaxOwner, SyntaxPointContext,
};
pub use subtree_fingerprint::{
    ExactSubtreeFingerprint, IdentifierSurface, NormalizedSubtreeFingerprint,
    PublicApiNormalization, RenamedIdentifierEvidence, RenamedTokenEvidence,
    SUBTREE_FINGERPRINT_POLICY_SCHEMA, SUBTREE_FINGERPRINT_SCHEMA, SubtreeFingerprint,
    SubtreeFingerprintError, SubtreeFingerprintPolicy, SubtreeFingerprintPolicyId,
    derive_subtree_fingerprint,
};
pub use system_dependence::{
    CallSite, CallSiteDraft, CallSiteKey, CallableSummary, CallableSummaryDraft,
    CallableSummaryKey, GlobalSummary, GlobalSummaryDraft, GlobalSummaryKey, OutputBinding,
    OutputBindingDraft, ParameterBinding, ParameterBindingDraft, SYSTEM_DEPENDENCE_POLICY_SCHEMA,
    SYSTEM_DEPENDENCE_SCHEMA, SystemDependenceBuildError, SystemDependenceBuilder,
    SystemDependenceCapabilityEvidence, SystemDependenceCoverageEvidence, SystemDependenceDocument,
    SystemDependenceEdge, SystemDependenceEdgeKey, SystemDependenceEdgeKind,
    SystemDependenceEndpoint, SystemDependenceGap, SystemDependenceGapKey, SystemDependenceGapKind,
    SystemDependencePolicyId, SystemDependenceProjection,
};

thread_local! {
    static PARSE_SOURCE_INVOCATIONS: Cell<usize> = const { Cell::new(0) };
}

/// Reset the current thread's source-parse invocation counter.
///
/// This is public instrumentation for algorithm regression tests and future one-parse ownership work.
#[doc(hidden)]
pub fn reset_parse_source_invocations() {
    PARSE_SOURCE_INVOCATIONS.set(0);
}

/// Return the current thread's source-parse invocation count since the last reset.
#[doc(hidden)]
pub fn parse_source_invocations() -> usize {
    PARSE_SOURCE_INVOCATIONS.get()
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub lang: Lang,
    pub text: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    pub fn read(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Ok(Self::new(path.to_path_buf(), text))
    }

    pub fn new(path: PathBuf, text: String) -> Self {
        let lang = detect_lang(&path);
        Self::new_with_lang(path, text, lang)
    }

    pub fn new_with_lang(path: PathBuf, text: String, lang: Lang) -> Self {
        let line_starts = line_starts(&text);
        Self {
            path,
            lang,
            text,
            line_starts,
        }
    }

    pub fn lines(&self) -> Vec<&str> {
        self.text.lines().collect()
    }

    pub fn line_start_byte(&self, one_based_line: usize) -> usize {
        self.line_starts
            .get(one_based_line.saturating_sub(1))
            .copied()
            .unwrap_or(self.text.len())
    }

    pub fn line_end_byte(&self, one_based_line: usize) -> usize {
        self.line_starts
            .get(one_based_line)
            .copied()
            .map(|idx| idx.saturating_sub(1))
            .unwrap_or(self.text.len())
    }

    pub fn line_text(&self, one_based_line: usize) -> &str {
        let start = self.line_start_byte(one_based_line);
        let end = self.line_end_byte(one_based_line);
        self.text.get(start..end).unwrap_or("")
    }

    pub fn region_text(&self, start_line: usize, end_line: usize) -> String {
        let start = self.line_start_byte(start_line);
        let end = self
            .line_starts
            .get(end_line)
            .copied()
            .unwrap_or(self.text.len());
        self.text.get(start..end).unwrap_or("").to_string()
    }

    pub fn line_for_byte(&self, byte: usize) -> usize {
        match self.line_starts.binary_search(&byte) {
            Ok(idx) => idx + 1,
            Err(idx) => idx,
        }
        .max(1)
    }

    pub fn enclosing_region_for_span(&self, start_line: usize, end_line: usize) -> RegionSpan {
        let start_byte = self.line_start_byte(start_line);
        let end_byte = self.line_end_byte(end_line).max(start_byte);
        enclosing_region_for_source(self, start_byte, end_byte).unwrap_or(RegionSpan {
            start_line,
            end_line,
            start_byte,
            end_byte,
        })
    }
}

pub fn parse_tree(lang: Lang, text: &str) -> Result<Option<Tree>> {
    let registry = Registry::default();
    let pack = registry.pack_for_lang(lang);
    let Some(mut parser) = parser_for_pack(pack, None)? else {
        return Ok(None);
    };
    Ok(parser.parse(text, None))
}

pub fn parse_source(source: &SourceFile) -> Result<Option<Tree>> {
    let registry = Registry::default();
    let pack = registry.pack_for_path(&source.path);
    let Some(mut parser) = parser_for_pack(pack, Some(&source.path))? else {
        return Ok(None);
    };
    PARSE_SOURCE_INVOCATIONS.with(|count| count.set(count.get() + 1));
    Ok(parser.parse(&source.text, None))
}

pub fn has_tree_sitter_errors(lang: Lang, text: &str) -> Result<Option<bool>> {
    let Some(tree) = parse_tree(lang, text)? else {
        return Ok(None);
    };
    Ok(Some(tree.root_node().has_error()))
}

pub fn parses_without_errors(lang: Lang, text: &str) -> Result<Option<bool>> {
    Ok(has_tree_sitter_errors(lang, text)?.map(|has_errors| !has_errors))
}

pub fn source_parses_without_errors(source: &SourceFile) -> Result<Option<bool>> {
    Ok(parse_source(source)?.map(|tree| !tree.root_node().has_error()))
}

pub fn analysis_provenance(source: &SourceFile) -> Result<AnalysisProvenance> {
    let registry = Registry::default();
    let pack = registry.pack_for_path(&source.path);
    if pack.grammar_for_path(&source.path).is_none() {
        return Ok(AnalysisProvenance::unsupported(vec![AnalysisDiagnostic {
            code: "parser-unavailable".to_string(),
            message: "no tree-sitter grammar is available; syntax-backed analysis is partial"
                .to_string(),
            span: None,
        }]));
    }
    let Some(tree) = parse_source(source)? else {
        return Ok(AnalysisProvenance::failed(vec![AnalysisDiagnostic {
            code: "parser-no-tree".to_string(),
            message: "tree-sitter returned no syntax tree; analysis failed".to_string(),
            span: None,
        }]));
    };
    Ok(analysis_provenance_for_tree(&tree))
}

pub fn analysis_provenance_or_failed(source: &SourceFile) -> AnalysisProvenance {
    analysis_provenance(source).unwrap_or_else(|error| {
        AnalysisProvenance::failed(vec![AnalysisDiagnostic {
            code: "parser-failure".to_string(),
            message: format!("tree-sitter analysis failed: {error}; rewrite authority is denied"),
            span: None,
        }])
    })
}

pub fn analysis_provenance_for_tree(tree: &Tree) -> AnalysisProvenance {
    if !tree.root_node().has_error() {
        return AnalysisProvenance::complete();
    }
    let mut diagnostics = Vec::new();
    collect_parse_diagnostics(tree.root_node(), &mut diagnostics);
    if diagnostics.is_empty() {
        diagnostics.push(AnalysisDiagnostic {
            code: "tree-sitter-error".to_string(),
            message: "tree-sitter reported syntax recovery; syntax-backed analysis is partial"
                .to_string(),
            span: None,
        });
    }
    AnalysisProvenance::partial(diagnostics)
}

fn collect_parse_diagnostics(
    node: tree_sitter::Node<'_>,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
) {
    if node.is_error() || node.is_missing() {
        let (code, message) = if node.is_missing() {
            (
                "tree-sitter-missing-node",
                format!(
                    "tree-sitter inserted missing `{}` syntax; syntax-backed analysis is partial",
                    node.kind()
                ),
            )
        } else {
            (
                "tree-sitter-error",
                format!(
                    "tree-sitter emitted `{}` recovery syntax; syntax-backed analysis is partial",
                    node.kind()
                ),
            )
        };
        diagnostics.push(AnalysisDiagnostic {
            code: code.to_string(),
            message,
            span: Some(Span::new(
                node.start_position().row + 1,
                node.end_position().row + 1,
                node.start_byte(),
                node.end_byte(),
            )),
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_parse_diagnostics(child, diagnostics);
    }
}

pub fn enclosing_region(
    lang: Lang,
    text: &str,
    start_byte: usize,
    end_byte: usize,
) -> Option<RegionSpan> {
    let registry = Registry::default();
    let pack = registry.pack_for_lang(lang);
    let tree = parse_tree(pack.lang(), text).ok().flatten()?;
    if tree.root_node().has_error() {
        return None;
    }
    let root = tree.root_node();
    let end_byte = end_byte.max(start_byte).min(text.len());
    let node = root.descendant_for_byte_range(start_byte, end_byte)?;
    pack.enclosing_region(node, text)
}

fn enclosing_region_for_source(
    source: &SourceFile,
    start_byte: usize,
    end_byte: usize,
) -> Option<RegionSpan> {
    let registry = Registry::default();
    let pack = registry.pack_for_path(&source.path);
    let tree = parse_source(source).ok().flatten()?;
    if tree.root_node().has_error() {
        return None;
    }
    let root = tree.root_node();
    let end_byte = end_byte.max(start_byte).min(source.text.len());
    let node = root.descendant_for_byte_range(start_byte, end_byte)?;
    pack.enclosing_region(node, &source.text)
}

pub fn is_supported_source(path: &Path) -> bool {
    deslop_lang::is_supported_source(path)
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut out = vec![0];
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            out.push(idx + 1);
        }
    }
    out
}

fn parser_for_pack(pack: &dyn LangPack, path: Option<&Path>) -> Result<Option<Parser>> {
    let language = path.map_or_else(|| pack.grammar(), |path| pack.grammar_for_path(path));
    let Some(language) = language else {
        return Ok(None);
    };
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .with_context(|| format!("failed to load {} tree-sitter grammar", pack.name()))?;
    Ok(Some(parser))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TYPED_TYPESCRIPT: &str = include_str!("../../../tests/fixtures/typescript/typed.ts");
    const TYPED_TSX: &str = include_str!("../../../tests/fixtures/typescript/component.tsx");
    const JSX: &str = include_str!("../../../tests/fixtures/typescript/component.jsx");
    const MALFORMED_TYPESCRIPT: &str =
        include_str!("../../../tests/fixtures/typescript/malformed.ts");
    const MALFORMED_TSX: &str = include_str!("../../../tests/fixtures/typescript/malformed.tsx");
    const PYTHON_BEHAVIORAL: &str = include_str!("../../../tests/fixtures/python/behavioral.py");
    const CLOJURE_CONTROL_EDGES: &str =
        include_str!("../../../tests/fixtures/clojure/control_edges.clj");

    #[test]
    fn public_surface_has_no_borrowed_tree_sitter_handle() {
        for (name, source) in [
            ("lib", include_str!("lib.rs")),
            ("aggregation", include_str!("aggregation.rs")),
            ("identity", include_str!("identity.rs")),
            ("incremental", include_str!("incremental.rs")),
            ("instrumentation", include_str!("instrumentation.rs")),
            ("query", include_str!("query.rs")),
            ("scope_graph", include_str!("scope_graph.rs")),
            ("snapshot", include_str!("snapshot.rs")),
        ] {
            let mut public_header = String::new();
            for line in source.lines() {
                let trimmed = line.trim_start();
                if public_header.is_empty() && !trimmed.starts_with("pub ") {
                    continue;
                }
                public_header.push_str(trimmed);
                public_header.push(' ');
                if trimmed.contains('{') || trimmed.ends_with(';') {
                    for forbidden in [
                        "tree_sitter::Node",
                        "tree_sitter::TreeCursor",
                        "tree_sitter::QueryCursor",
                        "Node<'",
                        "TreeCursor<'",
                        "QueryCursor<'",
                    ] {
                        assert!(
                            !public_header.contains(forbidden),
                            "{name} exposes borrowed Tree-sitter handle {forbidden}: {public_header}"
                        );
                    }
                    public_header.clear();
                }
            }
        }

        let identity = include_str!("identity.rs");
        let node_id = &identity[identity
            .find("#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\npub struct NodeId")
            .unwrap()
            ..identity.find("impl fmt::Debug for NodeId").unwrap()];
        assert!(!node_id.contains("Serialize"));
        assert!(!node_id.contains("Deserialize"));
    }

    #[test]
    fn extracts_clojure_top_level_list_region() {
        let source = SourceFile::new(
            PathBuf::from("sample.clj"),
            "(ns sample)\n\n(defn f [xs]\n  (when xs\n    (= (count xs) 0)))\n\n(defn g [] true)\n"
                .into(),
        );
        assert_enclosing_region(&source, 5, 3, 5, "defn f");
    }

    #[test]
    fn parses_clojure_reader_and_macro_edge_fixture() {
        let source = SourceFile::new(
            PathBuf::from("control_edges.clj"),
            CLOJURE_CONTROL_EDGES.to_string(),
        );
        let tree = parse_source(&source)
            .expect("Clojure parse")
            .expect("Clojure tree");

        assert!(!tree.root_node().has_error());
        for (kind, expected) in [
            ("quoting_lit", 1),
            ("dis_expr", 1),
            ("syn_quoting_lit", 1),
            ("unquoting_lit", 1),
            ("unquote_splicing_lit", 1),
        ] {
            assert_eq!(tree_kind_count(tree.root_node(), kind), expected, "{kind}");
        }
        assert_enclosing_region(&source, 19, 17, 23, "defn quoted-and-discarded");
        assert_enclosing_region(&source, 28, 25, 29, "defn consume");
    }

    #[test]
    fn extracts_julia_function_region() {
        let source = SourceFile::new(
            PathBuf::from("sample.jl"),
            "module Demo\n\nfunction f(xs)\n    length(xs) == 0\nend\n\nstruct Box\n    x\nend\nend\n"
                .into(),
        );
        assert_enclosing_region(&source, 4, 3, 5, "function f");
    }

    #[test]
    fn extracts_rust_function_region() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "mod demo {\n    fn f(xs: Vec<i32>) -> usize {\n        return xs.len();\n    }\n}\n"
                .into(),
        );
        assert_enclosing_region(&source, 3, 2, 4, "fn f");
    }

    #[test]
    fn python_regions_include_decorators_and_prefer_nested_functions() {
        let source = SourceFile::new(
            PathBuf::from("behavioral.py"),
            PYTHON_BEHAVIORAL.to_string(),
        );
        let tree = parse_source(&source)
            .expect("Python parse")
            .expect("Python tree");

        assert!(!tree.root_node().has_error());
        assert_eq!(tree_kind_count(tree.root_node(), "decorated_definition"), 2);
        assert_eq!(tree_kind_count(tree.root_node(), "function_definition"), 4);
        assert_eq!(tree_kind_count(tree.root_node(), "async"), 2);
        assert_enclosing_region(&source, 7, 5, 7, "@wraps(function)");
        assert_enclosing_region(&source, 14, 13, 18, "@traced");
        assert_enclosing_region(&source, 16, 15, 16, "def normalize");
        assert_enclosing_region(&source, 12, 12, 18, "class Service");
        assert_eq!(
            source.enclosing_region_for_span(7, 7),
            RegionSpan {
                start_line: 5,
                end_line: 7,
                start_byte: 56,
                end_byte: 159,
            }
        );
        assert_eq!(
            source.enclosing_region_for_span(14, 14),
            RegionSpan {
                start_line: 13,
                end_line: 18,
                start_byte: 201,
                end_byte: 363,
            }
        );
        assert_eq!(
            source.enclosing_region_for_span(16, 16),
            RegionSpan {
                start_line: 15,
                end_line: 16,
                start_byte: 254,
                end_byte: 308,
            }
        );
    }

    #[test]
    fn selects_javascript_typescript_and_tsx_grammars_by_dialect() {
        let jsx = "const view = <div>{value}</div>;\n";
        let typed = "const value: number = 1;\n";
        for extension in ["js", "jsx"] {
            let source = SourceFile::new(
                PathBuf::from(format!("sample.{extension}")),
                jsx.to_string(),
            );
            assert_eq!(source.lang, Lang::JavaScript);
            assert_eq!(source_parses_without_errors(&source).unwrap(), Some(true));
            let tree = parse_source(&source).unwrap().expect("JavaScript tree");
            assert!(tree_has_kind(tree.root_node(), "jsx_element"));
        }
        for extension in ["ts", "mts", "cts"] {
            let source = SourceFile::new(
                PathBuf::from(format!("sample.{extension}")),
                typed.to_string(),
            );
            assert_eq!(source.lang, Lang::TypeScript);
            assert_eq!(source_parses_without_errors(&source).unwrap(), Some(true));
            let tree = parse_source(&source).unwrap().expect("TypeScript tree");
            assert!(tree_has_kind(tree.root_node(), "type_annotation"));
        }
        let tsx = SourceFile::new(
            PathBuf::from("sample.tsx"),
            "const view: JSX.Element = <div>{value}</div>;\n".into(),
        );

        assert_eq!(tsx.lang, Lang::TypeScript);
        assert_eq!(source_parses_without_errors(&tsx).unwrap(), Some(true));
        let tsx_tree = parse_source(&tsx).unwrap().expect("TSX tree");
        assert!(tree_has_kind(tsx_tree.root_node(), "type_annotation"));
        assert!(tree_has_kind(tsx_tree.root_node(), "jsx_element"));
        assert_eq!(
            parses_without_errors(Lang::JavaScript, typed).unwrap(),
            Some(false),
            "the JavaScript grammar must not silently accept typed syntax"
        );
        assert_eq!(
            parses_without_errors(Lang::TypeScript, &tsx.text).unwrap(),
            Some(false),
            "the TypeScript grammar must not silently accept TSX syntax"
        );
    }

    #[test]
    fn parses_typed_typescript_and_tsx_construct_matrix() {
        let typescript = SourceFile::new(PathBuf::from("typed.ts"), TYPED_TYPESCRIPT.to_string());
        let tsx = SourceFile::new(PathBuf::from("component.tsx"), TYPED_TSX.to_string());
        let jsx = SourceFile::new(PathBuf::from("component.jsx"), JSX.to_string());

        let typescript_tree = parse_source(&typescript)
            .expect("TypeScript parse")
            .expect("TypeScript tree");
        assert!(!typescript_tree.root_node().has_error());
        for kind in [
            "interface_declaration",
            "type_alias_declaration",
            "function_signature",
            "function_declaration",
            "class_declaration",
            "decorator",
            "satisfies_expression",
            "internal_module",
        ] {
            assert!(
                tree_has_kind(typescript_tree.root_node(), kind),
                "missing {kind}"
            );
        }

        let tsx_tree = parse_source(&tsx).expect("TSX parse").expect("TSX tree");
        assert!(!tsx_tree.root_node().has_error());
        for kind in [
            "interface_declaration",
            "type_alias_declaration",
            "type_parameters",
            "arrow_function",
            "function_declaration",
            "jsx_element",
            "jsx_expression",
            "spread_element",
            "member_expression",
            "type_arguments",
        ] {
            assert!(tree_has_kind(tsx_tree.root_node(), kind), "missing {kind}");
        }

        let jsx_tree = parse_source(&jsx).expect("JSX parse").expect("JSX tree");
        assert_eq!(jsx.lang, Lang::JavaScript);
        assert!(!jsx_tree.root_node().has_error());
        for kind in [
            "function_declaration",
            "jsx_element",
            "jsx_self_closing_element",
            "jsx_expression",
            "spread_element",
            "member_expression",
        ] {
            assert!(tree_has_kind(jsx_tree.root_node(), kind), "missing {kind}");
        }
    }

    #[test]
    fn typed_typescript_and_tsx_regions_use_behavioral_boundaries() {
        let typescript = SourceFile::new(PathBuf::from("typed.ts"), TYPED_TYPESCRIPT.to_string());
        let tsx = SourceFile::new(PathBuf::from("component.tsx"), TYPED_TSX.to_string());
        let jsx = SourceFile::new(PathBuf::from("component.jsx"), JSX.to_string());

        assert_enclosing_region(&typescript, 22, 21, 24, "add(item: T)");
        assert_enclosing_region(&tsx, 14, 11, 21, "function View");
        assert_enclosing_region(&jsx, 6, 1, 10, "function JsxView");
    }

    #[test]
    fn malformed_typescript_and_tsx_report_explicit_error_nodes() {
        for (path, text, expected_span) in [
            (
                "malformed.ts",
                MALFORMED_TYPESCRIPT,
                Span::new(2, 2, 62, 63),
            ),
            ("malformed.tsx", MALFORMED_TSX, Span::new(1, 2, 0, 96)),
        ] {
            let source = SourceFile::new(PathBuf::from(path), text.to_string());
            let tree = parse_source(&source)
                .unwrap_or_else(|error| panic!("{path}: {error:#}"))
                .expect("tree");

            assert_eq!(source_parses_without_errors(&source).unwrap(), Some(false));
            assert!(tree.root_node().has_error(), "{path}");
            assert!(tree_has_error_or_missing(tree.root_node()), "{path}");
            let analysis = analysis_provenance(&source).expect("analysis provenance");
            assert_eq!(analysis.status, deslop_core::AnalysisStatus::Partial);
            assert!(!analysis.permits_rewrites());
            assert!(!analysis.diagnostics.is_empty(), "{path}");
            assert_eq!(analysis.diagnostics.len(), 1, "{path}");
            assert_eq!(analysis.diagnostics[0].span, Some(expected_span), "{path}");
            assert!(
                analysis.diagnostics.iter().all(|diagnostic| {
                    matches!(
                        diagnostic.code.as_str(),
                        "tree-sitter-error" | "tree-sitter-missing-node"
                    ) && diagnostic.span.is_some()
                }),
                "{path}: {:#?}",
                analysis.diagnostics
            );
        }
    }

    fn tree_has_kind(node: tree_sitter::Node<'_>, expected: &str) -> bool {
        if node.kind() == expected {
            return true;
        }
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .any(|child| tree_has_kind(child, expected))
    }

    fn tree_kind_count(node: tree_sitter::Node<'_>, expected: &str) -> usize {
        let mut count = usize::from(node.kind() == expected);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            count += tree_kind_count(child, expected);
        }
        count
    }

    fn tree_has_error_or_missing(node: tree_sitter::Node<'_>) -> bool {
        if node.is_error() || node.is_missing() {
            return true;
        }
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .any(tree_has_error_or_missing)
    }

    fn assert_enclosing_region(
        source: &SourceFile,
        line: usize,
        start_line: usize,
        end_line: usize,
        expected: &str,
    ) {
        let region = source.enclosing_region_for_span(line, line);
        assert_eq!(region.start_line, start_line);
        assert_eq!(region.end_line, end_line);
        assert_region_contains(source, region, expected);
    }

    fn assert_region_contains(source: &SourceFile, region: RegionSpan, expected: &str) {
        assert!(
            source
                .region_text(region.start_line, region.end_line)
                .contains(expected)
        );
    }
}
