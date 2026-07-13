use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path};
use std::sync::Arc;

use anyhow::anyhow;
use blake3::Hasher;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::arena::{ArenaNodeIndex, RAW_ARENA_SCHEMA, SyntaxArena, SyntaxSpan};
use crate::snapshot::FileRevisionKey;

pub const NODE_KEY_SCHEMA: &str = "deslop.node-key/1";
pub const NODE_BASELINE_SCHEMA: &str = "deslop.node-baseline/1";

/// Process-local identity for one node in one immutable `ProjectAnalysis`.
///
/// `NodeId` deliberately has no Serde implementation. Use `NodeKey` for revision-bound storage.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId {
    pub(crate) owner: u64,
    pub(crate) index: u32,
}

impl fmt::Debug for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NodeId")
            .field("owner", &self.owner)
            .field("index", &self.index)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeLookupError {
    WrongAnalysis,
    OutOfRange { requested: u32, node_count: u32 },
}

impl fmt::Display for NodeLookupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongAnalysis => {
                formatter.write_str("node belongs to a different project analysis")
            }
            Self::OutOfRange {
                requested,
                node_count,
            } => write!(
                formatter,
                "node index {requested} is outside analysis node count {node_count}"
            ),
        }
    }
}

impl std::error::Error for NodeLookupError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKeyLookupError {
    UnsupportedSchema,
    FileRevisionExpired,
    NotFound,
}

impl fmt::Display for NodeKeyLookupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema => formatter.write_str("unsupported node-key schema"),
            Self::FileRevisionExpired => formatter.write_str("node-key file revision has expired"),
            Self::NotFound => formatter.write_str("node-key structural identity is not present"),
        }
    }
}

impl std::error::Error for NodeKeyLookupError {}

/// Alias-free raw structural shape plus fixed-width exact source coordinates.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAnchor {
    structural_digest: String,
    start_byte: u64,
    end_byte: u64,
    start_row: u64,
    start_column: u64,
    end_row: u64,
    end_column: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeAnchorWire {
    structural_digest: String,
    start_byte: u64,
    end_byte: u64,
    start_row: u64,
    start_column: u64,
    end_row: u64,
    end_column: u64,
}

impl<'de> Deserialize<'de> for NodeAnchor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = NodeAnchorWire::deserialize(deserializer)?;
        let anchor = Self {
            structural_digest: wire.structural_digest,
            start_byte: wire.start_byte,
            end_byte: wire.end_byte,
            start_row: wire.start_row,
            start_column: wire.start_column,
            end_row: wire.end_row,
            end_column: wire.end_column,
        };
        validate_anchor(&anchor).map_err(D::Error::custom)?;
        Ok(anchor)
    }
}

impl NodeAnchor {
    fn from_span(span: SyntaxSpan, structural_digest: String) -> Self {
        Self {
            structural_digest,
            start_byte: span.start_byte() as u64,
            end_byte: span.end_byte() as u64,
            start_row: span.start_point().row() as u64,
            start_column: span.start_point().column() as u64,
            end_row: span.end_point().row() as u64,
            end_column: span.end_point().column() as u64,
        }
    }

    pub fn structural_digest(&self) -> &str {
        &self.structural_digest
    }

    pub fn start_byte(&self) -> u64 {
        self.start_byte
    }

    pub fn end_byte(&self) -> u64 {
        self.end_byte
    }

    pub fn start_row(&self) -> u64 {
        self.start_row
    }

    pub fn start_column(&self) -> u64 {
        self.start_column
    }

    pub fn end_row(&self) -> u64 {
        self.end_row
    }

    pub fn end_column(&self) -> u64 {
        self.end_column
    }
}

/// Serialized identity for exactly one node under exactly one file revision and grammar.
///
/// This is stable for storage but intentionally expires with any `FileRevisionKey` change. It is
/// correlation identity only and can never replace an exact `RevisionGuard`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeKey {
    schema: String,
    arena_schema: String,
    file: Arc<FileRevisionKey>,
    raw_grammar_kind: String,
    raw_grammar_kind_id: u16,
    field_path: Arc<[Option<String>]>,
    anchor: NodeAnchor,
    collision_ordinal: u32,
}

