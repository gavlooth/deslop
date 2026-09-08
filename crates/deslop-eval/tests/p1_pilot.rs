//! P1 pilot contract tests — FICTIONAL engineering fixtures only.
//!
use std::path::{Path, PathBuf};

use deslop_eval::pilot::{
    AdjudicationRecord, AnnotationProvenance, AnnotationRecord, CleanupJudgment, ObservationLabel,
    PILOT_EVAL_SCHEMA, PILOT_IMPORT_SCHEMA, PilotCase, PilotEligibility, PilotLanguage, PilotSplit,
    ProvenanceKind, ReviewerRecord, analyzer_config_pin, evaluate_dir, import_bundle,
};
// blake3 is a main dependency of deslop-eval, hence linkable from this
// integration test; checksums below are computed, never hardcoded.
use serde_json::json;

const PROTOCOL_PIN: &str = "fiction-protocol-pin";

fn grant(role: &str) -> serde_json::Value {
    json!({
        "decision": "approved",
        "spdx": "MIT",
        "evidence_uri": format!("fiction://grant/p1-contract-test/{role}"),
        "checked_on": "2026-09-08",
        "reason": format!("fiction {role} grant for contract tests")
    })
}

fn human_pair(obs: ObservationLabel, cleanup: CleanupJudgment) -> AnnotationRecord {
    AnnotationRecord {
        reviewers: vec![
            ReviewerRecord {
                reviewer_id: "fiction-rater-a".into(),
                observation: obs,
                cleanup_judgment: cleanup,
                reviewer_declared_provenance: AnnotationProvenance::IndependentHuman,
                reviewer_declared_blinded: true,
            },
            ReviewerRecord {
                reviewer_id: "fiction-rater-b".into(),
                observation: obs,
                cleanup_judgment: cleanup,
                reviewer_declared_provenance: AnnotationProvenance::IndependentHuman,
                reviewer_declared_blinded: true,
            },
        ],
        adjudication: None,
    }
}

fn dup_source() -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/sloppy/duplication.rs"),
    )
    .unwrap()
}

fn closure_source() -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/sloppy/rust_idioms.rs"),
    )
    .unwrap()
}

fn checksum_of(text: &str) -> String {
    // Resolved via dev-dependency blake3 (added alongside this test).
    format!("blake3:{}", blake3::hash(text.as_bytes()).to_hex())
}

#[allow(clippy::too_many_arguments)]
fn case(
    family: &str,
    language: PilotLanguage,
    path: &str,
    text: String,
    range: (usize, usize),
    provenance: ProvenanceKind,
    split: PilotSplit,
    annotation: AnnotationRecord,
    repo_family: &str,
    extra: Option<serde_json::Value>,
) -> serde_json::Value {
    let checksum = checksum_of(&text);
    let mut v = json!({
        "repo_family": repo_family,
        "clone_group": "fiction-clone-1",
        "leakage": {},
        "revision": "fiction-rev-1",
        "unit_kind": "callable",
        "family": family,
        "language": language,
        "source_path": path,
        "source_text": text,
        "source_range": {"start_line": range.0, "end_line": range.1},
        "task_contract": "fiction contract: keep behavior",
        "declared_check": "unknown",
        "check_evidence": null,
        "supplier_outcome": "evaluated",
        "preconditions": [],
        "counterexamples": [],
        "selection_probability": 0.5,
        "workload_stratum": "fiction-small",
        "split": split,
        "annotation": annotation,
        "provenance": {"kind": provenance, "detail": "fiction"},
        "retrieval_checksum": checksum,
    });
    if let Some(patch) = extra {
        for (k, val) in patch.as_object().unwrap() {
            v[k] = val.clone();
        }
    }
    v
}

fn bundle(
    cases: Vec<serde_json::Value>,
    source_license: serde_json::Value,
    annotation_license: serde_json::Value,
) -> serde_json::Value {
    json!({
        "schema": PILOT_IMPORT_SCHEMA,
        "protocol_pin": PROTOCOL_PIN,
        "analyzer_config_pin": analyzer_config_pin(),
        "source_license": source_license,
        "annotation_license": annotation_license,
        "cases": cases,
    })
}

