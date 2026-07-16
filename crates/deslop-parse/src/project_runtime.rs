//! Deterministic parallel region computation and resumable analysis budgets (M9.4-M9.5).

use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const DETERMINISTIC_COMMIT_SCHEMA: &str = "deslop.deterministic-graph-commit/1";
pub const ANALYSIS_BUDGET_SCHEMA: &str = "deslop.analysis-budget/1";
const COMMIT_DOMAIN: &str = "deslop deterministic graph commit v1";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisWorkCost {
    pub files: u64,
    pub nodes: u64,
    pub input_bytes: u64,
    pub results: u64,
    pub evidence_bytes: u64,
}

impl AnalysisWorkCost {
    fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            files: self.files.checked_add(other.files)?,
            nodes: self.nodes.checked_add(other.nodes)?,
            input_bytes: self.input_bytes.checked_add(other.input_bytes)?,
            results: self.results.checked_add(other.results)?,
            evidence_bytes: self.evidence_bytes.checked_add(other.evidence_bytes)?,
        })
    }

    fn fits_within(self, budget: &AnalysisBudget) -> bool {
        self.files <= budget.max_files
            && self.nodes <= budget.max_nodes
            && self.input_bytes <= budget.max_input_bytes
            && self.results <= budget.max_results
            && self.evidence_bytes <= budget.max_evidence_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisBudget {
    schema: String,
    pub max_files: u64,
    pub max_nodes: u64,
    pub max_input_bytes: u64,
    pub max_results: u64,
    pub max_evidence_bytes: u64,
    pub max_elapsed_millis: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalysisBudgetWire {
    schema: String,
    max_files: u64,
    max_nodes: u64,
    max_input_bytes: u64,
    max_results: u64,
    max_evidence_bytes: u64,
    max_elapsed_millis: u64,
}

impl<'de> Deserialize<'de> for AnalysisBudget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = AnalysisBudgetWire::deserialize(deserializer)?;
        let value = Self {
            schema: wire.schema,
            max_files: wire.max_files,
            max_nodes: wire.max_nodes,
            max_input_bytes: wire.max_input_bytes,
            max_results: wire.max_results,
            max_evidence_bytes: wire.max_evidence_bytes,
            max_elapsed_millis: wire.max_elapsed_millis,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl AnalysisBudget {
    pub fn new(
        max_files: u64,
        max_nodes: u64,
        max_input_bytes: u64,
        max_results: u64,
        max_evidence_bytes: u64,
        max_elapsed_millis: u64,
    ) -> Result<Self, AnalysisBudgetError> {
        let value = Self {
            schema: ANALYSIS_BUDGET_SCHEMA.into(),
            max_files,
            max_nodes,
            max_input_bytes,
            max_results,
            max_evidence_bytes,
            max_elapsed_millis,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn unbounded() -> Self {
        Self {
            schema: ANALYSIS_BUDGET_SCHEMA.into(),
            max_files: u64::MAX,
            max_nodes: u64::MAX,
            max_input_bytes: u64::MAX,
            max_results: u64::MAX,
            max_evidence_bytes: u64::MAX,
            max_elapsed_millis: u64::MAX,
        }
    }

    fn validate(&self) -> Result<(), AnalysisBudgetError> {
        if self.schema != ANALYSIS_BUDGET_SCHEMA {
            return Err(AnalysisBudgetError::Invalid(format!(
                "unsupported analysis budget schema {}",
                self.schema
            )));
        }
        if [
            self.max_files,
            self.max_nodes,
            self.max_input_bytes,
            self.max_results,
            self.max_evidence_bytes,
            self.max_elapsed_millis,
        ]
        .contains(&0)
        {
            return Err(AnalysisBudgetError::Invalid(
                "analysis budget limits must be positive".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionWorkItem<T> {
    key: String,
    cost: AnalysisWorkCost,
    input: T,
}

impl<T> RegionWorkItem<T> {
    pub fn new(
        key: impl Into<String>,
        cost: AnalysisWorkCost,
        input: T,
    ) -> Result<Self, AnalysisBudgetError> {
        let key = key.into();
        if key.trim().is_empty() {
            return Err(AnalysisBudgetError::Invalid(
                "region work key must not be empty".into(),
            ));
        }
        Ok(Self { key, cost, input })
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn cost(&self) -> AnalysisWorkCost {
        self.cost
    }

    pub fn input(&self) -> &T {
        &self.input
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeterministicCommitEntry {
    key: String,
    artifact_digest: String,
    artifact: Vec<u8>,
}

impl DeterministicCommitEntry {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn artifact_digest(&self) -> &str {
        &self.artifact_digest
    }

    pub fn artifact(&self) -> &[u8] {
        &self.artifact
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeterministicCommitBatch {
    schema: String,
    id: String,
    entries: Vec<DeterministicCommitEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeterministicCommitBatchWire {
    schema: String,
    id: String,
    entries: Vec<DeterministicCommitEntry>,
}

impl<'de> Deserialize<'de> for DeterministicCommitBatch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = DeterministicCommitBatchWire::deserialize(deserializer)?;
        build_commit_batch(wire.entries)
            .and_then(|rebuilt| {
                if wire.schema != rebuilt.schema || wire.id != rebuilt.id {
                    return Err(RegionExecutionError::Invalid(
                        "deterministic commit identity does not match its entries".into(),
                    ));
                }
                Ok(rebuilt)
            })
            .map_err(serde::de::Error::custom)
    }
}

impl DeterministicCommitBatch {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn entries(&self) -> &[DeterministicCommitEntry] {
        &self.entries
    }

    pub fn from_artifacts(
        mut artifacts: Vec<(String, Vec<u8>)>,
    ) -> Result<Self, RegionExecutionError> {
        artifacts.sort_by(|left, right| left.0.cmp(&right.0));
        let entries = artifacts
            .into_iter()
            .map(|(key, artifact)| DeterministicCommitEntry {
                artifact_digest: format!("blake3:{}", blake3::hash(&artifact).to_hex()),
                key,
                artifact,
            })
            .collect();
        build_commit_batch(entries)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeterministicRegionExecutor {
    workers: usize,
}

impl DeterministicRegionExecutor {
    pub fn new(workers: usize) -> Result<Self, RegionExecutionError> {
        if workers == 0 {
            return Err(RegionExecutionError::Invalid(
                "worker count must be positive".into(),
            ));
        }
        Ok(Self { workers })
    }

    pub fn workers(&self) -> usize {
        self.workers
    }

    /// Compute regions independently in parallel, then serialize graph commits
    /// in canonical key order. Worker completion order is never observable.
    pub fn execute<T, F>(
        &self,
        mut items: Vec<RegionWorkItem<T>>,
        analyze: F,
    ) -> Result<DeterministicCommitBatch, RegionExecutionError>
    where
        T: Send + Sync,
        F: Fn(&RegionWorkItem<T>) -> Result<Vec<u8>, String> + Sync,
    {
        items.sort_by(|left, right| left.key.cmp(&right.key));
        if items
            .windows(2)
            .any(|window| window[0].key == window[1].key)
        {
            return Err(RegionExecutionError::Invalid(
                "region work keys must be unique".into(),
            ));
        }
        let results = Mutex::new(
            std::iter::repeat_with(|| None)
                .take(items.len())
                .collect::<Vec<Option<Result<Vec<u8>, String>>>>(),
        );
        let next = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..self.workers.min(items.len().max(1)) {
                let items = &items;
                let analyze = &analyze;
                let results = &results;
                let next = &next;
                scope.spawn(move || {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else {
                            break;
                        };
                        let result = analyze(item);
                        results.lock().unwrap_or_else(|poison| poison.into_inner())[index] =
                            Some(result);
                    }
                });
            }
        });

        let mut locked = results
            .into_inner()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut entries = Vec::with_capacity(items.len());
        for (item, result) in items.iter().zip(locked.iter_mut()) {
            let artifact = result
                .take()
                .expect("every scheduled region stores one result")
                .map_err(|message| RegionExecutionError::Worker {
                    key: item.key.clone(),
                    message,
                })?;
            entries.push(DeterministicCommitEntry {
                key: item.key.clone(),
                artifact_digest: format!("blake3:{}", blake3::hash(&artifact).to_hex()),
                artifact,
            });
        }
        build_commit_batch(entries)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisContinuation {
    after_key: Option<String>,
}

impl AnalysisContinuation {
    pub fn start() -> Self {
        Self { after_key: None }
    }

    pub fn after_key(&self) -> Option<&str> {
        self.after_key.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetExhaustionReason {
    ResourceLimit,
    ElapsedTime,
    ItemExceedsBudget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum BudgetStatus {
    Complete,
    Partial {
        continuation: AnalysisContinuation,
        reason: BudgetExhaustionReason,
    },
    Pending {
        continuation: AnalysisContinuation,
        reason: BudgetExhaustionReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetedAnalysis {
    status: BudgetStatus,
    consumed: AnalysisWorkCost,
    commit: DeterministicCommitBatch,
}

impl BudgetedAnalysis {
    pub fn status(&self) -> &BudgetStatus {
        &self.status
    }

    pub fn consumed(&self) -> AnalysisWorkCost {
        self.consumed
    }

    pub fn commit(&self) -> &DeterministicCommitBatch {
        &self.commit
    }
}

impl DeterministicRegionExecutor {
    pub fn execute_budgeted<T, F>(
        &self,
        mut items: Vec<RegionWorkItem<T>>,
        continuation: &AnalysisContinuation,
        budget: &AnalysisBudget,
        analyze: F,
    ) -> Result<BudgetedAnalysis, RegionExecutionError>
    where
        T: Send + Sync,
        F: Fn(&RegionWorkItem<T>) -> Result<Vec<u8>, String> + Sync,
    {
        budget
            .validate()
            .map_err(|error| RegionExecutionError::Invalid(error.to_string()))?;
        items.sort_by(|left, right| left.key.cmp(&right.key));
        if items
            .windows(2)
            .any(|window| window[0].key == window[1].key)
        {
            return Err(RegionExecutionError::Invalid(
                "region work keys must be unique".into(),
            ));
        }
        let remaining = items
            .into_iter()
            .filter(|item| {
                continuation
                    .after_key()
                    .is_none_or(|after| item.key.as_str() > after)
            })
            .collect::<Vec<_>>();
        let started = Instant::now();
        let mut consumed = AnalysisWorkCost::default();
        let mut selected = Vec::new();
        let mut reason = None;
        for item in remaining {
            if started.elapsed() >= Duration::from_millis(budget.max_elapsed_millis) {
                reason = Some(BudgetExhaustionReason::ElapsedTime);
                break;
            }
            let Some(next) = consumed.checked_add(item.cost) else {
                reason = Some(BudgetExhaustionReason::ResourceLimit);
                break;
            };
            if !next.fits_within(budget) {
                reason = Some(if selected.is_empty() && !item.cost.fits_within(budget) {
                    BudgetExhaustionReason::ItemExceedsBudget
                } else {
                    BudgetExhaustionReason::ResourceLimit
                });
                break;
            }
            consumed = next;
            selected.push(item);
        }

        let last_key = selected
            .last()
            .map(|item| item.key.clone())
            .or_else(|| continuation.after_key.clone());
        let had_work = !selected.is_empty();
        let commit = self.execute(selected, analyze)?;
        let status = match reason {
            None => BudgetStatus::Complete,
            Some(reason) if had_work => BudgetStatus::Partial {
                continuation: AnalysisContinuation {
                    after_key: last_key,
                },
                reason,
            },
            Some(reason) => BudgetStatus::Pending {
                continuation: continuation.clone(),
                reason,
            },
        };
        Ok(BudgetedAnalysis {
            status,
            consumed,
            commit,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisBudgetError {
    Invalid(String),
}

impl fmt::Display for AnalysisBudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "invalid analysis budget: {message}"),
        }
    }
}

impl std::error::Error for AnalysisBudgetError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionExecutionError {
    Invalid(String),
    Worker { key: String, message: String },
    Serialization(String),
}

impl fmt::Display for RegionExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "invalid region execution: {message}"),
            Self::Worker { key, message } => write!(formatter, "region {key} failed: {message}"),
            Self::Serialization(message) => {
                write!(formatter, "region commit serialization failed: {message}")
            }
        }
    }
}

impl std::error::Error for RegionExecutionError {}

fn build_commit_batch(
    entries: Vec<DeterministicCommitEntry>,
) -> Result<DeterministicCommitBatch, RegionExecutionError> {
    if entries
        .windows(2)
        .any(|window| window[0].key >= window[1].key)
    {
        return Err(RegionExecutionError::Invalid(
            "deterministic commit entries must be sorted and unique".into(),
        ));
    }
    for entry in &entries {
        let expected = format!("blake3:{}", blake3::hash(&entry.artifact).to_hex());
        if entry.key.trim().is_empty() || entry.artifact_digest != expected {
            return Err(RegionExecutionError::Invalid(
                "deterministic commit entry digest does not match its artifact".into(),
            ));
        }
    }
    let bytes = serde_json::to_vec(&entries)
        .map_err(|error| RegionExecutionError::Serialization(error.to_string()))?;
    let digest = blake3::derive_key(COMMIT_DOMAIN, &bytes);
    Ok(DeterministicCommitBatch {
        schema: DETERMINISTIC_COMMIT_SCHEMA.into(),
        id: format!("dgc1_{}", blake3::Hash::from_bytes(digest).to_hex()),
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: &str, nodes: u64) -> RegionWorkItem<String> {
        RegionWorkItem::new(
            key,
            AnalysisWorkCost {
                files: 1,
                nodes,
                input_bytes: nodes * 10,
                results: 1,
                evidence_bytes: nodes,
            },
            key.to_uppercase(),
        )
        .unwrap()
    }

    #[test]
    fn worker_counts_produce_byte_identical_sorted_graph_commits() {
        let work = || vec![item("c", 1), item("a", 1), item("b", 1)];
        let one = DeterministicRegionExecutor::new(1)
            .unwrap()
            .execute(work(), |item| Ok(item.input().as_bytes().to_vec()))
            .unwrap();
        let four = DeterministicRegionExecutor::new(4)
            .unwrap()
            .execute(work(), |item| {
                if item.key() == "a" {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Ok(item.input().as_bytes().to_vec())
            })
            .unwrap();

        assert_eq!(one, four);
        assert_eq!(
            one.entries()
                .iter()
                .map(|entry| entry.key())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn budgets_report_partial_then_resume_without_duplicate_commits() {
        let executor = DeterministicRegionExecutor::new(2).unwrap();
        let budget = AnalysisBudget::new(2, 10, 100, 2, 10, 10_000).unwrap();
        let analyze = |item: &RegionWorkItem<String>| Ok(item.input().as_bytes().to_vec());
        let first = executor
            .execute_budgeted(
                vec![item("c", 1), item("a", 1), item("b", 1)],
                &AnalysisContinuation::start(),
                &budget,
                analyze,
            )
            .unwrap();
        let BudgetStatus::Partial { continuation, .. } = first.status() else {
            panic!("bounded first page must be partial");
        };
        assert_eq!(continuation.after_key(), Some("b"));
        assert_eq!(first.commit().entries().len(), 2);

        let second = executor
            .execute_budgeted(
                vec![item("c", 1), item("a", 1), item("b", 1)],
                continuation,
                &budget,
                analyze,
            )
            .unwrap();
        assert_eq!(second.status(), &BudgetStatus::Complete);
        assert_eq!(second.commit().entries()[0].key(), "c");
    }

    #[test]
    fn oversized_first_item_is_pending_not_empty_complete() {
        let executor = DeterministicRegionExecutor::new(1).unwrap();
        let budget = AnalysisBudget::new(1, 2, 20, 1, 2, 10_000).unwrap();
        let result = executor
            .execute_budgeted(
                vec![item("large", 3)],
                &AnalysisContinuation::start(),
                &budget,
                |_| Ok(Vec::new()),
            )
            .unwrap();
        assert!(matches!(
            result.status(),
            BudgetStatus::Pending {
                reason: BudgetExhaustionReason::ItemExceedsBudget,
                ..
            }
        ));
        assert!(result.commit().entries().is_empty());
    }

    #[test]
    fn invalid_wire_budget_fails_closed() {
        let value = serde_json::json!({
            "schema": ANALYSIS_BUDGET_SCHEMA,
            "max_files": 0,
            "max_nodes": 1,
            "max_input_bytes": 1,
            "max_results": 1,
            "max_evidence_bytes": 1,
            "max_elapsed_millis": 1
        });
        assert!(serde_json::from_value::<AnalysisBudget>(value).is_err());
    }

    #[test]
    fn persisted_commit_revalidates_entry_and_batch_digests() {
        let batch = DeterministicRegionExecutor::new(1)
            .unwrap()
            .execute(vec![item("a", 1)], |item| {
                Ok(item.input().as_bytes().to_vec())
            })
            .unwrap();
        let mut value = serde_json::to_value(batch).unwrap();
        value["entries"][0]["artifact"][0] = serde_json::json!(0);
        assert!(serde_json::from_value::<DeterministicCommitBatch>(value).is_err());
    }
}
