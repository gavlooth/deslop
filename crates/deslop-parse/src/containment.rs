use anyhow::{Result, bail};

use crate::arena::{
    ArenaNodeIndex, ArenaSegmentIndex, SyntaxNode, SyntaxSegment, SyntaxSegmentOwner,
};

/// Immutable indices derived from one validated preorder arena and its exclusive byte partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContainmentIndex {
    subtree_ends: Box<[u32]>,
    depths: Box<[u32]>,
    minimal_zero_width_nodes: Box<[(usize, u32)]>,
}

impl ContainmentIndex {
    pub(crate) fn build(nodes: &[SyntaxNode], segments: &[SyntaxSegment]) -> Result<Self> {
        let mut subtree_ends = (1..=nodes.len())
            .map(u32::try_from)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (raw_index, node) in nodes.iter().enumerate().rev() {
            let index = ArenaNodeIndex::from_usize(raw_index)
                .expect("syntax arena construction already bounded node indices");
            if let Some(parent) = node.parent() {
                if parent >= index {
                    bail!("syntax preorder parent is not before child {raw_index}");
                }
                subtree_ends[parent.as_usize()] =
                    subtree_ends[parent.as_usize()].max(subtree_ends[raw_index]);
            }
        }

        for (raw_index, node) in nodes.iter().enumerate() {
            let end = subtree_ends[raw_index] as usize;
            if end <= raw_index || end > nodes.len() {
                bail!("syntax subtree {raw_index} has invalid end {end}");
            }
            if raw_index == 0 {
                if node.parent().is_some() {
                    bail!("syntax preorder root has a parent");
                }
            } else if node.parent().is_none() {
                bail!("non-root syntax node {raw_index} has no parent");
            }
            let mut expected_child = raw_index + 1;
            for child in node.children() {
                let child = child.as_usize();
                if child <= raw_index || child >= end {
                    bail!("syntax child {child} escapes preorder subtree {raw_index}..{end}");
                }
                if child != expected_child {
                    bail!(
                        "syntax child {child} is not contiguous after preorder slot {expected_child}"
                    );
                }
                expected_child = subtree_ends[child] as usize;
            }
            if expected_child != end {
                bail!("syntax subtree {raw_index} has unowned slots before end {end}");
            }
        }

        let mut depths = Vec::with_capacity(nodes.len());
        for node in nodes {
            let depth = node
                .parent()
                .map_or(0, |parent| depths[parent.as_usize()] + 1);
            depths.push(depth);
        }

        let zero_width_nodes = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                let span = node.span();
                (span.start_byte() == span.end_byte()).then_some((span.start_byte(), index as u32))
            })
            .collect::<Vec<_>>();
        if !zero_width_nodes
            .windows(2)
            .all(|nodes| nodes[0] <= nodes[1])
        {
            bail!("zero-width syntax nodes are not in byte/preorder order");
        }
        let minimal_zero_width_nodes = zero_width_nodes
            .iter()
            .enumerate()
            .filter_map(|(position, &(byte, index))| {
                let ancestor = ArenaNodeIndex::from_u32(index);
                let owns_deeper_zero =
                    zero_width_nodes
                        .get(position + 1)
                        .is_some_and(|&(next_byte, next_index)| {
                            byte == next_byte
                                && ancestor.as_usize() <= next_index as usize
                                && (next_index as usize)
                                    < subtree_ends[ancestor.as_usize()] as usize
                        });
                (!owns_deeper_zero).then_some((byte, index))
            })
            .collect::<Box<_>>();

        if !segments.windows(2).all(|regions| {
            regions[0].byte_range().end == regions[1].byte_range().start
                && regions[0].byte_range().end < regions[1].byte_range().end
        }) {
            bail!("exclusive syntax regions are not strictly ordered");
        }

        Ok(Self {
            subtree_ends: subtree_ends.into_boxed_slice(),
            depths: depths.into_boxed_slice(),
            minimal_zero_width_nodes,
        })
    }

    pub(crate) fn subtree_end(&self, node: ArenaNodeIndex) -> Option<ArenaNodeIndex> {
        let end = *self.subtree_ends.get(node.as_usize())?;
        Some(ArenaNodeIndex::from_u32(end))
    }

    pub(crate) fn contains(&self, ancestor: ArenaNodeIndex, descendant: ArenaNodeIndex) -> bool {
        self.subtree_end(ancestor).is_some_and(|end| {
            ancestor.as_usize() <= descendant.as_usize() && descendant.as_usize() < end.as_usize()
        })
    }

    pub(crate) fn exclusive_region_at(
        &self,
        segments: &[SyntaxSegment],
        byte: usize,
    ) -> Option<ArenaSegmentIndex> {
        let index = segments.partition_point(|segment| segment.byte_range().end <= byte);
        (index < segments.len()).then(|| ArenaSegmentIndex::from_usize(index))
    }

    pub(crate) fn smallest_containing_node(
        &self,
        nodes: &[SyntaxNode],
        segments: &[SyntaxSegment],
        start: usize,
        end: usize,
    ) -> Option<ArenaNodeIndex> {
        debug_assert!(start < end);
        let start = self.exclusive_region_at(segments, start)?;
        let end = self.exclusive_region_at(segments, end - 1)?;
        let SyntaxSegmentOwner::Node(start) = segments[start.as_usize()].owner() else {
            return None;
        };
        let SyntaxSegmentOwner::Node(end) = segments[end.as_usize()].owner() else {
            return None;
        };
        Some(self.lowest_common_ancestor(nodes, start, end))
    }

    pub(crate) fn zero_width_nodes_at(&self, point: usize) -> &[(usize, u32)] {
        let start = self
            .minimal_zero_width_nodes
            .partition_point(|(byte, _)| *byte < point);
        let end = self
            .minimal_zero_width_nodes
            .partition_point(|(byte, _)| *byte <= point);
        &self.minimal_zero_width_nodes[start..end]
    }

    fn lowest_common_ancestor(
        &self,
        nodes: &[SyntaxNode],
        mut left: ArenaNodeIndex,
        mut right: ArenaNodeIndex,
    ) -> ArenaNodeIndex {
        while self.depths[left.as_usize()] > self.depths[right.as_usize()] {
            left = nodes[left.as_usize()]
                .parent()
                .expect("deeper syntax node has a parent");
        }
        while self.depths[right.as_usize()] > self.depths[left.as_usize()] {
            right = nodes[right.as_usize()]
                .parent()
                .expect("deeper syntax node has a parent");
        }
        while left != right {
            left = nodes[left.as_usize()]
                .parent()
                .expect("same-file syntax nodes share a root");
            right = nodes[right.as_usize()]
                .parent()
                .expect("same-file syntax nodes share a root");
        }
        left
    }
}
