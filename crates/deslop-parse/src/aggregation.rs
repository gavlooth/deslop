use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::identity::{NodeId, NodeLookupError};
use crate::snapshot::FileRevisionKey;

/// Declares how node-local syntax aggregates propagate to ancestors.
///
/// A reset node always retains its own declared-inclusive aggregate. Under `ResetAt`, that value
/// does not propagate into its parent's or File's declared projection; full-inclusive values remain
/// available independently. Callers must supply semantic reset nodes explicitly; the raw syntax
/// layer never guesses callable or region roles from grammar kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InclusiveSyntaxPolicy<'nodes> {
    /// Include every descendant aggregate all the way through the grammar root and file owner.
    AllDescendants,
    /// Stop propagation at each declared node while retaining that node's own inclusive value.
    ResetAt(&'nodes [NodeId]),
}

impl<'nodes> InclusiveSyntaxPolicy<'nodes> {
    pub(crate) fn reset_nodes(self) -> &'nodes [NodeId] {
        match self {
            Self::AllDescendants => &[],
            Self::ResetAt(nodes) => nodes,
        }
    }
}

/// Owned owner context retained in initialization, fold, and merge failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxAggregateOwner {
    File,
    Node(NodeId),
}

/// Identifies which inclusive projection a merge was deriving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxAggregateProjection {
    FullInclusive,
    DeclaredInclusive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxAggregationError<E = std::convert::Infallible> {
    FileNotFound {
        path: PathBuf,
    },
    SyntaxUnavailable {
        path: PathBuf,
    },
    InvalidResetNode {
        node: NodeId,
        error: NodeLookupError,
    },
    ResetNodeOutsideFile {
        node: NodeId,
        path: PathBuf,
    },
    InitializeOwner {
        path: PathBuf,
        owner: SyntaxAggregateOwner,
        error: E,
    },
    FoldRegion {
        path: PathBuf,
        owner: SyntaxAggregateOwner,
        range: Range<usize>,
        error: E,
    },
    Merge {
        path: PathBuf,
        projection: SyntaxAggregateProjection,
        parent: SyntaxAggregateOwner,
        child: SyntaxAggregateOwner,
        error: E,
    },
}

impl<E: fmt::Display> fmt::Display for SyntaxAggregationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileNotFound { path } => {
                write!(formatter, "analysis has no source file {}", path.display())
            }
            Self::SyntaxUnavailable { path } => {
                write!(
                    formatter,
                    "source file {} has no syntax arena",
                    path.display()
                )
            }
            Self::InvalidResetNode { node, error } => {
                write!(
                    formatter,
                    "invalid syntax aggregation reset node {node:?}: {error}"
                )
            }
            Self::ResetNodeOutsideFile { node, path } => write!(
                formatter,
                "syntax aggregation reset node {node:?} is not in {}",
                path.display()
            ),
            Self::InitializeOwner { path, owner, error } => write!(
                formatter,
                "failed to initialize {owner:?} syntax aggregate for {}: {error}",
                path.display()
            ),
            Self::FoldRegion {
                path,
                owner,
                range,
                error,
            } => write!(
                formatter,
                "failed to fold syntax region {}..{} into {owner:?} for {}: {error}",
                range.start,
                range.end,
                path.display()
            ),
            Self::Merge {
                path,
                projection,
                parent,
                child,
                error,
            } => write!(
                formatter,
                "failed to merge {child:?} into {parent:?} for {projection:?} projection of {}: {error}",
                path.display()
            ),
        }
    }
}

impl<E> std::error::Error for SyntaxAggregationError<E>
where
    E: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidResetNode { error, .. } => Some(error),
            Self::InitializeOwner { error, .. }
            | Self::FoldRegion { error, .. }
            | Self::Merge { error, .. } => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxAggregateLookupError {
    WrongAnalysis,
    NodeOutsideFile { requested: u32, path: PathBuf },
}

impl fmt::Display for SyntaxAggregateLookupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongAnalysis => {
                formatter.write_str("node belongs to a different project analysis")
            }
            Self::NodeOutsideFile { requested, path } => write!(
                formatter,
                "node index {requested} is outside syntax aggregation for {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for SyntaxAggregateLookupError {}

/// The direct-owner, full-inclusive, and declared-inclusive values for one raw syntax node.
#[derive(Debug, Clone, Copy)]
pub struct SyntaxNodeAggregate<'aggregate, T> {
    id: NodeId,
    local: &'aggregate T,
    full_inclusive: &'aggregate T,
    declared_inclusive: &'aggregate T,
    resets_parent: bool,
}

impl<'aggregate, T> SyntaxNodeAggregate<'aggregate, T> {
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Once-per-node initializer value plus exclusive regions owned directly by this node.
    pub fn local(&self) -> &'aggregate T {
        self.local
    }

    /// Full value derived bottom-up from this local value and every descendant value.
    pub fn full_inclusive(&self) -> &'aggregate T {
        self.full_inclusive
    }

    /// Inclusive value under the explicitly supplied reset policy.
    ///
    /// This equals `full_inclusive()` for `AllDescendants`. Under `ResetAt`, declared reset
    /// children do not contribute, while this node remains included in its own value.
    pub fn declared_inclusive(&self) -> &'aggregate T {
        self.declared_inclusive
    }

    /// Whether this node's declared-inclusive value is excluded from its parent and File views.
    pub fn resets_parent(&self) -> bool {
        self.resets_parent
    }
}

