//! Strict terminal M10 release-evidence join.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const M10_RELEASE_EVIDENCE_SCHEMA: &str = "deslop.release-evidence/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseGateStatus {
    Pass,
    Downgraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseGateDecision {
    pub id: String,
    pub status: ReleaseGateStatus,
    pub evidence: Vec<String>,
    pub limitation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseArtifactEvidence {
    pub path: String,
    pub schema: String,
    pub digest: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseException {
    pub id: String,
    pub status: String,
    pub scope: String,
    pub evidence: String,
    pub release_effect: String,
    pub recheck_condition: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShippedCapability {
    pub surface: String,
    pub tier: String,
    pub disposition: String,
    pub authority: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseSummary {
    pub canonical_programs: usize,
    pub canonical_languages: usize,
    pub external_projects: usize,
    pub external_source_files: usize,
    pub external_source_lines: usize,
    pub external_test_passes: usize,
    pub dogfood_files: usize,
    pub dogfood_lines: usize,
    pub dogfood_findings: usize,
    pub dogfood_production_safe_auto: usize,
    pub dogfood_recipe_candidates: usize,
    pub dogfood_recipe_abstentions: usize,
    pub llm_paired_tasks: usize,
    pub llm_accepted_patch_delta: f64,
    pub llm_ci95_lower: f64,
    pub readability_pairs: usize,
    pub readability_disposition: String,
    pub scale_projects: usize,
    pub scale_all_gates_pass: bool,
    pub recipe_opportunities: usize,
    pub recipe_hard_negatives: usize,
    pub recipe_thresholds_pass: bool,
    pub clean_checkout_gate_pass: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseEvidence {
    pub schema: String,
    pub release_id: String,
    pub product_version: String,
    pub release_disposition: String,
    pub versions: BTreeMap<String, String>,
    pub artifacts: Vec<ReleaseArtifactEvidence>,
    pub documentation: Vec<String>,
    pub capabilities: Vec<ShippedCapability>,
    pub exceptions: Vec<ReleaseException>,
    pub gates: Vec<ReleaseGateDecision>,
    pub summary: ReleaseSummary,
}

impl ReleaseEvidence {
    pub fn validate(&self) -> Result<()> {
        if self.schema != M10_RELEASE_EVIDENCE_SCHEMA
            || self.product_version != "0.1.0-evidence.1"
            || self.release_disposition != "stable-evidence-limited"
            || self.versions != frozen_versions()
        {
            bail!("M10 release identity/version disposition is not frozen");
        }
        if self.artifacts.len() != artifact_specs().len()
            || self
                .artifacts
                .windows(2)
                .any(|pair| pair[0].path >= pair[1].path)
            || self.artifacts.iter().any(|artifact| {
                artifact.path.is_empty()
                    || artifact.schema.is_empty()
                    || !artifact.digest.starts_with("blake3:")
                    || artifact.bytes == 0
            })
        {
            bail!("release artifact ledger is incomplete or noncanonical");
        }
        let expected_docs = documentation_paths();
        if self.documentation != expected_docs {
            bail!("release documentation ledger is incomplete");
        }
        let expected_gates = expected_gate_ids();
        let actual_gates = self
            .gates
            .iter()
            .map(|gate| gate.id.as_str())
            .collect::<BTreeSet<_>>();
        if actual_gates != expected_gates
            || self.gates.len() != expected_gates.len()
            || self.gates.windows(2).any(|pair| pair[0].id >= pair[1].id)
            || self.gates.iter().any(|gate| {
                gate.evidence.is_empty()
                    || (gate.status == ReleaseGateStatus::Downgraded
                        && gate.limitation.as_deref().is_none_or(str::is_empty))
                    || (gate.status == ReleaseGateStatus::Pass && gate.limitation.is_some())
            })
        {
            bail!("release gate ledger is incomplete, duplicated, or dishonest");
        }
        if self.exceptions.len() != 10
            || self
                .exceptions
                .windows(2)
                .any(|pair| pair[0].id >= pair[1].id)
            || self.exceptions.iter().any(|exception| {
                !matches!(
                    exception.status.as_str(),
                    "closed" | "downgraded" | "environment-qualified"
                ) || exception.scope.is_empty()
                    || exception.evidence.is_empty()
                    || exception.release_effect.is_empty()
                    || exception.recheck_condition.is_empty()
            })
        {
            bail!("release exception ledger is not terminal");
        }
        if self.capabilities.len() != 12
            || self
                .capabilities
                .windows(2)
                .any(|pair| pair[0].surface >= pair[1].surface)
            || self.capabilities.iter().any(|capability| {
                capability.surface.is_empty()
                    || capability.tier.is_empty()
                    || capability.disposition.is_empty()
                    || capability.authority.is_empty()
            })
        {
            bail!("release capability matrix is incomplete");
        }
        let summary = &self.summary;
        if summary.canonical_programs < 600
            || summary.canonical_languages != 6
            || summary.external_projects != 18
            || summary.external_source_files == 0
            || summary.external_source_lines == 0
            || summary.dogfood_files == 0
            || summary.dogfood_lines == 0
            || summary.dogfood_findings == 0
            || summary.dogfood_production_safe_auto != 0
            || summary.llm_paired_tasks != 240
            || summary.llm_accepted_patch_delta < 0.10
            || summary.llm_ci95_lower <= 0.0
            || summary.readability_pairs < 300
            || summary.readability_disposition != "evidence_only"
            || summary.scale_projects < 3
            || !summary.scale_all_gates_pass
            || summary.recipe_opportunities < 1_000
            || summary.recipe_hard_negatives < 1_000
            || !summary.recipe_thresholds_pass
            || !summary.clean_checkout_gate_pass
        {
            bail!("release numerical summary contradicts the evidence-limited bar");
        }
        let expected = release_id(self)?;
        if self.release_id != expected {
            bail!("release evidence identity mismatch: expected {expected}");
        }
        Ok(())
    }
}

pub fn assemble_release_evidence(root: &Path) -> Result<ReleaseEvidence> {
    let root = root
        .canonicalize()
        .with_context(|| format!("resolve release root {}", root.display()))?;
    let mut artifacts = Vec::new();
    let mut values = BTreeMap::<String, Value>::new();
    for spec in artifact_specs() {
        let path = root.join(spec.path);
        let bytes =
            fs::read(&path).with_context(|| format!("read release artifact {}", path.display()))?;
        let value: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("decode release artifact {}", path.display()))?;
        let schema = value
            .pointer(spec.schema_pointer)
            .and_then(Value::as_str)
            .with_context(|| format!("artifact {} lacks schema", spec.path))?;
        if schema != spec.schema {
            bail!(
                "artifact {} schema {schema} is not {}",
                spec.path,
                spec.schema
            );
        }
        artifacts.push(ReleaseArtifactEvidence {
            path: spec.path.into(),
            schema: schema.into(),
            digest: digest("deslop m10 release artifact v1", &bytes),
            bytes: bytes.len(),
        });
        values.insert(spec.path.into(), value);
    }
    artifacts.sort_by(|left, right| left.path.cmp(&right.path));
    for documentation in documentation_paths() {
        let path = root.join(&documentation);
        if fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or_default()
            == 0
        {
            bail!(
                "release documentation {} is missing or empty",
                path.display()
            );
        }
    }
    let summary = release_summary(&values)?;
    let mut evidence = ReleaseEvidence {
        schema: M10_RELEASE_EVIDENCE_SCHEMA.into(),
        release_id: String::new(),
        product_version: "0.1.0-evidence.1".into(),
        release_disposition: "stable-evidence-limited".into(),
        versions: frozen_versions(),
        artifacts,
        documentation: documentation_paths(),
        capabilities: shipped_capabilities(),
        exceptions: release_exceptions(),
        gates: release_gate_decisions(),
        summary,
    };
    evidence.release_id = release_id(&evidence)?;
    evidence.validate()?;
    Ok(evidence)
}

pub fn write_release_evidence(path: &Path, evidence: &ReleaseEvidence) -> Result<()> {
    evidence.validate()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(evidence)?)?;
    Ok(())
}

pub fn verify_release_evidence(root: &Path, path: &Path) -> Result<ReleaseEvidence> {
    let stored: ReleaseEvidence = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read release evidence {}", path.display()))?,
    )?;
    stored.validate()?;
    let recomputed = assemble_release_evidence(root)?;
    if stored != recomputed {
        bail!("stored release evidence disagrees with current frozen artifacts/docs");
    }
    Ok(stored)
}

struct ArtifactSpec {
    path: &'static str,
    schema_pointer: &'static str,
    schema: &'static str,
}

fn artifact_specs() -> Vec<ArtifactSpec> {
    vec![
        ArtifactSpec {
            path: ".agents/benchmarks/m10_canonical_corpus_v1.json",
            schema_pointer: "/schema",
            schema: "deslop.m10-canonical-corpus/1",
        },
        ArtifactSpec {
            path: ".agents/benchmarks/m10_dogfood_report_v1.json",
            schema_pointer: "/schema",
            schema: "deslop.m10-dogfood/1",
        },
        ArtifactSpec {
            path: ".agents/benchmarks/m10_external_projects_v1.json",
            schema_pointer: "/schema",
            schema: "deslop.m10-external-projects/1",
        },
        ArtifactSpec {
            path: ".agents/benchmarks/m10_external_report_v1.json",
            schema_pointer: "/schema",
            schema: "deslop.m10-external-report/1",
        },
        ArtifactSpec {
            path: ".agents/benchmarks/m10_gate_report_v1.json",
            schema_pointer: "/schema",
            schema: "deslop.m10-gate-report/1",
        },
        ArtifactSpec {
            path: ".agents/benchmarks/m6_llm_report_v1.json",
            schema_pointer: "/schema",
            schema: "deslop.m6-llm-benchmark-report/1",
        },
        ArtifactSpec {
            path: ".agents/benchmarks/m6_llm_tasks_v1.json",
            schema_pointer: "/schema",
            schema: "deslop.m6-llm-task-manifest/1",
        },
        ArtifactSpec {
            path: ".agents/benchmarks/m9_scale_report_v1.json",
            schema_pointer: "/schema",
            schema: "deslop.m9-scale-benchmark/1",
        },
        ArtifactSpec {
            path: "crates/deslop-eval/evaluation/m8/corpus.json",
            schema_pointer: "/schema",
            schema: "deslop.readability-corpus/1",
        },
        ArtifactSpec {
            path: "crates/deslop-eval/evaluation/m8/dataset_registry.json",
            schema_pointer: "/schema",
            schema: "deslop.readability-datasets/1",
        },
        ArtifactSpec {
            path: "crates/deslop-eval/evaluation/m8/evaluation_report.json",
            schema_pointer: "/report/schema",
            schema: "deslop.readability-evaluation/1",
        },
        ArtifactSpec {
            path: "crates/deslop-recipes/corpus/unreachable_literal_rust_v1.json",
            schema_pointer: "/schema",
            schema: "deslop.recipe-evaluation-corpus/1",
        },
        ArtifactSpec {
            path: "crates/deslop-recipes/evaluation/unreachable_literal_rust_v1_report.json",
            schema_pointer: "/schema",
            schema: "deslop.recipe-evaluation-report/1",
        },
        ArtifactSpec {
            path: "tests/fixtures/m4_graph_gold.json",
            schema_pointer: "/schema",
            schema: "deslop.m4-graph-gold/1",
        },
        ArtifactSpec {
            path: "tests/fixtures/resolution_m3_7_adversarial_gold.json",
            schema_pointer: "/schema",
            schema: "deslop.resolution-adversarial-gold/1",
        },
    ]
}

fn documentation_paths() -> Vec<String> {
    [
        "docs/ADAPTER_AUTHORING.md",
        "docs/AGENT_INTEGRATION.md",
        "docs/M10_CAPABILITY_MATRIX.md",
        "docs/M10_FAILURE_RISK_REGISTER.md",
        "docs/M10_MIGRATION.md",
        "docs/M10_RELEASE_REPORT.md",
        "docs/SECURITY.md",
        "docs/UNDO_RECOVERY.md",
        "docs/VERIFIER_POLICY.md",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn frozen_versions() -> BTreeMap<String, String> {
    [
        (
            "adapter-capabilities",
            "deslop.language-adapter-capabilities/2",
        ),
        (
            "canonical-role-projection",
            "deslop.canonical-role-projection/1",
        ),
        ("graph-wire", "deslop.graph/2"),
        ("readability-features", "deslop.readability-features/1"),
        ("readability-model", "unshipped:evidence-only"),
        ("recipe", "deslop.transformation-recipe/1"),
        (
            "transformation-candidate",
            "deslop.transformation-candidate/1",
        ),
        ("work-order-legacy", "deslop.workorder/3"),
        ("work-order-plan", "deslop.work-order-plan/1"),
        ("work-order-service", "deslop.work-order-service/1"),
        ("work-order-shared", "deslop.work-order/1"),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect()
}

fn shipped_capabilities() -> Vec<ShippedCapability> {
    let mut capabilities = vec![
        capability(
            "agent-protocol",
            "bounded-v1",
            "shipped",
            "content-addressed handles, pagination, budgets, stale rejection",
        ),
        capability(
            "clojure",
            "S1",
            "shipped-syntax",
            "owned CST, canonical roles, explicit unknown S2-S4",
        ),
        capability(
            "dogfood-finding-proposal",
            "whole-project",
            "unshipped",
            "15-minute timeout; use bounded protocol operations",
        ),
        capability(
            "javascript",
            "S1",
            "shipped-syntax",
            "owned CST, canonical roles, explicit unknown S2-S4",
        ),
        capability(
            "julia",
            "S1",
            "shipped-syntax",
            "owned CST, canonical roles, explicit unknown S2-S4",
        ),
        capability(
            "python",
            "S1",
            "shipped-syntax",
            "owned CST, canonical roles, explicit unknown S2-S4",
        ),
        capability(
            "readability",
            "evidence-only",
            "shipped-without-labels",
            "transparent axes only; no probability or readability label",
        ),
        capability(
            "rust",
            "S1+reviewed-graph-recipes",
            "shipped-limited",
            "gold graph fixtures; B2/B7 SafeAuto authority only for frozen unreachable-literal recipe; budget abstentions stay explicit",
        ),
        capability(
            "scale",
            "480-file-incremental",
            "shipped-limited",
            "M9 measured; no 1-MLOC release claim",
        ),
        capability(
            "typescript",
            "S1",
            "shipped-syntax",
            "owned CST, canonical roles, explicit unknown S2-S4",
        ),
        capability(
            "upstream-project-tests",
            "18-project-environment",
            "qualified",
            "2 pass; failures/timeouts/unavailable tools retained",
        ),
        capability(
            "whole-recipe-catalog",
            "review-only",
            "unshipped-safe-auto",
            "only the frozen Rust recipe slice has B2/B7 numerical authority",
        ),
    ];
    capabilities.sort_by(|left, right| left.surface.cmp(&right.surface));
    capabilities
}

fn capability(surface: &str, tier: &str, disposition: &str, authority: &str) -> ShippedCapability {
    ShippedCapability {
        surface: surface.into(),
        tier: tier.into(),
        disposition: disposition.into(),
        authority: authority.into(),
    }
}

fn release_exceptions() -> Vec<ReleaseException> {
    let mut exceptions = vec![
        exception(
            "EX-01",
            "closed",
            "Python blank-run SafeAuto",
            "dogfood correction and focused tests",
            "Python preserves two blank lines",
            "recheck only with syntax-context proof",
        ),
        exception(
            "EX-02",
            "downgraded",
            "whole-project finding proposal",
            "dogfood 15-minute timeout",
            "unshipped; bounded protocol only",
            "measured whole-project time/RSS budget passes",
        ),
        exception(
            "EX-03",
            "downgraded",
            "oversized dogfood recipe partitions",
            "30-second partition abstentions",
            "no candidate authority for those files",
            "each partition completes below budget",
        ),
        exception(
            "EX-04",
            "environment-qualified",
            "external repository tests",
            "refreshed warm-cache report: 4 pass, 11 fail, 3 tool unavailable; prior run retained 2 timeouts",
            "scan evidence only; no behavior-oracle claim",
            "isolated pinned dependency caches/runtimes in a new report",
        ),
        exception(
            "EX-05",
            "downgraded",
            "readability labels",
            "M8 lower bound/ECE/holdout failures",
            "evidence-only UX",
            "all frozen M8 model gates pass",
        ),
        exception(
            "EX-06",
            "downgraded",
            "global transformation precision",
            "B2/B7 Rust unreachable-literal slice only",
            "other recipes review-only",
            "balanced per-language/family corpus passes",
        ),
        exception(
            "EX-07",
            "downgraded",
            "canonical macro F1",
            "600-case compatibility corpus lacks independent full gold annotations",
            "ship syntax tiers backed by adapter gold fixtures, not a 600-case F1 claim",
            "independent 600-case annotations meet B6",
        ),
        exception(
            "EX-08",
            "downgraded",
            "human preference",
            "M8 model decision and external review-pending workflow",
            "no B9 or preference claim",
            "blinded accepted-patch preference gate passes",
        ),
        exception(
            "EX-09",
            "downgraded",
            "one-million-LOC scale",
            "M9 covers three 480-file projects",
            "no B11 1-MLOC claim",
            "recorded 1-MLOC cold/incremental/RSS run passes",
        ),
        exception(
            "EX-10",
            "downgraded",
            "artifact signature trust",
            "all artifacts content-addressed; no signing trust root",
            "digest integrity only",
            "configured signing identity produces a verifiable signature",
        ),
    ];
    exceptions.sort_by(|left, right| left.id.cmp(&right.id));
    exceptions
}

fn exception(
    id: &str,
    status: &str,
    scope: &str,
    evidence: &str,
    release_effect: &str,
    recheck: &str,
) -> ReleaseException {
    ReleaseException {
        id: id.into(),
        status: status.into(),
        scope: scope.into(),
        evidence: evidence.into(),
        release_effect: release_effect.into(),
        recheck_condition: recheck.into(),
    }
}

fn release_gate_decisions() -> Vec<ReleaseGateDecision> {
    let mut gates = vec![
        pass("M10.1", "m10 dogfood report and explicit dispositions"),
        downgraded(
            "M10.2",
            "18-project workflow report",
            "review workflow completed to review-pending; no synthetic human approval",
        ),
        pass("M10.3", "M6 paired 240-task graph/baseline report"),
        pass("M10.4", "M10 release report and joined artifact ledger"),
        pass("M10.5", "M10 capability matrix and failure/risk register"),
        pass("M10.6", "terminal ten-entry exception ledger"),
        pass("M10.7", "frozen version map and M10 migration guide"),
        pass(
            "M10.8",
            "security/verifier/undo/adapter/agent documentation set",
        ),
        pass("M10.9", "clean-checkout M10 gate report"),
        pass("M10.DoD", "stable evidence-limited capability matrix"),
        pass(
            "G1",
            "dogfood seven unique transformation work orders, zero duplicates",
        ),
        pass("G2", "M3 adversarial resolution gold and full gate"),
        pass("G3", "adapter capability manifests and full gate"),
        pass(
            "G4",
            "zero dogfood production SafeAuto; verifier/demotion gates",
        ),
        pass("G5", "M7 injected rollback tests and full gate"),
        pass("G6", "content-addressed artifacts and exact verification"),
        downgraded(
            "G7",
            "M8 frozen evaluation",
            "readability labels remain unshipped",
        ),
        pass("G8", "M6 +44.17pp paired improvement, CI excludes zero"),
        pass("G9", "M9 deterministic bounded incremental benchmark"),
        pass(
            "G10",
            "current docs, schemas, fixtures, reports, and negative memory",
        ),
        downgraded(
            "B1",
            "600-case six-language compatibility corpus",
            "independent full-corpus gold F1 is not claimed",
        ),
        downgraded(
            "B2",
            "1,000 opportunity + 1,000 negative Rust corpus",
            "global language/family coverage remains unshipped",
        ),
        downgraded(
            "B3",
            "18 exact repository pins and scans",
            "upstream test/cache environment is qualified, not isolated or all-green",
        ),
        pass("B4", "300 readability pairs and 240 paired LLM tasks"),
        downgraded(
            "B5",
            "licences/prompts/models/seeds/environment/schema digests",
            "content digests are not a cryptographic signer trust root",
        ),
        downgraded(
            "B6",
            "adapter/M3/M4 exact gold fixtures",
            "no independent numerical macro F1 over all 600 compatibility cases",
        ),
        downgraded(
            "B7",
            "Rust recipe B7 thresholds pass",
            "other languages/families remain review-only",
        ),
        pass(
            "B8",
            "zero known SafeAuto semantic failure; verifier authority retained",
        ),
        downgraded(
            "B9",
            "M8 evidence-only decision",
            "no human-preference shipping claim",
        ),
        pass("B10", "M6 paired graph-grounding gates all pass"),
        downgraded(
            "B11",
            "M9 480-file cold/incremental benchmark",
            "no one-million-LOC claim",
        ),
        pass(
            "B12",
            "macro/worst-slice/coverage/failure/prior-delta release report",
        ),
    ];
    gates.sort_by(|left, right| left.id.cmp(&right.id));
    gates
}

fn pass(id: &str, evidence: &str) -> ReleaseGateDecision {
    ReleaseGateDecision {
        id: id.into(),
        status: ReleaseGateStatus::Pass,
        evidence: vec![evidence.into()],
        limitation: None,
    }
}

fn downgraded(id: &str, evidence: &str, limitation: &str) -> ReleaseGateDecision {
    ReleaseGateDecision {
        id: id.into(),
        status: ReleaseGateStatus::Downgraded,
        evidence: vec![evidence.into()],
        limitation: Some(limitation.into()),
    }
}

fn expected_gate_ids() -> BTreeSet<&'static str> {
    [
        "M10.1", "M10.2", "M10.3", "M10.4", "M10.5", "M10.6", "M10.7", "M10.8", "M10.9", "M10.DoD",
        "G1", "G2", "G3", "G4", "G5", "G6", "G7", "G8", "G9", "G10", "B1", "B2", "B3", "B4", "B5",
        "B6", "B7", "B8", "B9", "B10", "B11", "B12",
    ]
    .into_iter()
    .collect()
}

fn release_summary(values: &BTreeMap<String, Value>) -> Result<ReleaseSummary> {
    let canonical = value(values, ".agents/benchmarks/m10_canonical_corpus_v1.json")?;
    let external = value(values, ".agents/benchmarks/m10_external_report_v1.json")?;
    let dogfood = value(values, ".agents/benchmarks/m10_dogfood_report_v1.json")?;
    let llm = value(values, ".agents/benchmarks/m6_llm_report_v1.json")?;
    let readability = value(
        values,
        "crates/deslop-eval/evaluation/m8/evaluation_report.json",
    )?;
    let scale = value(values, ".agents/benchmarks/m9_scale_report_v1.json")?;
    let recipe = value(
        values,
        "crates/deslop-recipes/evaluation/unreachable_literal_rust_v1_report.json",
    )?;
    let gate = value(values, ".agents/benchmarks/m10_gate_report_v1.json")?;
    Ok(ReleaseSummary {
        canonical_programs: usize_at(canonical, "/total_cases")?,
        canonical_languages: 6,
        external_projects: array_len(external, "/projects")?,
        external_source_files: sum_usize(external, "/projects", "source_files")?,
        external_source_lines: sum_usize(external, "/projects", "source_lines")?,
        external_test_passes: external
            .pointer("/projects")
            .and_then(Value::as_array)
            .context("external projects missing")?
            .iter()
            .filter(|project| {
                project.pointer("/test/status").and_then(Value::as_str) == Some("passed")
            })
            .count(),
        dogfood_files: usize_at(dogfood, "/source_files")?,
        dogfood_lines: usize_at(dogfood, "/source_lines")?,
        dogfood_findings: array_len(dogfood, "/findings")?,
        dogfood_production_safe_auto: usize_at(dogfood, "/production_safe_auto_findings")?,
        dogfood_recipe_candidates: array_len(dogfood, "/recipe_candidates")?,
        dogfood_recipe_abstentions: sum_usize(dogfood, "/recipe_partitions", "abstention_count")?,
        llm_paired_tasks: usize_at(llm, "/paired_tasks")?,
        llm_accepted_patch_delta: f64_at(llm, "/accepted_patch_delta")?,
        llm_ci95_lower: f64_at(llm, "/paired_ci95_lower")?,
        readability_pairs: usize_at(readability, "/report/corpus/pairs")?,
        readability_disposition: string_at(readability, "/report/decision/disposition")?.into(),
        scale_projects: array_len(scale, "/projects")?,
        scale_all_gates_pass: bool_at(scale, "/all_deterministic")?
            && bool_at(scale, "/all_incremental_parse_advantage")?
            && bool_at(scale, "/all_bounded_fan_out")?
            && bool_at(scale, "/all_measured_latency_advantage")?,
        recipe_opportunities: usize_at(recipe, "/raw_totals/true_positive")?,
        recipe_hard_negatives: usize_at(recipe, "/raw_totals/true_negative")?,
        recipe_thresholds_pass: bool_at(recipe, "/threshold_results/passed")?,
        clean_checkout_gate_pass: bool_at(gate, "/all_passed")?
            && bool_at(gate, "/clean_checkout")?,
    })
}

fn value<'a>(values: &'a BTreeMap<String, Value>, path: &str) -> Result<&'a Value> {
    values
        .get(path)
        .with_context(|| format!("release artifact {path} missing"))
}

fn usize_at(value: &Value, pointer: &str) -> Result<usize> {
    value
        .pointer(pointer)
        .and_then(Value::as_u64)
        .and_then(|number| number.try_into().ok())
        .with_context(|| format!("missing usize {pointer}"))
}
fn f64_at(value: &Value, pointer: &str) -> Result<f64> {
    value
        .pointer(pointer)
        .and_then(Value::as_f64)
        .with_context(|| format!("missing f64 {pointer}"))
}
fn bool_at(value: &Value, pointer: &str) -> Result<bool> {
    value
        .pointer(pointer)
        .and_then(Value::as_bool)
        .with_context(|| format!("missing bool {pointer}"))
}
fn string_at<'a>(value: &'a Value, pointer: &str) -> Result<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .with_context(|| format!("missing string {pointer}"))
}
fn array_len(value: &Value, pointer: &str) -> Result<usize> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::len)
        .with_context(|| format!("missing array {pointer}"))
}
fn sum_usize(value: &Value, pointer: &str, field: &str) -> Result<usize> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .with_context(|| format!("missing array {pointer}"))?
        .iter()
        .map(|item| usize_at(item, &format!("/{field}")))
        .sum()
}

