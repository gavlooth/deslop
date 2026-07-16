use std::path::PathBuf;

use deslop_eval::m8_calibration::{
    CalibrationCorpus, CleanupTaskClass, CorpusMinimums, DatasetRegistry, EvaluationPolicy,
    FeatureCapture, ModelDisposition, RankerKind, evaluate_calibration, model_card,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("evaluation/m8")
        .join(name)
}

#[test]
fn m8_definition_of_done_preserves_axes_and_refuses_an_unproven_label() {
    let registry: DatasetRegistry =
        serde_json::from_slice(&std::fs::read(fixture("dataset_registry.json")).unwrap()).unwrap();
    let corpus_bytes = std::fs::read(fixture("corpus.json")).unwrap();
    let corpus: CalibrationCorpus = serde_json::from_slice(&corpus_bytes).unwrap();
    assert!(
        !String::from_utf8_lossy(&corpus_bytes).contains("\"authorship\""),
        "row-level authorship must not be a feature or target"
    );

    let capture = FeatureCapture::capture(&corpus).unwrap();
    let report = evaluate_calibration(
        &registry,
        &corpus,
        &capture,
        EvaluationPolicy::default(),
        CorpusMinimums::M8,
    )
    .unwrap();
    let card = model_card(&report, &registry).unwrap();

    assert_eq!(report.corpus.pairs, 300);
    assert_eq!(report.corpus.comprehension_samples, 1_727);
    assert_eq!(report.corpus.cleanup_tasks, 240);
    assert_eq!(report.corpus.unsafe_near_misses, 40);
    assert_eq!(
        corpus
            .cleanup_tasks
            .iter()
            .filter(|task| task.class == CleanupTaskClass::Cleanup)
            .count(),
        200
    );
    assert_eq!(report.corpus.languages, 8);
    assert_eq!(report.corpus.roles, 4);
    assert_eq!(report.corpus.projects, 1);
    assert_eq!(report.leave_language_out.len(), 8);
    assert_eq!(report.leave_project_out.len(), 1);
    assert_eq!(report.ablations.len(), 8);
    assert_eq!(card.transparent_axes.len(), 8);

    let challenger = report
        .overall
        .models
        .iter()
        .find(|metrics| metrics.model == RankerKind::PortableChallenger)
        .unwrap();
    let size = report
        .overall
        .models
        .iter()
        .find(|metrics| metrics.model == RankerKind::SizeBaseline)
        .unwrap();
    assert_eq!(challenger.correct, 171);
    assert!((challenger.accuracy - 0.57).abs() < 1e-12);
    assert!(challenger.accuracy > size.accuracy);
    assert!(challenger.accuracy_ci95.lower < 0.60);
    assert!(challenger.ece > 0.05);

    assert_eq!(report.decision.disposition, ModelDisposition::EvidenceOnly);
    assert!(!report.decision.readability_label_permitted);
    assert!(report.decision.model_id.is_none());
    assert!(
        report
            .decision
            .reasons
            .iter()
            .any(|reason| { reason.contains("unknown transparent axis") })
    );
    assert!(
        report
            .decision
            .reasons
            .iter()
            .any(|reason| { reason.contains("broader-than-perceived-readability") })
    );
    assert!(
        report
            .decision
            .reasons
            .iter()
            .any(|reason| { reason.contains("1 projects observed") })
    );
}