/// Revision-local local/inclusive aggregate views for one source file.
///
/// Values are process-local and keyed by scan-local `NodeId`; this type deliberately has no Serde
/// implementation. Local values contain the once-per-owner initializer plus directly owned
/// exclusive regions. Full-inclusive and declared reset-aware values are derived bottom-up without
/// revisiting source regions.
#[derive(Debug, Clone)]
pub struct SyntaxAggregates<'analysis, T> {
    analysis_id: &'analysis crate::snapshot::ProjectAnalysisId,
    file_key: &'analysis FileRevisionKey,
    owner: u64,
    file_start: u32,
    file_local: T,
    file_full_inclusive: T,
    file_declared_inclusive: T,
    node_local: Box<[T]>,
    node_full_inclusive: Box<[T]>,
    node_declared_inclusive: Box<[T]>,
    resets_parent: Box<[bool]>,
    reset_nodes: Box<[NodeId]>,
}

pub(crate) struct SyntaxAggregateParts<'analysis, T> {
    pub(crate) analysis_id: &'analysis crate::snapshot::ProjectAnalysisId,
    pub(crate) file_key: &'analysis FileRevisionKey,
    pub(crate) owner: u64,
    pub(crate) file_start: u32,
    pub(crate) file_local: T,
    pub(crate) file_full_inclusive: T,
    pub(crate) file_declared_inclusive: T,
    pub(crate) node_local: Box<[T]>,
    pub(crate) node_full_inclusive: Box<[T]>,
    pub(crate) node_declared_inclusive: Box<[T]>,
    pub(crate) resets_parent: Box<[bool]>,
    pub(crate) reset_nodes: Box<[NodeId]>,
}

impl<'analysis, T> SyntaxAggregates<'analysis, T> {
    pub(crate) fn from_parts(parts: SyntaxAggregateParts<'analysis, T>) -> Self {
        debug_assert_eq!(parts.node_local.len(), parts.node_full_inclusive.len());
        debug_assert_eq!(parts.node_local.len(), parts.node_declared_inclusive.len());
        debug_assert_eq!(parts.node_local.len(), parts.resets_parent.len());
        Self {
            analysis_id: parts.analysis_id,
            file_key: parts.file_key,
            owner: parts.owner,
            file_start: parts.file_start,
            file_local: parts.file_local,
            file_full_inclusive: parts.file_full_inclusive,
            file_declared_inclusive: parts.file_declared_inclusive,
            node_local: parts.node_local,
            node_full_inclusive: parts.node_full_inclusive,
            node_declared_inclusive: parts.node_declared_inclusive,
            resets_parent: parts.resets_parent,
            reset_nodes: parts.reset_nodes,
        }
    }

    pub fn analysis_id(&self) -> &'analysis crate::snapshot::ProjectAnalysisId {
        self.analysis_id
    }

    pub fn file_key(&self) -> &'analysis FileRevisionKey {
        self.file_key
    }

    pub fn path(&self) -> &'analysis Path {
        &self.file_key.path
    }

    /// Once-per-File initializer value plus root-external positive-width regions.
    pub fn file_local(&self) -> &T {
        &self.file_local
    }

    /// File-local value plus the full grammar-root value, independent of reset policy.
    pub fn file_full_inclusive(&self) -> &T {
        &self.file_full_inclusive
    }

    /// File value under the explicitly supplied reset policy; reset subtrees remain separately
    /// available from their node `declared_inclusive()` values.
    pub fn file_declared_inclusive(&self) -> &T {
        &self.file_declared_inclusive
    }

    /// Normalized, deduplicated reset nodes in analysis-global node order.
    pub fn reset_nodes(&self) -> &[NodeId] {
        &self.reset_nodes
    }

    /// The normalized declared policy. `ResetAt(&[])` is equivalent to `AllDescendants`.
    pub fn policy(&self) -> InclusiveSyntaxPolicy<'_> {
        if self.reset_nodes.is_empty() {
            InclusiveSyntaxPolicy::AllDescendants
        } else {
            InclusiveSyntaxPolicy::ResetAt(&self.reset_nodes)
        }
    }

    pub fn len(&self) -> usize {
        self.node_local.len()
    }

    pub fn is_empty(&self) -> bool {
        self.node_local.is_empty()
    }

    pub fn node(
        &self,
        id: NodeId,
    ) -> Result<SyntaxNodeAggregate<'_, T>, SyntaxAggregateLookupError> {
        if id.owner != self.owner {
            return Err(SyntaxAggregateLookupError::WrongAnalysis);
        }
        let Some(offset) = id
            .index
            .checked_sub(self.file_start)
            .filter(|offset| (*offset as usize) < self.node_local.len())
        else {
            return Err(SyntaxAggregateLookupError::NodeOutsideFile {
                requested: id.index,
                path: self.file_key.path.clone(),
            });
        };
        Ok(self.node_at(offset as usize))
    }

    /// Iterate all node aggregates in deterministic grammar preorder.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = SyntaxNodeAggregate<'_, T>> + '_ {
        (0..self.node_local.len()).map(|offset| self.node_at(offset))
    }

    fn node_at(&self, offset: usize) -> SyntaxNodeAggregate<'_, T> {
        SyntaxNodeAggregate {
            id: NodeId {
                owner: self.owner,
                index: self.file_start + offset as u32,
            },
            local: &self.node_local[offset],
            full_inclusive: &self.node_full_inclusive[offset],
            declared_inclusive: &self.node_declared_inclusive[offset],
            resets_parent: self.resets_parent[offset],
        }
    }
}