fn release_id(evidence: &ReleaseEvidence) -> Result<String> {
    #[derive(Serialize)]
    struct Identity<'a> {
        product_version: &'a str,
        release_disposition: &'a str,
        versions: &'a BTreeMap<String, String>,
        artifacts: &'a [ReleaseArtifactEvidence],
        documentation: &'a [String],
        capabilities: &'a [ShippedCapability],
        exceptions: &'a [ReleaseException],
        gates: &'a [ReleaseGateDecision],
        summary: &'a ReleaseSummary,
    }
    let identity = Identity {
        product_version: &evidence.product_version,
        release_disposition: &evidence.release_disposition,
        versions: &evidence.versions,
        artifacts: &evidence.artifacts,
        documentation: &evidence.documentation,
        capabilities: &evidence.capabilities,
        exceptions: &evidence.exceptions,
        gates: &evidence.gates,
        summary: &evidence.summary,
    };
    Ok(format!(
        "m10rel1_{}",
        &digest_json("deslop m10 release evidence v1", &identity)?[7..]
    ))
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
    fn terminal_release_ledger_names_every_m10_global_and_benchmark_gate() {
        let decisions = release_gate_decisions();
        assert_eq!(decisions.len(), 32);
        assert_eq!(
            decisions
                .iter()
                .map(|gate| gate.id.as_str())
                .collect::<BTreeSet<_>>(),
            expected_gate_ids()
        );
        assert_eq!(release_exceptions().len(), 10);
        assert_eq!(shipped_capabilities().len(), 12);
    }

    #[test]
    fn frozen_public_versions_match_compiled_schema_constants() {
        let versions = frozen_versions();
        assert_eq!(
            versions["adapter-capabilities"],
            deslop_lang::LANGUAGE_ADAPTER_CAPABILITY_SCHEMA
        );
        assert_eq!(versions["graph-wire"], deslop_graph::GRAPH_SCHEMA);
        assert_eq!(
            versions["canonical-role-projection"],
            deslop_parse::CANONICAL_ROLE_PROJECTION_SCHEMA
        );
        assert_eq!(
            versions["readability-features"],
            deslop_metrics::READABILITY_FEATURE_SCHEMA_ID
        );
        assert_eq!(
            versions["recipe"],
            deslop_recipes::TRANSFORMATION_RECIPE_SCHEMA
        );
        assert_eq!(
            versions["transformation-candidate"],
            deslop_recipes::TRANSFORMATION_CANDIDATE_SCHEMA
        );
        assert_eq!(
            versions["work-order-legacy"],
            deslop_protocol::LEGACY_WORK_ORDER_SCHEMA
        );
        assert_eq!(
            versions["work-order-shared"],
            deslop_protocol::SHARED_WORK_ORDER_SCHEMA
        );
        assert_eq!(
            versions["work-order-plan"],
            deslop_protocol::WORK_ORDER_PLAN_SCHEMA
        );
        assert_eq!(
            versions["work-order-service"],
            deslop_protocol::WORK_ORDER_SERVICE_SCHEMA
        );
    }
}
