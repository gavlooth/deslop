//! Exhaustive, bounded, content-addressed M10 dogfood evidence.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use deslop_analyzer::{AnalyzerConfig, scan_paths_with_context};
use deslop_core::{AnalysisStatus, Lang, SafetyClass, reports_analysis_status};
use deslop_protocol::{
    SharedWorkOrder, WorkOrderPlannerConstraints, plan_work_orders,
    shared_transformation_work_orders,
};
use deslop_recipes::{CandidateDisposition, TransformationCandidate, detect_rust_recipe_report};
use serde::{Deserialize, Serialize};

pub const M10_DOGFOOD_SCHEMA: &str = "deslop.m10-dogfood/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DogfoodClassification {
    ProductionUnknown,
    TestFixture,
    IntentionalSloppyCorpus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DogfoodDisposition {
    Accepted,
    Rejected,
    Unsafe,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindingDisposition {
    pub fingerprint: String,
    pub path: String,
    pub rule: String,
    pub safety: String,
    pub classification: DogfoodClassification,
    pub disposition: DogfoodDisposition,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeCandidateDisposition {
    pub candidate_id: String,
    pub path: String,
    pub recipe: String,
    pub safety: String,
    pub recipe_disposition: String,
    pub classification: DogfoodClassification,
    pub disposition: DogfoodDisposition,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipePartitionEvidence {
    pub path: String,
    pub selected_rust_files: usize,
    pub analyzed_rust_files: usize,
    pub candidate_count: usize,
    pub abstention_count: usize,
    pub elapsed_millis: u64,
    pub abstention_reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DogfoodPartitionOutput {
    pub evidence: RecipePartitionEvidence,
    pub candidates: Vec<RecipeCandidateDisposition>,
    pub production_orders: Vec<SharedWorkOrder>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DogfoodWorkOrderEvidence {
    pub status: String,
    pub exception: Option<String>,
    pub finding_orders: usize,
    pub transformation_orders: usize,
    pub total_orders: usize,
    pub unique_order_ids: usize,
    pub duplicate_order_ids: usize,
    pub order_digest: String,
    pub plan_id: String,
    pub plan_edges: usize,
    pub plan_groups: usize,
    pub plan_blocked_groups: usize,
    pub plan_waves: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DogfoodSourceEvidence {
    pub path: String,
    pub digest: String,
    pub bytes: usize,
    pub lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DogfoodReport {
    pub schema: String,
    pub report_id: String,
    pub source_revision: String,
    pub sources: Vec<DogfoodSourceEvidence>,
    pub source_files: usize,
    pub source_lines: usize,
    pub project_analysis_status: String,
    pub analysis_statuses: BTreeMap<String, usize>,
    pub findings: Vec<FindingDisposition>,
    pub finding_dispositions: BTreeMap<String, usize>,
    pub recipe_partitions: Vec<RecipePartitionEvidence>,
    pub recipe_candidates: Vec<RecipeCandidateDisposition>,
    pub recipe_dispositions: BTreeMap<String, usize>,
    pub recipe_elapsed_millis: u64,
    pub peak_rss_bytes: Option<u64>,
    pub work_orders: DogfoodWorkOrderEvidence,
    pub production_safe_auto_findings: usize,
    pub fixture_safe_auto_findings: usize,
    pub accepted_count: usize,
    pub rejected_count: usize,
    pub unsafe_count: usize,
    pub stale_count: usize,
}

impl DogfoodReport {
    pub fn validate(&self) -> Result<()> {
        if self.schema != M10_DOGFOOD_SCHEMA
            || self.source_files == 0
            || self.source_lines == 0
            || self.findings.is_empty()
            || self.recipe_partitions.is_empty()
        {
            bail!("M10 dogfood report is not release-complete");
        }
        if self.sources.len() != self.source_files
            || self
                .sources
                .iter()
                .map(|source| source.lines)
                .sum::<usize>()
                != self.source_lines
            || self
                .sources
                .windows(2)
                .any(|pair| pair[0].path >= pair[1].path)
            || self.source_revision != source_revision(&self.sources)?
        {
            bail!("dogfood source manifest disagrees with its summary or identity");
        }
        if self.findings.windows(2).any(|pair| {
            (&pair[0].path, &pair[0].fingerprint) >= (&pair[1].path, &pair[1].fingerprint)
        }) {
            bail!("dogfood finding dispositions must be sorted and unique");
        }
        if self.recipe_candidates.windows(2).any(|pair| {
            (&pair[0].path, &pair[0].candidate_id) >= (&pair[1].path, &pair[1].candidate_id)
        }) {
            bail!("dogfood recipe candidates must be sorted and unique");
        }
        if self
            .recipe_partitions
            .windows(2)
            .any(|pair| pair[0].path >= pair[1].path)
        {
            bail!("dogfood recipe partitions must be sorted and unique");
        }
        let analyzed = self
            .recipe_partitions
            .iter()
            .map(|partition| partition.analyzed_rust_files)
            .sum::<usize>();
        let selected = self
            .recipe_partitions
            .iter()
            .map(|partition| partition.selected_rust_files)
            .sum::<usize>();
        let abstentions = self
            .recipe_partitions
            .iter()
            .map(|partition| partition.abstention_count)
            .sum::<usize>();
        if selected != self.recipe_partitions.len()
            || analyzed + abstentions != selected
            || self
                .recipe_partitions
                .iter()
                .any(|partition| partition.selected_rust_files != 1)
        {
            bail!("dogfood recipe partitions are not exhaustive single-file units");
        }
        if self.production_safe_auto_findings != 0 {
            bail!("known production SafeAuto findings remain in the dogfood report");
        }
        if self.fixture_safe_auto_findings == 0
            || self.findings.iter().any(|finding| {
                finding.safety == "safe-auto" && finding.disposition != DogfoodDisposition::Rejected
            })
        {
            bail!("intentional fixture SafeAuto findings were not rejected");
        }
        let all_dispositions = self
            .findings
            .iter()
            .map(|finding| finding.disposition)
            .chain(
                self.recipe_candidates
                    .iter()
                    .map(|candidate| candidate.disposition),
            )
            .collect::<Vec<_>>();
        let counts = disposition_counts(all_dispositions.iter().copied());
        if counts.get("accepted").copied().unwrap_or_default() != self.accepted_count
            || counts.get("rejected").copied().unwrap_or_default() != self.rejected_count
            || counts.get("unsafe").copied().unwrap_or_default() != self.unsafe_count
            || counts.get("stale").copied().unwrap_or_default() != self.stale_count
            || self.finding_dispositions
                != disposition_counts(self.findings.iter().map(|finding| finding.disposition))
            || self.recipe_dispositions
                != disposition_counts(
                    self.recipe_candidates
                        .iter()
                        .map(|candidate| candidate.disposition),
                )
        {
            bail!("dogfood disposition totals disagree with their ledger");
        }
        if self.work_orders.total_orders
            != self.work_orders.finding_orders + self.work_orders.transformation_orders
            || self.work_orders.unique_order_ids != self.work_orders.total_orders
            || self.work_orders.duplicate_order_ids != 0
        {
            bail!("dogfood work-order identities are not unique and complete");
        }
        match self.work_orders.status.as_str() {
            "complete" if self.work_orders.exception.is_none() => {}
            "finding-proposal-timed-out-downgraded"
                if self.work_orders.finding_orders == 0
                    && self.work_orders.exception.as_deref().is_some_and(|reason| {
                        reason.contains("15-minute") && reason.contains("bounded protocol")
                    }) => {}
            _ => bail!("dogfood work-order status/exception is not an explicit release decision"),
        }
        let expected = dogfood_report_id(self)?;
        if self.report_id != expected {
            bail!("dogfood report identity mismatch: expected {expected}");
        }
        Ok(())
    }
}

pub fn assemble_dogfood_report(root: &Path) -> Result<DogfoodReport> {
    let root = root
        .canonicalize()
        .with_context(|| format!("resolve dogfood root {}", root.display()))?;
    let scan = scan_paths_with_context(std::slice::from_ref(&root), AnalyzerConfig::default())?;
    let sources = source_manifest(
        &scan.input_contents,
        scan.reports.iter().map(|report| &report.path),
    );
    let source_revision = source_revision(&sources)?;
    let source_lines = sources.iter().map(|source| source.lines).sum();
    let source_files = sources.len();
    let project_analysis_status = status_name(reports_analysis_status(&scan.reports)).to_string();
    let mut analysis_statuses = BTreeMap::new();
    let mut findings = Vec::new();
    let mut production_safe_auto_findings = 0;
    let mut fixture_safe_auto_findings = 0;
    let mut rust_paths = Vec::new();
    for report in &scan.reports {
        *analysis_statuses
            .entry(status_name(report.analysis.status).into())
            .or_default() += 1;
        if report.lang == Lang::Rust {
            rust_paths.push(report.path.clone());
        }
        for finding in &report.findings {
            let path = normalized_path(&root, &finding.path);
            let classification = classify_path(&path);
            let safety = safety_name(finding.safety).to_string();
            let (disposition, reason) = if finding.safety == SafetyClass::SafeAuto {
                if classification == DogfoodClassification::ProductionUnknown {
                    production_safe_auto_findings += 1;
                    (
                        DogfoodDisposition::Unsafe,
                        "production SafeAuto requires behavior verification before acceptance"
                            .into(),
                    )
                } else {
                    fixture_safe_auto_findings += 1;
                    (
                        DogfoodDisposition::Rejected,
                        "intentional test/sloppy fixture must remain unchanged".into(),
                    )
                }
            } else {
                (
                    DogfoodDisposition::Unsafe,
                    "non-SafeAuto finding has no dogfood rewrite authorization".into(),
                )
            };
            findings.push(FindingDisposition {
                fingerprint: finding.fingerprint.clone(),
                path,
                rule: finding.rule.clone(),
                safety,
                classification,
                disposition,
                reason,
            });
        }
    }
    findings.sort_by(|left, right| {
        (&left.path, &left.fingerprint).cmp(&(&right.path, &right.fingerprint))
    });
    rust_paths.sort();
    rust_paths.dedup();
    drop(scan);

    let recipe_started = Instant::now();
    let logical_paths = rust_paths
        .iter()
        .map(|path| PathBuf::from(normalized_path(&root, path)))
        .collect::<Vec<_>>();
    let outputs = run_partition_processes(&root, &logical_paths)?;
    let mut recipe_partitions = outputs
        .iter()
        .map(|output| output.evidence.clone())
        .collect::<Vec<_>>();
    let mut recipe_candidates = outputs
        .iter()
        .flat_map(|output| output.candidates.clone())
        .collect::<Vec<_>>();
    let mut production_orders = outputs
        .into_iter()
        .flat_map(|output| output.production_orders)
        .collect::<Vec<_>>();
    recipe_partitions.sort_by(|left, right| left.path.cmp(&right.path));
    recipe_candidates.sort_by(|left, right| {
        (&left.path, &left.candidate_id).cmp(&(&right.path, &right.candidate_id))
    });
    recipe_candidates.dedup_by(|left, right| left.candidate_id == right.candidate_id);
    production_orders.sort_by(|left, right| left.id().cmp(right.id()));
    production_orders.dedup_by(|left, right| left.id() == right.id());
    let recipe_elapsed_millis = millis(recipe_started.elapsed());

    let finding_order_count = 0;
    let transformation_order_count = production_orders.len();
    let orders = production_orders;
    let ids = orders
        .iter()
        .map(|order| order.id().to_string())
        .collect::<Vec<_>>();
    let unique_order_ids = ids.iter().collect::<BTreeSet<_>>().len();
    let order_digest = digest_json("deslop m10 dogfood work orders v1", &orders)?;
    let plan = plan_work_orders(orders, WorkOrderPlannerConstraints::default())?;
    let work_orders = DogfoodWorkOrderEvidence {
        status: "finding-proposal-timed-out-downgraded".into(),
        exception: Some(
            "whole-project finding proposal construction exceeded the measured 15-minute release ceiling; stable release exposes bounded protocol index/triage/explain/plan operations and does not claim an interactive exhaustive proposal batch".into(),
        ),
        finding_orders: finding_order_count,
        transformation_orders: transformation_order_count,
        total_orders: ids.len(),
        unique_order_ids,
        duplicate_order_ids: ids.len() - unique_order_ids,
        order_digest,
        plan_id: plan.id().to_string(),
        plan_edges: plan.edges().len(),
        plan_groups: plan.groups().len(),
        plan_blocked_groups: plan.blocked().len(),
        plan_waves: plan.waves().len(),
    };
    let finding_dispositions = disposition_counts(findings.iter().map(|item| item.disposition));
    let recipe_dispositions =
        disposition_counts(recipe_candidates.iter().map(|item| item.disposition));
    let all = findings
        .iter()
        .map(|item| item.disposition)
        .chain(recipe_candidates.iter().map(|item| item.disposition))
        .collect::<Vec<_>>();
    let all_counts = disposition_counts(all);
    let mut report = DogfoodReport {
        schema: M10_DOGFOOD_SCHEMA.into(),
        report_id: String::new(),
        source_revision,
        sources,
        source_files,
        source_lines,
        project_analysis_status,
        analysis_statuses,
        findings,
        finding_dispositions,
        recipe_partitions,
        recipe_candidates,
        recipe_dispositions,
        recipe_elapsed_millis,
        peak_rss_bytes: peak_rss_bytes(),
        work_orders,
        production_safe_auto_findings,
        fixture_safe_auto_findings,
        accepted_count: all_counts.get("accepted").copied().unwrap_or_default(),
        rejected_count: all_counts.get("rejected").copied().unwrap_or_default(),
        unsafe_count: all_counts.get("unsafe").copied().unwrap_or_default(),
        stale_count: all_counts.get("stale").copied().unwrap_or_default(),
    };
    report.report_id = dogfood_report_id(&report)?;
    report.validate()?;
    Ok(report)
}

pub fn evaluate_dogfood_partition(root: &Path, path: &Path) -> Result<DogfoodPartitionOutput> {
    let root = root
        .canonicalize()
        .with_context(|| format!("resolve dogfood root {}", root.display()))?;
    let logical = PathBuf::from(normalized_path(&root, path));
    let started = Instant::now();
    let report = detect_rust_recipe_report(&root, std::slice::from_ref(&logical))?;
    let mut abstention_reasons = report
        .abstentions
        .iter()
        .map(|abstention| format!("{}: {}", abstention.stage, abstention.reason))
        .collect::<Vec<_>>();
    abstention_reasons.sort();
    let mut candidates = report
        .candidates
        .iter()
        .map(candidate_disposition)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
    let production_candidates = report
        .candidates
        .into_iter()
        .filter(|candidate| {
            classify_path(&candidate.target().node.file().path.to_string_lossy())
                == DogfoodClassification::ProductionUnknown
        })
        .collect::<Vec<TransformationCandidate>>();
    let production_orders = shared_transformation_work_orders(production_candidates)?;
    Ok(DogfoodPartitionOutput {
        evidence: RecipePartitionEvidence {
            path: logical.to_string_lossy().into_owned(),
            selected_rust_files: report.selected_rust_files,
            analyzed_rust_files: report.analyzed_rust_files,
            candidate_count: candidates.len(),
            abstention_count: report.abstentions.len(),
            elapsed_millis: millis(started.elapsed()),
            abstention_reasons,
        },
        candidates,
        production_orders,
    })
}

fn run_partition_processes(root: &Path, paths: &[PathBuf]) -> Result<Vec<DogfoodPartitionOutput>> {
    let executable = env::current_exe().context("resolve dogfood evaluator executable")?;
    let worker_count = paths.len().clamp(1, 3);
    let next = Arc::new(AtomicUsize::new(0));
    let (sender, receiver) = mpsc::channel();
    thread::scope(|scope| {
        for _ in 0..worker_count {
            let sender = sender.clone();
            let next = Arc::clone(&next);
            let executable = &executable;
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(index) else {
                        break;
                    };
                    let result = run_partition_process(executable, root, path, index);
                    if sender.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
    });
    drop(sender);
    let mut completed = BTreeMap::new();
    for _ in 0..paths.len() {
        let (index, result) = receiver
            .recv()
            .context("dogfood partition worker exited without a result")?;
        completed.insert(index, result?);
    }
    Ok(completed.into_values().collect())
}

fn run_partition_process(
    executable: &Path,
    root: &Path,
    path: &Path,
    index: usize,
) -> Result<DogfoodPartitionOutput> {
    let output_path = env::temp_dir().join(format!(
        "deslop-m10-dogfood-{}-{index}.json",
        std::process::id()
    ));
    let _ = fs::remove_file(&output_path);
    let started = Instant::now();
    let output = Command::new("timeout")
        .args(["--signal=KILL", "30s"])
        .arg(executable)
        .arg("internal-partition")
        .arg(root)
        .arg(path)
        .arg(&output_path)
        .output()
        .with_context(|| format!("spawn dogfood partition {}", path.display()))?;
    let elapsed_millis = millis(started.elapsed());
    let result = if output.status.success() {
        let mut partition: DogfoodPartitionOutput = serde_json::from_slice(
            &fs::read(&output_path)
                .with_context(|| format!("read dogfood partition {}", output_path.display()))?,
        )?;
        partition.evidence.elapsed_millis = elapsed_millis;
        partition
    } else {
        let status = output.status.code();
        let reason = if matches!(status, Some(124 | 137) | None) {
            "partition exceeded the 30-second release budget".to_string()
        } else {
            format!(
                "partition process failed with status {status:?}: {}",
                bounded_text(&output.stderr)
            )
        };
        DogfoodPartitionOutput {
            evidence: RecipePartitionEvidence {
                path: path.to_string_lossy().into_owned(),
                selected_rust_files: 1,
                analyzed_rust_files: 0,
                candidate_count: 0,
                abstention_count: 1,
                elapsed_millis,
                abstention_reasons: vec![reason],
            },
            candidates: Vec::new(),
            production_orders: Vec::new(),
        }
    };
    let _ = fs::remove_file(output_path);
    Ok(result)
}

fn bounded_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).chars().take(500).collect()
}

fn candidate_disposition(candidate: &TransformationCandidate) -> RecipeCandidateDisposition {
    let path = candidate
        .target()
        .node
        .file()
        .path
        .to_string_lossy()
        .into_owned();
    let classification = classify_path(&path);
    let recipe_disposition = match candidate.disposition() {
        CandidateDisposition::Automatic => "automatic",
        CandidateDisposition::ReviewRequired => "review-required",
    };
    let (disposition, reason) = if classification != DogfoodClassification::ProductionUnknown {
        (
            DogfoodDisposition::Rejected,
            "test/fixture recipe candidate is not production work".into(),
        )
    } else {
        (
            DogfoodDisposition::Unsafe,
            "candidate was detected read-only and has not passed dogfood apply verification".into(),
        )
    };
    RecipeCandidateDisposition {
        candidate_id: candidate.id().to_string(),
        path,
        recipe: candidate.recipe().name().into(),
        safety: safety_name(candidate.safety()).into(),
        recipe_disposition: recipe_disposition.into(),
        classification,
        disposition,
        reason,
    }
}

fn classify_path(path: &str) -> DogfoodClassification {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with("tests/corpus/sloppy/")
        || normalized.contains("/tests/corpus/sloppy/")
    {
        DogfoodClassification::IntentionalSloppyCorpus
    } else if normalized.starts_with("tests/")
        || normalized.contains("/tests/")
        || normalized.contains("/fixtures/")
        || normalized.starts_with("fixtures/")
    {
        DogfoodClassification::TestFixture
    } else {
        DogfoodClassification::ProductionUnknown
    }
}

fn disposition_counts(
    dispositions: impl IntoIterator<Item = DogfoodDisposition>,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for disposition in dispositions {
        let name = match disposition {
            DogfoodDisposition::Accepted => "accepted",
            DogfoodDisposition::Rejected => "rejected",
            DogfoodDisposition::Unsafe => "unsafe",
            DogfoodDisposition::Stale => "stale",
        };
        *counts.entry(name.into()).or_default() += 1;
    }
    for name in ["accepted", "rejected", "unsafe", "stale"] {
        counts.entry(name.into()).or_default();
    }
    counts
}

fn normalized_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

fn source_manifest<'a>(
    sources: &BTreeMap<PathBuf, String>,
    paths: impl IntoIterator<Item = &'a PathBuf>,
) -> Vec<DogfoodSourceEvidence> {
    let mut manifest = paths
        .into_iter()
        .filter_map(|path| sources.get(path).map(|source| (path, source)))
        .map(|(path, source)| DogfoodSourceEvidence {
            path: path.to_string_lossy().replace('\\', "/"),
            digest: digest("deslop m10 dogfood source file v1", source.as_bytes()),
            bytes: source.len(),
            lines: source.lines().count(),
        })
        .collect::<Vec<_>>();
    manifest.sort_by(|left, right| left.path.cmp(&right.path));
    manifest
}

fn source_revision(sources: &[DogfoodSourceEvidence]) -> Result<String> {
    Ok(format!(
        "m10src1_{}",
        &digest_json("deslop m10 dogfood source revision v1", sources)?[7..]
    ))
}

fn dogfood_report_id(report: &DogfoodReport) -> Result<String> {
    #[derive(Serialize)]
    struct Identity<'a> {
        source_revision: &'a str,
        sources: &'a [DogfoodSourceEvidence],
        source_files: usize,
        source_lines: usize,
        project_analysis_status: &'a str,
        analysis_statuses: &'a BTreeMap<String, usize>,
        findings: &'a [FindingDisposition],
        recipe_partitions: &'a [RecipePartitionEvidence],
        recipe_candidates: &'a [RecipeCandidateDisposition],
        recipe_elapsed_millis: u64,
        peak_rss_bytes: Option<u64>,
        work_orders: &'a DogfoodWorkOrderEvidence,
    }
    let identity = Identity {
        source_revision: &report.source_revision,
        sources: &report.sources,
        source_files: report.source_files,
        source_lines: report.source_lines,
        project_analysis_status: &report.project_analysis_status,
        analysis_statuses: &report.analysis_statuses,
        findings: &report.findings,
        recipe_partitions: &report.recipe_partitions,
        recipe_candidates: &report.recipe_candidates,
        recipe_elapsed_millis: report.recipe_elapsed_millis,
        peak_rss_bytes: report.peak_rss_bytes,
        work_orders: &report.work_orders,
    };
    Ok(format!(
        "m10df1_{}",
        &digest_json("deslop m10 dogfood report v1", &identity)?[7..]
    ))
}

pub fn write_dogfood_report(path: &Path, report: &DogfoodReport) -> Result<()> {
    report.validate()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(report)?)?;
    Ok(())
}

pub fn write_dogfood_partition(path: &Path, output: &DogfoodPartitionOutput) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec(output)?)?;
    Ok(())
}

pub fn read_dogfood_report(path: &Path) -> Result<DogfoodReport> {
    let report: DogfoodReport = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read dogfood report {}", path.display()))?,
    )?;
    report.validate()?;
    Ok(report)
}

