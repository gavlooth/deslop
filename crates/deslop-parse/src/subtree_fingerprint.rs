use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use blake3::Hasher;
use deslop_core::AnalysisStatus;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{LexicalTokenClass, LexicalTokenProjection, NodeId, NodeKey, ProjectAnalysis};

pub const SUBTREE_FINGERPRINT_SCHEMA: &str = "deslop.subtree-fingerprint/1";
pub const SUBTREE_FINGERPRINT_POLICY_SCHEMA: &str = "deslop.subtree-fingerprint-policy/1";

const EXACT_DOMAIN: &str = "deslop.exact-subtree-fingerprint/1";
const NORMALIZED_DOMAIN: &str = "deslop.normalized-subtree-fingerprint/1";
const POLICY_DOMAIN: &str = "deslop.subtree-fingerprint-policy-id/1";

macro_rules! fingerprint_digest {
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

fingerprint_digest!(ExactSubtreeFingerprint, "stx1_");
fingerprint_digest!(NormalizedSubtreeFingerprint, "stn1_");
fingerprint_digest!(SubtreeFingerprintPolicyId, "stp1_");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PublicApiNormalization {
    Preserve,
    RecipeAllowed { recipe: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubtreeFingerprintPolicy {
    schema: String,
    id: SubtreeFingerprintPolicyId,
    public_api: PublicApiNormalization,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubtreeFingerprintPolicyWire {
    schema: String,
    id: SubtreeFingerprintPolicyId,
    public_api: PublicApiNormalization,
}

impl<'de> Deserialize<'de> for SubtreeFingerprintPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SubtreeFingerprintPolicyWire::deserialize(deserializer)?;
        let policy = Self {
            schema: wire.schema,
            id: wire.id,
            public_api: wire.public_api,
        };
        policy.validate().map_err(D::Error::custom)?;
        Ok(policy)
    }
}

impl SubtreeFingerprintPolicy {
    pub fn preserve_public_api() -> Self {
        Self::new(PublicApiNormalization::Preserve)
            .expect("the built-in fingerprint policy is valid")
    }

    pub fn allow_public_api_for_recipe(
        recipe: impl Into<String>,
    ) -> Result<Self, SubtreeFingerprintError> {
        Self::new(PublicApiNormalization::RecipeAllowed {
            recipe: recipe.into(),
        })
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn id(&self) -> &SubtreeFingerprintPolicyId {
        &self.id
    }

    pub fn public_api(&self) -> &PublicApiNormalization {
        &self.public_api
    }

    fn new(public_api: PublicApiNormalization) -> Result<Self, SubtreeFingerprintError> {
        validate_public_api_policy(&public_api)?;
        let mut hasher = domain_hasher(POLICY_DOMAIN);
        hash_part(&mut hasher, SUBTREE_FINGERPRINT_POLICY_SCHEMA.as_bytes());
        match &public_api {
            PublicApiNormalization::Preserve => hash_part(&mut hasher, b"preserve"),
            PublicApiNormalization::RecipeAllowed { recipe } => {
                hash_part(&mut hasher, b"recipe-allowed");
                hash_part(&mut hasher, recipe.as_bytes());
            }
        }
        Ok(Self {
            schema: SUBTREE_FINGERPRINT_POLICY_SCHEMA.into(),
            id: SubtreeFingerprintPolicyId(format!("stp1_{}", hasher.finalize().to_hex())),
            public_api,
        })
    }

    fn validate(&self) -> Result<(), SubtreeFingerprintError> {
        if self.schema != SUBTREE_FINGERPRINT_POLICY_SCHEMA {
            return Err(SubtreeFingerprintError::InvalidPolicy(
                "unsupported subtree fingerprint policy schema".into(),
            ));
        }
        validate_public_api_policy(&self.public_api)?;
        let expected = Self::new(self.public_api.clone())?;
        if self.id != expected.id {
            return Err(SubtreeFingerprintError::InvalidPolicy(
                "subtree fingerprint policy id does not match its content".into(),
            ));
        }
        Ok(())
    }

    fn normalizes_public_api(&self) -> bool {
        matches!(
            self.public_api,
            PublicApiNormalization::RecipeAllowed { .. }
        )
    }
}

impl Default for SubtreeFingerprintPolicy {
    fn default() -> Self {
        Self::preserve_public_api()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdentifierSurface {
    Local,
    PublicApi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenamedIdentifierEvidence {
    pub node: NodeKey,
    pub symbol: String,
    pub surface: IdentifierSurface,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenamedTokenEvidence {
    identifiers: Vec<RenamedIdentifierEvidence>,
}

impl RenamedTokenEvidence {
    pub fn new(
        mut identifiers: Vec<RenamedIdentifierEvidence>,
    ) -> Result<Self, SubtreeFingerprintError> {
        identifiers.sort_by(|left, right| left.node.cmp(&right.node));
        for identifier in &identifiers {
            if identifier.symbol.trim().is_empty() {
                return Err(SubtreeFingerprintError::InvalidEvidence(
                    "renamed-token symbol identity must not be empty".into(),
                ));
            }
        }
        if identifiers
            .windows(2)
            .any(|pair| pair[0].node == pair[1].node)
        {
            return Err(SubtreeFingerprintError::InvalidEvidence(
                "one syntax token has duplicate renamed-token evidence".into(),
            ));
        }
        Ok(Self { identifiers })
    }

    pub fn identifiers(&self) -> &[RenamedIdentifierEvidence] {
        &self.identifiers
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubtreeFingerprint {
    schema: String,
    root: NodeKey,
    policy: SubtreeFingerprintPolicy,
    exact: ExactSubtreeFingerprint,
    normalized: NormalizedSubtreeFingerprint,
    node_count: u64,
    token_count: u64,
    normalized_identifier_count: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubtreeFingerprintWire {
    schema: String,
    root: NodeKey,
    policy: SubtreeFingerprintPolicy,
    exact: ExactSubtreeFingerprint,
    normalized: NormalizedSubtreeFingerprint,
    node_count: u64,
    token_count: u64,
    normalized_identifier_count: u64,
}

impl<'de> Deserialize<'de> for SubtreeFingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SubtreeFingerprintWire::deserialize(deserializer)?;
        if wire.schema != SUBTREE_FINGERPRINT_SCHEMA {
            return Err(D::Error::custom("unsupported subtree fingerprint schema"));
        }
        if wire.node_count == 0
            || wire.token_count > wire.node_count
            || wire.normalized_identifier_count > wire.token_count
        {
            return Err(D::Error::custom("invalid subtree fingerprint counts"));
        }
        Ok(Self {
            schema: wire.schema,
            root: wire.root,
            policy: wire.policy,
            exact: wire.exact,
            normalized: wire.normalized,
            node_count: wire.node_count,
            token_count: wire.token_count,
            normalized_identifier_count: wire.normalized_identifier_count,
        })
    }
}

impl SubtreeFingerprint {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn root(&self) -> &NodeKey {
        &self.root
    }

    pub fn policy(&self) -> &SubtreeFingerprintPolicy {
        &self.policy
    }

    pub fn exact(&self) -> &ExactSubtreeFingerprint {
        &self.exact
    }

    pub fn normalized(&self) -> &NormalizedSubtreeFingerprint {
        &self.normalized
    }

    pub fn node_count(&self) -> u64 {
        self.node_count
    }

    pub fn token_count(&self) -> u64 {
        self.token_count
    }

    pub fn normalized_identifier_count(&self) -> u64 {
        self.normalized_identifier_count
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtreeFingerprintError {
    Root(String),
    Lexical(String),
    IncompleteSyntax { path: String, detail: String },
    InvalidEvidence(String),
    InvalidPolicy(String),
    SizeOverflow,
}

impl fmt::Display for SubtreeFingerprintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root(detail) => write!(formatter, "subtree root is unavailable: {detail}"),
            Self::Lexical(detail) => {
                write!(formatter, "lexical evidence is incompatible: {detail}")
            }
            Self::IncompleteSyntax { path, detail } => {
                write!(
                    formatter,
                    "subtree syntax is incomplete for {path}: {detail}"
                )
            }
            Self::InvalidEvidence(detail) => {
                write!(formatter, "invalid renamed-token evidence: {detail}")
            }
            Self::InvalidPolicy(detail) => {
                write!(formatter, "invalid subtree fingerprint policy: {detail}")
            }
            Self::SizeOverflow => formatter.write_str("subtree fingerprint counts exceed u64"),
        }
    }
}

impl std::error::Error for SubtreeFingerprintError {}

/// Derive exact and alpha-normalized content addresses for one retained owned syntax subtree.
///
/// Identifier normalization is evidence-driven and is matching evidence only. The result never
/// replaces revision-bound lookup or an exact `RevisionGuard` for a write.
pub fn derive_subtree_fingerprint(
    analysis: &Arc<ProjectAnalysis>,
    root: &NodeKey,
    lexical: &LexicalTokenProjection,
    evidence: &RenamedTokenEvidence,
    policy: SubtreeFingerprintPolicy,
) -> Result<SubtreeFingerprint, SubtreeFingerprintError> {
    policy.validate()?;
    if !Arc::ptr_eq(analysis, lexical.analysis()) {
        return Err(SubtreeFingerprintError::Lexical(
            "projection belongs to another analysis".into(),
        ));
    }
    let root_view = analysis
        .node_by_key(root)
        .map_err(|error| SubtreeFingerprintError::Root(error.to_string()))?;
    if lexical.path() != root_view.path() {
        return Err(SubtreeFingerprintError::Lexical(
            "projection belongs to another source file".into(),
        ));
    }
    require_complete_file(analysis, root_view.path())?;

    let subtree = analysis
        .subtree_node_ids(root_view.id())
        .map_err(|error| SubtreeFingerprintError::Root(error.to_string()))?
        .collect::<Vec<_>>();
    for node in &subtree {
        let view = analysis
            .node(*node)
            .map_err(|error| SubtreeFingerprintError::Root(error.to_string()))?;
        if view.is_error() || view.is_missing() || view.has_error() {
            return Err(SubtreeFingerprintError::IncompleteSyntax {
                path: view.path().display().to_string(),
                detail: format!(
                    "{} contains error, missing, or recovered syntax",
                    view.raw_kind()
                ),
            });
        }
    }

    let subtree_nodes = subtree.iter().copied().collect::<BTreeSet<_>>();
    let token_classes = lexical
        .facts()
        .iter()
        .filter(|fact| subtree_nodes.contains(&fact.node()))
        .map(|fact| (fact.node(), fact.classification().token_class()))
        .collect::<BTreeMap<_, _>>();
    let normalized =
        normalization_ordinals(analysis, &subtree_nodes, &token_classes, evidence, &policy)?;

    let mut exact_digests = BTreeMap::<NodeId, String>::new();
    let mut normalized_digests = BTreeMap::<NodeId, String>::new();
    for node in subtree.iter().rev() {
        let view = analysis
            .node(*node)
            .map_err(|error| SubtreeFingerprintError::Root(error.to_string()))?;
        let mut exact_hasher = node_hasher(EXACT_DOMAIN, view.grammar().identity_bytes(), &view);
        let mut normalized_hasher =
            node_hasher(NORMALIZED_DOMAIN, view.grammar().identity_bytes(), &view);
        hash_part(&mut normalized_hasher, policy.id().as_str().as_bytes());

        if view.is_leaf() {
            hash_part(&mut exact_hasher, b"exact-token");
            hash_part(&mut exact_hasher, view.bytes());
            if let Some(ordinal) = normalized.get(node) {
                hash_part(&mut normalized_hasher, b"alpha-identifier");
                hash_part(&mut normalized_hasher, &ordinal.to_le_bytes());
            } else {
                hash_part(&mut normalized_hasher, b"exact-token");
                hash_part(&mut normalized_hasher, view.bytes());
            }
        }
        for child in view.children() {
            let child_view = analysis
                .node(child)
                .map_err(|error| SubtreeFingerprintError::Root(error.to_string()))?;
            match child_view.field() {
                Some(field) => {
                    hash_part(&mut exact_hasher, b"field");
                    hash_part(&mut exact_hasher, field.as_bytes());
                    hash_part(&mut normalized_hasher, b"field");
                    hash_part(&mut normalized_hasher, field.as_bytes());
                }
                None => {
                    hash_part(&mut exact_hasher, b"no-field");
                    hash_part(&mut normalized_hasher, b"no-field");
                }
            }
            hash_part(
                &mut exact_hasher,
                exact_digests
                    .get(&child)
                    .expect("subtree postorder contains child digests")
                    .as_bytes(),
            );
            hash_part(
                &mut normalized_hasher,
                normalized_digests
                    .get(&child)
                    .expect("subtree postorder contains child digests")
                    .as_bytes(),
            );
        }
        exact_digests.insert(*node, exact_hasher.finalize().to_hex().to_string());
        normalized_digests.insert(*node, normalized_hasher.finalize().to_hex().to_string());
    }

    let node_count =
        u64::try_from(subtree.len()).map_err(|_| SubtreeFingerprintError::SizeOverflow)?;
    let token_count =
        u64::try_from(token_classes.len()).map_err(|_| SubtreeFingerprintError::SizeOverflow)?;
    let normalized_identifier_count =
        u64::try_from(normalized.len()).map_err(|_| SubtreeFingerprintError::SizeOverflow)?;
    Ok(SubtreeFingerprint {
        schema: SUBTREE_FINGERPRINT_SCHEMA.into(),
        root: root.clone(),
        policy,
        exact: ExactSubtreeFingerprint(format!(
            "stx1_{}",
            exact_digests
                .get(&root_view.id())
                .expect("subtree root digest exists")
        )),
        normalized: NormalizedSubtreeFingerprint(format!(
            "stn1_{}",
            normalized_digests
                .get(&root_view.id())
                .expect("subtree root digest exists")
        )),
        node_count,
        token_count,
        normalized_identifier_count,
    })
}

fn require_complete_file(
    analysis: &ProjectAnalysis,
    path: &Path,
) -> Result<(), SubtreeFingerprintError> {
    let file = analysis.file(path).ok_or_else(|| {
        SubtreeFingerprintError::Root(format!("analysis has no file {}", path.display()))
    })?;
    if file.provenance().status != AnalysisStatus::Complete {
        return Err(SubtreeFingerprintError::IncompleteSyntax {
            path: path.display().to_string(),
            detail: format!("parse provenance is {:?}", file.provenance().status),
        });
    }
    Ok(())
}

fn normalization_ordinals(
    analysis: &ProjectAnalysis,
    subtree: &BTreeSet<NodeId>,
    token_classes: &BTreeMap<NodeId, LexicalTokenClass>,
    evidence: &RenamedTokenEvidence,
    policy: &SubtreeFingerprintPolicy,
) -> Result<BTreeMap<NodeId, u64>, SubtreeFingerprintError> {
    let mut symbols = BTreeMap::<NodeId, (&str, IdentifierSurface)>::new();
    for identifier in evidence.identifiers() {
        let view = analysis.node_by_key(&identifier.node).map_err(|error| {
            SubtreeFingerprintError::InvalidEvidence(format!(
                "identifier node is unavailable: {error}"
            ))
        })?;
        if !subtree.contains(&view.id()) {
            return Err(SubtreeFingerprintError::InvalidEvidence(
                "identifier node is outside the owned subtree".into(),
            ));
        }
        if !view.is_leaf() || token_classes.get(&view.id()) != Some(&LexicalTokenClass::Identifier)
        {
            return Err(SubtreeFingerprintError::InvalidEvidence(
                "renamed-token evidence must name a classified identifier leaf".into(),
            ));
        }
        symbols.insert(view.id(), (&identifier.symbol, identifier.surface));
    }

    let mut next = 0_u64;
    let mut ordinals = BTreeMap::<&str, u64>::new();
    let mut normalized = BTreeMap::new();
    for node in subtree {
        let Some((symbol, surface)) = symbols.get(node).copied() else {
            continue;
        };
        if surface == IdentifierSurface::PublicApi && !policy.normalizes_public_api() {
            continue;
        }
        let ordinal = match ordinals.get(symbol) {
            Some(ordinal) => *ordinal,
            None => {
                let ordinal = next;
                next = next
                    .checked_add(1)
                    .ok_or(SubtreeFingerprintError::SizeOverflow)?;
                ordinals.insert(symbol, ordinal);
                ordinal
            }
        };
        normalized.insert(*node, ordinal);
    }
    Ok(normalized)
}

fn node_hasher(domain: &str, grammar: Vec<u8>, view: &crate::NodeView<'_>) -> Hasher {
    let mut hasher = domain_hasher(domain);
    hash_part(&mut hasher, &grammar);
    hash_part(&mut hasher, &view.raw_grammar_kind_id().to_le_bytes());
    hash_part(&mut hasher, view.raw_grammar_kind().as_bytes());
    hash_part(
        &mut hasher,
        &[
            view.is_named() as u8,
            view.is_extra() as u8,
            view.is_error() as u8,
            view.is_missing() as u8,
            view.has_error() as u8,
            view.is_leaf() as u8,
        ],
    );
    hasher
}

fn validate_public_api_policy(
    policy: &PublicApiNormalization,
) -> Result<(), SubtreeFingerprintError> {
    if let PublicApiNormalization::RecipeAllowed { recipe } = policy
        && recipe.trim().is_empty()
    {
        return Err(SubtreeFingerprintError::InvalidPolicy(
            "public API normalization requires a named recipe".into(),
        ));
    }
    Ok(())
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
    use std::path::Path;
    use std::sync::Arc;

    use super::*;
    use crate::{ProjectAnalysis, ProjectSnapshotBuilder, RepositoryId};

    struct Fixture {
        analysis: Arc<ProjectAnalysis>,
        root: NodeKey,
        lexical: LexicalTokenProjection,
        evidence: RenamedTokenEvidence,
    }

    fn fixture(
        source: &str,
        root_kind: &str,
        symbols: &[(&str, &str, IdentifierSurface)],
    ) -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = ProjectSnapshotBuilder::new(
            temp.path(),
            RepositoryId::explicit("subtree-fingerprint-fixture").unwrap(),
        )
        .unwrap()
        .with_overlay("fixture.rs", source.as_bytes().to_vec())
        .unwrap()
        .build()
        .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let root = analysis
            .file_node_ids(Path::new("fixture.rs"))
            .unwrap()
            .map(|node| analysis.node(node).unwrap())
            .find(|node| node.raw_kind() == root_kind)
            .unwrap();
        let root_id = root.id();
        let root = root.key().clone();
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
        Fixture {
            analysis,
            root,
            lexical,
            evidence: RenamedTokenEvidence::new(identifiers).unwrap(),
        }
    }

    fn fingerprint(fixture: &Fixture, policy: SubtreeFingerprintPolicy) -> SubtreeFingerprint {
        derive_subtree_fingerprint(
            &fixture.analysis,
            &fixture.root,
            &fixture.lexical,
            &fixture.evidence,
            policy,
        )
        .unwrap()
    }

    #[test]
    fn equal_structure_with_alpha_renamed_locals_has_one_normalized_digest() {
        let first = fixture(
            "fn run(alpha: i32) -> i32 { let beta = alpha + 1; beta }\n",
            "block",
            &[
                ("alpha", "parameter", IdentifierSurface::Local),
                ("beta", "result", IdentifierSurface::Local),
            ],
        );
        let renamed = fixture(
            "fn run(input: i32) -> i32 { let output = input + 1; output }\n",
            "block",
            &[
                ("input", "parameter", IdentifierSurface::Local),
                ("output", "result", IdentifierSurface::Local),
            ],
        );

        let first = fingerprint(&first, SubtreeFingerprintPolicy::default());
        let renamed = fingerprint(&renamed, SubtreeFingerprintPolicy::default());
        assert_ne!(first.exact(), renamed.exact());
        assert_eq!(first.normalized(), renamed.normalized());
        assert_eq!(first.normalized_identifier_count(), 3);
        assert_eq!(
            first,
            fingerprint(
                &fixture(
                    "fn run(alpha: i32) -> i32 { let beta = alpha + 1; beta }\n",
                    "block",
                    &[
                        ("alpha", "parameter", IdentifierSurface::Local),
                        ("beta", "result", IdentifierSurface::Local),
                    ],
                ),
                SubtreeFingerprintPolicy::default()
            )
        );
    }

    #[test]
    fn different_structure_does_not_match() {
        let straight = fixture(
            "fn run(value: i32) -> i32 { let output = value + 1; output }\n",
            "block",
            &[
                ("value", "parameter", IdentifierSurface::Local),
                ("output", "result", IdentifierSurface::Local),
            ],
        );
        let branched = fixture(
            "fn run(input: i32) -> i32 { if input > 0 { input } else { 0 } }\n",
            "block",
            &[("input", "parameter", IdentifierSurface::Local)],
        );
        assert_ne!(
            fingerprint(&straight, SubtreeFingerprintPolicy::default()).normalized(),
            fingerprint(&branched, SubtreeFingerprintPolicy::default()).normalized()
        );
    }

    #[test]
    fn literal_and_operator_changes_remain_exact_in_normalized_digest() {
        let base = fixture(
            "fn run(value: i32) -> i32 { value + 1 }\n",
            "block",
            &[("value", "parameter", IdentifierSurface::Local)],
        );
        let literal = fixture(
            "fn run(input: i32) -> i32 { input + 2 }\n",
            "block",
            &[("input", "parameter", IdentifierSurface::Local)],
        );
        let operator = fixture(
            "fn run(input: i32) -> i32 { input - 1 }\n",
            "block",
            &[("input", "parameter", IdentifierSurface::Local)],
        );
        let base = fingerprint(&base, SubtreeFingerprintPolicy::default());
        assert_ne!(
            base.normalized(),
            fingerprint(&literal, SubtreeFingerprintPolicy::default()).normalized()
        );
        assert_ne!(
            base.normalized(),
            fingerprint(&operator, SubtreeFingerprintPolicy::default()).normalized()
        );
    }

    #[test]
    fn public_api_surface_is_preserved_until_a_named_recipe_allows_it() {
        let first = fixture(
            "fn first(value: i32) -> i32 { value + 1 }\n",
            "function_item",
            &[
                ("first", "callable", IdentifierSurface::PublicApi),
                ("value", "parameter", IdentifierSurface::Local),
            ],
        );
        let renamed = fixture(
            "fn second(input: i32) -> i32 { input + 1 }\n",
            "function_item",
            &[
                ("second", "callable", IdentifierSurface::PublicApi),
                ("input", "parameter", IdentifierSurface::Local),
            ],
        );
        assert_ne!(
            fingerprint(&first, SubtreeFingerprintPolicy::default()).normalized(),
            fingerprint(&renamed, SubtreeFingerprintPolicy::default()).normalized()
        );
        let policy =
            SubtreeFingerprintPolicy::allow_public_api_for_recipe("clone-public-api-review")
                .unwrap();
        assert_eq!(
            fingerprint(&first, policy.clone()).normalized(),
            fingerprint(&renamed, policy).normalized()
        );
    }

    #[test]
    fn incomplete_or_recovered_syntax_emits_no_fingerprint() {
        let malformed = fixture(
            "fn run(value: i32) -> i32 { let output = value + ; output }\n",
            "function_item",
            &[],
        );
        assert!(matches!(
            derive_subtree_fingerprint(
                &malformed.analysis,
                &malformed.root,
                &malformed.lexical,
                &malformed.evidence,
                SubtreeFingerprintPolicy::default(),
            ),
            Err(SubtreeFingerprintError::IncompleteSyntax { .. })
        ));
    }

    #[test]
    fn wire_types_reject_tampered_policy_and_digest_content() {
        let fixture = fixture(
            "fn run(value: i32) -> i32 { value + 1 }\n",
            "block",
            &[("value", "parameter", IdentifierSurface::Local)],
        );
        let fingerprint = fingerprint(&fixture, SubtreeFingerprintPolicy::default());
        let encoded = serde_json::to_string(&fingerprint).unwrap();
        assert_eq!(
            serde_json::from_str::<SubtreeFingerprint>(&encoded).unwrap(),
            fingerprint
        );

        let mut tampered = serde_json::to_value(&fingerprint).unwrap();
        tampered["exact"] = serde_json::json!(format!("stx1_{}", "A".repeat(64)));
        assert!(serde_json::from_value::<SubtreeFingerprint>(tampered).is_err());

        let mut tampered = serde_json::to_value(&fingerprint).unwrap();
        tampered["policy"]["id"] = serde_json::json!(format!("stp1_{}", "0".repeat(64)));
        assert!(serde_json::from_value::<SubtreeFingerprint>(tampered).is_err());
    }
}
