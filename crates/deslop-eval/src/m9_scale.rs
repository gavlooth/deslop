//! Convergent cold/full versus warm/incremental scale benchmark for M9.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use deslop_analyzer::{AnalyzerConfig, scan_analysis_regions_with_cache, scan_analysis_with_cache};
use deslop_parse::{
    PersistentArtifactCache, ProjectAnalysis, ProjectInvalidationPlan, ProjectSnapshot,
    ProjectSnapshotBuilder, ProjectionDependencyIndex, RepositoryId, ScopeSpec,
};
use serde::{Deserialize, Serialize};

pub const M9_SCALE_BENCHMARK_SCHEMA: &str = "deslop.m9-scale-benchmark/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M9ScaleBenchmarkConfig {
    pub iterations: usize,
    pub files_per_project: usize,
}

impl M9ScaleBenchmarkConfig {
    pub fn validate(&self) -> Result<()> {
        if self.iterations < 2 {
            bail!("M9 benchmark requires at least two iterations");
        }
        if self.files_per_project < 6 {
            bail!("M9 benchmark requires at least six files per project");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkEnvironment {
    pub os: String,
    pub architecture: String,
    pub logical_workers: usize,
    pub rustc: String,
    pub build_profile: String,
    pub filesystem_root: String,
    pub cache_state: String,
    pub peak_rss_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectScaleMeasurement {
    pub project: String,
    pub language: String,
    pub files: usize,
    pub source_bytes: usize,
    pub nodes: usize,
    pub cold_full_micros: Vec<u64>,
    pub warm_incremental_micros: Vec<u64>,
    pub successor_micros: Vec<u64>,
    pub changed_region_micros: Vec<u64>,
    pub cold_full_p95_micros: u64,
    pub warm_incremental_p95_micros: u64,
    pub incremental_to_full_ratio: f64,
    pub cold_parse_count: usize,
    pub incremental_parse_count_max: usize,
    pub incremental_reused_files_min: usize,
    pub repeated_cache_hits: usize,
    pub repeated_cache_misses: usize,
    pub repeated_cache_hit_rate: f64,
    pub incremental_candidate_artifacts_reused_min: usize,
    pub candidate_cache_misses_max: usize,
    pub retained_memory_bytes_lower_bound: usize,
    pub invalidation_fan_out_max: usize,
    pub full_projection_files: usize,
    pub incremental_projection_files_max: usize,
    pub clean_incremental_digest_equal: bool,
    pub analyzer_output_digest_equal: bool,
    pub throughput_bytes_per_second: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M9ScaleBenchmarkReport {
    pub schema: String,
    pub config: M9ScaleBenchmarkConfig,
    pub environment: BenchmarkEnvironment,
    pub projects: Vec<ProjectScaleMeasurement>,
    pub all_deterministic: bool,
    pub all_incremental_parse_advantage: bool,
    pub all_bounded_fan_out: bool,
    pub all_measured_latency_advantage: bool,
}

impl M9ScaleBenchmarkReport {
    pub fn validate_structural(&self) -> Result<()> {
        if self.schema != M9_SCALE_BENCHMARK_SCHEMA {
            bail!("unsupported M9 benchmark schema {}", self.schema);
        }
        self.config.validate()?;
        if self.projects.len() < 3 {
            bail!("M9 benchmark requires at least three representative projects");
        }
        if !self.all_deterministic
            || !self.all_incremental_parse_advantage
            || !self.all_bounded_fan_out
        {
            bail!("M9 benchmark structural gates did not all pass");
        }
        for project in &self.projects {
            if project.files != self.config.files_per_project
                || project.cold_parse_count != project.files
                || project.incremental_parse_count_max >= project.cold_parse_count
                || project.incremental_projection_files_max >= project.full_projection_files
                || project.repeated_cache_hits != project.files
                || project.repeated_cache_misses != 0
                || project.incremental_candidate_artifacts_reused_min >= project.files
                || project.invalidation_fan_out_max >= project.files
                || !project.clean_incremental_digest_equal
                || !project.analyzer_output_digest_equal
            {
                bail!(
                    "project {} failed its M9 terminal measurement",
                    project.project
                );
            }
        }
        Ok(())
    }

    pub fn validate_terminal(&self) -> Result<()> {
        self.validate_structural()?;
        if self.environment.build_profile != "release" {
            bail!("M9 performance evidence must use an optimized release build");
        }
        if !self.all_measured_latency_advantage {
            bail!("M9 benchmark did not meet the frozen incremental latency floor");
        }
        Ok(())
    }
}

pub fn run_m9_scale_benchmark(
    root: &Path,
    cache_root: &Path,
    config: M9ScaleBenchmarkConfig,
) -> Result<M9ScaleBenchmarkReport> {
    config.validate()?;
    if cache_root.exists() && fs::read_dir(cache_root)?.next().is_some() {
        bail!("M9 benchmark cache directory must be empty to prove cold misses");
    }
    fs::create_dir_all(cache_root)?;
    let mut projects = Vec::new();
    for spec in [
        ProjectSpec::rust(config.files_per_project),
        ProjectSpec::python(config.files_per_project),
        ProjectSpec::typescript(config.files_per_project),
    ] {
        projects.push(measure_project(root, cache_root, &config, spec)?);
    }
    let all_deterministic = projects.iter().all(|project| {
        project.clean_incremental_digest_equal && project.analyzer_output_digest_equal
    });
    let all_incremental_parse_advantage = projects.iter().all(|project| {
        project.incremental_parse_count_max < project.cold_parse_count
            && project.incremental_projection_files_max < project.full_projection_files
            && project.candidate_cache_misses_max < project.files
    });
    let all_bounded_fan_out = projects
        .iter()
        .all(|project| project.invalidation_fan_out_max < project.files);
    let all_measured_latency_advantage = projects.iter().all(|project| {
        project.warm_incremental_p95_micros <= 500_000 && project.incremental_to_full_ratio <= 0.05
    });
    let report = M9ScaleBenchmarkReport {
        schema: M9_SCALE_BENCHMARK_SCHEMA.into(),
        config,
        environment: benchmark_environment(root, cache_root),
        projects,
        all_deterministic,
        all_incremental_parse_advantage,
        all_bounded_fan_out,
        all_measured_latency_advantage,
    };
    Ok(report)
}

struct ProjectSpec {
    name: &'static str,
    language: &'static str,
    paths: Vec<PathBuf>,
    base_sources: Vec<Vec<u8>>,
    changed_source: fn(usize) -> Vec<u8>,
}

impl ProjectSpec {
    fn rust(files: usize) -> Self {
        Self::new(
            "fixture-rust-library",
            "rust",
            "rs",
            files,
            |index, revision| {
                format!(
                "pub fn value_{index:03}(input: i32) -> i32 {{ if input > {revision} {{ input + {index} }} else {{ {revision} }} }}\n"
            )
            .into_bytes()
            },
        )
    }

    fn python(files: usize) -> Self {
        Self::new(
            "fixture-python-package",
            "python",
            "py",
            files,
            |index, revision| {
                format!(
                "def value_{index:03}(input_value):\n    if input_value > {revision}:\n        return input_value + {index}\n    return {revision}\n"
            )
            .into_bytes()
            },
        )
    }

    fn typescript(files: usize) -> Self {
        Self::new(
            "fixture-typescript-app",
            "typescript",
            "ts",
            files,
            |index, revision| {
                format!(
                "export function value_{index:03}(input: number): number {{ return input > {revision} ? input + {index} : {revision}; }}\n"
            )
            .into_bytes()
            },
        )
    }

    fn new(
        name: &'static str,
        language: &'static str,
        extension: &'static str,
        files: usize,
        source: fn(usize, usize) -> Vec<u8>,
    ) -> Self {
        let paths = (0..files)
            .map(|index| PathBuf::from(format!("src/file_{index:03}.{extension}")))
            .collect::<Vec<_>>();
        let base_sources = (0..files).map(|index| source(index, 0)).collect();
        let changed_source = match language {
            "rust" => rust_changed as fn(usize) -> Vec<u8>,
            "python" => python_changed as fn(usize) -> Vec<u8>,
            "typescript" => typescript_changed as fn(usize) -> Vec<u8>,
            _ => unreachable!(),
        };
        Self {
            name,
            language,
            paths,
            base_sources,
            changed_source,
        }
    }
}

fn rust_changed(revision: usize) -> Vec<u8> {
    format!(
        "pub fn value_000(input: i32) -> i32 {{ if input > {revision} {{ input }} else {{ {revision} }} }}\n"
    )
    .into_bytes()
}

fn python_changed(revision: usize) -> Vec<u8> {
    format!(
        "def value_000(input_value):\n    if input_value > {revision}:\n        return input_value\n    return {revision}\n"
    )
    .into_bytes()
}

fn typescript_changed(revision: usize) -> Vec<u8> {
    format!(
        "export function value_000(input: number): number {{ return input > {revision} ? input : {revision}; }}\n"
    )
    .into_bytes()
}

fn measure_project(
    root: &Path,
    cache_root: &Path,
    config: &M9ScaleBenchmarkConfig,
    spec: ProjectSpec,
) -> Result<ProjectScaleMeasurement> {
    let repository = RepositoryId::explicit(format!("m9-benchmark:{}", spec.name))?;
    let mut analyzer_config = AnalyzerConfig::default();
    analyzer_config.boundary.enabled = false;
    analyzer_config.min_duplication_tokens = 10_000;
    let base = build_snapshot(root, repository.clone(), &spec, 0)?;
    let source_bytes = base.entries().map(|entry| entry.bytes().len()).sum();

    let mut cold_full_micros = Vec::new();
    let mut cold_parse_count = 0;
    let mut nodes = 0;
    for iteration in 0..config.iterations {
        let cold_cache = PersistentArtifactCache::open(
            cache_root.join(format!("cold-{}-{iteration}", spec.name)),
        )?;
        let started = Instant::now();
        let analysis = ProjectAnalysis::build(base.clone())?;
        let projection =
            scan_analysis_with_cache(analysis.clone(), analyzer_config.clone(), cold_cache)?;
        cold_full_micros.push(elapsed_micros(started));
        cold_parse_count = analysis.instrumentation().parse.parser_invocations;
        nodes = analysis.node_count();
        if projection.local_cache_hits != 0 || projection.local_cache_misses != spec.paths.len() {
            bail!("cold benchmark unexpectedly reused candidate artifacts");
        }
    }

    let warm_cache = PersistentArtifactCache::open(cache_root.join(format!("warm-{}", spec.name)))?;
    let mut current = ProjectAnalysis::build(base)?;
    let seeded =
        scan_analysis_with_cache(current.clone(), analyzer_config.clone(), warm_cache.clone())?;
    if seeded.local_cache_misses != spec.paths.len() {
        bail!("warm benchmark seed did not populate every file artifact");
    }
    let repeated =
        scan_analysis_with_cache(current.clone(), analyzer_config.clone(), warm_cache.clone())?;
    if repeated.local_cache_hits != spec.paths.len() || repeated.local_cache_misses != 0 {
        bail!("exact-repeat benchmark did not hit every persisted file artifact");
    }
    let repeated_cache_hits = repeated.local_cache_hits;
    let repeated_cache_misses = repeated.local_cache_misses;
    let repeated_cache_hit_rate = ratio(repeated_cache_hits, spec.paths.len());
    let mut retained_reports = repeated.reports;
    let mut warm_incremental_micros = Vec::new();
    let mut successor_micros = Vec::new();
    let mut changed_region_micros = Vec::new();
    let mut incremental_parse_count_max = 0;
    let mut incremental_reused_files_min = usize::MAX;
    let mut incremental_candidate_artifacts_reused_min = usize::MAX;
    let mut candidate_cache_misses_max = 0;
    let mut invalidation_fan_out_max = 0;
    let mut incremental_projection_files_max = 0;
    let mut retained_memory_bytes_lower_bound = 0;
    let mut clean_incremental_digest_equal = true;
    let mut analyzer_output_digest_equal = true;

    for iteration in 1..=config.iterations {
        let next_snapshot = build_snapshot(root, repository.clone(), &spec, iteration)?;
        let started = Instant::now();
        let update = current.successor(next_snapshot.clone())?;
        successor_micros.push(elapsed_micros(started));
        let update_instrumentation = update.instrumentation();
        let mut dependencies = ProjectionDependencyIndex::new(spec.paths.clone());
        for path in &spec.paths {
            dependencies.record_dependencies(path.clone(), Vec::<PathBuf>::new());
        }
        let invalidation = ProjectInvalidationPlan::derive(&update, &dependencies);
        let incremental = update.into_current();
        let region_started = Instant::now();
        let projection = scan_analysis_regions_with_cache(
            incremental.clone(),
            vec![spec.paths[0].clone()],
            analyzer_config.clone(),
            warm_cache.clone(),
        )?;
        changed_region_micros.push(elapsed_micros(region_started));
        if projection.project_semantics_complete || projection.reports.len() != 1 {
            bail!("bounded incremental projection did not expose its partial project state");
        }
        warm_incremental_micros.push(elapsed_micros(started));

        incremental_parse_count_max =
            incremental_parse_count_max.max(incremental.instrumentation().parse.parser_invocations);
        incremental_reused_files_min =
            incremental_reused_files_min.min(update_instrumentation.reused_files);
        incremental_candidate_artifacts_reused_min =
            incremental_candidate_artifacts_reused_min.min(update_instrumentation.reused_files);
        candidate_cache_misses_max = candidate_cache_misses_max.max(projection.local_cache_misses);
        invalidation_fan_out_max = invalidation_fan_out_max.max(invalidation.fan_out());
        incremental_projection_files_max =
            incremental_projection_files_max.max(projection.reports.len());
        retained_memory_bytes_lower_bound = retained_memory_bytes_lower_bound
            .max(incremental.instrumentation().memory.known_bytes_lower_bound);

        let clean = ProjectAnalysis::build(next_snapshot)?;
        clean_incremental_digest_equal &= clean.instrumentation().node_order_digest
            == incremental.instrumentation().node_order_digest;
        let clean_projection =
            scan_analysis_with_cache(clean, analyzer_config.clone(), warm_cache.clone())?;
        let changed_report = projection
            .reports
            .into_iter()
            .next()
            .expect("bounded projection has one report");
        let retained = retained_reports
            .iter_mut()
            .find(|report| report.path == changed_report.path)
            .expect("changed report path belongs to retained full projection");
        *retained = changed_report;
        analyzer_output_digest_equal &=
            report_digest(&clean_projection.reports)? == report_digest(&retained_reports)?;
        current = incremental;
    }

    let cold_full_p95_micros = percentile_95(&cold_full_micros);
    let warm_incremental_p95_micros = percentile_95(&warm_incremental_micros);
    let incremental_to_full_ratio = ratio(
        warm_incremental_p95_micros as usize,
        cold_full_p95_micros as usize,
    );
    let throughput_bytes_per_second = if cold_full_p95_micros == 0 {
        0.0
    } else {
        source_bytes as f64 / (cold_full_p95_micros as f64 / 1_000_000.0)
    };
    Ok(ProjectScaleMeasurement {
        project: spec.name.into(),
        language: spec.language.into(),
        files: spec.paths.len(),
        source_bytes,
        nodes,
        cold_full_micros,
        warm_incremental_micros,
        successor_micros,
        changed_region_micros,
        cold_full_p95_micros,
        warm_incremental_p95_micros,
        incremental_to_full_ratio,
        cold_parse_count,
        incremental_parse_count_max,
        incremental_reused_files_min,
        repeated_cache_hits,
        repeated_cache_misses,
        repeated_cache_hit_rate,
        incremental_candidate_artifacts_reused_min,
        candidate_cache_misses_max,
        retained_memory_bytes_lower_bound,
        invalidation_fan_out_max,
        full_projection_files: spec.paths.len(),
        incremental_projection_files_max,
        clean_incremental_digest_equal,
        analyzer_output_digest_equal,
        throughput_bytes_per_second,
    })
}

fn build_snapshot(
    root: &Path,
    repository: RepositoryId,
    spec: &ProjectSpec,
    revision: usize,
) -> Result<std::sync::Arc<ProjectSnapshot>> {
    let mut builder = ProjectSnapshotBuilder::new(root, repository)?
        .with_scope_spec(ScopeSpec::ExactLogicalFiles(spec.paths.clone()));
    for (index, (path, base)) in spec.paths.iter().zip(&spec.base_sources).enumerate() {
        let source = if index == 0 && revision > 0 {
            (spec.changed_source)(revision)
        } else {
            base.clone()
        };
        builder = builder.with_overlay(path, source)?;
    }
    builder.build()
}

fn report_digest(reports: &[deslop_core::FileReport]) -> Result<String> {
    let bytes = serde_json::to_vec(reports)?;
    Ok(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
}

fn elapsed_micros(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}

fn percentile_95(samples: &[u64]) -> u64 {
    let mut samples = samples.to_vec();
    samples.sort_unstable();
    let index = (samples.len() * 95).div_ceil(100).saturating_sub(1);
    samples[index]
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn benchmark_environment(root: &Path, cache_root: &Path) -> BenchmarkEnvironment {
    BenchmarkEnvironment {
        os: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        logical_workers: std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1),
        rustc: rustc_version(),
        build_profile: if cfg!(debug_assertions) {
            "debug".into()
        } else {
            "release".into()
        },
        filesystem_root: root.display().to_string(),
        cache_state: format!(
            "cold-empty; warm-content-addressed; cache_root={}",
            cache_root.display()
        ),
        peak_rss_bytes: peak_rss_bytes(),
    }
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unavailable".into())
}

fn peak_rss_bytes() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let kib = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    kib.checked_mul(1024)
}

pub fn write_m9_scale_report(path: &Path, report: &M9ScaleBenchmarkReport) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(report)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create M9 report directory {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("write M9 report {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_matrix_proves_deterministic_bounded_incremental_advantage() {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let cache_root = cache.path().join("empty");
        let report = run_m9_scale_benchmark(
            root.path(),
            &cache_root,
            M9ScaleBenchmarkConfig {
                iterations: 2,
                files_per_project: 6,
            },
        )
        .unwrap();
        report.validate_structural().unwrap();
        assert!(report.projects.iter().all(|project| {
            project.incremental_parse_count_max == 1
                && project.repeated_cache_hits == 6
                && project.repeated_cache_misses == 0
                && project.incremental_candidate_artifacts_reused_min == 5
                && project.candidate_cache_misses_max == 1
                && project.invalidation_fan_out_max == 1
        }));
    }
}
