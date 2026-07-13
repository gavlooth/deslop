use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tree_sitter::{
    CaptureQuantifier, Node, Query, QueryCursor, QueryErrorKind, QueryPredicateArg, QueryProperty,
    StreamingIterator, Tree,
};

use crate::arena::{ArenaNodeIndex, SyntaxArena};
use crate::identity::{NodeId, NodeLookupError};
use crate::snapshot::{GrammarSelection, ProjectAnalysis};

const SYNTAX_QUERY_DOMAIN: &str = "deslop syntax query v1";
const QUERY_MATCH_LIMIT: u32 = 65_536;

/// Exact grammar-bound identity for compiled raw syntax query text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SyntaxQueryId(String);

impl SyntaxQueryId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An owned, reusable query compiled against one exact grammar selection.
#[derive(Clone)]
pub struct SyntaxQuery {
    id: SyntaxQueryId,
    grammar: GrammarSelection,
    source: Arc<str>,
    capture_names: Box<[Box<str>]>,
    patterns: Box<[SyntaxQueryPattern]>,
    raw: Arc<Query>,
}

impl fmt::Debug for SyntaxQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SyntaxQuery")
            .field("id", &self.id)
            .field("grammar", &self.grammar)
            .field("source", &self.source)
            .field("capture_names", &self.capture_names)
            .field("patterns", &self.patterns)
            .finish_non_exhaustive()
    }
}

impl SyntaxQuery {
    pub fn id(&self) -> &SyntaxQueryId {
        &self.id
    }