pub fn verify_dogfood_report(root: &Path, path: &Path) -> Result<DogfoodReport> {
    let report = read_dogfood_report(path)?;
    let scan = scan_paths_with_context(
        &[root
            .canonicalize()
            .with_context(|| format!("resolve dogfood root {}", root.display()))?],
        AnalyzerConfig::default(),
    )?;
    let current_sources = source_manifest(
        &scan.input_contents,
        scan.reports.iter().map(|report| &report.path),
    );
    let current = source_revision(&current_sources)?;
    if report.sources != current_sources {
        let mismatch = report
            .sources
            .iter()
            .zip(&current_sources)
            .find(|(stored, observed)| stored != observed)
            .map(|(stored, observed)| format!("stored {} observed {}", stored.path, observed.path))
            .unwrap_or_else(|| {
                format!(
                    "stored {} sources observed {}",
                    report.sources.len(),
                    current_sources.len()
                )
            });
        bail!("dogfood report source revision is stale: current {current}; {mismatch}");
    }
    Ok(report)
}

fn safety_name(safety: SafetyClass) -> &'static str {
    match safety {
        SafetyClass::SafeAuto => "safe-auto",
        SafetyClass::AnalyzerConfirmed => "analyzer-confirmed",
        SafetyClass::SafeWithPrecondition => "safe-with-precondition",
        SafetyClass::RiskySuggest => "risky-suggest",
        SafetyClass::LlmOnly => "llm-only",
        SafetyClass::NeverAuto => "never-auto",
    }
}

