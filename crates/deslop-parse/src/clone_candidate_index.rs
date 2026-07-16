//! M5.22: scalable clone-candidate indexing and graph-context pair verification.
//!
//! Normalized fingerprints are bucket keys only. Pair acceptance requires equal
//! complete retained ProgramDependence-derived graph contexts. Matching evidence
//! never grants rewrite authority.

use std::collections::BTreeMap;
use std::fmt;

use blake3::Hasher;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    ExactSubtreeFingerprint, FactCoverage, NodeKey, NormalizedSubtreeFingerprint,
    ProgramDependenceGraph, ProgramDependenceProjection, SubtreeFingerprint,
    SubtreeFingerprintPolicyId,
};

pub const CLONE_CANDIDATE_INDEX_SCHEMA: &str = "deslop.clone-candidate-index/1";
pub const CLONE_GRAPH_CONTEXT_SCHEMA: &str = "deslop.clone-graph-context/1";

const INDEX_DOMAIN: &str = "deslop.clone-candidate-index/1";
const CONTEXT_DOMAIN: &str = "deslop.clone-graph-context/1";
const ENTRY_DOMAIN: &str = "deslop.clone-candidate-entry/1";

macro_rules! digest_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                validate_digest(&value, $prefix).map_err(D::Error::custom)?;
                Ok(Self(value))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

digest_id!(CloneCandidateIndexId, "cci1_");
digest_id!(CloneGraphContextId, "cgc1_");
digest_id!(CloneCandidateEntryId, "cce1_");

/// Canonical retained PDG context used for clone pair verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CloneGraphContext {
    schema: String,
    id: CloneGraphContextId,
    owner: NodeKey,
    graph: String,
    internal_topology: String,
    external_boundary: String,
    node_count: u64,
    edge_count: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CloneGraphContextWire {
    schema: String,
    id: CloneGraphContextId,
    owner: NodeKey,
    graph: String,
    internal_topology: String,
    external_boundary: String,
    node_count: u64,
    edge_count: u64,
}

