use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use deslop_analyzer::{AnalyzerConfig, scan_paths_with_config};
use deslop_core::{FileReport, Finding};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub mod m10_canonical;
pub mod m10_dogfood;
pub mod m10_external;
pub mod m10_release;
pub mod m6_benchmark;
pub mod m8_calibration;
pub mod m9_scale;
pub mod refactor_eval;

const DEFAULT_EPSILON: f64 = 0.0001;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalManifest {
    pub schema: String,
    #[serde(default = "default_epsilon")]
    pub epsilon: f64,
    pub cases: Vec<CorpusCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusCase {
    pub path: PathBuf,
    pub label: QualityLabel,
    pub language: String,
    pub expectations: Vec<RuleExpectation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualityLabel {
    Clean,
    Sloppy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleExpectation {
    pub rule: String,
    pub should_fire: bool,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub schema: String,
    pub corpus: CorpusSummary,
    pub overall: RuleScore,
    pub rules: Vec<RuleScore>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusSummary {
    pub cases: usize,
    pub clean_cases: usize,
    pub sloppy_cases: usize,
    pub languages: BTreeMap<String, usize>,
    pub expectations_by_rule: BTreeMap<String, RuleExpectationCount>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleExpectationCount {
    pub should_fire: usize,
    pub should_not_fire: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleScore {
    pub rule: String,
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalBaseline {
    pub schema: String,
    pub epsilon: f64,
    pub rules: Vec<RuleBaseline>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleBaseline {
    pub rule: String,
    pub precision: f64,
    pub recall: f64,
}

pub fn run_eval(corpus_root: &Path) -> Result<EvalReport> {
    let manifest = read_manifest(corpus_root)?;
    run_eval_with_manifest(corpus_root, &manifest)
}

pub fn read_manifest(corpus_root: &Path) -> Result<EvalManifest> {
    let path = corpus_root.join("manifest.json");
    let manifest: EvalManifest = read_json_file(&path)?;
    if manifest.schema != "deslop.eval-manifest/1" {
        bail!("unsupported eval manifest schema `{}`", manifest.schema);
    }
    Ok(manifest)
}

pub fn read_baseline(corpus_root: &Path) -> Result<EvalBaseline> {
    let path = corpus_root.join("baseline.json");
    let baseline: EvalBaseline = read_json_file(&path)?;
    if baseline.schema != "deslop.eval-baseline/1" {
        bail!("unsupported eval baseline schema `{}`", baseline.schema);
    }
    Ok(baseline)
}

pub fn assert_baseline(report: &EvalReport, baseline: &EvalBaseline) -> Result<()> {
    let scores = report
        .rules
        .iter()
        .map(|score| (score.rule.as_str(), score))
        .collect::<BTreeMap<_, _>>();
    for expected in &baseline.rules {
        let Some(score) = scores.get(expected.rule.as_str()) else {
            bail!("baseline rule `{}` missing from eval report", expected.rule);
        };
        let min_precision = expected.precision - baseline.epsilon;
        let min_recall = expected.recall - baseline.epsilon;
        if score.precision < min_precision {
            bail!(
                "precision for `{}` regressed: {:.4} < baseline {:.4} - epsilon {:.4}",
                expected.rule,
                score.precision,
                expected.precision,
                baseline.epsilon
            );
        }
        if score.recall < min_recall {
            bail!(
                "recall for `{}` regressed: {:.4} < baseline {:.4} - epsilon {:.4}",
                expected.rule,
                score.recall,
                expected.recall,
                baseline.epsilon
            );
        }
    }
    Ok(())
}

pub fn render_eval_json(report: &EvalReport) -> Result<String> {
    Ok(serde_json::to_string_pretty(report)?)
}

pub fn render_eval_text(report: &EvalReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "corpus: {} cases ({} clean, {} sloppy)\n",
        report.corpus.cases, report.corpus.clean_cases, report.corpus.sloppy_cases
    ));
    out.push_str("languages:\n");
    for (language, count) in &report.corpus.languages {
        out.push_str(&format!("  {language:<8} {count}\n"));
    }
    out.push_str("\nrule                     tp   fp   fn   precision  recall  f1\n");
    out.push_str("----------------------------------------------------------------\n");
    for score in &report.rules {
        out.push_str(&format!(
            "{:<24} {:>3} {:>4} {:>4} {:>10.3} {:>7.3} {:>5.3}\n",
            score.rule,
            score.true_positives,
            score.false_positives,
            score.false_negatives,
            score.precision,
            score.recall,
            score.f1
        ));
    }
    out.push_str("----------------------------------------------------------------\n");
    out.push_str(&format!(
        "{:<24} {:>3} {:>4} {:>4} {:>10.3} {:>7.3} {:>5.3}\n",
        "overall",
        report.overall.true_positives,
        report.overall.false_positives,
        report.overall.false_negatives,
        report.overall.precision,
        report.overall.recall,
        report.overall.f1
    ));
    out
}

pub fn append_false_positive_feedback(
    corpus_root: &Path,
    report: &FileReport,
    finding: &Finding,
) -> Result<PathBuf> {
    let mut manifest = read_manifest(corpus_root)?;
    let feedback_dir = corpus_root.join("feedback");
    fs::create_dir_all(&feedback_dir)
        .with_context(|| format!("failed to create {}", feedback_dir.display()))?;
    let extension = report
        .path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("txt");
    let relative_case_path = PathBuf::from("feedback").join(format!(
        "{}.{}",
        safe_feedback_filename(&finding.fingerprint),
        extension
    ));
    let absolute_case_path = corpus_root.join(&relative_case_path);
    if !absolute_case_path.exists() {
        fs::copy(&report.path, &absolute_case_path).with_context(|| {
            format!(
                "failed to copy {} to {}",
                report.path.display(),
                absolute_case_path.display()
            )
        })?;
    }
    if !manifest
        .cases
        .iter()
        .any(|case| case.path == relative_case_path)
    {
        manifest.cases.push(CorpusCase {
            path: relative_case_path.clone(),
            label: QualityLabel::Clean,
            language: report.lang.to_string(),
            expectations: vec![RuleExpectation {
                rule: finding.rule.to_owned(),
                should_fire: false,
                start_line: Some(finding.span.start_line),
                end_line: Some(finding.span.end_line),
                note: Some(format!(
                    "false-positive feedback for fingerprint {}",
                    finding.fingerprint
                )),
            }],
        });
        write_manifest(corpus_root, &manifest)?;
    }
    Ok(relative_case_path)
}

fn write_manifest(corpus_root: &Path, manifest: &EvalManifest) -> Result<()> {
    let path = corpus_root.join("manifest.json");
    let rendered = serde_json::to_string_pretty(manifest)?;
    fs::write(&path, format!("{rendered}\n"))
        .with_context(|| format!("failed to write {}", path.display()))
}

fn safe_feedback_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn run_eval_with_manifest(corpus_root: &Path, manifest: &EvalManifest) -> Result<EvalReport> {
    let mut rule_counts = BTreeMap::<String, RuleScore>::new();
    let mut summary = empty_corpus_summary(manifest);
    let case_paths = manifest
        .cases
        .iter()
        .map(|case| corpus_root.join(&case.path))
        .collect::<Vec<_>>();
    let reports = scan_paths_with_config(&case_paths, AnalyzerConfig::default())?
        .into_iter()
        .map(|report| (report.path.clone(), report))
        .collect::<BTreeMap<_, _>>();

    for case in &manifest.cases {
        record_case_summary(case, &mut summary, &mut rule_counts);
        let path = corpus_root.join(&case.path);
        let report = reports.get(&path).ok_or_else(|| {
            anyhow::anyhow!(
                "analyzer returned no report for corpus case {}",
                case.path.display()
            )
        })?;
        score_case(case, report, &mut rule_counts);
    }

    let rules = finalized_rule_scores(rule_counts);
    let overall = overall_score(&rules);

    Ok(EvalReport {
        schema: "deslop.eval/1".to_string(),
        corpus: summary,
        overall,
        rules,
    })
}

pub(crate) fn read_json_file<T>(path: &Path) -> Result<T>
where
    T: DeserializeOwned,
{
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

fn empty_corpus_summary(manifest: &EvalManifest) -> CorpusSummary {
    CorpusSummary {
        cases: manifest.cases.len(),
        clean_cases: 0,
        sloppy_cases: 0,
        languages: BTreeMap::new(),
        expectations_by_rule: BTreeMap::new(),
    }
}

fn record_case_summary(
    case: &CorpusCase,
    summary: &mut CorpusSummary,
    rule_counts: &mut BTreeMap<String, RuleScore>,
) {
    match case.label {
        QualityLabel::Clean => summary.clean_cases += 1,
        QualityLabel::Sloppy => summary.sloppy_cases += 1,
    }
    *summary
        .languages
        .entry(case.language.to_owned())
        .or_default() += 1;
    for expectation in &case.expectations {
        let counts = summary
            .expectations_by_rule
            .entry(expectation.rule.to_owned())
            .or_default();
        if expectation.should_fire {
            counts.should_fire += 1;
        } else {
            counts.should_not_fire += 1;
        }
        ensure_rule_score(rule_counts, &expectation.rule);
    }
}

fn score_case(
    case: &CorpusCase,
    report: &FileReport,
    rule_counts: &mut BTreeMap<String, RuleScore>,
) {
    let mut matched_findings = BTreeSet::new();
    for expectation in &case.expectations {
        let match_idx = find_matching_finding(expectation, report, &matched_findings);
        record_expectation_result(expectation, match_idx, rule_counts, &mut matched_findings);
    }
    record_unmatched_findings(report, rule_counts, &matched_findings);
}

fn find_matching_finding(
    expectation: &RuleExpectation,
    report: &FileReport,
    matched_findings: &BTreeSet<usize>,
) -> Option<usize> {
    report
        .findings
        .iter()
        .enumerate()
        .find(|(idx, finding)| {
            !matched_findings.contains(idx)
                && finding.rule == expectation.rule
                && expectation_matches_finding(expectation, finding)
        })
        .map(|(idx, _)| idx)
}

fn record_expectation_result(
    expectation: &RuleExpectation,
    match_idx: Option<usize>,
    rule_counts: &mut BTreeMap<String, RuleScore>,
    matched_findings: &mut BTreeSet<usize>,
) {
    let score = ensure_rule_score(rule_counts, &expectation.rule);
    match (expectation.should_fire, match_idx) {
        (true, Some(idx)) => {
            score.true_positives += 1;
            matched_findings.insert(idx);
        }
        (true, None) => score.false_negatives += 1,
        (false, Some(idx)) => {
            score.false_positives += 1;
            matched_findings.insert(idx);
        }
        (false, None) => {}
    }
}

fn record_unmatched_findings(
    report: &FileReport,
    rule_counts: &mut BTreeMap<String, RuleScore>,
    matched_findings: &BTreeSet<usize>,
) {
    for (idx, finding) in report.findings.iter().enumerate() {
        if matched_findings.contains(&idx) {
            continue;
        }
        ensure_rule_score(rule_counts, &finding.rule).false_positives += 1;
    }
}

fn expectation_matches_finding(expectation: &RuleExpectation, finding: &Finding) -> bool {
    let Some(start) = expectation.start_line else {
        return true;
    };
    let end = expectation.end_line.unwrap_or(start);
    finding.span.start_line <= end && finding.span.end_line >= start
}

fn empty_score(rule: String) -> RuleScore {
    RuleScore {
        rule,
        ..RuleScore::default()
    }
}

fn ensure_rule_score<'a>(
    rule_counts: &'a mut BTreeMap<String, RuleScore>,
    rule: &str,
) -> &'a mut RuleScore {
    rule_counts
        .entry(rule.to_owned())
        .or_insert_with(|| empty_score(rule.to_owned()))
}

fn finalized_rule_scores(rule_counts: BTreeMap<String, RuleScore>) -> Vec<RuleScore> {
    let mut rules = rule_counts.into_values().collect::<Vec<_>>();
    for score in &mut rules {
        finalize_score(score);
    }
    rules
}

fn overall_score(rules: &[RuleScore]) -> RuleScore {
    let mut overall = RuleScore {
        rule: "overall".to_string(),
        true_positives: rules.iter().map(|score| score.true_positives).sum(),
        false_positives: rules.iter().map(|score| score.false_positives).sum(),
        false_negatives: rules.iter().map(|score| score.false_negatives).sum(),
        precision: 0.0,
        recall: 0.0,
        f1: 0.0,
    };
    finalize_score(&mut overall);
    overall
}

fn finalize_score(score: &mut RuleScore) {
    score.precision = ratio(
        score.true_positives,
        score.true_positives + score.false_positives,
    );
    score.recall = ratio(
        score.true_positives,
        score.true_positives + score.false_negatives,
    );
    score.f1 = if (score.precision + score.recall).abs() < f64::EPSILON {
        0.0
    } else {
        2.0 * score.precision * score.recall / (score.precision + score.recall)
    };
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        1.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn default_epsilon() -> f64 {
    DEFAULT_EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus")
    }

    #[test]
    fn eval_corpus_matches_quality_baseline() {
        let root = corpus_root();
        deslop_parse::reset_parse_source_invocations();
        let report = run_eval(&root).expect("eval report");
        let baseline = read_baseline(&root).expect("baseline");
        assert_baseline(&report, &baseline).expect("baseline ratchet");
        assert_eq!(deslop_parse::parse_source_invocations(), 0);
        let production = include_str!("lib.rs").split("#[cfg(test)]").next().unwrap();
        assert!(!production.contains("scan_file("));
        assert!(!production.contains("parse_source"));
        assert!(!production.contains("SourceFile::read"));
    }

    #[test]
    fn eval_report_renders_text_and_json() {
        let root = corpus_root();
        let report = run_eval(&root).expect("eval report");
        assert!(render_eval_text(&report).contains("precision"));
        let json = render_eval_json(&report).expect("json");
        assert!(json.contains("\"schema\": \"deslop.eval/1\""));
    }

    #[test]
    fn appends_false_positive_feedback_case() {
        let temp = tempfile::tempdir().expect("tempdir");
        let corpus = temp.path();
        fs::write(
            corpus.join("manifest.json"),
            r#"{"schema":"deslop.eval-manifest/1","cases":[]}"#,
        )
        .expect("manifest");
        let source = corpus.join("sample.py");
        fs::write(&source, "if value == None:\n    pass\n").expect("source");
        let report = FileReport {
            path: source,
            lang: deslop_core::Lang::Python,
            analysis: deslop_core::AnalysisProvenance::complete(),
            findings: Vec::new(),
        };
        let finding = Finding {
            path: report.path.clone(),
            span: deslop_core::Span::new(1, 1, 0, 17),
            rule: "py-none-comparison".to_string(),
            severity: deslop_core::Severity::Minor,
            safety: deslop_core::SafetyClass::SafeWithPrecondition,
            detected_by: deslop_core::DetectedBy::Idiom,
            message: "message".to_string(),
            suggestion: "suggestion".to_string(),
            precondition: None,
            edit: None,
            fingerprint: "abc123".to_string(),
        };
        let case_path =
            append_false_positive_feedback(corpus, &report, &finding).expect("append feedback");
        assert_eq!(case_path, PathBuf::from("feedback/abc123.py"));
        let manifest = read_manifest(corpus).expect("read manifest");
        assert_eq!(manifest.cases.len(), 1);
        assert!(!manifest.cases[0].expectations[0].should_fire);
    }
}