fn write_bundle(dir: &Path, name: &str, value: &serde_json::Value) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, serde_json::to_string_pretty(value).unwrap()).unwrap();
    p
}

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

// 1. Determinism: same bundle into two fresh roots => identical case IDs.
#[test]
fn import_is_deterministic_across_fresh_roots() {
    let text = dup_source();
    let lines = text.lines().count();
    let ann = human_pair(ObservationLabel::Present, CleanupJudgment::Uncertain);
    let b = bundle(
        vec![case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_dup.rs",
            text,
            (1, lines),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            ann,
            "fiction-family-a",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let d1 = work.path().join("out1");
    let d2 = work.path().join("out2");
    let c1 = import_bundle(&bp, &d1).unwrap();
    let c2 = import_bundle(&bp, &d2).unwrap();
    assert_eq!(
        c1, c2,
        "fiction determinism: identical inputs, identical cases"
    );
    let r1 = evaluate_dir(&d1, PROTOCOL_PIN).unwrap();
    let r2 = evaluate_dir(&d2, PROTOCOL_PIN).unwrap();
    assert_eq!(r1.strata, r2.strata);
    assert_eq!(r1.schema, PILOT_EVAL_SCHEMA);
}

// 2. Synthetic rows: analyzer runs (observable prediction) but never scores.
#[test]
fn synthetic_rows_never_score_as_truth() {
    let text = dup_source();
    let lines = text.lines().count();
    let ann = human_pair(ObservationLabel::Present, CleanupJudgment::Desirable);
    let b = bundle(
        vec![case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_synth.rs",
            text,
            (1, lines),
            ProvenanceKind::EngineeringSynthetic,
            PilotSplit::Calibration,
            ann,
            "fiction-family-s",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let out = work.path().join("out");
    let cases = import_bundle(&bp, &out).unwrap();
    assert_eq!(cases.len(), 1);
    let report = evaluate_dir(&out, PROTOCOL_PIN).unwrap();
    let stratum = report
        .strata
        .iter()
        .find(|s| s.provenance == ProvenanceKind::EngineeringSynthetic)
        .expect("fiction synthetic stratum reported");
    assert_eq!(stratum.true_positives + stratum.false_negatives, 0);
    assert_eq!(stratum.precision, None, "null rate, never zero-as-healthy");
    assert_eq!(stratum.recall, None);
    assert!(
        report.unresolved_case_ids.contains(&cases[0].case_id),
        "synthetic stays visible as unscored"
    );
    // The analyzer still ran: observable prediction exists with null truth.
    let pred = report
        .predictions
        .iter()
        .find(|p| p.case_id == cases[0].case_id)
        .expect("fiction synthetic prediction observable");
    assert_eq!(pred.analysis_status, "Complete");
    assert!(
        pred.predicted,
        "duplication fixture fires on synthetic bytes too"
    );
    assert!(pred.rules.contains(&"duplicate-block".to_string()));
}

// 3. Branch on Rust/Python: unsupported, counted, never scored.
#[test]
fn rust_branch_simplification_is_unsupported_never_scored() {
    let text = "fn f(x: bool) -> bool {\n    if x { true } else { false }\n}\n".to_string();
    let ann = human_pair(ObservationLabel::Present, CleanupJudgment::Uncertain);
    let b = bundle(
        vec![case(
            "branch-simplification",
            PilotLanguage::Rust,
            "src/fiction_branch.rs",
            text,
            (1, 3),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            ann,
            "fiction-family-b",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let out = work.path().join("out");
    import_bundle(&bp, &out).unwrap();
    let report = evaluate_dir(&out, PROTOCOL_PIN).unwrap();
    let s = report
        .strata
        .iter()
        .find(|s| s.family == "branch-simplification")
        .unwrap();
    assert_eq!(s.unsupported, 1);
    assert_eq!(s.true_positives + s.false_positives + s.false_negatives, 0);
    assert_eq!(s.precision, None);
    assert_eq!(s.recall, None);
}

// 4. Sealed rows: excluded before analysis, counted only in sealed_excluded.
#[test]
fn sealed_rows_never_reach_analysis_or_denominators() {
    let text = dup_source();
    let lines = text.lines().count();
    let ann = human_pair(ObservationLabel::Present, CleanupJudgment::Desirable);
    let b = bundle(
        vec![case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_sealed.rs",
            text,
            (1, lines),
            ProvenanceKind::Challenge,
            PilotSplit::SealedConfirmatory,
            ann,
            "fiction-family-sealed",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let out = work.path().join("out");
    import_bundle(&bp, &out).unwrap();
    let report = evaluate_dir(&out, PROTOCOL_PIN).unwrap();
    assert_eq!(report.sealed_excluded, 1);
    assert_eq!(report.cases_total, 1);
    assert!(report.strata.is_empty(), "sealed labels never enter strata");
    assert!(report.failures.is_empty());
}

// 5. Source tamper after import fails revalidation (identity/digest gate).
#[test]
fn tampered_source_bytes_fail_eval() {
    let text = dup_source();
    let lines = text.lines().count();
    let ann = human_pair(ObservationLabel::Absent, CleanupJudgment::Undesirable);
    let b = bundle(
        vec![case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_tamper.rs",
            text,
            (1, lines),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            ann,
            "fiction-family-t",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let out = work.path().join("out");
    let cases = import_bundle(&bp, &out).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap();
    let rel = manifest["cases"][&cases[0].case_id]
        .as_str()
        .unwrap()
        .to_string();
    let p = out.join(&rel);
    let mut stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    // Same line count, same selected range: only a same-line literal changes,
    // so the range gate still passes and ONLY the digest/identity gate fires.
    let tampered = stored["source_text"]
        .as_str()
        .unwrap()
        .replacen("value * 2", "value * 3", 1);
    assert_ne!(tampered, stored["source_text"].as_str().unwrap());
    assert_eq!(tampered.lines().count(), lines);
    stored["source_text"] = json!(tampered);
    std::fs::write(&p, serde_json::to_string_pretty(&stored).unwrap()).unwrap();
    let err = evaluate_dir(&out, PROTOCOL_PIN).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("mismatch") && !msg.contains("source_range"),
        "tamper must trip the digest gate, not the range gate: {err}"
    );
}

// 6. Stored-derived tamper (eligibility flip) fails without raw change.
#[test]
fn stored_eligibility_flip_fails_revalidation() {
    let text = dup_source();
    let lines = text.lines().count();
    // Model-derived pair: derived truth is Uncertain/PilotOnly at import.
    let ann = AnnotationRecord {
        reviewers: vec![
            ReviewerRecord {
                reviewer_id: "fiction-model-a".into(),
                observation: ObservationLabel::Present,
                cleanup_judgment: CleanupJudgment::Desirable,
                reviewer_declared_provenance: AnnotationProvenance::ModelDerived,
                reviewer_declared_blinded: true,
            },
            ReviewerRecord {
                reviewer_id: "fiction-model-b".into(),
                observation: ObservationLabel::Present,
                cleanup_judgment: CleanupJudgment::Desirable,
                reviewer_declared_provenance: AnnotationProvenance::ModelDerived,
                reviewer_declared_blinded: true,
            },
        ],
        adjudication: None,
    };
    let b = bundle(
        vec![case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_derived.rs",
            text,
            (1, lines),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            ann,
            "fiction-family-d",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let out = work.path().join("out");
    let cases = import_bundle(&bp, &out).unwrap();
    assert_eq!(cases[0].eligibility, PilotEligibility::PilotOnly);
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap();
    let rel = manifest["cases"][&cases[0].case_id]
        .as_str()
        .unwrap()
        .to_string();
    let p = out.join(&rel);
    let mut stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    stored["eligibility"] = json!("eligible");
    std::fs::write(&p, serde_json::to_string_pretty(&stored).unwrap()).unwrap();
    let err = evaluate_dir(&out, PROTOCOL_PIN).unwrap_err();
    assert!(
        err.to_string().contains("eligibility"),
        "derived flip must fail: {err}"
    );
}

// 7. Each rejected grant blocks import BEFORE publish: source-rejected and
// annotation-rejected are independent guards (valid bases, targeted errors).
#[test]
fn rejected_grants_block_before_publish() {
    let mk_bundle = |source: serde_json::Value, annotation: serde_json::Value| {
        let text = dup_source();
        let lines = text.lines().count();
        bundle(
            vec![case(
                "duplication",
                PilotLanguage::Rust,
                "src/fiction_lic.rs",
                text,
                (1, lines),
                ProvenanceKind::Challenge,
                PilotSplit::Calibration,
                human_pair(ObservationLabel::Absent, CleanupJudgment::Undesirable),
                "fiction-family-l",
                None,
            )],
            source,
            annotation,
        )
    };
    let mut src_rej = grant("source");
    src_rej["decision"] = json!("rejected");
    for (label, b) in [
        ("source", mk_bundle(src_rej, grant("annotation"))),
        (
            "annotation",
            mk_bundle(grant("source"), {
                let mut a = grant("annotation");
                a["decision"] = json!("rejected");
                a
            }),
        ),
    ] {
        let work = tmp();
        let bp = write_bundle(work.path(), "b.json", &b);
        let out = work.path().join("out");
        let err = import_bundle(&bp, &out).unwrap_err();
        assert!(err.to_string().contains(label), "{label} gate: {err}");
        assert!(err.to_string().contains("license"), "{label} gate: {err}");
        assert!(
            !out.join("manifest.json").exists(),
            "nothing published on gate failure"
        );
    }
}

// 8. Leakage: one lineage group across splits is rejected.
#[test]
fn lineage_group_cannot_span_splits() {
    let text = dup_source();
    let lines = text.lines().count();
    let mk = |split, fam: &str| {
        case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_leak.rs",
            text.clone(),
            (1, lines),
            ProvenanceKind::Challenge,
            split,
            human_pair(ObservationLabel::Uncertain, CleanupJudgment::Uncertain),
            fam,
            None,
        )
    };
    // Same repo_family => same leakage group, different splits => reject.
    let b = bundle(
        vec![
            mk(PilotSplit::Calibration, "fiction-fam-x"),
            mk(PilotSplit::SealedConfirmatory, "fiction-fam-x"),
        ],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let err = import_bundle(&bp, &work.path().join("out")).unwrap_err();
    assert!(
        err.to_string().contains("leakage"),
        "cross-split lineage: {err}"
    );
}

// 9. Duplicate reviewer IDs rejected (valid base, targeted guard).
#[test]
fn duplicate_reviewer_ids_rejected() {
    let text = dup_source();
    let lines = text.lines().count();
    let dup_ann = AnnotationRecord {
        reviewers: vec![
            ReviewerRecord {
                reviewer_id: "fiction-rater-a".into(),
                observation: ObservationLabel::Present,
                cleanup_judgment: CleanupJudgment::Desirable,
                reviewer_declared_provenance: AnnotationProvenance::IndependentHuman,
                reviewer_declared_blinded: true,
            },
            ReviewerRecord {
                reviewer_id: "fiction-rater-a".into(),
                observation: ObservationLabel::Present,
                cleanup_judgment: CleanupJudgment::Desirable,
                reviewer_declared_provenance: AnnotationProvenance::IndependentHuman,
                reviewer_declared_blinded: true,
            },
        ],
        adjudication: None,
    };
    let b = bundle(
        vec![case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_dupid.rs",
            text,
            (1, lines),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            dup_ann,
            "fiction-family-dup",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let err = import_bundle(&bp, &work.path().join("out")).unwrap_err();
    assert!(
        err.to_string().contains("duplicate reviewer"),
        "dup rater: {err}"
    );
}

// 10. Disagreement without adjudication rejected (valid base, targeted guard).
#[test]
fn disagreement_requires_third_party_adjudication() {
    let text = dup_source();
    let lines = text.lines().count();
    let split_ann = AnnotationRecord {
        reviewers: vec![
            ReviewerRecord {
                reviewer_id: "fiction-rater-a".into(),
                observation: ObservationLabel::Present,
                cleanup_judgment: CleanupJudgment::Desirable,
                reviewer_declared_provenance: AnnotationProvenance::IndependentHuman,
                reviewer_declared_blinded: true,
            },
            ReviewerRecord {
                reviewer_id: "fiction-rater-b".into(),
                observation: ObservationLabel::Absent,
                cleanup_judgment: CleanupJudgment::Undesirable,
                reviewer_declared_provenance: AnnotationProvenance::IndependentHuman,
                reviewer_declared_blinded: true,
            },
        ],
        adjudication: None,
    };
    let b = bundle(
        vec![case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_split.rs",
            text,
            (1, lines),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            split_ann,
            "fiction-family-s2",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let err = import_bundle(&bp, &work.path().join("out")).unwrap_err();
    assert!(
        err.to_string().contains("adjudication"),
        "needs adjudicator: {err}"
    );
}

// 11. Adjudicator must be a distinct third party (valid base, targeted guard).
#[test]
fn adjudicator_must_be_distinct_third_party() {
    let text = dup_source();
    let lines = text.lines().count();
    let bad_adj = AnnotationRecord {
        reviewers: vec![
            ReviewerRecord {
                reviewer_id: "fiction-rater-a".into(),
                observation: ObservationLabel::Present,
                cleanup_judgment: CleanupJudgment::Desirable,
                reviewer_declared_provenance: AnnotationProvenance::IndependentHuman,
                reviewer_declared_blinded: true,
            },
            ReviewerRecord {
                reviewer_id: "fiction-rater-b".into(),
                observation: ObservationLabel::Absent,
                cleanup_judgment: CleanupJudgment::Undesirable,
                reviewer_declared_provenance: AnnotationProvenance::IndependentHuman,
                reviewer_declared_blinded: true,
            },
        ],
        adjudication: Some(AdjudicationRecord {
            adjudicator_id: "fiction-rater-a".into(),
            adjudicator_declared_provenance: AnnotationProvenance::IndependentHuman,
            adjudicator_declared_blinded: true,
            resolved_observation: ObservationLabel::Present,
            resolved_cleanup: CleanupJudgment::Desirable,
            rationale: "fiction rationale".into(),
        }),
    };
    let b = bundle(
        vec![case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_adj.rs",
            text,
            (1, lines),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            bad_adj,
            "fiction-family-a3",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let err = import_bundle(&bp, &work.path().join("out")).unwrap_err();
    assert!(
        err.to_string().contains("third party"),
        "distinct adjudicator: {err}"
    );
}

// 12. Supplier outcome never gates static detection: a stale
// `supplier_outcome: rejected` row still runs the analyzer and scores.
#[test]
fn supplier_outcome_does_not_gate_static_detection() {
    let text = dup_source();
    let lines = text.lines().count();
    let ann = human_pair(ObservationLabel::Present, CleanupJudgment::Desirable);
    let b = bundle(
        vec![case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_supplier.rs",
            text,
            (1, lines),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            ann,
            "fiction-family-sup",
            Some(json!({"supplier_outcome": "rejected"})),
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let out = work.path().join("out");
    import_bundle(&bp, &out).unwrap();
    let report = evaluate_dir(&out, PROTOCOL_PIN).unwrap();
    let s = report
        .strata
        .iter()
        .find(|s| s.family == "duplication")
        .unwrap();
    // Analyzer still ran: a hit yields TP, a miss yields FN — never zeroed by
    // the supplier flag. Duplication fixture fires, so expect TP.
    assert_eq!(
        s.true_positives, 1,
        "static detection ran despite supplier flag"
    );
    assert_eq!(s.false_negatives, 0);
    // Every unsealed row is analyzed and represented in current report counters.
    assert_eq!(
        report.supplier_outcomes.rejected, 1,
        "supplier bucket reports"
    );
    assert_eq!(
        report.analyses_complete, 1,
        "analysis ran despite supplier flag"
    );
    assert!(report.predictions[0].predicted, "fixture fires");
}

// 12b. Range-scoped hit: a Present selected range containing the detector
#[test]
fn selected_range_hit_scores_true_positive() {
    let text = closure_source();
    let b = bundle(
        vec![case(
            "wrappers-indirection",
            PilotLanguage::Rust,
            "src/fiction_range_hit.rs",
            text,
            (9, 11),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            human_pair(ObservationLabel::Present, CleanupJudgment::Uncertain),
            "fiction-family-range",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let out = work.path().join("out");
    import_bundle(&bp, &out).unwrap();
    let report = evaluate_dir(&out, PROTOCOL_PIN).unwrap();
    let s = report
        .strata
        .iter()
        .find(|s| s.family == "wrappers-indirection")
        .unwrap();
    assert_eq!(s.true_positives, 1, "selected-unit hit must score TP");
}

// 12c. Range-scoped negative: an Absent selected range with a positive
// elsewhere in the same file must NOT inherit the file-level hit.
#[test]
fn absent_selected_range_with_positive_elsewhere_is_not_a_hit() {
    let text = closure_source();
    let b = bundle(
        vec![case(
            "wrappers-indirection",
            PilotLanguage::Rust,
            "src/fiction_range_miss.rs",
            text,
            (1, 3),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            human_pair(ObservationLabel::Absent, CleanupJudgment::Undesirable),
            "fiction-family-range-neg",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let out = work.path().join("out");
    import_bundle(&bp, &out).unwrap();
    let report = evaluate_dir(&out, PROTOCOL_PIN).unwrap();
    let s = report
        .strata
        .iter()
        .find(|s| s.family == "wrappers-indirection")
        .unwrap();
    assert_eq!(s.false_positives, 0, "off-range hit must not leak into FP");
    assert_eq!(s.true_negatives, 1, "clean selected unit scores TN");
}

// 12d. Distinct repo/unit contexts with identical selected lines are NOT
// deduped: two different units each count.
#[test]
fn distinct_unit_contexts_are_not_deduped() {
    let text = dup_source();
    let lines = text.lines().count();
    let mk = |fam: &str, unit: &str| {
        let mut c = case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_ctx.rs",
            text.clone(),
            (1, lines),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            human_pair(ObservationLabel::Present, CleanupJudgment::Uncertain),
            fam,
            Some(json!({"unit_kind": unit})),
        );
        c["clone_group"] = json!(format!("fiction-clone-{fam}-{unit}"));
        c
    };
    // Same unit_kind (one stratum) but different repo families: both count.
    let b = bundle(
        vec![
            mk("fiction-fam-1", "callable"),
            mk("fiction-fam-2", "callable"),
        ],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let out = work.path().join("out");
    import_bundle(&bp, &out).unwrap();
    let report = evaluate_dir(&out, PROTOCOL_PIN).unwrap();
    let tp: usize = report.strata.iter().map(|s| s.true_positives).sum();
    assert_eq!(
        tp, 2,
        "distinct units each score, none swallowed as overlap"
    );
}

// 13. Threshold-positive stays in the denominator (accepted R5-withdrawn rule):
// a tiny duplication Present is a genuine FN, not an exclusion.
#[test]
fn sub_threshold_duplication_present_is_a_miss_not_an_exclusion() {
    let text = "fn a() {}\nfn b() {}\n".to_string();
    let ann = human_pair(ObservationLabel::Present, CleanupJudgment::Uncertain);
    let b = bundle(
        vec![case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_tiny.rs",
            text,
            (1, 2),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            ann,
            "fiction-family-tiny",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let out = work.path().join("out");
    import_bundle(&bp, &out).unwrap();
    let report = evaluate_dir(&out, PROTOCOL_PIN).unwrap();
    let s = report
        .strata
        .iter()
        .find(|s| s.family == "duplication")
        .unwrap();
    assert_eq!(
        s.false_negatives, 1,
        "below-threshold hit-less Present is FN"
    );
    assert_eq!(s.recall, Some(0.0));
    assert_eq!(s.input_total, 1, "row stays in honest denominator");
    assert_eq!(
        s.recommendation_rate,
        Some(0.0),
        "predicted-positive/input rate"
    );
    assert_eq!(s.analysis_coverage, Some(1.0), "Complete supported/input");
    assert_eq!(report.analyses_complete, 1);
}

// 14. Uncertain observation abstains with null rates (no zero-as-healthy).
#[test]
fn uncertain_observation_abstains_with_null_rates() {
    let text = dup_source();
    let lines = text.lines().count();
    let ann = human_pair(ObservationLabel::Uncertain, CleanupJudgment::Uncertain);
    let b = bundle(
        vec![case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_unc.rs",
            text,
            (1, lines),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            ann,
            "fiction-family-u",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let out = work.path().join("out");
    import_bundle(&bp, &out).unwrap();
    let report = evaluate_dir(&out, PROTOCOL_PIN).unwrap();
    let s = report
        .strata
        .iter()
        .find(|s| s.family == "duplication")
        .unwrap();
    assert_eq!(s.abstained, 1);
    assert_eq!(s.precision, None);
    assert_eq!(s.recall, None);
}

// 15. Duplicate case content is rejected, not double-counted.
#[test]
fn duplicate_case_content_rejected() {
    let text = dup_source();
    let lines = text.lines().count();
    let mk = case(
        "duplication",
        PilotLanguage::Rust,
        "src/fiction_dd.rs",
        text.clone(),
        (1, lines),
        ProvenanceKind::Challenge,
        PilotSplit::Calibration,
        human_pair(ObservationLabel::Uncertain, CleanupJudgment::Uncertain),
        "fiction-family-dd",
        None,
    );
    let b = bundle(vec![mk.clone(), mk], grant("source"), grant("annotation"));
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let err = import_bundle(&bp, &work.path().join("out")).unwrap_err();
    assert!(
        err.to_string().contains("duplicate declared case-unit"),
        "dup unit rejected at import: {err}"
    );
}

// 16. Case IDs + checksums stable when raw data unchanged (re-import equal).
#[test]
fn case_ids_and_checksums_stable_on_reimport() {
    let text = closure_source();
    let b = bundle(
        vec![case(
            "wrappers-indirection",
            PilotLanguage::Rust,
            "src/fiction_wrap.rs",
            text,
            (9, 11),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            human_pair(ObservationLabel::Present, CleanupJudgment::Uncertain),
            "fiction-family-w",
            None,
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let a: Vec<PilotCase> = import_bundle(&bp, &work.path().join("o1")).unwrap();
    let c: Vec<PilotCase> = import_bundle(&bp, &work.path().join("o2")).unwrap();
    assert_eq!(a[0].case_id, c[0].case_id);
    assert_eq!(a[0].source_checksum, c[0].source_checksum);
    assert_eq!(a[0].retrieval_checksum, c[0].retrieval_checksum);
    assert!(a[0].case_id.starts_with("pilot1_"));
    assert_eq!(
        a[0].case_id.len(),
        "pilot1_".len() + 64,
        "full blake3-bound id"
    );
}

// 17. Each grant needs its own evidence: empty evidence_uri on either side
// blocks import before publish (valid base, targeted guard).
#[test]
fn each_grant_needs_own_evidence() {
    let mk_bundle = |source: serde_json::Value, annotation: serde_json::Value| {
        let text = dup_source();
        let lines = text.lines().count();
        bundle(
            vec![case(
                "duplication",
                PilotLanguage::Rust,
                "src/fiction_grant.rs",
                text,
                (1, lines),
                ProvenanceKind::Challenge,
                PilotSplit::Calibration,
                human_pair(ObservationLabel::Absent, CleanupJudgment::Undesirable),
                "fiction-family-g",
                None,
            )],
            source,
            annotation,
        )
    };
    let mut src_empty = grant("source");
    src_empty["evidence_uri"] = json!("");
    let mut ann_empty = grant("annotation");
    ann_empty["evidence_uri"] = json!("");
    for (label, b) in [
        ("source", mk_bundle(src_empty, grant("annotation"))),
        ("annotation", mk_bundle(grant("source"), ann_empty)),
    ] {
        let work = tmp();
        let bp = write_bundle(work.path(), "b.json", &b);
        let out = work.path().join("out");
        let err = import_bundle(&bp, &out).unwrap_err();
        assert!(err.to_string().contains(label), "{label} evidence: {err}");
        assert!(
            err.to_string().contains("evidence_uri"),
            "{label} evidence: {err}"
        );
        assert!(!out.join("manifest.json").exists());
    }
}

// 17b. Old single-grant keyword-coverage test DELETED (superseded by the
// dual-grant evidence tests above): reason text is free-form, never parsed.

// 18. Preserved declared check without evidence binding is rejected.
#[test]
fn unbound_preserved_claim_rejected() {
    let text = dup_source();
    let lines = text.lines().count();
    let ann = human_pair(ObservationLabel::Absent, CleanupJudgment::Undesirable);
    let b = bundle(
        vec![case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_unbound.rs",
            text,
            (1, lines),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            ann,
            "fiction-family-ub",
            Some(json!({"declared_check": "preserved", "check_evidence": null})),
        )],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let err = import_bundle(&bp, &work.path().join("out")).unwrap_err();
    assert!(
        err.to_string().contains("check_evidence"),
        "unbound claim: {err}"
    );
}

// 19. Frozen probability boundary: p=0 rejected; None allowed for challenge,
// still required for natural. Valid bases, targeted guards.
#[test]
fn selection_probability_zero_rejected_none_allowed_for_challenge() {
    let text = dup_source();
    let lines = text.lines().count();
    let mk = |prob: serde_json::Value, prov: ProvenanceKind| {
        case(
            "duplication",
            PilotLanguage::Rust,
            "src/fiction_prob.rs",
            text.clone(),
            (1, lines),
            prov,
            PilotSplit::Calibration,
            human_pair(ObservationLabel::Uncertain, CleanupJudgment::Uncertain),
            "fiction-family-p",
            Some(json!({"selection_probability": prob})),
        )
    };
    let work = tmp();
    // p=0 rejected.
    let b0 = bundle(
        vec![mk(json!(0.0), ProvenanceKind::Challenge)],
        grant("source"),
        grant("annotation"),
    );
    let err = import_bundle(
        &write_bundle(work.path(), "b0.json", &b0),
        &work.path().join("o0"),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("selection_probability"),
        "p=0: {err}"
    );
    // None allowed for challenge: imports and evaluates.
    let bn = bundle(
        vec![mk(json!(null), ProvenanceKind::Challenge)],
        grant("source"),
        grant("annotation"),
    );
    let bp = write_bundle(work.path(), "bn.json", &bn);
    let out = work.path().join("on");
    import_bundle(&bp, &out).unwrap();
    evaluate_dir(&out, PROTOCOL_PIN).unwrap();
    // None still rejected for natural.
    let bnat = bundle(
        vec![mk(json!(null), ProvenanceKind::Natural)],
        grant("source"),
        grant("annotation"),
    );
    let err = import_bundle(
        &write_bundle(work.path(), "bnat.json", &bnat),
        &work.path().join("onat"),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("selection_probability"),
        "natural None: {err}"
    );
}

// 20. unit_kind splits strata: file vs callable never pool.
#[test]
fn unit_kind_stratifies_never_pooled() {
    let text = dup_source();
    let lines = text.lines().count();
    let mk = |unit: &str| {
        let mut c = case(
            "duplication",
            PilotLanguage::Rust,
            if unit == "callable" {
                "src/fiction_callable.rs"
            } else {
                "src/fiction_file.rs"
            },
            text.clone(),
            (1, lines),
            ProvenanceKind::Challenge,
            PilotSplit::Calibration,
            human_pair(ObservationLabel::Uncertain, CleanupJudgment::Uncertain),
            "fiction-family-uk",
            Some(json!({"unit_kind": unit})),
        );
        c["clone_group"] = json!(format!("fiction-clone-{unit}"));
        c
    };
    let b = bundle(
        vec![mk("callable"), mk("file")],
        grant("source"),
        grant("annotation"),
    );
    let work = tmp();
    let bp = write_bundle(work.path(), "b.json", &b);
    let out = work.path().join("out");
    import_bundle(&bp, &out).unwrap();
    let report = evaluate_dir(&out, PROTOCOL_PIN).unwrap();
    let kinds: Vec<&str> = report
        .strata
        .iter()
        .filter(|s| s.family == "duplication")
        .map(|s| s.unit_kind.as_str())
        .collect();
    assert!(
        kinds.contains(&"callable") && kinds.contains(&"file"),
        "split strata: {kinds:?}"
    );
    assert_eq!(kinds.len(), 2, "never pooled: {kinds:?}");
}