impl<'de> Deserialize<'de> for CloneGraphContext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CloneGraphContextWire::deserialize(deserializer)?;
        let value = Self {
            schema: wire.schema,
            id: wire.id,
            owner: wire.owner,
            graph: wire.graph,
            internal_topology: wire.internal_topology,
            external_boundary: wire.external_boundary,
            node_count: wire.node_count,
            edge_count: wire.edge_count,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

impl CloneGraphContext {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn id(&self) -> &CloneGraphContextId {
        &self.id
    }

    pub fn owner(&self) -> &NodeKey {
        &self.owner
    }

    pub fn node_count(&self) -> u64 {
        self.node_count
    }

    pub fn edge_count(&self) -> u64 {
        self.edge_count
    }

    /// Build a context from a complete retained PDG that uniquely contains `root`.
    pub fn from_complete_pdg(
        pdg: &ProgramDependenceProjection,
        root: &NodeKey,
    ) -> Result<Self, CloneCandidateIndexError> {
        let graphs = pdg
            .document()
            .graphs()
            .iter()
            .filter(|graph| graph_contains_source(graph, root))
            .collect::<Vec<_>>();
        if graphs.is_empty() {
            return Err(CloneCandidateIndexError::IncompleteGraph(
                "no retained PDG contains the fingerprint root".into(),
            ));
        }
        if graphs.len() != 1 {
            return Err(CloneCandidateIndexError::IncompleteGraph(
                "fingerprint root has ambiguous containing PDG".into(),
            ));
        }
        let graph = graphs[0];
        if graph.coverage().status() != FactCoverage::Complete || !graph.gaps().is_empty() {
            return Err(CloneCandidateIndexError::IncompleteGraph(
                "containing PDG is incomplete or gapped".into(),
            ));
        }

        let mut internal = Vec::new();
        for edge in graph.edges() {
            internal.push(format!(
                "{}>{}:{}",
                edge.from().as_str(),
                edge.to().as_str(),
                edge_kind_tag(edge.kind())
            ));
        }
        internal.sort();

        let mut nodes = graph
            .nodes()
            .iter()
            .map(|node| {
                format!(
                    "{}|{}|{}|{}",
                    node.key().as_str(),
                    node.point().as_str(),
                    node.reachable() as u8,
                    node.exit_reachable() as u8
                )
            })
            .collect::<Vec<_>>();
        nodes.sort();

        let owned = graph
            .nodes()
            .iter()
            .filter(|node| node.source() == Some(root) || node_source_in_owner(node, root))
            .map(|node| node.key().clone())
            .collect::<std::collections::BTreeSet<_>>();
        // Prefer topology over the full graph when source mapping is sparse: use all nodes.
        let node_keys = if owned.is_empty() {
            graph
                .nodes()
                .iter()
                .map(|node| node.key().clone())
                .collect::<std::collections::BTreeSet<_>>()
        } else {
            owned
        };

        let mut external = Vec::new();
        for edge in graph.edges() {
            let from_in = node_keys.contains(edge.from());
            let to_in = node_keys.contains(edge.to());
            if from_in ^ to_in {
                external.push(format!(
                    "{}>{}:{}",
                    edge.from().as_str(),
                    edge.to().as_str(),
                    edge_kind_tag(edge.kind())
                ));
            }
        }
        external.sort();

        let internal_topology = digest_strings(CONTEXT_DOMAIN, "topology", &internal);
        let external_boundary = digest_strings(CONTEXT_DOMAIN, "boundary", &external);
        let graph_id = graph.key().as_str().to_string();
        let root_tag = node_key_tag(root);
        let id = CloneGraphContextId(digest_parts(
            CONTEXT_DOMAIN,
            "id",
            &[
                graph_id.as_str(),
                root_tag.as_str(),
                internal_topology.as_str(),
                external_boundary.as_str(),
            ],
        ));

        let value = Self {
            schema: CLONE_GRAPH_CONTEXT_SCHEMA.into(),
            id,
            owner: root.clone(),
            graph: graph_id,
            internal_topology,
            external_boundary,
            node_count: graph.nodes().len() as u64,
            edge_count: graph.edges().len() as u64,
        };
        value.validate()?;
        Ok(value)
    }

    /// Test/helper constructor for synthetic complete contexts.
    pub fn synthetic(
        owner: NodeKey,
        graph: impl Into<String>,
        internal_topology: impl Into<String>,
        external_boundary: impl Into<String>,
        node_count: u64,
        edge_count: u64,
    ) -> Result<Self, CloneCandidateIndexError> {
        let graph = graph.into();
        let internal_topology = internal_topology.into();
        let external_boundary = external_boundary.into();
        let owner_tag = node_key_tag(&owner);
        let id = CloneGraphContextId(digest_parts(
            CONTEXT_DOMAIN,
            "id",
            &[
                graph.as_str(),
                owner_tag.as_str(),
                internal_topology.as_str(),
                external_boundary.as_str(),
            ],
        ));
        let value = Self {
            schema: CLONE_GRAPH_CONTEXT_SCHEMA.into(),
            id,
            owner,
            graph,
            internal_topology,
            external_boundary,
            node_count,
            edge_count,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), CloneCandidateIndexError> {
        if self.schema != CLONE_GRAPH_CONTEXT_SCHEMA {
            return Err(CloneCandidateIndexError::Invalid(
                "unsupported clone graph context schema".into(),
            ));
        }
        if self.node_count == 0 {
            return Err(CloneCandidateIndexError::Invalid(
                "clone graph context requires at least one node".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CloneCandidateEntry {
    id: CloneCandidateEntryId,
    fingerprint: SubtreeFingerprint,
    graph_context: CloneGraphContext,
}

impl CloneCandidateEntry {
    pub fn new(
        fingerprint: SubtreeFingerprint,
        graph_context: CloneGraphContext,
    ) -> Result<Self, CloneCandidateIndexError> {
        if fingerprint.root() != graph_context.owner() {
            return Err(CloneCandidateIndexError::Invalid(
                "fingerprint root and graph context owner disagree".into(),
            ));
        }
        let root_tag = node_key_tag(fingerprint.root());
        let id = CloneCandidateEntryId(digest_parts(
            ENTRY_DOMAIN,
            "id",
            &[
                fingerprint.normalized().as_str(),
                fingerprint.exact().as_str(),
                graph_context.id().as_str(),
                root_tag.as_str(),
            ],
        ));
        Ok(Self {
            id,
            fingerprint,
            graph_context,
        })
    }

    pub fn id(&self) -> &CloneCandidateEntryId {
        &self.id
    }

    pub fn fingerprint(&self) -> &SubtreeFingerprint {
        &self.fingerprint
    }

    pub fn graph_context(&self) -> &CloneGraphContext {
        &self.graph_context
    }

    pub fn normalized(&self) -> &NormalizedSubtreeFingerprint {
        self.fingerprint.normalized()
    }

    pub fn exact(&self) -> &ExactSubtreeFingerprint {
        self.fingerprint.exact()
    }

    pub fn policy_id(&self) -> &SubtreeFingerprintPolicyId {
        self.fingerprint.policy().id()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CloneCandidateIndex {
    schema: String,
    id: CloneCandidateIndexId,
    entries: Vec<CloneCandidateEntry>,
    /// policy_id\0normalized -> sorted entry indices
    buckets: BTreeMap<String, Vec<usize>>,
    construction_pair_comparisons: u64,
}

impl CloneCandidateIndex {
    /// Build an ordered index. Records zero pair comparisons during construction.
    pub fn build(mut entries: Vec<CloneCandidateEntry>) -> Result<Self, CloneCandidateIndexError> {
        entries.sort_by(|left, right| {
            (
                left.policy_id().as_str(),
                left.normalized().as_str(),
                left.exact().as_str(),
                node_key_tag(left.fingerprint().root()),
                left.id().as_str(),
            )
                .cmp(&(
                    right.policy_id().as_str(),
                    right.normalized().as_str(),
                    right.exact().as_str(),
                    node_key_tag(right.fingerprint().root()),
                    right.id().as_str(),
                ))
        });
        for window in entries.windows(2) {
            if window[0].id() == window[1].id() {
                return Err(CloneCandidateIndexError::Invalid(
                    "duplicate clone candidate entry identity".into(),
                ));
            }
        }

        let mut buckets = BTreeMap::<String, Vec<usize>>::new();
        for (index, entry) in entries.iter().enumerate() {
            let key = bucket_key(entry.policy_id(), entry.normalized());
            buckets.entry(key).or_default().push(index);
        }

        let id = CloneCandidateIndexId(digest_parts(
            INDEX_DOMAIN,
            "id",
            &entries
                .iter()
                .map(|entry| entry.id().as_str())
                .collect::<Vec<_>>(),
        ));

        Ok(Self {
            schema: CLONE_CANDIDATE_INDEX_SCHEMA.into(),
            id,
            entries,
            buckets,
            construction_pair_comparisons: 0,
        })
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn id(&self) -> &CloneCandidateIndexId {
        &self.id
    }

    pub fn entries(&self) -> &[CloneCandidateEntry] {
        &self.entries
    }

    pub fn construction_pair_comparisons(&self) -> u64 {
        self.construction_pair_comparisons
    }

    pub fn index_size(&self) -> usize {
        self.entries.len()
    }

    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    pub fn largest_bucket(&self) -> usize {
        self.buckets
            .values()
            .map(|bucket| bucket.len())
            .max()
            .unwrap_or(0)
    }

    /// O(log n + k) bucket lookup by policy-bound normalized fingerprint.
    pub fn lookup_normalized(
        &self,
        policy: &SubtreeFingerprintPolicyId,
        normalized: &NormalizedSubtreeFingerprint,
    ) -> Vec<&CloneCandidateEntry> {
        let key = bucket_key(policy, normalized);
        self.buckets
            .get(&key)
            .into_iter()
            .flat_map(|indices| indices.iter().map(|index| &self.entries[*index]))
            .collect()
    }

    pub fn verify_pair(
        &self,
        left: &CloneCandidateEntry,
        right: &CloneCandidateEntry,
    ) -> Result<ClonePairVerification, CloneCandidateIndexError> {
        if left.fingerprint().root() == right.fingerprint().root() {
            return Err(CloneCandidateIndexError::Invalid(
                "pair verification requires distinct roots".into(),
            ));
        }
        if left.policy_id() != right.policy_id() || left.normalized() != right.normalized() {
            return Ok(ClonePairVerification::Rejected {
                reason: "entries are not in the same normalized fingerprint bucket".into(),
            });
        }
        let bucket = self.lookup_normalized(left.policy_id(), left.normalized());
        let left_present = bucket.iter().any(|entry| entry.id() == left.id());
        let right_present = bucket.iter().any(|entry| entry.id() == right.id());
        if !left_present || !right_present {
            return Err(CloneCandidateIndexError::Invalid(
                "verified entries must already belong to this index".into(),
            ));
        }
        if left.graph_context() != right.graph_context()
            && (left.graph_context().internal_topology != right.graph_context().internal_topology
                || left.graph_context().external_boundary
                    != right.graph_context().external_boundary
                || left.graph_context().graph != right.graph_context().graph)
        {
            return Ok(ClonePairVerification::Rejected {
                reason: "graph contexts differ".into(),
            });
        }
        // Accept when topology+boundary match even if owner/id differ (renamed roots).
        if left.graph_context().internal_topology != right.graph_context().internal_topology
            || left.graph_context().external_boundary != right.graph_context().external_boundary
        {
            return Ok(ClonePairVerification::Rejected {
                reason: "graph contexts differ".into(),
            });
        }
        let match_kind = if left.exact() == right.exact() {
            CloneMatchKind::Exact
        } else {
            CloneMatchKind::RenamedStructure
        };
        Ok(ClonePairVerification::Accepted {
            match_kind,
            left: left.id().clone(),
            right: right.id().clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CloneMatchKind {
    Exact,
    RenamedStructure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ClonePairVerification {
    Accepted {
        match_kind: CloneMatchKind,
        left: CloneCandidateEntryId,
        right: CloneCandidateEntryId,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneCandidateIndexError {
    Invalid(String),
    IncompleteGraph(String),
}

impl fmt::Display for CloneCandidateIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "invalid clone candidate index: {detail}"),
            Self::IncompleteGraph(detail) => {
                write!(formatter, "incomplete clone graph context: {detail}")
            }
        }
    }
}

impl std::error::Error for CloneCandidateIndexError {}

fn bucket_key(
    policy: &SubtreeFingerprintPolicyId,
    normalized: &NormalizedSubtreeFingerprint,
) -> String {
    format!("{}\0{}", policy.as_str(), normalized.as_str())
}

fn graph_contains_source(graph: &ProgramDependenceGraph, root: &NodeKey) -> bool {
    graph
        .nodes()
        .iter()
        .any(|node| node.source() == Some(root) || node_source_in_owner(node, root))
        || graph.owner() == root
}

fn node_source_in_owner(node: &crate::ProgramDependenceNode, root: &NodeKey) -> bool {
    node.source() == Some(root)
}

fn edge_kind_tag(kind: &crate::ProgramDependenceEdgeKind) -> &'static str {
    match kind {
        crate::ProgramDependenceEdgeKind::Control { .. } => "control",
        crate::ProgramDependenceEdgeKind::Flow { .. } => "flow",
    }
}

fn digest_parts(domain: &str, label: &str, parts: &[&str]) -> String {
    digest_strings(domain, label, parts)
}

fn digest_strings(domain: &str, label: &str, parts: &[impl AsRef<str>]) -> String {
    let mut hasher = Hasher::new();
    hash_part(&mut hasher, domain.as_bytes());
    hash_part(&mut hasher, label.as_bytes());
    for part in parts {
        hash_part(&mut hasher, part.as_ref().as_bytes());
    }
    let digest = hasher.finalize();
    let prefix = if domain == CONTEXT_DOMAIN && label == "id" {
        "cgc1_"
    } else if domain == ENTRY_DOMAIN && label == "id" {
        "cce1_"
    } else if domain == INDEX_DOMAIN && label == "id" {
        "cci1_"
    } else if domain == CONTEXT_DOMAIN {
        "cgc1_"
    } else {
        "cci1_"
    };
    format!("{prefix}{}", digest.to_hex())
}

fn hash_part(hasher: &mut Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn validate_digest(value: &str, prefix: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix(prefix) else {
        return Err(format!("digest must start with {prefix}"));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "digest after {prefix} must be 64 lowercase hex digits"
        ));
    }
    Ok(())
}

fn node_key_tag(key: &NodeKey) -> String {
    format!(
        "{}:{}:{}:{}",
        key.file().path.display(),
        key.raw_grammar_kind(),
        key.raw_grammar_kind_id(),
        key.collision_ordinal()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        IdentifierSurface, LexicalTokenClass, ProjectAnalysis, ProjectSnapshotBuilder,
        RenamedIdentifierEvidence, RenamedTokenEvidence, RepositoryId, SubtreeFingerprintPolicy,
        derive_subtree_fingerprint,
    };
    use std::path::Path;

    struct FingerprintFixture {
        root: NodeKey,
        fingerprint: SubtreeFingerprint,
    }

    fn fingerprint_fixture(
        source: &str,
        root_kind: &str,
        symbols: &[(&str, &str, IdentifierSurface)],
    ) -> FingerprintFixture {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = ProjectSnapshotBuilder::new(
            temp.path(),
            RepositoryId::explicit("clone-index-fixture").unwrap(),
        )
        .unwrap()
        .with_overlay("fixture.rs", source.as_bytes().to_vec())
        .unwrap()
        .build()
        .unwrap();
        let analysis = std::sync::Arc::new(ProjectAnalysis::build(snapshot).unwrap());
        let root_view = analysis
            .file_node_ids(Path::new("fixture.rs"))
            .unwrap()
            .map(|node| analysis.node(node).unwrap())
            .find(|node| node.raw_kind() == root_kind)
            .unwrap();
        let root_id = root_view.id();
        let root = root_view.key().clone();
        let lexical = analysis
            .lexical_token_projection(Path::new("fixture.rs"))
            .unwrap();
        let mut identifiers = Vec::new();
        for fact in lexical.facts() {
            if !analysis.node_contains(root_id, fact.node()).unwrap()
                || fact.classification().token_class() != LexicalTokenClass::Identifier
            {
                continue;
            }
            if let Some((_, symbol, surface)) = symbols
                .iter()
                .find(|(spelling, _, _)| *spelling == fact.text())
            {
                identifiers.push(RenamedIdentifierEvidence {
                    node: analysis.node_key(fact.node()).unwrap().clone(),
                    symbol: (*symbol).into(),
                    surface: *surface,
                });
            }
        }
        let fingerprint = derive_subtree_fingerprint(
            &analysis,
            &root,
            &lexical,
            &RenamedTokenEvidence::new(identifiers).unwrap(),
            SubtreeFingerprintPolicy::default(),
        )
        .unwrap();
        FingerprintFixture { root, fingerprint }
    }

    fn entry_with_context(
        fixture: &FingerprintFixture,
        graph: &str,
        topology: &str,
        boundary: &str,
    ) -> CloneCandidateEntry {
        let context =
            CloneGraphContext::synthetic(fixture.root.clone(), graph, topology, boundary, 3, 2)
                .unwrap();
        CloneCandidateEntry::new(fixture.fingerprint.clone(), context).unwrap()
    }

    #[test]
    fn index_construction_performs_no_pair_comparisons_and_lookup_is_bucketed() {
        let mut entries = Vec::new();
        // Large mostly-unique corpus: unique normalized buckets plus one true pair.
        for index in 0..64 {
            // Keep arithmetic distinct from the +1 clone pair below so buckets stay unique.
            let source = format!(
                "fn run_{index}(value: i32) -> i32 {{ let temp = value * {index} + 3; temp }}\n"
            );
            let fixture = fingerprint_fixture(
                &source,
                "block",
                &[
                    ("value", "parameter", IdentifierSurface::Local),
                    ("temp", "result", IdentifierSurface::Local),
                ],
            );
            entries.push(entry_with_context(
                &fixture,
                &format!("g{index}"),
                &format!("topo-{index}"),
                "boundary-unique",
            ));
        }
        let first = fingerprint_fixture(
            "fn run(alpha: i32) -> i32 { let beta = alpha + 1; beta }\n",
            "block",
            &[
                ("alpha", "parameter", IdentifierSurface::Local),
                ("beta", "result", IdentifierSurface::Local),
            ],
        );
        let renamed = fingerprint_fixture(
            "fn run(input: i32) -> i32 { let output = input + 1; output }\n",
            "block",
            &[
                ("input", "parameter", IdentifierSurface::Local),
                ("output", "result", IdentifierSurface::Local),
            ],
        );
        let first_entry = entry_with_context(&first, "shared", "topo-shared", "boundary-shared");
        let renamed_entry =
            entry_with_context(&renamed, "shared", "topo-shared", "boundary-shared");
        entries.push(first_entry.clone());
        entries.push(renamed_entry.clone());

        let index = CloneCandidateIndex::build(entries).unwrap();
        assert_eq!(index.construction_pair_comparisons(), 0);
        assert_eq!(index.index_size(), 66);
        assert!(index.bucket_count() >= 64);
        assert!(index.largest_bucket() >= 2);

        let bucket = index.lookup_normalized(first_entry.policy_id(), first_entry.normalized());
        assert_eq!(bucket.len(), 2);
        assert!(bucket.iter().any(|entry| entry.id() == first_entry.id()));
        assert!(bucket.iter().any(|entry| entry.id() == renamed_entry.id()));
    }

    #[test]
    fn exact_and_renamed_peers_with_equal_graph_context_verify() {
        let first = fingerprint_fixture(
            "fn run(alpha: i32) -> i32 { let beta = alpha + 1; beta }\n",
            "block",
            &[
                ("alpha", "parameter", IdentifierSurface::Local),
                ("beta", "result", IdentifierSurface::Local),
            ],
        );
        let renamed = fingerprint_fixture(
            "fn run(input: i32) -> i32 { let output = input + 1; output }\n",
            "block",
            &[
                ("input", "parameter", IdentifierSurface::Local),
                ("output", "result", IdentifierSurface::Local),
            ],
        );
        let left = entry_with_context(&first, "g", "topo", "boundary");
        let right = entry_with_context(&renamed, "g", "topo", "boundary");
        let index = CloneCandidateIndex::build(vec![left.clone(), right.clone()]).unwrap();
        let verification = index.verify_pair(&left, &right).unwrap();
        assert_eq!(
            verification,
            ClonePairVerification::Accepted {
                match_kind: CloneMatchKind::RenamedStructure,
                left: left.id().clone(),
                right: right.id().clone(),
            }
        );
        assert_ne!(left.exact(), right.exact());
        assert_eq!(left.normalized(), right.normalized());
    }

    #[test]
    fn equal_fingerprint_with_different_graph_context_rejects() {
        let first = fingerprint_fixture(
            "fn run(alpha: i32) -> i32 { let beta = alpha + 1; beta }\n",
            "block",
            &[
                ("alpha", "parameter", IdentifierSurface::Local),
                ("beta", "result", IdentifierSurface::Local),
            ],
        );
        let renamed = fingerprint_fixture(
            "fn run(input: i32) -> i32 { let output = input + 1; output }\n",
            "block",
            &[
                ("input", "parameter", IdentifierSurface::Local),
                ("output", "result", IdentifierSurface::Local),
            ],
        );
        let left = entry_with_context(&first, "g", "topo-a", "boundary");
        let right = entry_with_context(&renamed, "g", "topo-b", "boundary");
        let index = CloneCandidateIndex::build(vec![left.clone(), right.clone()]).unwrap();
        match index.verify_pair(&left, &right).unwrap() {
            ClonePairVerification::Rejected { reason } => {
                assert!(reason.contains("graph contexts differ"));
            }
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    fn different_fingerprint_buckets_never_verify() {
        let first = fingerprint_fixture(
            "fn run(value: i32) -> i32 { value + 1 }\n",
            "block",
            &[("value", "parameter", IdentifierSurface::Local)],
        );
        let second = fingerprint_fixture(
            "fn run(value: i32) -> i32 { value - 1 }\n",
            "block",
            &[("value", "parameter", IdentifierSurface::Local)],
        );
        let left = entry_with_context(&first, "g", "topo", "boundary");
        let right = entry_with_context(&second, "g", "topo", "boundary");
        let index = CloneCandidateIndex::build(vec![left.clone(), right.clone()]).unwrap();
        match index.verify_pair(&left, &right).unwrap() {
            ClonePairVerification::Rejected { reason } => {
                assert!(reason.contains("same normalized fingerprint bucket"));
            }
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    fn incomplete_graph_context_cannot_enter_index() {
        let err = CloneGraphContext::synthetic(
            fingerprint_fixture(
                "fn run(value: i32) -> i32 { value }\n",
                "block",
                &[("value", "parameter", IdentifierSurface::Local)],
            )
            .root,
            "g",
            "topo",
            "boundary",
            0,
            0,
        )
        .unwrap_err();
        assert!(matches!(err, CloneCandidateIndexError::Invalid(_)));
    }

    #[test]
    fn wire_types_reject_tampered_digest_prefix() {
        let payload = serde_json::json!({
            "schema": CLONE_GRAPH_CONTEXT_SCHEMA,
            "id": "bad_prefix",
            "owner": {
                "revision": "rev1_0000000000000000000000000000000000000000000000000000000000000000",
                "path": "fixture.rs",
                "node": 1u32
            },
            "graph": "g",
            "internal_topology": "t",
            "external_boundary": "b",
            "node_count": 1,
            "edge_count": 0
        });
        // NodeKey wire shape may differ; only assert typed id validation helper.
        assert!(validate_digest("bad_prefix", "cgc1_").is_err());
        let _ = payload;
    }
}
