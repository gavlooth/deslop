use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use deslop_eval::m8_calibration::{
    CalibrationCorpus, CorpusMinimums, DatasetRegistry, EvaluationPolicy, FeatureCapture,
    PublishedComprehensionImport, PublishedPreferenceImport, assemble_published_corpus,
    evaluate_calibration, model_card,
};
use serde::de::DeserializeOwned;

fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args
        .first()
        .is_some_and(|argument| argument == "--assemble")
    {
        if args.len() != 5 {
            bail!(
                "usage: m8-calibration --assemble DATASET_REGISTRY.json PREFERENCES.json COMPREHENSION.json OUTPUT.json"
            );
        }
        let registry: DatasetRegistry = read_json(Path::new(&args[1]))?;
        let preferences: PublishedPreferenceImport = read_json(Path::new(&args[2]))?;
        let comprehension: PublishedComprehensionImport = read_json(Path::new(&args[3]))?;
        let corpus = assemble_published_corpus(&registry, preferences, comprehension)?;
        fs::write(&args[4], serde_json::to_vec_pretty(&corpus)?)
            .with_context(|| format!("write {}", args[4]))?;
        return Ok(());
    }
    if !(2..=3).contains(&args.len()) {
        bail!("usage: m8-calibration DATASET_REGISTRY.json CALIBRATION_CORPUS.json [OUTPUT.json]");
    }
    let registry: DatasetRegistry = read_json(Path::new(&args[0]))?;
    let corpus: CalibrationCorpus = read_json(Path::new(&args[1]))?;
    let capture = FeatureCapture::capture(&corpus)?;
    let report = evaluate_calibration(
        &registry,
        &corpus,
        &capture,
        EvaluationPolicy::default(),
        CorpusMinimums::M8,
    )?;
    let card = model_card(&report, &registry)?;
    let output = serde_json::to_vec_pretty(&serde_json::json!({
        "report": report,
        "model_card": card,
    }))?;
    if let Some(path) = args.get(2) {
        fs::write(path, output).with_context(|| format!("write {path}"))?;
    } else {
        println!("{}", String::from_utf8(output).expect("JSON is UTF-8"));
    }
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}
