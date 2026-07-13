use std::ops::Range;

use anyhow::{Result, bail};
use tree_sitter::{Node, Tree};

use crate::snapshot::GrammarSelection;

pub(crate) const RAW_ARENA_SCHEMA: &str = "deslop-raw-arena/1";

/// A deterministic, file-local slot in an owned syntax arena.
///
/// This is intentionally not the revision-bound `NodeId` introduced by M1.4. It is meaningful only
/// while used with the `SyntaxArena` that returned it and is never a wire identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ArenaNodeIndex(u32);

impl ArenaNodeIndex {
    pub fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ArenaSegmentIndex(u32);

impl ArenaSegmentIndex {
    pub fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SourcePoint {
    row: usize,
    /// Zero-based byte offset within the row, not a character or UTF-16 column.
    column: usize,
}

impl SourcePoint {
    pub fn row(self) -> usize {
        self.row
    }

    pub fn column(self) -> usize {
        self.column
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SyntaxSpan {
    start_byte: usize,
    end_byte: usize,
    start_point: SourcePoint,
    end_point: SourcePoint,
}

impl SyntaxSpan {
    pub fn start_byte(self) -> usize {
        self.start_byte
    }

    pub fn end_byte(self) -> usize {
        self.end_byte
    }

    pub fn byte_range(self) -> Range<usize> {
        self.start_byte..self.end_byte
    }

    pub fn start_point(self) -> SourcePoint {
        self.start_point
    }

    pub fn end_point(self) -> SourcePoint {
        self.end_point
    }
}

/// Raw byte ownership, not a language-semantic token classification.
///
/// `Token` is a positive-width leaf outside a non-error Tree-sitter `extra` subtree. `Trivia` is a
/// direct-child gap, bytes under a non-error `extra` subtree (normally comments), or source bytes
/// before/after the grammar root. Recovery `ERROR` nodes remain tokens even when Tree-sitter marks
/// them extra. Together the segments partition the source exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SyntaxSegmentKind {
    Token,
    Trivia,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SyntaxSegmentOwner {
    File,
    Node(ArenaNodeIndex),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxSegment {
    kind: SyntaxSegmentKind,
    start_byte: usize,
    end_byte: usize,
    owner: SyntaxSegmentOwner,
}

impl SyntaxSegment {
    pub fn kind(&self) -> SyntaxSegmentKind {
        self.kind
    }

    pub fn byte_range(&self) -> Range<usize> {
        self.start_byte..self.end_byte
    }

    pub fn owner(&self) -> SyntaxSegmentOwner {
        self.owner
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxNode {
    raw_kind: Box<str>,
    raw_kind_id: u16,
    raw_grammar_kind: Box<str>,
    raw_grammar_kind_id: u16,
    field: Option<Box<str>>,
    span: SyntaxSpan,
    parent: Option<ArenaNodeIndex>,
    children: Box<[ArenaNodeIndex]>,
    owned_segments: Box<[ArenaSegmentIndex]>,
    named: bool,
    extra: bool,
    error: bool,
    missing: bool,
    has_error: bool,
}

impl SyntaxNode {
    pub fn raw_kind(&self) -> &str {
        &self.raw_kind
    }

    pub fn raw_kind_id(&self) -> u16 {
        self.raw_kind_id
    }

    pub fn raw_grammar_kind(&self) -> &str {
        &self.raw_grammar_kind
    }

    pub fn raw_grammar_kind_id(&self) -> u16 {
        self.raw_grammar_kind_id
    }

    pub fn field(&self) -> Option<&str> {
        self.field.as_deref()
    }

    pub fn span(&self) -> SyntaxSpan {
        self.span
    }

    pub fn parent(&self) -> Option<ArenaNodeIndex> {
        self.parent
    }

    pub fn children(&self) -> &[ArenaNodeIndex] {
        &self.children
    }

    pub fn owned_segment_indices(&self) -> &[ArenaSegmentIndex] {
        &self.owned_segments
    }

    pub fn is_named(&self) -> bool {
        self.named
    }

    pub fn is_extra(&self) -> bool {
        self.extra
    }

    pub fn is_error(&self) -> bool {
        self.error
    }

    pub fn is_missing(&self) -> bool {
        self.missing
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxArena {
    grammar: GrammarSelection,
    root: ArenaNodeIndex,
    source_len: usize,
    nodes: Box<[SyntaxNode]>,
    segments: Box<[SyntaxSegment]>,
}

impl SyntaxArena {
    pub(crate) fn from_tree(tree: &Tree, source: &[u8], grammar: GrammarSelection) -> Result<Self> {
        let root = tree.root_node();
        if root.start_byte() > root.end_byte() || root.end_byte() > source.len() {
            bail!(
                "syntax root has invalid coverage {}..{} for source 0..{}",
                root.start_byte(),
                root.end_byte(),
                source.len()
            );
        }

        let mut builders = Vec::<NodeBuilder>::new();
        let mut pending: Vec<(Node<'_>, Option<ArenaNodeIndex>, Option<&'static str>)> =
            vec![(root, None, None)];
        while let Some((node, parent, field)) = pending.pop() {
            let index = arena_node_index(builders.len())?;
            let span = syntax_span(node);
            let within_extra = (node.is_extra() && !node.is_error())
                || parent.is_some_and(|parent| builders[parent.as_usize()].within_extra);
            if let Some(parent_index) = parent {
                let parent_node = &mut builders[parent_index.as_usize()];
                if span.start_byte < parent_node.span.start_byte
                    || span.end_byte > parent_node.span.end_byte
                {
                    bail!(
                        "child {} span {}..{} escapes parent {} span {}..{}",
                        node.kind(),
                        span.start_byte,
                        span.end_byte,
                        parent_node.raw_kind,
                        parent_node.span.start_byte,
                        parent_node.span.end_byte
                    );
                }
                parent_node.children.push(index);
            }
            builders.push(NodeBuilder {
                raw_kind: node.kind().into(),
                raw_kind_id: node.kind_id(),
                raw_grammar_kind: node.grammar_name().into(),
                raw_grammar_kind_id: node.grammar_id(),
                field: field.map(Into::into),
                span,
                parent,
                children: Vec::new(),
                owned_segments: Vec::new(),
                named: node.is_named(),
                extra: node.is_extra(),
                error: node.is_error(),
                missing: node.is_missing(),
                has_error: node.has_error(),
                within_extra,
            });

            let mut cursor = node.walk();
            let children = node
                .children(&mut cursor)
                .enumerate()
                .map(|(child_index, child)| {
                    (
                        child,
                        Some(index),
                        node.field_name_for_child(child_index as u32),
                    )
                })
                .collect::<Vec<_>>();
            pending.extend(children.into_iter().rev());
        }

        let mut segments = Vec::new();
        for index in 0..builders.len() {
            collect_exclusive_segments(arena_node_index(index)?, &builders, &mut segments)?;
        }
        push_segment(
            &mut segments,
            SyntaxSegmentKind::Trivia,
            0,
            root.start_byte(),
            SyntaxSegmentOwner::File,
        );
        push_segment(
            &mut segments,
            SyntaxSegmentKind::Trivia,
            root.end_byte(),
            source.len(),
            SyntaxSegmentOwner::File,
        );
        segments.sort_by_key(|segment| (segment.start_byte, segment.end_byte));
        validate_partition(&segments, source.len())?;

        for (index, segment) in segments.iter().enumerate() {
            if let SyntaxSegmentOwner::Node(owner) = segment.owner {
                builders[owner.as_usize()]
                    .owned_segments
                    .push(arena_segment_index(index)?);
            }
        }

        let nodes = builders
            .into_iter()
            .map(NodeBuilder::finish)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Ok(Self {
            grammar,
            root: ArenaNodeIndex(0),
            source_len: source.len(),
            nodes,
            segments: segments.into_boxed_slice(),
        })
    }

    pub fn schema(&self) -> &'static str {
        RAW_ARENA_SCHEMA
    }

    pub fn grammar(&self) -> &GrammarSelection {
        &self.grammar
    }

    pub fn root(&self) -> ArenaNodeIndex {
        self.root
    }

    pub fn source_len(&self) -> usize {
        self.source_len
    }

    pub fn nodes(&self) -> &[SyntaxNode] {
        &self.nodes
    }

    pub fn indexed_nodes(&self) -> impl ExactSizeIterator<Item = (ArenaNodeIndex, &SyntaxNode)> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (ArenaNodeIndex(index as u32), node))
    }

    pub fn node(&self, index: ArenaNodeIndex) -> Option<&SyntaxNode> {
        self.nodes.get(index.as_usize())
    }

    pub fn segments(&self) -> &[SyntaxSegment] {
        &self.segments
    }

    pub fn indexed_segments(
        &self,
    ) -> impl ExactSizeIterator<Item = (ArenaSegmentIndex, &SyntaxSegment)> {
        self.segments
            .iter()
            .enumerate()
            .map(|(index, segment)| (ArenaSegmentIndex(index as u32), segment))
    }

    pub fn segment(&self, index: ArenaSegmentIndex) -> Option<&SyntaxSegment> {
        self.segments.get(index.as_usize())
    }

    pub fn node_source<'a>(&self, source: &'a [u8], index: ArenaNodeIndex) -> Option<&'a [u8]> {
        let range = self.node(index)?.span.byte_range();
        source.get(range)
    }

    pub fn segment_source<'a>(
        &self,
        source: &'a [u8],
        index: ArenaSegmentIndex,
    ) -> Option<&'a [u8]> {
        source.get(self.segment(index)?.byte_range())
    }
}

#[derive(Debug)]
struct NodeBuilder {
    raw_kind: Box<str>,
    raw_kind_id: u16,
    raw_grammar_kind: Box<str>,
    raw_grammar_kind_id: u16,
    field: Option<Box<str>>,
    span: SyntaxSpan,
    parent: Option<ArenaNodeIndex>,
    children: Vec<ArenaNodeIndex>,
    owned_segments: Vec<ArenaSegmentIndex>,
    named: bool,
    extra: bool,
    error: bool,
    missing: bool,
    has_error: bool,
    within_extra: bool,
}

impl NodeBuilder {
    fn finish(self) -> SyntaxNode {
        SyntaxNode {
            raw_kind: self.raw_kind,
            raw_kind_id: self.raw_kind_id,
            raw_grammar_kind: self.raw_grammar_kind,
            raw_grammar_kind_id: self.raw_grammar_kind_id,
            field: self.field,
            span: self.span,
            parent: self.parent,
            children: self.children.into_boxed_slice(),
            owned_segments: self.owned_segments.into_boxed_slice(),
            named: self.named,
            extra: self.extra,
            error: self.error,
            missing: self.missing,
            has_error: self.has_error,
        }
    }
}

fn syntax_span(node: Node<'_>) -> SyntaxSpan {
    let start = node.start_position();
    let end = node.end_position();
    SyntaxSpan {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_point: SourcePoint {
            row: start.row,
            column: start.column,
        },
        end_point: SourcePoint {
            row: end.row,
            column: end.column,
        },
    }
}

fn collect_exclusive_segments(
    owner: ArenaNodeIndex,
    nodes: &[NodeBuilder],
    out: &mut Vec<SyntaxSegment>,
) -> Result<()> {
    let node = &nodes[owner.as_usize()];
    if node.children.is_empty() {
        push_segment(
            out,
            if node.within_extra {
                SyntaxSegmentKind::Trivia
            } else {
                SyntaxSegmentKind::Token
            },
            node.span.start_byte,
            node.span.end_byte,
            SyntaxSegmentOwner::Node(owner),
        );
        return Ok(());
    }

    let mut cursor = node.span.start_byte;
    for child_index in &node.children {
        let child = &nodes[child_index.as_usize()];
        if child.span.start_byte < cursor && child.span.start_byte < child.span.end_byte {
            bail!("children of {} overlap at byte {cursor}", node.raw_kind);
        }
        if child.span.start_byte > cursor {
            push_segment(
                out,
                SyntaxSegmentKind::Trivia,
                cursor,
                child.span.start_byte,
                SyntaxSegmentOwner::Node(owner),
            );
        }
        cursor = cursor.max(child.span.end_byte);
    }
    if cursor < node.span.end_byte {
        push_segment(
            out,
            SyntaxSegmentKind::Trivia,
            cursor,
            node.span.end_byte,
            SyntaxSegmentOwner::Node(owner),
        );
    }
    Ok(())
}

fn push_segment(
    out: &mut Vec<SyntaxSegment>,
    kind: SyntaxSegmentKind,
    start_byte: usize,
    end_byte: usize,
    owner: SyntaxSegmentOwner,
) {
    if start_byte < end_byte {
        out.push(SyntaxSegment {
            kind,
            start_byte,
            end_byte,
            owner,
        });
    }
}

fn validate_partition(segments: &[SyntaxSegment], source_len: usize) -> Result<()> {
    let mut cursor = 0;
    for segment in segments {
        if segment.start_byte != cursor {
            bail!(
                "syntax token/trivia partition expected byte {cursor}, found {}..{}",
                segment.start_byte,
                segment.end_byte
            );
        }
        if segment.end_byte <= segment.start_byte || segment.end_byte > source_len {
            bail!(
                "invalid syntax segment {}..{} for source length {source_len}",
                segment.start_byte,
                segment.end_byte
            );
        }
        cursor = segment.end_byte;
    }
    if cursor != source_len {
        bail!("syntax token/trivia partition ends at {cursor}, expected {source_len}");
    }
    Ok(())
}

fn arena_node_index(index: usize) -> Result<ArenaNodeIndex> {
    Ok(ArenaNodeIndex(u32::try_from(index).map_err(|_| {
        anyhow::anyhow!("syntax arena has more than {} nodes", u32::MAX)
    })?))
}

fn arena_segment_index(index: usize) -> Result<ArenaSegmentIndex> {
    Ok(ArenaSegmentIndex(u32::try_from(index).map_err(|_| {
        anyhow::anyhow!("syntax arena has more than {} segments", u32::MAX)
    })?))
}
