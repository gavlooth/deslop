/// Parse ownership totals for one immutable project analysis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseOwnershipInstrumentation {
    pub file_revisions: usize,
    pub requested: usize,
    pub owners: usize,
    pub parser_invocations: usize,
    pub reused: usize,
    pub syntax_unavailable: usize,
    pub invariant_violations: usize,
}

impl ParseOwnershipInstrumentation {
    /// Every file revision has one request and owner, and at most one parse or exact reuse.
    pub fn invariant_holds(self) -> bool {
        self.invariant_violations == 0
    }
}

/// Deterministic structural counts derived from the retained syntax arenas.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnalysisStructureInstrumentation {
    pub files: usize,
    pub source_bytes: usize,
    pub utf8_text_bytes: usize,
    pub nodes: usize,
    pub syntax_segments: usize,
    pub child_edges: usize,
    pub owned_segment_references: usize,
    pub zero_width_nodes: usize,
    pub line_start_entries: usize,
    pub node_key_field_path_entries: usize,
    pub max_node_key_field_path_depth: usize,
}

/// Attributed retained storage visible to deslop.
///
/// Byte values are deterministic lower bounds: they include sized Rust records and visible string,
/// slice, and source payloads, but exclude allocator headers, `BTreeMap` node overhead, `Arc`
/// control blocks, and opaque Tree-sitter allocations. Shared predecessor storage is attributed to
/// every analysis that retains it, so this is not a process-wide unique-allocation measurement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnalysisMemoryInstrumentation {
    pub known_bytes_lower_bound: usize,
    pub source_store_revisions: usize,
    pub source_store_bytes: usize,
    pub parsed_utf8_text_bytes: usize,
    pub arena_bytes_lower_bound: usize,
    pub containment_index_bytes: usize,
    pub line_index_bytes: usize,
    pub query_node_index_bytes: usize,
    pub node_range_bytes_lower_bound: usize,
    pub node_key_lookup_index_bytes: usize,
    pub node_key_bytes_lower_bound: usize,
    pub node_key_file_revision_payload_bytes: usize,
    pub node_key_field_path_bytes: usize,
    pub parse_ledger_bytes_lower_bound: usize,
    pub opaque_tree_count: usize,
}

/// One identity-neutral instrumentation snapshot for a `ProjectAnalysis`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectAnalysisInstrumentation {
    pub parse: ParseOwnershipInstrumentation,
    pub structure: AnalysisStructureInstrumentation,
    pub memory: AnalysisMemoryInstrumentation,
    /// Digest of revision-bound node keys in public deterministic preorder.
    pub node_order_digest: String,
}

/// Retained query source and owned metadata visible outside opaque Tree-sitter storage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyntaxQueryInstrumentation {
    pub source_bytes: usize,
    pub capture_names: usize,
    pub capture_name_bytes: usize,
    pub patterns: usize,
    pub capture_quantifiers: usize,
    pub property_settings: usize,
    pub property_predicates: usize,
    pub general_predicates: usize,
    pub predicate_arguments: usize,
    pub metadata_string_bytes: usize,
    pub known_bytes_lower_bound: usize,
}

/// Retained query-result storage, attributing shared capture strings only once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyntaxQueryResultsInstrumentation {
    pub captures: usize,
    pub capture_records_bytes: usize,
    pub unique_capture_name_allocations: usize,
    pub unique_capture_name_bytes: usize,
    pub known_bytes_lower_bound: usize,
}

/// Deterministic work and retained-storage counts for one immutable successor update.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectAnalysisUpdateInstrumentation {
    pub files: usize,
    pub reused_files: usize,
    pub incremental_files: usize,
    pub rebuilt_files: usize,
    pub added_files: usize,
    pub removed_files: usize,
    pub source_edits: usize,
    pub syntax_changed_ranges: usize,
    pub sequential_edit_validation_bytes_upper_bound: usize,
    pub derived_diff_bytes_upper_bound: usize,
    pub previous_nodes: usize,
    pub current_nodes: usize,
    pub incrementally_rebuilt_nodes: usize,
    pub fully_rebuilt_nodes: usize,
    pub successor_assembly_nodes: usize,
    pub transition_entries: usize,
    pub retained_transitions: usize,
    pub reanchored_transitions: usize,
    pub expired_transitions: usize,
    pub transition_bytes_lower_bound: usize,
}

/// Retained storage for one point-context result.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyntaxPointContextInstrumentation {
    pub exact_zero_width_nodes: usize,
    pub exact_zero_width_bytes: usize,
    pub known_bytes_lower_bound: usize,
}
