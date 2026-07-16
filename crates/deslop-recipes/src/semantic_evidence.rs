use std::collections::BTreeSet;
use std::fmt;

use deslop_core::Span;
use deslop_parse::{CanonicalRole, FactCoverage, ResolutionStatus};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{CandidateDisposition, CandidateId, TransformationCandidate};

pub const ROLE_SCOPE_COMMENT_EVIDENCE_SCHEMA: &str = "deslop.role-scope-comment-evidence/1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RoleScopeCommentEvidenceId(String);

impl RoleScopeCommentEvidenceId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for RoleScopeCommentEvidenceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        validate_digest_id(&value, "rsc1_").map_err(D::Error::custom)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdentifierSemanticRole {
    Declaration,
    Definition,
    Parameter,
    Read,
    Write,
    CallTarget,
    TypeUse,
    PublicApiSurface,
}

impl IdentifierSemanticRole {
    fn is_reference(self) -> bool {
        matches!(
            self,
            Self::Read | Self::Write | Self::CallTarget | Self::TypeUse
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentifierSemanticEvidence {
    pub source_path: String,
    pub span: Span,
    pub spelling: String,
    pub role: IdentifierSemanticRole,
    pub canonical_roles: Vec<CanonicalRole>,
    pub scope_fact: String,
    pub binding_fact: Option<String>,
    pub resolution: Option<ResolutionStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommentIntent {
    Ordinary,
    Documentation,
    Rationale,
    Suppression,
    Generated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommentSemanticEvidence {
    pub source_path: String,
    pub span: Span,
    pub text: String,
    pub intent: CommentIntent,
    pub owner_scope_fact: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticEvidenceDisposition {
    AutomaticCompatible,
    ReviewRequired,
    AutomaticRationaleDeletionBlocked,
}

#[derive(Debug, Clone)]
pub struct RoleScopeCommentEvidenceInput {
    pub identifier_coverage: FactCoverage,
    pub comment_coverage: FactCoverage,
    pub identifiers: Vec<IdentifierSemanticEvidence>,
    pub comments: Vec<CommentSemanticEvidence>,
}

/// Role-, scope-, and comment-aware evidence for an existing guarded candidate.
///
/// This projection never changes the candidate or grants write authority. In particular, an
/// automatic candidate whose exact edit would remove rationale text is explicitly blocked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleScopeCommentEvidence {
    schema: String,
    id: RoleScopeCommentEvidenceId,
    candidate: CandidateId,
    identifier_coverage: FactCoverage,
    comment_coverage: FactCoverage,
    identifiers: Vec<IdentifierSemanticEvidence>,
    comments: Vec<CommentSemanticEvidence>,
    disposition: SemanticEvidenceDisposition,
}

impl RoleScopeCommentEvidence {
    pub fn id(&self) -> &RoleScopeCommentEvidenceId {
        &self.id
    }

    pub fn candidate(&self) -> &CandidateId {
        &self.candidate
    }

    pub fn identifiers(&self) -> &[IdentifierSemanticEvidence] {
        &self.identifiers
    }

    pub fn comments(&self) -> &[CommentSemanticEvidence] {
        &self.comments
    }

    pub fn disposition(&self) -> SemanticEvidenceDisposition {
        self.disposition
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_record(self)
    }
}

pub fn role_scope_comment_evidence(
    candidate: &TransformationCandidate,
    mut input: RoleScopeCommentEvidenceInput,
) -> Result<RoleScopeCommentEvidence, String> {
    if input.identifier_coverage != FactCoverage::Complete
        || input.comment_coverage != FactCoverage::Complete
    {
        return Err("role/scope and comment evidence must both be complete".into());
    }

    canonicalize_identifiers(&mut input.identifiers);
    canonicalize_comments(&mut input.comments);
    validate_identifiers(&input.identifiers)?;
    validate_comments(&input.comments)?;

    let edits = candidate
        .edits()
        .iter()
        .map(|edit| SemanticEditRef {
            source_path: edit.target.file().path.to_string_lossy().into_owned(),
            span: edit.span,
            after: &edit.after,
        })
        .collect::<Vec<_>>();
    let disposition = semantic_disposition(
        candidate.disposition(),
        &edits,
        &input.identifiers,
        &input.comments,
    );

    let mut record = RoleScopeCommentEvidence {
        schema: ROLE_SCOPE_COMMENT_EVIDENCE_SCHEMA.into(),
        id: RoleScopeCommentEvidenceId(String::new()),
        candidate: candidate.id().clone(),
        identifier_coverage: input.identifier_coverage,
        comment_coverage: input.comment_coverage,
        identifiers: input.identifiers,
        comments: input.comments,
        disposition,
    };
    record.id = derive_id(&record)?;
    record.validate()?;
    Ok(record)
}

fn spans_overlap(left: Span, right: Span) -> bool {
    left.start_byte < right.end_byte && right.start_byte < left.end_byte
}

#[derive(Clone)]
struct SemanticEditRef<'a> {
    source_path: String,
    span: Span,
    after: &'a str,
}

fn semantic_disposition(
    candidate_disposition: CandidateDisposition,
    edits: &[SemanticEditRef<'_>],
    identifiers: &[IdentifierSemanticEvidence],
    comments: &[CommentSemanticEvidence],
) -> SemanticEvidenceDisposition {
    let protected_identifier = identifiers
        .iter()
        .any(|evidence| evidence.role == IdentifierSemanticRole::PublicApiSurface);
    let protected_comment = comments.iter().any(|evidence| {
        matches!(
            evidence.intent,
            CommentIntent::Documentation | CommentIntent::Rationale | CommentIntent::Suppression
        )
    });
    let deletes_rationale = comments.iter().any(|comment| {
        comment.intent == CommentIntent::Rationale
            && edits.iter().any(|edit| {
                edit.source_path == comment.source_path
                    && spans_overlap(edit.span, comment.span)
                    && !edit.after.contains(&comment.text)
            })
    });

    if candidate_disposition == CandidateDisposition::Automatic && deletes_rationale {
        SemanticEvidenceDisposition::AutomaticRationaleDeletionBlocked
    } else if candidate_disposition == CandidateDisposition::ReviewRequired
        || protected_identifier
        || protected_comment
    {
        SemanticEvidenceDisposition::ReviewRequired
    } else {
        SemanticEvidenceDisposition::AutomaticCompatible
    }
}

fn canonicalize_identifiers(evidence: &mut [IdentifierSemanticEvidence]) {
    for item in evidence.iter_mut() {
        item.canonical_roles.sort();
        item.canonical_roles.dedup();
    }
    evidence.sort_by(|left, right| {
        (
            &left.source_path,
            left.span.start_byte,
            left.span.end_byte,
            left.role,
            &left.scope_fact,
        )
            .cmp(&(
                &right.source_path,
                right.span.start_byte,
                right.span.end_byte,
                right.role,
                &right.scope_fact,
            ))
    });
}

fn canonicalize_comments(evidence: &mut [CommentSemanticEvidence]) {
    evidence.sort_by(|left, right| {
        (
            &left.source_path,
            left.span.start_byte,
            left.span.end_byte,
            left.intent,
        )
            .cmp(&(
                &right.source_path,
                right.span.start_byte,
                right.span.end_byte,
                right.intent,
            ))
    });
}

fn validate_record(record: &RoleScopeCommentEvidence) -> Result<(), String> {
    if record.schema != ROLE_SCOPE_COMMENT_EVIDENCE_SCHEMA {
        return Err("unsupported role/scope/comment evidence schema".into());
    }
    validate_digest_id(record.id.as_str(), "rsc1_")?;
    if record.identifier_coverage != FactCoverage::Complete
        || record.comment_coverage != FactCoverage::Complete
    {
        return Err("role/scope and comment evidence must both be complete".into());
    }
    validate_identifiers(&record.identifiers)?;
    validate_comments(&record.comments)?;

    let mut canonical_identifiers = record.identifiers.clone();
    canonicalize_identifiers(&mut canonical_identifiers);
    let mut canonical_comments = record.comments.clone();
    canonicalize_comments(&mut canonical_comments);
    if canonical_identifiers != record.identifiers || canonical_comments != record.comments {
        return Err("role/scope/comment evidence is not canonical".into());
    }
    if derive_id(record)? != record.id {
        return Err("role/scope/comment evidence identity is stale".into());
    }
    Ok(())
}

fn validate_identifiers(evidence: &[IdentifierSemanticEvidence]) -> Result<(), String> {
    let mut keys = BTreeSet::new();
    for item in evidence {
        validate_text("identifier source path", &item.source_path)?;
        validate_span(item.span)?;
        validate_text("identifier spelling", &item.spelling)?;
        validate_fact_key("identifier scope fact", &item.scope_fact)?;
        if item.canonical_roles.is_empty() {
            return Err("identifier evidence requires at least one canonical role".into());
        }
        if item.role.is_reference() {
            if item.resolution != Some(ResolutionStatus::Unique) || item.binding_fact.is_none() {
                return Err("identifier references require one exact resolved binding".into());
            }
        } else if item.resolution.is_some() {
            return Err("non-reference identifier evidence cannot carry resolution status".into());
        }
        if let Some(binding) = item.binding_fact.as_deref() {
            validate_fact_key("identifier binding fact", binding)?;
        }
        let key = (&item.source_path, item.span.start_byte, item.span.end_byte);
        if !keys.insert(key) {
            return Err("duplicate identifier evidence span".into());
        }
    }
    Ok(())
}

fn validate_comments(evidence: &[CommentSemanticEvidence]) -> Result<(), String> {
    let mut keys = BTreeSet::new();
    for item in evidence {
        validate_text("comment source path", &item.source_path)?;
        validate_span(item.span)?;
        validate_text("comment text", &item.text)?;
        validate_fact_key("comment owner scope fact", &item.owner_scope_fact)?;
        let key = (&item.source_path, item.span.start_byte, item.span.end_byte);
        if !keys.insert(key) {
            return Err("duplicate comment evidence span".into());
        }
    }
    Ok(())
}

fn validate_span(span: Span) -> Result<(), String> {
    if span.start_byte >= span.end_byte || span.start_line > span.end_line {
        return Err("semantic evidence span is empty or reversed".into());
    }
    Ok(())
}

fn validate_fact_key(label: &str, value: &str) -> Result<(), String> {
    validate_digest_id(value, "sf1_").map_err(|error| format!("{label}: {error}"))
}

fn validate_text(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value != value.trim() {
        return Err(format!("{label} must be nonempty and trimmed"));
    }
    Ok(())
}

fn derive_id(record: &RoleScopeCommentEvidence) -> Result<RoleScopeCommentEvidenceId, String> {
    #[derive(Serialize)]
    struct Payload<'a> {
        schema: &'a str,
        candidate: &'a CandidateId,
        identifier_coverage: FactCoverage,
        comment_coverage: FactCoverage,
        identifiers: &'a [IdentifierSemanticEvidence],
        comments: &'a [CommentSemanticEvidence],
        disposition: SemanticEvidenceDisposition,
    }

    let payload = Payload {
        schema: &record.schema,
        candidate: &record.candidate,
        identifier_coverage: record.identifier_coverage,
        comment_coverage: record.comment_coverage,
        identifiers: &record.identifiers,
        comments: &record.comments,
        disposition: record.disposition,
    };
    let bytes = serde_json::to_vec(&payload).map_err(|error| error.to_string())?;
    Ok(RoleScopeCommentEvidenceId(format!(
        "rsc1_{}",
        blake3::hash(&bytes).to_hex()
    )))
}

fn validate_digest_id(value: &str, prefix: &str) -> Result<(), String> {
    let Some(digest) = value.strip_prefix(prefix) else {
        return Err(format!("identity must start with {prefix}"));
    };
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("identity must contain one 64-character hexadecimal digest".into());
    }
    Ok(())
}

impl fmt::Display for RoleScopeCommentEvidenceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(byte: char) -> String {
        format!("sf1_{}", byte.to_string().repeat(64))
    }

    fn identifier(scope_fact: String, start: usize) -> IdentifierSemanticEvidence {
        IdentifierSemanticEvidence {
            source_path: "src/lib.rs".into(),
            span: Span::new(1, 1, start, start + 5),
            spelling: "value".into(),
            role: IdentifierSemanticRole::Read,
            canonical_roles: vec![CanonicalRole::Expression, CanonicalRole::Read],
            scope_fact,
            binding_fact: Some(scope('b')),
            resolution: Some(ResolutionStatus::Unique),
        }
    }

    #[test]
    fn same_spelling_in_distinct_scopes_remains_distinct_and_canonical() {
        let mut identifiers = vec![identifier(scope('2'), 20), identifier(scope('1'), 10)];
        canonicalize_identifiers(&mut identifiers);
        validate_identifiers(&identifiers).unwrap();

        assert_eq!(identifiers.len(), 2);
        assert_ne!(identifiers[0].scope_fact, identifiers[1].scope_fact);
        assert!(
            validate_identifiers(&[IdentifierSemanticEvidence {
                resolution: Some(ResolutionStatus::Ambiguous),
                ..identifier(scope('3'), 30)
            }])
            .is_err()
        );
    }

    #[test]
    fn automatic_rationale_deletion_is_blocked_but_exact_retention_requires_review() {
        let rationale = CommentSemanticEvidence {
            source_path: "src/lib.rs".into(),
            span: Span::new(1, 1, 10, 32),
            text: "// SAFETY: invariant".into(),
            intent: CommentIntent::Rationale,
            owner_scope_fact: scope('1'),
        };
        validate_comments(std::slice::from_ref(&rationale)).unwrap();
        assert_eq!(
            semantic_disposition(
                CandidateDisposition::Automatic,
                &[SemanticEditRef {
                    source_path: "src/lib.rs".into(),
                    span: Span::new(1, 1, 0, 40),
                    after: "replacement();",
                }],
                &[],
                std::slice::from_ref(&rationale),
            ),
            SemanticEvidenceDisposition::AutomaticRationaleDeletionBlocked
        );
        assert_eq!(
            semantic_disposition(
                CandidateDisposition::Automatic,
                &[SemanticEditRef {
                    source_path: "src/lib.rs".into(),
                    span: Span::new(1, 1, 0, 40),
                    after: "// SAFETY: invariant\nreplacement();",
                }],
                &[],
                &[rationale],
            ),
            SemanticEvidenceDisposition::ReviewRequired
        );
    }

    #[test]
    fn public_api_and_comment_intents_are_protected_roles() {
        let public = IdentifierSemanticEvidence {
            role: IdentifierSemanticRole::PublicApiSurface,
            resolution: None,
            binding_fact: Some(scope('b')),
            ..identifier(scope('1'), 0)
        };
        validate_identifiers(std::slice::from_ref(&public)).unwrap();
        assert_eq!(
            semantic_disposition(CandidateDisposition::Automatic, &[], &[public], &[]),
            SemanticEvidenceDisposition::ReviewRequired
        );
        for intent in [
            CommentIntent::Documentation,
            CommentIntent::Rationale,
            CommentIntent::Suppression,
        ] {
            assert!(matches!(
                intent,
                CommentIntent::Documentation
                    | CommentIntent::Rationale
                    | CommentIntent::Suppression
            ));
        }
    }

    #[test]
    fn partial_authority_and_duplicate_spans_fail_closed() {
        assert_ne!(FactCoverage::Partial, FactCoverage::Complete);
        let evidence = identifier(scope('1'), 10);
        assert!(validate_identifiers(&[evidence.clone(), evidence]).is_err());
    }
}