    pub fn grammar(&self) -> &GrammarSelection {
        &self.grammar
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn capture_names(&self) -> impl ExactSizeIterator<Item = &str> {
        self.capture_names.iter().map(AsRef::as_ref)
    }

    pub fn patterns(&self) -> &[SyntaxQueryPattern] {
        &self.patterns
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SyntaxCaptureQuantifier {
    Zero,
    ZeroOrOne,
    ZeroOrMore,
    One,
    OneOrMore,
}

impl From<CaptureQuantifier> for SyntaxCaptureQuantifier {
    fn from(value: CaptureQuantifier) -> Self {
        match value {
            CaptureQuantifier::Zero => Self::Zero,
            CaptureQuantifier::ZeroOrOne => Self::ZeroOrOne,
            CaptureQuantifier::ZeroOrMore => Self::ZeroOrMore,
            CaptureQuantifier::One => Self::One,
            CaptureQuantifier::OneOrMore => Self::OneOrMore,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxQueryProperty {
    key: Box<str>,
    value: Option<Box<str>>,
    capture_index: Option<usize>,
}

impl SyntaxQueryProperty {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    pub fn capture_index(&self) -> Option<usize> {
        self.capture_index
    }
}

impl From<&QueryProperty> for SyntaxQueryProperty {
    fn from(value: &QueryProperty) -> Self {
        Self {
            key: value.key.clone(),
            value: value.value.clone(),
            capture_index: value.capture_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxQueryPropertyPredicate {
    property: SyntaxQueryProperty,
    positive: bool,
}

impl SyntaxQueryPropertyPredicate {
    pub fn property(&self) -> &SyntaxQueryProperty {
        &self.property
    }

    pub fn is_positive(&self) -> bool {
        self.positive
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxQueryPredicateArgument {
    Capture(u32),
    String(Box<str>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxQueryPredicate {
    operator: Box<str>,
    arguments: Box<[SyntaxQueryPredicateArgument]>,
}

impl SyntaxQueryPredicate {
    pub fn operator(&self) -> &str {
        &self.operator
    }

    pub fn arguments(&self) -> &[SyntaxQueryPredicateArgument] {
        &self.arguments
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxQueryPattern {
    source_range: Range<usize>,
    rooted: bool,
    non_local: bool,
    capture_quantifiers: Box<[SyntaxCaptureQuantifier]>,
    property_settings: Box<[SyntaxQueryProperty]>,
    property_predicates: Box<[SyntaxQueryPropertyPredicate]>,
    general_predicates: Box<[SyntaxQueryPredicate]>,
}

impl SyntaxQueryPattern {
    pub fn source_range(&self) -> Range<usize> {
        self.source_range.clone()
    }

    pub fn is_rooted(&self) -> bool {
        self.rooted
    }

    pub fn is_non_local(&self) -> bool {
        self.non_local
    }

    pub fn capture_quantifiers(&self) -> &[SyntaxCaptureQuantifier] {
        &self.capture_quantifiers
    }

    pub fn property_settings(&self) -> &[SyntaxQueryProperty] {
        &self.property_settings
    }

    pub fn property_predicates(&self) -> &[SyntaxQueryPropertyPredicate] {
        &self.property_predicates
    }

    pub fn general_predicates(&self) -> &[SyntaxQueryPredicate] {
        &self.general_predicates
    }
}

/// One capture with no borrowed Tree-sitter handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedSyntaxCapture {
    node: NodeId,
    pattern_index: usize,
    capture_index: u32,
    capture_name: Box<str>,
}

impl OwnedSyntaxCapture {
    pub fn node(&self) -> NodeId {
        self.node
    }

    pub fn pattern_index(&self) -> usize {
        self.pattern_index
    }

    pub fn capture_index(&self) -> u32 {
        self.capture_index
    }

    pub fn capture_name(&self) -> &str {
        &self.capture_name
    }
}

/// One Tree-sitter match in query execution order, with its capture association retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedSyntaxMatch {
    ordinal: usize,
    pattern_index: usize,
    captures: Box<[OwnedSyntaxCapture]>,
}

impl OwnedSyntaxMatch {
    pub fn ordinal(&self) -> usize {
        self.ordinal
    }

    pub fn pattern_index(&self) -> usize {
        self.pattern_index
    }

    pub fn captures(&self) -> &[OwnedSyntaxCapture] {
        &self.captures
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxQueryCompileErrorKind {
    Syntax,
    NodeType,
    Field,
    Capture,
    Predicate,
    Structure,
    Language,
}

impl From<QueryErrorKind> for SyntaxQueryCompileErrorKind {
    fn from(value: QueryErrorKind) -> Self {
        match value {
            QueryErrorKind::Syntax => Self::Syntax,
            QueryErrorKind::NodeType => Self::NodeType,
            QueryErrorKind::Field => Self::Field,
            QueryErrorKind::Capture => Self::Capture,
            QueryErrorKind::Predicate => Self::Predicate,
            QueryErrorKind::Structure => Self::Structure,
            QueryErrorKind::Language => Self::Language,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxQueryError {
    FileNotFound(PathBuf),
    NodeLookup(NodeLookupError),
    SyntaxUnavailable {
        path: PathBuf,
    },
    GrammarMismatch {
        query: Box<GrammarSelection>,
        target: Box<GrammarSelection>,
    },
    Compile {
        path: PathBuf,
        row: usize,
        column: usize,
        offset: usize,
        kind: SyntaxQueryCompileErrorKind,
        message: String,
    },
    SourceTooLarge {
        path: PathBuf,
        bytes: usize,
        max: usize,
    },
    UnsupportedPredicate {
        pattern_index: usize,
        operator: Box<str>,
    },
    MatchLimitExceeded {
        path: PathBuf,
        limit: u32,
    },
    TreeArenaMismatch {
        path: PathBuf,
        detail: String,
    },
}

impl fmt::Display for SyntaxQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileNotFound(path) => {
                write!(formatter, "analysis has no source file {}", path.display())
            }
            Self::NodeLookup(error) => write!(formatter, "invalid query root: {error}"),
            Self::SyntaxUnavailable { path } => {
                write!(
                    formatter,
                    "syntax tree is unavailable for {}",
                    path.display()
                )
            }
            Self::GrammarMismatch { query, target } => write!(
                formatter,
                "query grammar {}:{}@{} does not match target grammar {}:{}@{}",
                query.dialect(),
                query.grammar_id(),
                query.grammar_version(),
                target.dialect(),
                target.grammar_id(),
                target.grammar_version()
            ),
            Self::Compile {
                path,
                row,
                column,
                kind,
                message,
                ..
            } => write!(
                formatter,
                "failed to compile syntax query for {} at {}:{} ({kind:?}): {message}",
                path.display(),
                row + 1,
                column + 1
            ),
            Self::SourceTooLarge { path, bytes, max } => write!(
                formatter,
                "syntax query for {} is {bytes} bytes, exceeding the {max}-byte Tree-sitter limit",
                path.display()
            ),
            Self::UnsupportedPredicate {
                pattern_index,
                operator,
            } => write!(
                formatter,
                "query pattern {pattern_index} uses unevaluated predicate #{operator}"
            ),
            Self::MatchLimitExceeded { path, limit } => write!(
                formatter,
                "syntax query for {} exceeded the in-progress match limit {limit}",
                path.display()
            ),
            Self::TreeArenaMismatch { path, detail } => write!(
                formatter,
                "private syntax tree and owned arena diverged for {}: {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for SyntaxQueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NodeLookup(error) => Some(error),
            _ => None,
        }
    }
}

impl ProjectAnalysis {
    /// Compile a raw Tree-sitter query against the exact grammar retained for `path`.
    pub fn compile_syntax_query(
        &self,
        path: &Path,
        source: &str,
    ) -> Result<SyntaxQuery, SyntaxQueryError> {
        let file = self
            .file(path)
            .ok_or_else(|| SyntaxQueryError::FileNotFound(path.to_path_buf()))?;
        validate_query_source_len(path, source.len())?;
        let raw = Query::new(file.query_language(), source).map_err(|error| {
            SyntaxQueryError::Compile {
                path: path.to_path_buf(),
                row: error.row,
                column: error.column,
                offset: error.offset,
                kind: error.kind.into(),
                message: error.message,
            }
        })?;
        let capture_names = raw
            .capture_names()
            .iter()
            .map(|name| Box::<str>::from(*name))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let patterns = (0..raw.pattern_count())
            .map(|pattern_index| own_pattern(&raw, pattern_index))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let id = syntax_query_id(file.grammar(), source);
        Ok(SyntaxQuery {
            id,
            grammar: file.grammar().clone(),
            source: Arc::from(source),
            capture_names,
            patterns,
            raw: Arc::new(raw),
        })
    }

    /// Return grouped matches in Tree-sitter's deterministic match discovery order.
    pub fn syntax_query_matches(
        &self,
        query: &SyntaxQuery,
        within: NodeId,
    ) -> Result<Vec<OwnedSyntaxMatch>, SyntaxQueryError> {
        self.syntax_query_matches_with_limit(query, within, QUERY_MATCH_LIMIT)
    }

    fn syntax_query_matches_with_limit(
        &self,
        query: &SyntaxQuery,
        within: NodeId,
        match_limit: u32,
    ) -> Result<Vec<OwnedSyntaxMatch>, SyntaxQueryError> {
        reject_unevaluated_predicates(query)?;
        let context = query_context(self, query, within)?;
        let mut cursor = QueryCursor::new();
        cursor.set_match_limit(match_limit);
        let mut owned = Vec::new();
        {
            let mut matches = cursor.matches(&query.raw, context.root, context.source);
            while let Some(query_match) = matches.next() {
                let captures = query_match
                    .captures
                    .iter()
                    .map(|capture| {
                        own_capture(
                            query,
                            &context.node_map,
                            context.owner,
                            context.file_start,
                            query_match.pattern_index,
                            *capture,
                            context.path,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice();
                owned.push(OwnedSyntaxMatch {
                    ordinal: owned.len(),
                    pattern_index: query_match.pattern_index,
                    captures,
                });
            }
        }
        finish_query_results(
            cursor.did_exceed_match_limit(),
            context.path,
            match_limit,
            owned,
        )
    }

    /// Return individual captures in Tree-sitter's deterministic source order.
    ///
    /// This stream intentionally omits match association; consumers that need to join captures from
    /// one match must use [`ProjectAnalysis::syntax_query_matches`].
    pub fn syntax_query_captures(
        &self,
        query: &SyntaxQuery,
        within: NodeId,
    ) -> Result<Vec<OwnedSyntaxCapture>, SyntaxQueryError> {
        self.syntax_query_captures_with_limit(query, within, QUERY_MATCH_LIMIT)
    }

    fn syntax_query_captures_with_limit(
        &self,
        query: &SyntaxQuery,
        within: NodeId,
        match_limit: u32,
    ) -> Result<Vec<OwnedSyntaxCapture>, SyntaxQueryError> {
        reject_unevaluated_predicates(query)?;
        let context = query_context(self, query, within)?;
        let mut cursor = QueryCursor::new();
        cursor.set_match_limit(match_limit);
        let mut owned = Vec::new();
        {
            let mut captures = cursor.captures(&query.raw, context.root, context.source);
            while let Some((query_match, capture_offset)) = captures.next() {
                let capture = query_match.captures[*capture_offset];
                owned.push(own_capture(
                    query,
                    &context.node_map,
                    context.owner,
                    context.file_start,
                    query_match.pattern_index,
                    capture,
                    context.path,
                )?);
            }
        }
        finish_query_results(
            cursor.did_exceed_match_limit(),
            context.path,
            match_limit,
            owned,
        )
    }
}

struct QueryContext<'tree, 'source, 'path> {
    root: Node<'tree>,
    source: &'source [u8],
    path: &'path Path,
    node_map: HashMap<usize, ArenaNodeIndex>,
    owner: u64,
    file_start: u32,
}

fn query_context<'analysis>(
    analysis: &'analysis ProjectAnalysis,
    query: &SyntaxQuery,
    within: NodeId,
) -> Result<QueryContext<'analysis, 'analysis, 'analysis>, SyntaxQueryError> {
    let view = analysis
        .node(within)
        .map_err(SyntaxQueryError::NodeLookup)?;
    if view.grammar() != query.grammar() {
        return Err(SyntaxQueryError::GrammarMismatch {
            query: Box::new(query.grammar().clone()),
            target: Box::new(view.grammar().clone()),
        });
    }
    let (file, arena, local) = view.query_parts();
    let path = file.key().path.as_path();
    let tree = file
        .query_tree()
        .ok_or_else(|| SyntaxQueryError::SyntaxUnavailable {
            path: path.to_path_buf(),
        })?;
    let (tree_nodes, node_map) = map_tree_nodes(tree, arena, path)?;
    let root =
        *tree_nodes
            .get(local.as_usize())
            .ok_or_else(|| SyntaxQueryError::TreeArenaMismatch {
                path: path.to_path_buf(),
                detail: format!("query root local index {} is absent", local.as_usize()),
            })?;
    let local_u32 = u32::try_from(local.as_usize()).expect("arena node indices are u32");
    let file_start =
        within
            .index
            .checked_sub(local_u32)
            .ok_or_else(|| SyntaxQueryError::TreeArenaMismatch {
                path: path.to_path_buf(),
                detail: "global query root precedes its file-local slot".to_string(),
            })?;
    Ok(QueryContext {
        root,
        source: file.source(),
        path,
        node_map,
        owner: within.owner,
        file_start,
    })
}

fn map_tree_nodes<'tree>(
    tree: &'tree Tree,
    arena: &SyntaxArena,
    path: &Path,
) -> Result<(Vec<Node<'tree>>, HashMap<usize, ArenaNodeIndex>), SyntaxQueryError> {
    let mut nodes = Vec::with_capacity(arena.nodes().len());
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        nodes.push(node);
        let mut cursor = node.walk();
        let children = node.children(&mut cursor).collect::<Vec<_>>();
        pending.extend(children.into_iter().rev());
    }
    if nodes.len() != arena.nodes().len() {
        return Err(SyntaxQueryError::TreeArenaMismatch {
            path: path.to_path_buf(),
            detail: format!(
                "tree has {} visible nodes but arena has {}",
                nodes.len(),
                arena.nodes().len()
            ),
        });
    }
    let mut node_map = HashMap::with_capacity(nodes.len());
    for (offset, (node, raw)) in nodes.iter().zip(arena.nodes()).enumerate() {
        let span = raw.span();
        if node.kind() != raw.raw_kind()
            || node.kind_id() != raw.raw_kind_id()
            || node.grammar_name() != raw.raw_grammar_kind()
            || node.grammar_id() != raw.raw_grammar_kind_id()
            || node.start_byte() != span.start_byte()
            || node.end_byte() != span.end_byte()
            || node.is_named() != raw.is_named()
            || node.is_extra() != raw.is_extra()
            || node.is_error() != raw.is_error()
            || node.is_missing() != raw.is_missing()
            || node.has_error() != raw.has_error()
        {
            return Err(SyntaxQueryError::TreeArenaMismatch {
                path: path.to_path_buf(),
                detail: format!("preorder node {offset} does not match its arena slot"),
            });
        }
        let local = ArenaNodeIndex::from_usize(offset).expect("validated arena indices fit u32");
        if node_map.insert(node.id(), local).is_some() {
            return Err(SyntaxQueryError::TreeArenaMismatch {
                path: path.to_path_buf(),
                detail: format!("Tree-sitter node id at preorder slot {offset} is not unique"),
            });
        }
    }
    Ok((nodes, node_map))
}

fn own_capture(
    query: &SyntaxQuery,
    node_map: &HashMap<usize, ArenaNodeIndex>,
    owner: u64,
    file_start: u32,
    pattern_index: usize,
    capture: tree_sitter::QueryCapture<'_>,
    path: &Path,
) -> Result<OwnedSyntaxCapture, SyntaxQueryError> {
    let local = node_map.get(&capture.node.id()).copied().ok_or_else(|| {
        SyntaxQueryError::TreeArenaMismatch {
            path: path.to_path_buf(),
            detail: format!(
                "captured Tree-sitter node {} has no owned arena slot",
                capture.node.id()
            ),
        }
    })?;
    let index = file_start
        .checked_add(u32::try_from(local.as_usize()).expect("arena node indices are u32"))
        .ok_or_else(|| SyntaxQueryError::TreeArenaMismatch {
            path: path.to_path_buf(),
            detail: "captured node global index overflowed".to_string(),
        })?;
    let capture_name = query
        .capture_names
        .get(capture.index as usize)
        .cloned()
        .ok_or_else(|| SyntaxQueryError::TreeArenaMismatch {
            path: path.to_path_buf(),
            detail: format!("query returned unknown capture index {}", capture.index),
        })?;
    Ok(OwnedSyntaxCapture {
        node: NodeId { owner, index },
        pattern_index,
        capture_index: capture.index,
        capture_name,
    })
}

fn own_pattern(query: &Query, pattern_index: usize) -> SyntaxQueryPattern {
    SyntaxQueryPattern {
        source_range: query.start_byte_for_pattern(pattern_index)
            ..query.end_byte_for_pattern(pattern_index),
        rooted: query.is_pattern_rooted(pattern_index),
        non_local: query.is_pattern_non_local(pattern_index),
        capture_quantifiers: query
            .capture_quantifiers(pattern_index)
            .iter()
            .copied()
            .map(Into::into)
            .collect(),
        property_settings: query
            .property_settings(pattern_index)
            .iter()
            .map(Into::into)
            .collect(),
        property_predicates: query
            .property_predicates(pattern_index)
            .iter()
            .map(|(property, positive)| SyntaxQueryPropertyPredicate {
                property: property.into(),
                positive: *positive,
            })
            .collect(),
        general_predicates: query
            .general_predicates(pattern_index)
            .iter()
            .map(|predicate| SyntaxQueryPredicate {
                operator: predicate.operator.clone(),
                arguments: predicate
                    .args
                    .iter()
                    .map(|argument| match argument {
                        QueryPredicateArg::Capture(index) => {
                            SyntaxQueryPredicateArgument::Capture(*index)
                        }
                        QueryPredicateArg::String(value) => {
                            SyntaxQueryPredicateArgument::String(value.clone())
                        }
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn reject_unevaluated_predicates(query: &SyntaxQuery) -> Result<(), SyntaxQueryError> {
    for (pattern_index, pattern) in query.patterns.iter().enumerate() {
        if let Some(predicate) = pattern.property_predicates.first() {
            return Err(SyntaxQueryError::UnsupportedPredicate {
                pattern_index,
                operator: if predicate.positive {
                    Box::from("is?")
                } else {
                    Box::from("is-not?")
                },
            });
        }
        if let Some(predicate) = pattern.general_predicates.first() {
            return Err(SyntaxQueryError::UnsupportedPredicate {
                pattern_index,
                operator: predicate.operator.clone(),
            });
        }
    }
    Ok(())
}

fn finish_query_results<T>(
    did_exceed_match_limit: bool,
    path: &Path,
    match_limit: u32,
    results: Vec<T>,
) -> Result<Vec<T>, SyntaxQueryError> {
    if did_exceed_match_limit {
        return Err(SyntaxQueryError::MatchLimitExceeded {
            path: path.to_path_buf(),
            limit: match_limit,
        });
    }
    Ok(results)
}

fn syntax_query_id(grammar: &GrammarSelection, source: &str) -> SyntaxQueryId {
    let mut hasher = blake3::Hasher::new();
    hash_query_part(&mut hasher, SYNTAX_QUERY_DOMAIN.as_bytes());
    hash_query_part(&mut hasher, &grammar.identity_bytes());
    hash_query_part(&mut hasher, source.as_bytes());
    SyntaxQueryId(format!("sq1_{}", hasher.finalize().to_hex()))
}

fn validate_query_source_len(path: &Path, bytes: usize) -> Result<(), SyntaxQueryError> {
    let max = u32::MAX as usize;
    if bytes > max {
        return Err(SyntaxQueryError::SourceTooLarge {
            path: path.to_path_buf(),
            bytes,
            max,
        });
    }
    Ok(())
}

fn hash_query_part(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{ProjectSnapshotBuilder, RepositoryId};

    fn analysis(files: &[(&str, &[u8])]) -> Arc<ProjectAnalysis> {
        let temp = tempfile::tempdir().unwrap();
        let mut builder = ProjectSnapshotBuilder::new(
            temp.path(),
            RepositoryId::explicit("query-test-repository").unwrap(),
        )
        .unwrap();
        for (path, source) in files {
            builder = builder.with_overlay(path, source.to_vec()).unwrap();
        }
        ProjectAnalysis::build(builder.build().unwrap()).unwrap()
    }

    fn file_root(analysis: &ProjectAnalysis, path: &str) -> NodeId {
        analysis
            .file_node_ids(Path::new(path))
            .unwrap()
            .next()
            .unwrap()
    }

    #[test]
    fn nested_query_oracle_preserves_engine_orders_fields_and_every_arena_node() {
        let source = b"fn outer() {\n    let closure = || { if true { value(); } };\n}\n";
        let analysis = analysis(&[("nested.rs", source)]);
        let root = file_root(&analysis, "nested.rs");
        assert_eq!(source.len(), 62);
        assert_eq!(
            analysis
                .file_node_ids(Path::new("nested.rs"))
                .unwrap()
                .len(),
            37
        );

        let wildcard = analysis
            .compile_syntax_query(Path::new("nested.rs"), "_ @any")
            .unwrap();
        let wildcard_captures = analysis.syntax_query_captures(&wildcard, root).unwrap();
        assert_eq!(wildcard_captures.len(), 37);
        assert_eq!(
            wildcard_captures
                .iter()
                .map(|capture| capture.node().index)
                .collect::<Vec<_>>(),
            (0..37).collect::<Vec<_>>()
        );
        assert_eq!(
            wildcard_captures
                .iter()
                .map(|capture| capture.node())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            37
        );
        let wildcard_nodes = wildcard_captures
            .iter()
            .map(|capture| analysis.node(capture.node()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            wildcard_nodes.iter().filter(|node| node.is_named()).count(),
            18
        );
        assert_eq!(
            wildcard_nodes
                .iter()
                .filter(|node| !node.is_named())
                .count(),
            19
        );
        assert!(
            wildcard_nodes
                .iter()
                .all(|node| !node.is_error() && !node.is_missing())
        );
        assert_ne!(wildcard_captures[19].node(), wildcard_captures[20].node());
        assert_eq!(
            wildcard_nodes[19].span().byte_range(),
            wildcard_nodes[20].span().byte_range()
        );
        assert_ne!(wildcard_captures[22].node(), wildcard_captures[23].node());
        assert_eq!(
            wildcard_nodes[22].span().byte_range(),
            wildcard_nodes[23].span().byte_range()
        );

        let query_source = concat!(
            "(function_item\n",
            "  name: (identifier) @function.name\n",
            "  body: (block) @function.body) @function\n\n",
            "(call_expression\n",
            "  function: (identifier) @call.name\n",
            "  arguments: (arguments) @call.arguments) @call\n\n",
            "(identifier) @identifier",
        );
        assert_eq!(query_source.len(), 220);
        let query = analysis
            .compile_syntax_query(Path::new("nested.rs"), query_source)
            .unwrap();
        assert_eq!(query.source(), query_source);
        assert_eq!(
            query.capture_names().collect::<Vec<_>>(),
            vec![
                "function.name",
                "function.body",
                "function",
                "call.name",
                "call.arguments",
                "call",
                "identifier"
            ]
        );
        assert_eq!(
            query
                .patterns()
                .iter()
                .map(SyntaxQueryPattern::source_range)
                .collect::<Vec<_>>(),
            vec![0..94, 94..196, 196..220]
        );
        assert_eq!(
            query
                .patterns()
                .iter()
                .map(|pattern| &query.source()[pattern.source_range()])
                .collect::<Vec<_>>(),
            vec![
                &query_source[0..94],
                &query_source[94..196],
                &query_source[196..220]
            ]
        );
        let one = SyntaxCaptureQuantifier::One;
        let zero = SyntaxCaptureQuantifier::Zero;
        assert_eq!(
            query
                .patterns()
                .iter()
                .map(|pattern| pattern.capture_quantifiers().to_vec())
                .collect::<Vec<_>>(),
            vec![
                vec![one, one, one, zero, zero, zero, zero],
                vec![zero, zero, zero, one, one, one, zero],
                vec![zero, zero, zero, zero, zero, zero, one],
            ]
        );

        let grouped = analysis.syntax_query_matches(&query, root).unwrap();
        assert_eq!(
            grouped
                .iter()
                .map(|matched| {
                    (
                        matched.ordinal(),
                        matched.pattern_index(),
                        matched
                            .captures()
                            .iter()
                            .map(|capture| (capture.capture_index(), capture.node().index))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (0, 2, vec![(6, 3)]),
                (1, 0, vec![(2, 1), (0, 3), (1, 7)]),
                (2, 2, vec![(6, 11)]),
                (3, 2, vec![(6, 28)]),
                (4, 1, vec![(5, 27), (3, 28), (4, 29)]),
            ]
        );
        assert_eq!(
            analysis
                .syntax_query_captures(&query, root)
                .unwrap()
                .iter()
                .map(|capture| {
                    (
                        capture.pattern_index(),
                        capture.capture_index(),
                        capture.node().index,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (0, 2, 1),
                (0, 0, 3),
                (2, 6, 3),
                (0, 1, 7),
                (2, 6, 11),
                (1, 5, 27),
                (1, 3, 28),
                (2, 6, 28),
                (1, 4, 29),
            ]
        );

        let field_query = analysis
            .compile_syntax_query(
                Path::new("nested.rs"),
                "(let_declaration pattern: (identifier) @binding value: (closure_expression) @value) @let",
            )
            .unwrap();
        let field_match = analysis
            .syntax_query_matches(&field_query, root)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(
            field_match
                .captures()
                .iter()
                .map(|capture| (capture.capture_index(), capture.node().index))
                .collect::<Vec<_>>(),
            vec![(2, 9), (0, 11), (1, 13)]
        );
        assert_eq!(
            field_match
                .captures()
                .iter()
                .map(|capture| {
                    analysis
                        .node(capture.node())
                        .unwrap()
                        .field()
                        .map(str::to_string)
                })
                .collect::<Vec<_>>(),
            vec![None, Some("pattern".to_string()), Some("value".to_string())]
        );
    }

    #[test]
    fn query_mapping_is_node_key_stable_when_global_indices_shift() {
        let nested = b"fn outer() {\n    let closure = || { if true { value(); } };\n}\n";
        let first = analysis(&[("nested.rs", nested)]);
        let shifted = analysis(&[("nested.rs", nested), ("0.rs", b"fn zero() {}\n")]);
        let keyed = |analysis: &ProjectAnalysis| {
            let query = analysis
                .compile_syntax_query(Path::new("nested.rs"), "_ @any")
                .unwrap();
            let root = file_root(analysis, "nested.rs");
            analysis
                .syntax_query_captures(&query, root)
                .unwrap()
                .iter()
                .map(|capture| {
                    (
                        capture.pattern_index(),
                        capture.capture_index(),
                        capture.capture_name().to_string(),
                        analysis.node_key(capture.node()).unwrap().clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(keyed(&first), keyed(&shifted));
        assert_eq!(file_root(&first, "nested.rs").index, 0);
        assert_eq!(file_root(&shifted, "nested.rs").index, 10);
    }

    #[test]
    fn finite_match_limits_fail_atomically_without_returning_partial_vectors() {
        let nested = b"fn outer() {\n    let closure = || { if true { value(); } };\n}\n";
        let analysis = analysis(&[("nested.rs", nested)]);
        let root = file_root(&analysis, "nested.rs");
        let query = analysis
            .compile_syntax_query(Path::new("nested.rs"), "(identifier) @id\n(identifier) @id")
            .unwrap();
        assert_eq!(
            analysis.syntax_query_matches(&query, root).unwrap().len(),
            6
        );
        assert_eq!(
            analysis.syntax_query_captures(&query, root).unwrap().len(),
            6
        );
        let expected = SyntaxQueryError::MatchLimitExceeded {
            path: PathBuf::from("nested.rs"),
            limit: 1,
        };
        assert_eq!(
            analysis
                .syntax_query_matches_with_limit(&query, root, 1)
                .unwrap_err(),
            expected
        );
        assert_eq!(
            analysis
                .syntax_query_captures_with_limit(&query, root, 1)
                .unwrap_err(),
            expected
        );
        assert_eq!(
            finish_query_results(true, Path::new("nested.rs"), 1, vec![1, 2, 3]).unwrap_err(),
            expected
        );
    }

    #[test]
    fn recovery_queries_own_missing_and_error_nodes_from_partial_trees() {
        let missing = analysis(&[("missing.ts", b"function f(a: string { return a; }\n")]);
        let missing_root = file_root(&missing, "missing.ts");
        let missing_query = missing
            .compile_syntax_query(Path::new("missing.ts"), r#"(MISSING ")") @missing"#)
            .unwrap();
        let missing_captures = missing
            .syntax_query_captures(&missing_query, missing_root)
            .unwrap();
        assert_eq!(missing_captures.len(), 1);
        assert_eq!(missing_captures[0].node().index, 12);
        let missing_node = missing.node(missing_captures[0].node()).unwrap();
        assert_eq!(missing_node.raw_kind(), ")");
        assert_eq!(missing_node.span().byte_range(), 20..20);
        assert!(missing_node.is_missing());
        assert!(!missing_node.is_error());
        assert!(!missing_node.is_named());

        let malformed_ts = include_bytes!("../../../tests/fixtures/typescript/malformed.ts");
        let malformed = analysis(&[("malformed.ts", malformed_ts)]);
        let error_query = malformed
            .compile_syntax_query(Path::new("malformed.ts"), "(ERROR) @error")
            .unwrap();
        let error_capture = malformed
            .syntax_query_captures(&error_query, file_root(&malformed, "malformed.ts"))
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(error_capture.node().index, 24);
        let error_node = malformed.node(error_capture.node()).unwrap();
        assert!(error_node.is_error());
        assert_eq!(error_node.span().byte_range(), 62..63);
        assert_eq!(error_node.text(), ".");

        let malformed_tsx = include_bytes!("../../../tests/fixtures/typescript/malformed.tsx");
        let malformed = analysis(&[("malformed.tsx", malformed_tsx)]);
        let error_query = malformed
            .compile_syntax_query(Path::new("malformed.tsx"), "(ERROR) @error")
            .unwrap();
        let error_capture = malformed
            .syntax_query_captures(&error_query, file_root(&malformed, "malformed.tsx"))
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(error_capture.node().index, 1);
        let error_node = malformed.node(error_capture.node()).unwrap();
        assert!(error_node.is_error());
        assert_eq!(error_node.span().byte_range(), 0..96);
    }

    #[test]
    fn query_and_results_are_owned_send_sync_static_values() {
        fn assert_send_sync_static<T: Send + Sync + 'static>() {}
        fn assert_clone_eq<T: Clone + Eq>() {}
        assert_send_sync_static::<SyntaxQuery>();
        assert_send_sync_static::<OwnedSyntaxCapture>();
        assert_send_sync_static::<OwnedSyntaxMatch>();
        assert_clone_eq::<OwnedSyntaxCapture>();
        assert_clone_eq::<OwnedSyntaxMatch>();

        let analysis = analysis(&[("owned.rs", b"fn owned() {}")]);
        let root = file_root(&analysis, "owned.rs");
        let source = String::from("(identifier) @name");
        let query = analysis
            .compile_syntax_query(Path::new("owned.rs"), &source)
            .unwrap();
        drop(source);
        assert_eq!(query.source(), "(identifier) @name");
        let captures = analysis.syntax_query_captures(&query, root).unwrap();
        drop(query);
        assert_eq!(captures[0].capture_name(), "name");
        assert_eq!(analysis.node(captures[0].node()).unwrap().text(), "owned");

        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            validate_query_source_len(Path::new("huge.rs"), u32::MAX as usize + 1).unwrap_err(),
            SyntaxQueryError::SourceTooLarge {
                path: PathBuf::from("huge.rs"),
                bytes: u32::MAX as usize + 1,
                max: u32::MAX as usize,
            }
        );
    }

    #[test]
    fn grouped_matches_and_source_ordered_captures_are_owned_and_do_not_reparse() {
        let source = b"fn alpha() -> i32 { 1 }\nfn beta() -> i32 { 2 }\n";
        let analysis = analysis(&[("functions.rs", source)]);
        let root = file_root(&analysis, "functions.rs");
        let counts_before = analysis.parse_counts();
        crate::reset_parse_source_invocations();
        let query_source = r#"
            (function_item
              name: (identifier) @name
              body: (block) @body) @function
        "#;
        let query = analysis
            .compile_syntax_query(Path::new("functions.rs"), query_source)
            .unwrap();

        assert!(query.id().as_str().starts_with("sq1_"));
        assert_eq!(query.id().as_str().len(), 68);
        assert_eq!(
            query.capture_names().collect::<Vec<_>>(),
            vec!["name", "body", "function"]
        );
        assert_eq!(query.patterns().len(), 1);
        assert_eq!(query.patterns()[0].capture_quantifiers().len(), 3);

        let matches = analysis.syntax_query_matches(&query, root).unwrap();
        assert_eq!(matches.len(), 2);
        assert!(matches.iter().all(|matched| matched.pattern_index() == 0));
        assert!(matches.iter().all(|matched| matched.captures().len() == 3));
        let grouped_names = matches
            .iter()
            .map(|matched| {
                matched
                    .captures()
                    .iter()
                    .map(|capture| capture.capture_name())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            grouped_names,
            vec![
                vec!["function", "name", "body"],
                vec!["function", "name", "body"]
            ]
        );

        let captures = analysis.syntax_query_captures(&query, root).unwrap();
        assert_eq!(captures.len(), 6);
        let starts = captures
            .iter()
            .map(|capture| analysis.node(capture.node()).unwrap().span().start_byte())
            .collect::<Vec<_>>();
        assert!(starts.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(
            captures
                .iter()
                .filter(|capture| capture.capture_name() == "name")
                .map(|capture| analysis.node(capture.node()).unwrap().text().to_string())
                .collect::<Vec<_>>(),
            vec!["alpha".to_string(), "beta".to_string()]
        );
        assert_eq!(analysis.parse_counts(), counts_before);
        assert_eq!(crate::parse_source_invocations(), 0);
    }

    #[test]
    fn equal_span_and_anonymous_captures_map_to_distinct_existing_node_ids() {
        let analysis = analysis(&[("equal.rs", b"fn only() {}")]);
        let root = file_root(&analysis, "equal.rs");
        let query = analysis
            .compile_syntax_query(
                Path::new("equal.rs"),
                r#"
                    (source_file) @root
                    (function_item) @function
                    "fn" @keyword
                    (identifier) @name
                "#,
            )
            .unwrap();
        let first = analysis.syntax_query_captures(&query, root).unwrap();
        let second = analysis.syntax_query_captures(&query, root).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 4);

        let by_name = first
            .iter()
            .map(|capture| (capture.capture_name(), capture.node()))
            .collect::<HashMap<_, _>>();
        let root_id = by_name["root"];
        let function_id = by_name["function"];
        assert_ne!(root_id, function_id);
        assert_eq!(
            analysis.node(root_id).unwrap().span().byte_range(),
            analysis.node(function_id).unwrap().span().byte_range()
        );
        let keyword = analysis.node(by_name["keyword"]).unwrap();
        assert!(!keyword.is_named());
        assert_eq!(keyword.text(), "fn");
        assert_eq!(analysis.node(by_name["name"]).unwrap().text(), "only");
    }

    #[test]
    fn subtree_scope_text_predicates_and_metadata_have_explicit_semantics() {
        let source = b"fn alpha() {}\nfn beta() { let local = 1; }\n";
        let analysis = analysis(&[("scope.rs", source)]);
        let beta = analysis
            .file_node_ids(Path::new("scope.rs"))
            .unwrap()
            .find(|id| {
                let node = analysis.node(*id).unwrap();
                node.raw_kind() == "function_item" && node.text().starts_with("fn beta")
            })
            .unwrap();
        let identifiers = analysis
            .compile_syntax_query(Path::new("scope.rs"), "(identifier) @id")
            .unwrap();
        assert_eq!(
            analysis
                .syntax_query_captures(&identifiers, beta)
                .unwrap()
                .iter()
                .map(|capture| analysis.node(capture.node()).unwrap().text().to_string())
                .collect::<Vec<_>>(),
            vec!["beta".to_string(), "local".to_string()]
        );

        let filtered = analysis
            .compile_syntax_query(
                Path::new("scope.rs"),
                r#"((identifier) @id (#eq? @id "beta"))"#,
            )
            .unwrap();
        assert_eq!(
            analysis
                .syntax_query_captures(&filtered, file_root(&analysis, "scope.rs"))
                .unwrap()
                .iter()
                .map(|capture| analysis.node(capture.node()).unwrap().text().to_string())
                .collect::<Vec<_>>(),
            vec!["beta".to_string()]
        );

        let settings = analysis
            .compile_syntax_query(
                Path::new("scope.rs"),
                r#"((identifier) @id (#set! role "binding"))"#,
            )
            .unwrap();
        assert_eq!(settings.patterns()[0].property_settings().len(), 1);
        let setting = &settings.patterns()[0].property_settings()[0];
        assert_eq!(setting.key(), "role");
        assert_eq!(setting.value(), Some("binding"));
        assert_eq!(setting.capture_index(), None);
        assert_eq!(
            analysis
                .syntax_query_captures(&settings, beta)
                .unwrap()
                .len(),
            2
        );

        let property_predicate = analysis
            .compile_syntax_query(Path::new("scope.rs"), r#"((identifier) @id (#is? local))"#)
            .unwrap();
        assert!(property_predicate.patterns()[0].property_predicates()[0].is_positive());
        assert_eq!(
            analysis
                .syntax_query_captures(&property_predicate, beta)
                .unwrap_err(),
            SyntaxQueryError::UnsupportedPredicate {
                pattern_index: 0,
                operator: Box::from("is?"),
            }
        );

        let general_predicate = analysis
            .compile_syntax_query(
                Path::new("scope.rs"),
                r#"((identifier) @id (#custom! @id "value"))"#,
            )
            .unwrap();
        assert_eq!(
            general_predicate.patterns()[0].general_predicates()[0].operator(),
            "custom!"
        );
        assert_eq!(
            general_predicate.patterns()[0].general_predicates()[0].arguments(),
            &[
                SyntaxQueryPredicateArgument::Capture(0),
                SyntaxQueryPredicateArgument::String(Box::from("value")),
            ]
        );
        assert_eq!(
            analysis
                .syntax_query_matches(&general_predicate, beta)
                .unwrap_err(),
            SyntaxQueryError::UnsupportedPredicate {
                pattern_index: 0,
                operator: Box::from("custom!"),
            }
        );

        let zero_capture = analysis
            .compile_syntax_query(Path::new("scope.rs"), "(identifier)")
            .unwrap();
        assert_eq!(
            analysis
                .syntax_query_matches(&zero_capture, file_root(&analysis, "scope.rs"))
                .unwrap()
                .len(),
            3
        );
        assert!(
            analysis
                .syntax_query_captures(&zero_capture, file_root(&analysis, "scope.rs"))
                .unwrap()
                .is_empty()
        );
        let empty = analysis
            .compile_syntax_query(Path::new("scope.rs"), " \n ; no patterns\n")
            .unwrap();
        assert!(empty.patterns().is_empty());
        assert_eq!(empty.capture_names().len(), 0);
        assert!(
            analysis
                .syntax_query_matches(&empty, file_root(&analysis, "scope.rs"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn query_identity_grammar_and_lookup_failures_are_typed() {
        let files: [(&str, &[u8]); 5] = [
            ("main.rs", b"fn main() {}"),
            ("main.py", b"def main():\n    pass\n"),
            ("main.js", b"const value = 1;\n"),
            ("main.jsx", b"const value = <div />;\n"),
            ("invalid.rs", &[0xff, 0xfe]),
        ];
        let first = analysis(&files);
        let second = analysis(&files);
        let counts_before = first.parse_counts();
        crate::reset_parse_source_invocations();
        let rust_query = first
            .compile_syntax_query(Path::new("main.rs"), "(identifier) @id")
            .unwrap();
        let same_query = second
            .compile_syntax_query(Path::new("main.rs"), "(identifier) @id")
            .unwrap();
        let changed_query = first
            .compile_syntax_query(Path::new("main.rs"), "(identifier) @name")
            .unwrap();
        let invalid_source_query = first
            .compile_syntax_query(Path::new("invalid.rs"), "_ @any")
            .unwrap();
        assert_eq!(rust_query.id(), same_query.id());
        assert_ne!(rust_query.id(), changed_query.id());
        assert_eq!(
            invalid_source_query.grammar().lang(),
            rust_query.grammar().lang()
        );
        assert_eq!(
            first
                .syntax_query_captures(&invalid_source_query, file_root(&first, "main.rs"))
                .unwrap()
                .len(),
            first.file_node_ids(Path::new("main.rs")).unwrap().len()
        );

        assert_eq!(
            first
                .compile_syntax_query(Path::new("absent.rs"), "(identifier) @id")
                .unwrap_err(),
            SyntaxQueryError::FileNotFound(PathBuf::from("absent.rs"))
        );
        assert!(matches!(
            first
                .compile_syntax_query(Path::new("main.rs"), "(not_a_rust_node) @bad")
                .unwrap_err(),
            SyntaxQueryError::Compile {
                kind: SyntaxQueryCompileErrorKind::NodeType,
                ..
            }
        ));

        let compile_errors = [
            ("(", SyntaxQueryCompileErrorKind::Syntax, 0, 1, 1, "(\n ^"),
            (
                "(no_such_node) @x",
                SyntaxQueryCompileErrorKind::NodeType,
                0,
                1,
                1,
                "no_such_node",
            ),
            (
                "(function_item bogus: (identifier) @x)",
                SyntaxQueryCompileErrorKind::Field,
                0,
                15,
                15,
                "bogus",
            ),
            (
                "((identifier) @x (#eq? @missing \"x\"))",
                SyntaxQueryCompileErrorKind::Capture,
                0,
                24,
                24,
                "missing",
            ),
            (
                "((identifier) @x (#eq? @x))",
                SyntaxQueryCompileErrorKind::Predicate,
                0,
                0,
                0,
                "Wrong number of arguments to #eq? predicate. Expected 2, got 1.",
            ),
            (
                "(identifier (identifier))",
                SyntaxQueryCompileErrorKind::Structure,
                0,
                12,
                12,
                "",
            ),
        ];
        for (source, expected_kind, expected_row, expected_column, expected_offset, fragment) in
            compile_errors
        {
            let SyntaxQueryError::Compile {
                row,
                column,
                offset,
                kind,
                message,
                ..
            } = first
                .compile_syntax_query(Path::new("main.rs"), source)
                .unwrap_err()
            else {
                panic!("expected a typed compile error for {source:?}");
            };
            assert_eq!(kind, expected_kind, "{source:?}");
            assert_eq!(
                (row, column, offset),
                (expected_row, expected_column, expected_offset),
                "{source:?}"
            );
            assert!(message.contains(fragment), "{source:?}: {message:?}");
        }
        assert_eq!(
            SyntaxQueryCompileErrorKind::from(QueryErrorKind::Language),
            SyntaxQueryCompileErrorKind::Language
        );

        let javascript_query = first
            .compile_syntax_query(Path::new("main.js"), "_ @any")
            .unwrap();
        let jsx_root = file_root(&first, "main.jsx");
        assert_eq!(
            javascript_query.grammar().grammar_id(),
            first.node(jsx_root).unwrap().grammar().grammar_id()
        );
        assert_ne!(
            javascript_query.grammar(),
            first.node(jsx_root).unwrap().grammar()
        );
        assert!(matches!(
            first
                .syntax_query_captures(&javascript_query, jsx_root)
                .unwrap_err(),
            SyntaxQueryError::GrammarMismatch { .. }
        ));

        let python_root = file_root(&first, "main.py");
        assert!(matches!(
            first
                .syntax_query_captures(&rust_query, python_root)
                .unwrap_err(),
            SyntaxQueryError::GrammarMismatch { .. }
        ));
        let foreign_root = file_root(&second, "main.rs");
        assert_eq!(
            first
                .syntax_query_captures(&rust_query, foreign_root)
                .unwrap_err(),
            SyntaxQueryError::NodeLookup(NodeLookupError::WrongAnalysis)
        );
        let out_of_range = NodeId {
            owner: file_root(&first, "main.rs").owner,
            index: first.node_count() as u32,
        };
        assert_eq!(
            first
                .syntax_query_captures(&rust_query, out_of_range)
                .unwrap_err(),
            SyntaxQueryError::NodeLookup(NodeLookupError::OutOfRange {
                requested: first.node_count() as u32,
                node_count: first.node_count() as u32,
            })
        );
        assert_eq!(first.parse_counts(), counts_before);
        assert_eq!(crate::parse_source_invocations(), 0);
    }
}