impl NodeKey {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn file(&self) -> &FileRevisionKey {
        &self.file
    }

    pub fn arena_schema(&self) -> &str {
        &self.arena_schema
    }

    pub fn raw_grammar_kind(&self) -> &str {
        &self.raw_grammar_kind
    }

    pub fn raw_grammar_kind_id(&self) -> u16 {
        self.raw_grammar_kind_id
    }

    pub fn field_path(&self) -> &[Option<String>] {
        &self.field_path
    }

    pub fn anchor(&self) -> &NodeAnchor {
        &self.anchor
    }

    pub fn collision_ordinal(&self) -> u32 {
        self.collision_ordinal
    }

    pub fn is_supported(&self) -> bool {
        self.schema == NODE_KEY_SCHEMA && self.arena_schema == RAW_ARENA_SCHEMA
    }

    pub(crate) fn instrumentation(&self) -> (usize, usize, usize, usize) {
        let file_payload = self.file.known_payload_bytes();
        let field_path_bytes = self.field_path.len() * std::mem::size_of::<Option<String>>()
            + self
                .field_path
                .iter()
                .flatten()
                .map(String::len)
                .sum::<usize>();
        let heap_payload = self.schema.len()
            + self.arena_schema.len()
            + self.raw_grammar_kind.len()
            + self.anchor.structural_digest.len();
        (
            heap_payload,
            file_payload,
            field_path_bytes,
            self.field_path.len(),
        )
    }

    pub(crate) fn field_path_allocation_id(&self) -> usize {
        Arc::as_ptr(&self.field_path) as *const () as usize
    }

    pub(crate) fn lookup_digest(&self) -> [u8; 16] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"deslop node-key lookup index v1\0");
        self.update_order_digest(&mut hasher);
        let mut digest = [0; 16];
        digest.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
        digest
    }

    pub(crate) fn update_order_digest(&self, hasher: &mut blake3::Hasher) {
        fn part(hasher: &mut blake3::Hasher, bytes: &[u8]) {
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        }
        part(hasher, self.schema.as_bytes());
        part(hasher, self.arena_schema.as_bytes());
        part(hasher, self.file.repository.as_str().as_bytes());
        part(
            hasher,
            self.file
                .path
                .to_str()
                .expect("snapshot paths are validated Unicode")
                .as_bytes(),
        );
        part(hasher, self.file.source.as_str().as_bytes());
        part(hasher, &self.file.grammar.identity_bytes());
        part(hasher, self.raw_grammar_kind.as_bytes());
        part(hasher, &self.raw_grammar_kind_id.to_le_bytes());
        for field in self.field_path.iter() {
            match field {
                Some(field) => {
                    part(hasher, &[1]);
                    part(hasher, field.as_bytes());
                }
                None => part(hasher, &[0]),
            }
        }
        part(hasher, self.anchor.structural_digest.as_bytes());
        for coordinate in [
            self.anchor.start_byte,
            self.anchor.end_byte,
            self.anchor.start_row,
            self.anchor.start_column,
            self.anchor.end_row,
            self.anchor.end_column,
        ] {
            part(hasher, &coordinate.to_le_bytes());
        }
        part(hasher, &self.collision_ordinal.to_le_bytes());
    }

    fn validate(&self) -> Result<(), String> {
        if !self.is_supported() {
            return Err("unsupported node-key or arena schema".to_string());
        }
        if self.file.repository.as_str().trim().is_empty() {
            return Err("node-key repository identity is empty".to_string());
        }
        validate_repo_path(&self.file.path)?;
        validate_prefixed_hex(self.file.source.as_str(), "sr1_")?;
        if self.file.grammar.dialect().is_empty()
            || self.file.grammar.selector().is_empty()
            || self.file.grammar.grammar_id().is_empty()
            || self.file.grammar.grammar_version().is_empty()
            || self.file.grammar.parser_build().is_empty()
        {
            return Err("node-key grammar selection contains an empty identity field".to_string());
        }
        if self.raw_grammar_kind.is_empty() {
            return Err("node-key raw grammar kind is empty".to_string());
        }
        if self.field_path.iter().flatten().any(String::is_empty) {
            return Err("node-key field path contains an empty field".to_string());
        }
        validate_anchor(&self.anchor)?;
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeKeyWire {
    schema: String,
    arena_schema: String,
    file: FileRevisionKey,
    raw_grammar_kind: String,
    raw_grammar_kind_id: u16,
    field_path: Vec<Option<String>>,
    anchor: NodeAnchor,
    collision_ordinal: u32,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct NodeKeyWireRef<'a> {
    schema: &'a str,
    arena_schema: &'a str,
    file: &'a FileRevisionKey,
    raw_grammar_kind: &'a str,
    raw_grammar_kind_id: u16,
    field_path: &'a [Option<String>],
    anchor: &'a NodeAnchor,
    collision_ordinal: u32,
}

impl Serialize for NodeKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        NodeKeyWireRef {
            schema: &self.schema,
            arena_schema: &self.arena_schema,
            file: &self.file,
            raw_grammar_kind: &self.raw_grammar_kind,
            raw_grammar_kind_id: self.raw_grammar_kind_id,
            field_path: &self.field_path,
            anchor: &self.anchor,
            collision_ordinal: self.collision_ordinal,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for NodeKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = NodeKeyWire::deserialize(deserializer)?;
        let key = Self {
            schema: wire.schema,
            arena_schema: wire.arena_schema,
            file: Arc::new(wire.file),
            raw_grammar_kind: wire.raw_grammar_kind,
            raw_grammar_kind_id: wire.raw_grammar_kind_id,
            field_path: Arc::from(wire.field_path),
            anchor: wire.anchor,
            collision_ordinal: wire.collision_ordinal,
        };
        key.validate().map_err(D::Error::custom)?;
        Ok(key)
    }
}