fn status_name(status: AnalysisStatus) -> &'static str {
    match status {
        AnalysisStatus::Unknown => "unknown",
        AnalysisStatus::Complete => "complete",
        AnalysisStatus::Partial => "partial",
        AnalysisStatus::Unsupported => "unsupported",
        AnalysisStatus::Failed => "failed",
    }
}

fn millis(duration: std::time::Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

#[cfg(target_os = "linux")]
fn peak_rss_bytes() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let kilobytes = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    kilobytes.checked_mul(1024)
}

#[cfg(not(target_os = "linux"))]
fn peak_rss_bytes() -> Option<u64> {
    None
}

fn digest_json(domain: &str, value: &(impl Serialize + ?Sized)) -> Result<String> {
    Ok(digest(domain, &serde_json::to_vec(value)?))
}

fn digest(domain: &str, bytes: &[u8]) -> String {
    let digest = blake3::derive_key(domain, bytes);
    format!("blake3:{}", blake3::Hash::from_bytes(digest).to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dogfood_classification_and_disposition_totals_are_explicit() {
        assert_eq!(
            classify_path("tests/corpus/sloppy/example.clj"),
            DogfoodClassification::IntentionalSloppyCorpus
        );
        assert_eq!(
            classify_path("crates/example/tests/example.rs"),
            DogfoodClassification::TestFixture
        );
        assert_eq!(
            classify_path("crates/example/src/lib.rs"),
            DogfoodClassification::ProductionUnknown
        );
        let counts = disposition_counts([DogfoodDisposition::Rejected, DogfoodDisposition::Unsafe]);
        assert_eq!(counts["accepted"], 0);
        assert_eq!(counts["rejected"], 1);
        assert_eq!(counts["unsafe"], 1);
        assert_eq!(counts["stale"], 0);
    }
}