/// Best-effort cross-revision comparison evidence for one raw syntax shape and trimmed text.
///
/// Equal values can be ambiguous (for example duplicate functions) and never authorize lookup,
/// re-anchoring, a `NodeKey`, a `RevisionGuard`, or a write.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct NodeBaselineFingerprint(String);

impl NodeBaselineFingerprint {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeBaselineFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for NodeBaselineFingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_prefixed_hex(&value, "nb1_").map_err(D::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct NodeKeyCollisionBase {
    raw_grammar_kind: String,
    raw_grammar_kind_id: u16,
    field_path: Vec<Option<String>>,
    anchor: NodeAnchor,
}

pub(crate) fn build_node_keys(
    file: &FileRevisionKey,
    arena: &SyntaxArena,
) -> anyhow::Result<Box<[NodeKey]>> {
    let file = Arc::new(file.clone());
    let mut collisions = BTreeMap::<NodeKeyCollisionBase, u32>::new();
    let mut field_paths = BTreeMap::<Vec<Option<String>>, Arc<[Option<String>]>>::new();
    let structural_digests = structural_digests(arena);
    let mut keys = Vec::with_capacity(arena.nodes().len());
    for (index, node) in arena.indexed_nodes() {
        let base = NodeKeyCollisionBase {
            raw_grammar_kind: node.raw_grammar_kind().to_string(),
            raw_grammar_kind_id: node.raw_grammar_kind_id(),
            field_path: field_path(arena, index),
            anchor: NodeAnchor::from_span(
                node.span(),
                structural_digests[index.as_usize()].clone(),
            ),
        };
        let collision_ordinal = next_collision_ordinal(&mut collisions, &base)?;
        let field_path = field_paths
            .entry(base.field_path.clone())
            .or_insert_with(|| Arc::from(base.field_path.clone()))
            .clone();
        keys.push(NodeKey {
            schema: NODE_KEY_SCHEMA.to_string(),
            arena_schema: RAW_ARENA_SCHEMA.to_string(),
            file: Arc::clone(&file),
            raw_grammar_kind: base.raw_grammar_kind,
            raw_grammar_kind_id: base.raw_grammar_kind_id,
            field_path,
            anchor: base.anchor,
            collision_ordinal,
        });
    }
    Ok(keys.into_boxed_slice())
}

fn structural_digests(arena: &SyntaxArena) -> Vec<String> {
    let mut digests = vec![String::new(); arena.nodes().len()];
    for (index, node) in arena.indexed_nodes().collect::<Vec<_>>().into_iter().rev() {
        let mut hasher = domain_hasher("deslop.raw-structural-anchor/1");
        hash_part(&mut hasher, &node.raw_grammar_kind_id().to_le_bytes());
        hash_part(&mut hasher, node.raw_grammar_kind().as_bytes());
        hash_part(
            &mut hasher,
            &[
                node.is_named() as u8,
                node.is_extra() as u8,
                node.is_error() as u8,
                node.is_missing() as u8,
                node.has_error() as u8,
            ],
        );
        for child in node.children() {
            let child_node = arena.node(*child).expect("arena child belongs to arena");
            match child_node.field() {
                Some(field) => {
                    hash_part(&mut hasher, &[1]);
                    hash_part(&mut hasher, field.as_bytes());
                }
                None => hash_part(&mut hasher, &[0]),
            }
            hash_part(&mut hasher, digests[child.as_usize()].as_bytes());
        }
        digests[index.as_usize()] = format!("nsa1_{}", hasher.finalize().to_hex());
    }
    digests
}

fn next_collision_ordinal(
    collisions: &mut BTreeMap<NodeKeyCollisionBase, u32>,
    base: &NodeKeyCollisionBase,
) -> anyhow::Result<u32> {
    let ordinal = collisions.entry(base.clone()).or_default();
    let current = *ordinal;
    *ordinal = ordinal
        .checked_add(1)
        .ok_or_else(|| anyhow!("one NodeKey collision class exceeds {} nodes", u32::MAX))?;
    Ok(current)
}

pub(crate) fn baseline_fingerprint(key: &NodeKey, exact_text: &str) -> NodeBaselineFingerprint {
    let mut hasher = domain_hasher(NODE_BASELINE_SCHEMA);
    hash_part(&mut hasher, key.file.repository.as_str().as_bytes());
    hash_part(&mut hasher, &normalized_path_bytes(&key.file.path));
    hash_part(&mut hasher, key.raw_grammar_kind.as_bytes());
    for field in key.field_path.iter() {
        match field {
            Some(field) => {
                hash_part(&mut hasher, &[1]);
                hash_part(&mut hasher, field.as_bytes());
            }
            None => hash_part(&mut hasher, &[0]),
        }
    }
    hash_part(&mut hasher, exact_text.trim().as_bytes());
    NodeBaselineFingerprint(format!("nb1_{}", hasher.finalize().to_hex()))
}

fn field_path(arena: &SyntaxArena, mut index: ArenaNodeIndex) -> Vec<Option<String>> {
    let mut fields = Vec::new();
    while let Some(node) = arena.node(index) {
        let Some(parent) = node.parent() else {
            break;
        };
        fields.push(node.field().map(ToOwned::to_owned));
        index = parent;
    }
    fields.reverse();
    fields
}

fn normalized_path_bytes(path: &Path) -> Vec<u8> {
    if path == Path::new(".") {
        return b".".to_vec();
    }
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(
                part.to_str()
                    .expect("snapshot paths are validated as Unicode"),
            ),
            Component::CurDir => None,
            _ => panic!("snapshot path is not normalized and relative"),
        })
        .collect::<Vec<_>>()
        .join("/")
        .into_bytes()
}

fn validate_repo_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err("node-key path must be non-empty and repository-relative".to_string());
    }
    for component in path.components() {
        match component {
            Component::Normal(part) if part.to_str().is_some() => {}
            Component::CurDir if path == Path::new(".") => {}
            _ => return Err("node-key path is not normalized Unicode".to_string()),
        }
    }
    Ok(())
}

fn validate_prefixed_hex(value: &str, prefix: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix(prefix) else {
        return Err(format!("identity must begin with {prefix}"));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "identity after {prefix} must be 64 lowercase hexadecimal digits"
        ));
    }
    Ok(())
}

fn validate_anchor(anchor: &NodeAnchor) -> Result<(), String> {
    validate_prefixed_hex(&anchor.structural_digest, "nsa1_")?;
    if anchor.start_byte > anchor.end_byte {
        return Err("node anchor byte range is reversed".to_string());
    }
    if anchor.start_row > anchor.end_row
        || (anchor.start_row == anchor.end_row && anchor.start_column > anchor.end_column)
    {
        return Err("node anchor point range is reversed".to_string());
    }
    Ok(())
}

fn domain_hasher(domain: &str) -> Hasher {
    let mut hasher = Hasher::new();
    hash_part(&mut hasher, domain.as_bytes());
    hasher
}

fn hash_part(hasher: &mut Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::{SourcePoint, SyntaxSpan};

    #[test]
    fn collision_ordinals_are_zero_based_per_exact_anchor_class() {
        let base = NodeKeyCollisionBase {
            raw_grammar_kind: "identifier".to_string(),
            raw_grammar_kind_id: 1,
            field_path: vec![Some("type".to_string())],
            anchor: NodeAnchor::from_span(
                SyntaxSpan::new_for_test(7, 7, 0, 7, 0, 7),
                format!("nsa1_{}", "0".repeat(64)),
            ),
        };
        let other = NodeKeyCollisionBase {
            raw_grammar_kind: "identifier".to_string(),
            raw_grammar_kind_id: 1,
            field_path: vec![Some("name".to_string())],
            anchor: base.anchor.clone(),
        };
        let inputs = [&base, &other, &base, &base, &other];
        let allocate = || {
            let mut collisions = BTreeMap::new();
            inputs
                .iter()
                .map(|base| next_collision_ordinal(&mut collisions, base).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(allocate(), [0, 0, 1, 2, 1]);
        assert_eq!(allocate(), allocate());
        let identities = inputs
            .iter()
            .zip(allocate())
            .map(|(base, ordinal)| (base.raw_grammar_kind_id, base.field_path.clone(), ordinal))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(identities.len(), 5);

        let mut other_id = base.clone();
        other_id.raw_grammar_kind_id = 2;
        let mut collisions = BTreeMap::new();
        assert_eq!(next_collision_ordinal(&mut collisions, &base).unwrap(), 0);
        assert_eq!(
            next_collision_ordinal(&mut collisions, &other_id).unwrap(),
            0
        );

        collisions.insert(base.clone(), u32::MAX);
        assert!(next_collision_ordinal(&mut collisions, &base).is_err());
    }

    #[test]
    fn source_point_type_is_owned_and_copyable() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<SourcePoint>();
    }

    #[test]
    fn standalone_node_anchors_preserve_their_wire_invariants() {
        let valid = NodeAnchor::from_span(
            SyntaxSpan::new_for_test(1, 2, 0, 1, 0, 2),
            format!("nsa1_{}", "a".repeat(64)),
        );
        let mut value = serde_json::to_value(valid).unwrap();
        value["structural_digest"] = serde_json::json!(format!("nsa1_{}", "A".repeat(64)));
        assert!(serde_json::from_value::<NodeAnchor>(value).is_err());

        let mut value = serde_json::json!({
            "structural_digest": format!("nsa1_{}", "a".repeat(64)),
            "start_byte": 2,
            "end_byte": 1,
            "start_row": 0,
            "start_column": 1,
            "end_row": 0,
            "end_column": 2
        });
        assert!(serde_json::from_value::<NodeAnchor>(value.clone()).is_err());
        value["start_byte"] = serde_json::json!(1);
        assert!(serde_json::from_value::<NodeAnchor>(value).is_ok());
    }
}
