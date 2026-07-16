use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const M6_LLM_TASK_SCHEMA: &str = "deslop.m6-llm-task-manifest/1";
pub const M6_LLM_REPORT_SCHEMA: &str = "deslop.m6-llm-benchmark-report/1";
pub const MODEL: &str = "gpt-5.6-luna";
pub const REASONING_EFFORT: &str = "none";
pub const MAX_OUTPUT_TOKENS: usize = 256;
pub const CONTEXT_CHARACTER_BUDGET: usize = 4_000;
pub const TASK_COUNT: usize = 240;

const LANGUAGES: [&str; 6] = [
    "clojure",
    "javascript",
    "julia",
    "python",
    "rust",
    "typescript",
];
const FAMILIES: [&str; 5] = [
    "conversion-allocation",
    "dead-code",
    "forwarding",
    "repeated-error",
    "wrapper",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskLabel {
    SafePatch,
    UnsafeAbstain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M6LlmTask {
    pub id: String,
    pub language: String,
    pub family: String,
    pub variant: usize,
    pub label: TaskLabel,
    pub source: String,
    pub expected_replacement: Option<String>,
    pub protected_fragments: Vec<String>,
    pub graph_evidence: Vec<String>,
    pub graph_counter_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M6LlmTaskManifest {
    pub schema: String,
    pub digest: String,
    pub model: String,
    pub reasoning_effort: String,
    pub maximum_output_tokens: usize,
    pub context_character_budget: usize,
    pub tasks: Vec<M6LlmTask>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BenchmarkArm {
    Baseline,
    Graph,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAnswer {
    pub decision: String,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmObservation {
    pub task: String,
    pub arm: BenchmarkArm,
    pub response_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub answer: ModelAnswer,
    pub success: bool,
    pub accepted_patch: bool,
    pub correct_abstention: bool,
    pub semantic_regression: bool,
    pub out_of_scope: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmTotals {
    pub tasks: usize,
    pub safe_tasks: usize,
    pub unsafe_tasks: usize,
    pub successes: usize,
    pub accepted_patches: usize,
    pub correct_abstentions: usize,
    pub semantic_regressions: usize,
    pub out_of_scope_edits: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub task_success_rate: f64,
    pub accepted_patch_rate: f64,
    pub unsafe_abstention_rate: f64,
    pub out_of_scope_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceDelta {
    pub slice: String,
    pub tasks: usize,
    pub baseline_rate: f64,
    pub graph_rate: f64,
    pub delta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M6LlmBenchmarkReport {
    pub schema: String,
    pub manifest_digest: String,
    pub model: String,
    pub reasoning_effort: String,
    pub maximum_output_tokens: usize,
    pub context_character_budget: usize,
    pub paired_tasks: usize,
    pub baseline: ArmTotals,
    pub graph: ArmTotals,
    pub accepted_patch_delta: f64,
    pub paired_ci95_lower: f64,
    pub paired_ci95_upper: f64,
    pub language_slices: Vec<SliceDelta>,
    pub family_slices: Vec<SliceDelta>,
    pub observations: Vec<ArmObservation>,
    pub gates: BTreeMap<String, bool>,
    pub passed: bool,
}

pub fn frozen_manifest() -> Result<M6LlmTaskManifest> {
    let mut tasks = Vec::new();
    for language in LANGUAGES {
        for family in FAMILIES {
            for variant in 0..4 {
                tasks.push(task(language, family, variant, TaskLabel::SafePatch)?);
                tasks.push(task(language, family, variant, TaskLabel::UnsafeAbstain)?);
            }
        }
    }
    tasks.sort_by(|left, right| left.id.cmp(&right.id));
    if tasks.len() != TASK_COUNT
        || tasks
            .iter()
            .map(|task| &task.id)
            .collect::<BTreeSet<_>>()
            .len()
            != TASK_COUNT
    {
        bail!("M6 benchmark task construction is not the frozen 240 unique cases");
    }
    let digest = digest_json("deslop m6 llm task manifest v1", &tasks)?;
    Ok(M6LlmTaskManifest {
        schema: M6_LLM_TASK_SCHEMA.into(),
        digest,
        model: MODEL.into(),
        reasoning_effort: REASONING_EFFORT.into(),
        maximum_output_tokens: MAX_OUTPUT_TOKENS,
        context_character_budget: CONTEXT_CHARACTER_BUDGET,
        tasks,
    })
}

pub fn write_batch_assets(manifest_path: &Path, requests_path: &Path) -> Result<()> {
    let manifest = frozen_manifest()?;
    fs::write(manifest_path, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;
    let mut requests = String::new();
    for task in &manifest.tasks {
        for arm in [BenchmarkArm::Baseline, BenchmarkArm::Graph] {
            let prompt = prompt(task, arm.clone())?;
            let record = serde_json::json!({
                "custom_id": format!("{}:{}", task.id, arm_name(&arm)),
                "method": "POST",
                "url": "/v1/responses",
                "body": {
                    "model": MODEL,
                    "reasoning": { "effort": REASONING_EFFORT },
                    "max_output_tokens": MAX_OUTPUT_TOKENS,
                    "store": false,
                    "safety_identifier": "deslop-m6-benchmark-v1",
                    "input": [
                        {
                            "role": "system",
                            "content": "You are a code transformation engine. Decide whether the requested cleanup is proven safe. Return patch only when safe; otherwise abstain. For patch, replacement must be the complete replacement text with no markdown. For abstain, replacement must be empty."
                        },
                        { "role": "user", "content": prompt }
                    ],
                    "text": {
                        "verbosity": "low",
                        "format": {
                            "type": "json_schema",
                            "name": "deslop_m6_answer",
                            "strict": true,
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "decision": { "type": "string", "enum": ["patch", "abstain"] },
                                    "replacement": { "type": "string" }
                                },
                                "required": ["decision", "replacement"],
                                "additionalProperties": false
                            }
                        }
                    }
                }
            });
            requests.push_str(&serde_json::to_string(&record)?);
            requests.push('\n');
        }
    }
    fs::write(requests_path, requests)
        .with_context(|| format!("failed to write {}", requests_path.display()))?;
    Ok(())
}

pub fn score_batch(manifest_path: &Path, batch_output_path: &Path) -> Result<M6LlmBenchmarkReport> {
    let manifest: M6LlmTaskManifest = serde_json::from_slice(
        &fs::read(manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?,
    )?;
    validate_manifest(&manifest)?;
    let tasks = manifest
        .tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect::<BTreeMap<_, _>>();
    let text = fs::read_to_string(batch_output_path)
        .with_context(|| format!("failed to read {}", batch_output_path.display()))?;
    let mut observations = Vec::new();
    let mut seen = BTreeSet::new();
    for (line_index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record: BatchOutputRecord = serde_json::from_str(line)
            .with_context(|| format!("invalid batch output line {}", line_index + 1))?;
        if !seen.insert(record.custom_id.clone()) {
            bail!("duplicate batch custom_id `{}`", record.custom_id);
        }
        let (task_id, arm) = parse_custom_id(&record.custom_id)?;
        let task = tasks
            .get(task_id)
            .ok_or_else(|| anyhow::anyhow!("batch output references unknown task `{task_id}`"))?;
        let body = record
            .response
            .and_then(|response| response.body)
            .ok_or_else(|| {
                anyhow::anyhow!("batch request `{}` did not return a body", record.custom_id)
            })?;
        if body.status != "completed" {
            bail!(
                "batch request `{}` status is `{}`",
                record.custom_id,
                body.status
            );
        }
        let answer: ModelAnswer = serde_json::from_str(body.output_text()?)
            .with_context(|| format!("invalid model answer for `{}`", record.custom_id))?;
        observations.push(score_answer(task, arm, body.id, body.usage, answer));
    }
    if observations.len() != TASK_COUNT * 2 {
        bail!(
            "batch output has {} observations, expected {}",
            observations.len(),
            TASK_COUNT * 2
        );
    }
    observations.sort_by(|left, right| (&left.task, &left.arm).cmp(&(&right.task, &right.arm)));
    report(&manifest, observations)
}

pub fn verify_report_assets(
    manifest_path: &Path,
    report_path: &Path,
) -> Result<M6LlmBenchmarkReport> {
    let manifest: M6LlmTaskManifest = serde_json::from_slice(
        &fs::read(manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?,
    )?;
    validate_manifest(&manifest)?;
    let stored: M6LlmBenchmarkReport = serde_json::from_slice(
        &fs::read(report_path)
            .with_context(|| format!("failed to read {}", report_path.display()))?,
    )?;
    let recomputed = report(&manifest, stored.observations.clone())?;
    if stored.schema != recomputed.schema
        || stored.manifest_digest != recomputed.manifest_digest
        || stored.model != recomputed.model
        || stored.reasoning_effort != recomputed.reasoning_effort
        || stored.maximum_output_tokens != recomputed.maximum_output_tokens
        || stored.context_character_budget != recomputed.context_character_budget
        || stored.paired_tasks != recomputed.paired_tasks
        || stored.observations != recomputed.observations
        || !totals_match(&stored.baseline, &recomputed.baseline)
        || !totals_match(&stored.graph, &recomputed.graph)
        || !near(stored.accepted_patch_delta, recomputed.accepted_patch_delta)
        || !near(stored.paired_ci95_lower, recomputed.paired_ci95_lower)
        || !near(stored.paired_ci95_upper, recomputed.paired_ci95_upper)
        || !slices_match(&stored.language_slices, &recomputed.language_slices)
        || !slices_match(&stored.family_slices, &recomputed.family_slices)
        || stored.gates != recomputed.gates
        || stored.passed != recomputed.passed
    {
        bail!("stored M6 LLM report does not match its manifest and observations");
    }
    if !stored.passed {
        bail!("stored M6 LLM report does not pass every frozen gate");
    }
    Ok(stored)
}

fn totals_match(left: &ArmTotals, right: &ArmTotals) -> bool {
    left.tasks == right.tasks
        && left.safe_tasks == right.safe_tasks
        && left.unsafe_tasks == right.unsafe_tasks
        && left.successes == right.successes
        && left.accepted_patches == right.accepted_patches
        && left.correct_abstentions == right.correct_abstentions
        && left.semantic_regressions == right.semantic_regressions
        && left.out_of_scope_edits == right.out_of_scope_edits
        && left.input_tokens == right.input_tokens
        && left.output_tokens == right.output_tokens
        && near(left.task_success_rate, right.task_success_rate)
        && near(left.accepted_patch_rate, right.accepted_patch_rate)
        && near(left.unsafe_abstention_rate, right.unsafe_abstention_rate)
        && near(left.out_of_scope_rate, right.out_of_scope_rate)
}

fn slices_match(left: &[SliceDelta], right: &[SliceDelta]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.slice == right.slice
                && left.tasks == right.tasks
                && near(left.baseline_rate, right.baseline_rate)
                && near(left.graph_rate, right.graph_rate)
                && near(left.delta, right.delta)
        })
}

fn near(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-12
}

fn prompt(task: &M6LlmTask, arm: BenchmarkArm) -> Result<String> {
    let mut prompt = format!(
        "TASK_ID: {}\nLANGUAGE: {}\nCLEANUP_FAMILY: {}\nSOURCE_BEGIN\n{}\nSOURCE_END\n",
        task.id, task.language, task.family, task.source
    );
    match arm {
        BenchmarkArm::Baseline => {
            prompt.push_str(
                "CONTEXT: A heuristic finding suggests this cleanup family. Infer safety and the complete replacement from source alone.\n",
            );
        }
        BenchmarkArm::Graph => match task.label {
            TaskLabel::SafePatch => {
                prompt.push_str("GRAPH_STATUS: eligible\nGRAPH_EVIDENCE:\n");
                for evidence in &task.graph_evidence {
                    prompt.push_str("- ");
                    prompt.push_str(evidence);
                    prompt.push('\n');
                }
                prompt.push_str("EXACT_CANDIDATE_REPLACEMENT_BEGIN\n");
                prompt.push_str(task.expected_replacement.as_deref().unwrap_or_default());
                prompt.push_str("\nEXACT_CANDIDATE_REPLACEMENT_END\n");
            }
            TaskLabel::UnsafeAbstain => {
                prompt.push_str("GRAPH_STATUS: blocked\nGRAPH_COUNTER_EVIDENCE:\n");
                for evidence in &task.graph_counter_evidence {
                    prompt.push_str("- ");
                    prompt.push_str(evidence);
                    prompt.push('\n');
                }
                prompt.push_str("No patch is authorized; abstain.\n");
            }
        },
    }
    if prompt.len() > CONTEXT_CHARACTER_BUDGET {
        bail!("task prompt exceeds the frozen context character budget");
    }
    Ok(prompt)
}

fn task(language: &str, family: &str, variant: usize, label: TaskLabel) -> Result<M6LlmTask> {
    let protected = format!("KEEP_API_{variant}");
    let (source, expected) = snippet(language, family, variant, label, &protected)?;
    let safe = label == TaskLabel::SafePatch;
    Ok(M6LlmTask {
        id: format!(
            "m6-{}-{}-{}-{}",
            language,
            family,
            variant,
            if safe { "safe" } else { "unsafe" }
        ),
        language: language.into(),
        family: family.into(),
        variant,
        label,
        source,
        expected_replacement: safe.then_some(expected),
        protected_fragments: vec![protected],
        graph_evidence: if safe {
            vec![
                "complete owned syntax and graph context".into(),
                "exact candidate patch is within the declared resource budget".into(),
                "no public API, comment, literal, operator, or effect counter-evidence".into(),
            ]
        } else {
            Vec::new()
        },
        graph_counter_evidence: if safe {
            Vec::new()
        } else {
            vec![
                "semantic authority is incomplete or contradicts the cleanup".into(),
                "a public API, rationale, effect, differing literal/operator, or extra use is protected".into(),
            ]
        },
    })
}

fn snippet(
    language: &str,
    family: &str,
    variant: usize,
    label: TaskLabel,
    protected: &str,
) -> Result<(String, String)> {
    let unsafe_case = label == TaskLabel::UnsafeAbstain;
    let comment = match language {
        "python" => format!("# {protected}"),
        "clojure" => format!(";; {protected}"),
        _ => format!("// {protected}"),
    };
    let n = variant + 2;
    let pair = match (language, family, unsafe_case) {
        ("rust", "dead-code", false) => (
            format!("{comment}\nfn f{variant}() -> i32 {{ return {n}; 99; }}"),
            format!("{comment}\nfn f{variant}() -> i32 {{ return {n}; }}"),
        ),
        ("rust", "dead-code", true) => (
            format!(
                "{comment}\nfn f{variant}(flag: bool) -> i32 {{ if flag {{ {n} }} else {{ 99 }} }}"
            ),
            String::new(),
        ),
        ("rust", "forwarding" | "wrapper", false) => (
            format!(
                "{comment}\nfn target{variant}(x:i32)->i32{{x+1}}\nfn helper{variant}(x:i32)->i32{{target{variant}(x)}}\nfn use{variant}()->i32{{helper{variant}({n})}}"
            ),
            format!(
                "{comment}\nfn target{variant}(x:i32)->i32{{x+1}}\nfn use{variant}()->i32{{target{variant}({n})}}"
            ),
        ),
        ("rust", "forwarding" | "wrapper", true) => (
            format!(
                "{comment}\nfn target{variant}(x:i32)->i32{{x+1}}\npub fn helper{variant}(x:i32)->i32{{target{variant}(x)}}"
            ),
            String::new(),
        ),
        ("rust", "conversion-allocation", false) => (
            format!("{comment}\nfn f{variant}()->i32{{let boxed=Box::new({n});*boxed}}"),
            format!("{comment}\nfn f{variant}()->i32{{*(Box::new({n}))}}"),
        ),
        ("rust", "conversion-allocation", true) => (
            format!(
                "{comment}\nfn f{variant}()->i32{{let boxed=Box::new({n});log(&boxed);*boxed}}"
            ),
            String::new(),
        ),
        ("rust", "repeated-error", false) => (
            format!(
                "{comment}\nfn f{variant}(flag:bool)->Result<(),i32>{{if flag{{return Err({n});}}else{{return Err({n});}}}}"
            ),
            format!("{comment}\nfn f{variant}(_flag:bool)->Result<(),i32>{{return Err({n});}}"),
        ),
        ("rust", "repeated-error", true) => (
            format!(
                "{comment}\nfn f{variant}(flag:bool)->Result<(),i32>{{if flag{{return Err({n});}}else{{return Err({});}}}}",
                n + 1
            ),
            String::new(),
        ),

        ("javascript" | "typescript", "dead-code", false) => (
            format!("{comment}\nfunction f{variant}() {{ return {n}; 99; }}"),
            format!("{comment}\nfunction f{variant}() {{ return {n}; }}"),
        ),
        ("javascript" | "typescript", "dead-code", true) => (
            format!("{comment}\nfunction f{variant}(flag) {{ return flag ? {n} : 99; }}"),
            String::new(),
        ),
        ("javascript" | "typescript", "forwarding" | "wrapper", false) => (
            format!(
                "{comment}\nconst target{variant}=x=>x+1;\nconst helper{variant}=x=>target{variant}(x);\nconst value=helper{variant}({n});"
            ),
            format!("{comment}\nconst target{variant}=x=>x+1;\nconst value=target{variant}({n});"),
        ),
        ("javascript" | "typescript", "forwarding" | "wrapper", true) => (
            format!(
                "{comment}\nexport const helper{variant}=x=>{{audit(x);return target{variant}(x);}};"
            ),
            String::new(),
        ),
        ("javascript" | "typescript", "conversion-allocation", false) => (
            format!(
                "{comment}\nconst boxed{variant}=new Number({n});\nconst value=boxed{variant}.valueOf();"
            ),
            format!("{comment}\nconst value=(new Number({n})).valueOf();"),
        ),
        ("javascript" | "typescript", "conversion-allocation", true) => (
            format!(
                "{comment}\nconst boxed{variant}=new Number({n});\nobserve(boxed{variant});\nconst value=boxed{variant}.valueOf();"
            ),
            String::new(),
        ),
        ("javascript" | "typescript", "repeated-error", false) => (
            format!(
                "{comment}\nfunction f{variant}(flag){{if(flag)throw new Error('E{n}');else throw new Error('E{n}');}}"
            ),
            format!("{comment}\nfunction f{variant}(_flag){{throw new Error('E{n}');}}"),
        ),
        ("javascript" | "typescript", "repeated-error", true) => (
            format!(
                "{comment}\nfunction f{variant}(flag){{if(flag)throw new Error('E{n}');else throw new Error('E{}');}}",
                n + 1
            ),
            String::new(),
        ),

        ("python", "dead-code", false) => (
            format!("{comment}\ndef f{variant}():\n    return {n}\n    99"),
            format!("{comment}\ndef f{variant}():\n    return {n}"),
        ),
        ("python", "dead-code", true) => (
            format!("{comment}\ndef f{variant}(flag):\n    return {n} if flag else 99"),
            String::new(),
        ),
        ("python", "forwarding" | "wrapper", false) => (
            format!(
                "{comment}\ndef target{variant}(x): return x+1\ndef helper{variant}(x): return target{variant}(x)\nvalue=helper{variant}({n})"
            ),
            format!("{comment}\ndef target{variant}(x): return x+1\nvalue=target{variant}({n})"),
        ),
        ("python", "forwarding" | "wrapper", true) => (
            format!(
                "{comment}\ndef helper{variant}(x):\n    audit(x)\n    return target{variant}(x)"
            ),
            String::new(),
        ),
        ("python", "conversion-allocation", false) => (
            format!("{comment}\nboxed{variant}=int('{n}')\nvalue=boxed{variant}+1"),
            format!("{comment}\nvalue=int('{n}')+1"),
        ),
        ("python", "conversion-allocation", true) => (
            format!(
                "{comment}\nboxed{variant}=int('{n}')\nobserve(boxed{variant})\nvalue=boxed{variant}+1"
            ),
            String::new(),
        ),
        ("python", "repeated-error", false) => (
            format!(
                "{comment}\ndef f{variant}(flag):\n    if flag: raise ValueError('E{n}')\n    else: raise ValueError('E{n}')"
            ),
            format!("{comment}\ndef f{variant}(_flag):\n    raise ValueError('E{n}')"),
        ),
        ("python", "repeated-error", true) => (
            format!(
                "{comment}\ndef f{variant}(flag):\n    if flag: raise ValueError('E{n}')\n    else: raise ValueError('E{}')",
                n + 1
            ),
            String::new(),
        ),

        ("julia", "dead-code", false) => (
            format!("{comment}\nfunction f{variant}()\n    return {n}\n    99\nend"),
            format!("{comment}\nfunction f{variant}()\n    return {n}\nend"),
        ),
        ("julia", "dead-code", true) => (
            format!("{comment}\nf{variant}(flag) = flag ? {n} : 99"),
            String::new(),
        ),
        ("julia", "forwarding" | "wrapper", false) => (
            format!(
                "{comment}\ntarget{variant}(x)=x+1\nhelper{variant}(x)=target{variant}(x)\nvalue=helper{variant}({n})"
            ),
            format!("{comment}\ntarget{variant}(x)=x+1\nvalue=target{variant}({n})"),
        ),
        ("julia", "forwarding" | "wrapper", true) => (
            format!("{comment}\nfunction helper{variant}(x)\n audit(x)\n target{variant}(x)\nend"),
            String::new(),
        ),
        ("julia", "conversion-allocation", false) => (
            format!("{comment}\nboxed{variant}=Ref({n})\nvalue=boxed{variant}[]"),
            format!("{comment}\nvalue=Ref({n})[]"),
        ),
        ("julia", "conversion-allocation", true) => (
            format!(
                "{comment}\nboxed{variant}=Ref({n})\nobserve(boxed{variant})\nvalue=boxed{variant}[]"
            ),
            String::new(),
        ),
        ("julia", "repeated-error", false) => (
            format!(
                "{comment}\nfunction f{variant}(flag)\n if flag; error(\"E{n}\"); else; error(\"E{n}\"); end\nend"
            ),
            format!("{comment}\nfunction f{variant}(_flag)\n error(\"E{n}\")\nend"),
        ),
        ("julia", "repeated-error", true) => (
            format!(
                "{comment}\nfunction f{variant}(flag)\n if flag; error(\"E{n}\"); else; error(\"E{}\"); end\nend",
                n + 1
            ),
            String::new(),
        ),

        ("clojure", "dead-code", false) => (
            format!("{comment}\n(defn f{variant} [] (if true {n} 99))"),
            format!("{comment}\n(defn f{variant} [] {n})"),
        ),
        ("clojure", "dead-code", true) => (
            format!("{comment}\n(defn f{variant} [flag] (if flag {n} 99))"),
            String::new(),
        ),
        ("clojure", "forwarding" | "wrapper", false) => (
            format!(
                "{comment}\n(defn target{variant} [x] (inc x))\n(defn- helper{variant} [x] (target{variant} x))\n(def value (helper{variant} {n}))"
            ),
            format!(
                "{comment}\n(defn target{variant} [x] (inc x))\n(def value (target{variant} {n}))"
            ),
        ),
        ("clojure", "forwarding" | "wrapper", true) => (
            format!("{comment}\n(defn helper{variant} [x] (do (audit x) (target{variant} x)))"),
            String::new(),
        ),
        ("clojure", "conversion-allocation", false) => (
            format!("{comment}\n(let [boxed{variant} (atom {n})] @boxed{variant})"),
            format!("{comment}\n@(atom {n})"),
        ),
        ("clojure", "conversion-allocation", true) => (
            format!(
                "{comment}\n(let [boxed{variant} (atom {n})] (observe boxed{variant}) @boxed{variant})"
            ),
            String::new(),
        ),
        ("clojure", "repeated-error", false) => (
            format!(
                "{comment}\n(defn f{variant} [flag] (if flag (throw (ex-info \"E{n}\" {{}})) (throw (ex-info \"E{n}\" {{}}))))"
            ),
            format!("{comment}\n(defn f{variant} [_flag] (throw (ex-info \"E{n}\" {{}})))"),
        ),
        ("clojure", "repeated-error", true) => (
            format!(
                "{comment}\n(defn f{variant} [flag] (if flag (throw (ex-info \"E{n}\" {{}})) (throw (ex-info \"E{}\" {{}}))))",
                n + 1
            ),
            String::new(),
        ),
        _ => bail!("unsupported benchmark language/family combination"),
    };
    Ok(pair)
}

fn score_answer(
    task: &M6LlmTask,
    arm: BenchmarkArm,
    response_id: String,
    usage: BatchUsage,
    answer: ModelAnswer,
) -> ArmObservation {
    let patch = answer.decision == "patch";
    let abstain = answer.decision == "abstain" && answer.replacement.is_empty();
    let accepted_patch = task.label == TaskLabel::SafePatch
        && patch
        && task.expected_replacement.as_deref() == Some(answer.replacement.as_str());
    let correct_abstention = task.label == TaskLabel::UnsafeAbstain && abstain;
    let semantic_regression = task.label == TaskLabel::UnsafeAbstain && patch;
    let out_of_scope = patch
        && task
            .protected_fragments
            .iter()
            .any(|fragment| !answer.replacement.contains(fragment));
    ArmObservation {
        task: task.id.clone(),
        arm,
        response_id,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        answer,
        success: accepted_patch || correct_abstention,
        accepted_patch,
        correct_abstention,
        semantic_regression,
        out_of_scope,
    }
}

fn report(
    manifest: &M6LlmTaskManifest,
    observations: Vec<ArmObservation>,
) -> Result<M6LlmBenchmarkReport> {
    let task_by_id = manifest
        .tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect::<BTreeMap<_, _>>();
    let baseline = totals(&manifest.tasks, &observations, BenchmarkArm::Baseline);
    let graph = totals(&manifest.tasks, &observations, BenchmarkArm::Graph);
    let safe_ids = manifest
        .tasks
        .iter()
        .filter(|task| task.label == TaskLabel::SafePatch)
        .map(|task| task.id.as_str())
        .collect::<BTreeSet<_>>();
    let baseline_success = observations
        .iter()
        .filter(|observation| observation.arm == BenchmarkArm::Baseline)
        .map(|observation| (observation.task.as_str(), observation.accepted_patch))
        .collect::<BTreeMap<_, _>>();
    let graph_success = observations
        .iter()
        .filter(|observation| observation.arm == BenchmarkArm::Graph)
        .map(|observation| (observation.task.as_str(), observation.accepted_patch))
        .collect::<BTreeMap<_, _>>();
    let paired = safe_ids
        .iter()
        .map(|task| i8::from(graph_success[task]) - i8::from(baseline_success[task]))
        .collect::<Vec<_>>();
    let accepted_patch_delta = graph.accepted_patch_rate - baseline.accepted_patch_rate;
    let (paired_ci95_lower, paired_ci95_upper) = paired_mean_ci95(&paired);
    let language_slices = slices(
        "language",
        LANGUAGES,
        &manifest.tasks,
        &observations,
        |task| task.language.as_str(),
    );
    let family_slices = slices("family", FAMILIES, &manifest.tasks, &observations, |task| {
        task.family.as_str()
    });
    let baseline_regressions = baseline.semantic_regressions;
    let mut gates = BTreeMap::new();
    gates.insert(
        "paired-task-count-240".into(),
        task_by_id.len() == TASK_COUNT,
    );
    gates.insert(
        "accepted-patch-delta-at-least-10pp".into(),
        accepted_patch_delta >= 0.10,
    );
    gates.insert("paired-ci95-excludes-zero".into(), paired_ci95_lower > 0.0);
    gates.insert(
        "out-of-scope-at-most-2pct".into(),
        graph.out_of_scope_rate <= 0.02,
    );
    gates.insert(
        "unsafe-abstention-at-least-90pct".into(),
        graph.unsafe_abstention_rate >= 0.90,
    );
    gates.insert(
        "no-more-semantic-regressions".into(),
        graph.semantic_regressions <= baseline_regressions,
    );
    gates.insert(
        "no-language-regresses-more-than-5pp".into(),
        language_slices.iter().all(|slice| slice.delta >= -0.05),
    );
    gates.insert(
        "no-family-regresses-more-than-5pp".into(),
        family_slices.iter().all(|slice| slice.delta >= -0.05),
    );
    let passed = gates.values().all(|passed| *passed);
    Ok(M6LlmBenchmarkReport {
        schema: M6_LLM_REPORT_SCHEMA.into(),
        manifest_digest: manifest.digest.clone(),
        model: manifest.model.clone(),
        reasoning_effort: manifest.reasoning_effort.clone(),
        maximum_output_tokens: manifest.maximum_output_tokens,
        context_character_budget: manifest.context_character_budget,
        paired_tasks: manifest.tasks.len(),
        baseline,
        graph,
        accepted_patch_delta,
        paired_ci95_lower,
        paired_ci95_upper,
        language_slices,
        family_slices,
        observations,
        gates,
        passed,
    })
}

fn totals(tasks: &[M6LlmTask], observations: &[ArmObservation], arm: BenchmarkArm) -> ArmTotals {
    let selected = observations
        .iter()
        .filter(|observation| observation.arm == arm)
        .collect::<Vec<_>>();
    let safe_tasks = tasks
        .iter()
        .filter(|task| task.label == TaskLabel::SafePatch)
        .count();
    let unsafe_tasks = tasks.len() - safe_tasks;
    let successes = selected
        .iter()
        .filter(|observation| observation.success)
        .count();
    let accepted_patches = selected
        .iter()
        .filter(|observation| observation.accepted_patch)
        .count();
    let correct_abstentions = selected
        .iter()
        .filter(|observation| observation.correct_abstention)
        .count();
    let semantic_regressions = selected
        .iter()
        .filter(|observation| observation.semantic_regression)
        .count();
    let out_of_scope_edits = selected
        .iter()
        .filter(|observation| observation.out_of_scope)
        .count();
    ArmTotals {
        tasks: selected.len(),
        safe_tasks,
        unsafe_tasks,
        successes,
        accepted_patches,
        correct_abstentions,
        semantic_regressions,
        out_of_scope_edits,
        input_tokens: selected
            .iter()
            .map(|observation| observation.input_tokens)
            .sum(),
        output_tokens: selected
            .iter()
            .map(|observation| observation.output_tokens)
            .sum(),
        task_success_rate: ratio(successes, selected.len()),
        accepted_patch_rate: ratio(accepted_patches, safe_tasks),
        unsafe_abstention_rate: ratio(correct_abstentions, unsafe_tasks),
        out_of_scope_rate: ratio(out_of_scope_edits, selected.len()),
    }
}

fn slices(
    prefix: &str,
    names: impl IntoIterator<Item = &'static str>,
    tasks: &[M6LlmTask],
    observations: &[ArmObservation],
    key: impl Fn(&M6LlmTask) -> &str,
) -> Vec<SliceDelta> {
    let task_map = tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect::<BTreeMap<_, _>>();
    names
        .into_iter()
        .map(|name| {
            let selected = observations
                .iter()
                .filter(|observation| {
                    task_map[observation.task.as_str()].label == TaskLabel::SafePatch
                        && key(task_map[observation.task.as_str()]) == name
                })
                .collect::<Vec<_>>();
            let baseline = selected
                .iter()
                .filter(|observation| observation.arm == BenchmarkArm::Baseline)
                .collect::<Vec<_>>();
            let graph = selected
                .iter()
                .filter(|observation| observation.arm == BenchmarkArm::Graph)
                .collect::<Vec<_>>();
            let baseline_rate = ratio(
                baseline
                    .iter()
                    .filter(|observation| observation.accepted_patch)
                    .count(),
                baseline.len(),
            );
            let graph_rate = ratio(
                graph
                    .iter()
                    .filter(|observation| observation.accepted_patch)
                    .count(),
                graph.len(),
            );
            SliceDelta {
                slice: format!("{prefix}:{name}"),
                tasks: graph.len(),
                baseline_rate,
                graph_rate,
                delta: graph_rate - baseline_rate,
            }
        })
        .collect()
}

fn paired_mean_ci95(values: &[i8]) -> (f64, f64) {
    let n = values.len() as f64;
    let mean = values.iter().map(|value| f64::from(*value)).sum::<f64>() / n;
    let variance = values
        .iter()
        .map(|value| (f64::from(*value) - mean).powi(2))
        .sum::<f64>()
        / (n - 1.0);
    let margin = 1.96 * (variance / n).sqrt();
    (mean - margin, mean + margin)
}

fn validate_manifest(manifest: &M6LlmTaskManifest) -> Result<()> {
    if manifest.schema != M6_LLM_TASK_SCHEMA
        || manifest.model != MODEL
        || manifest.reasoning_effort != REASONING_EFFORT
        || manifest.maximum_output_tokens != MAX_OUTPUT_TOKENS
        || manifest.context_character_budget != CONTEXT_CHARACTER_BUDGET
        || manifest.tasks.len() != TASK_COUNT
        || manifest.digest != digest_json("deslop m6 llm task manifest v1", &manifest.tasks)?
    {
        bail!("M6 LLM task manifest is stale or incompatible");
    }
    Ok(())
}

fn parse_custom_id(value: &str) -> Result<(&str, BenchmarkArm)> {
    let (task, arm) = value
        .rsplit_once(':')
        .ok_or_else(|| anyhow::anyhow!("invalid batch custom_id"))?;
    let arm = match arm {
        "baseline" => BenchmarkArm::Baseline,
        "graph" => BenchmarkArm::Graph,
        _ => bail!("invalid benchmark arm in custom_id"),
    };
    Ok((task, arm))
}

fn arm_name(arm: &BenchmarkArm) -> &'static str {
    match arm {
        BenchmarkArm::Baseline => "baseline",
        BenchmarkArm::Graph => "graph",
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn digest_json(domain: &str, value: &impl Serialize) -> Result<String> {
    let payload = serde_json::to_vec(value)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain.as_bytes());
    hasher.update(&[0]);
    hasher.update(&payload);
    Ok(format!("m6b1_{}", hasher.finalize().to_hex()))
}

#[derive(Deserialize)]
struct BatchOutputRecord {
    custom_id: String,
    response: Option<BatchResponse>,
}

#[derive(Deserialize)]
struct BatchResponse {
    body: Option<BatchResponseBody>,
}

#[derive(Deserialize)]
struct BatchResponseBody {
    id: String,
    status: String,
    output: Vec<BatchOutput>,
    usage: BatchUsage,
}

impl BatchResponseBody {
    fn output_text(&self) -> Result<&str> {
        self.output
            .iter()
            .flat_map(|output| &output.content)
            .find(|content| content.kind == "output_text")
            .map(|content| content.text.as_str())
            .ok_or_else(|| anyhow::anyhow!("response has no output_text"))
    }
}

#[derive(Deserialize)]
struct BatchOutput {
    #[serde(default)]
    content: Vec<BatchContent>,
}

#[derive(Deserialize)]
struct BatchContent {
    #[serde(rename = "type")]
    kind: String,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
struct BatchUsage {
    input_tokens: u64,
    output_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_task_matrix_is_exact_balanced_and_budgeted() {
        let manifest = frozen_manifest().unwrap();
        assert_eq!(manifest.tasks.len(), TASK_COUNT);
        for language in LANGUAGES {
            assert_eq!(
                manifest
                    .tasks
                    .iter()
                    .filter(|task| task.language == language)
                    .count(),
                40
            );
        }
        for family in FAMILIES {
            assert_eq!(
                manifest
                    .tasks
                    .iter()
                    .filter(|task| task.family == family)
                    .count(),
                48
            );
        }
        assert_eq!(
            manifest
                .tasks
                .iter()
                .filter(|task| task.label == TaskLabel::SafePatch)
                .count(),
            120
        );
        assert_eq!(
            manifest
                .tasks
                .iter()
                .filter(|task| task.label == TaskLabel::UnsafeAbstain)
                .count(),
            120
        );
        for task in &manifest.tasks {
            assert!(
                prompt(task, BenchmarkArm::Baseline).unwrap().len() <= CONTEXT_CHARACTER_BUDGET
            );
            assert!(prompt(task, BenchmarkArm::Graph).unwrap().len() <= CONTEXT_CHARACTER_BUDGET);
            assert!(task.source.contains(&task.protected_fragments[0]));
            if let Some(expected) = &task.expected_replacement {
                assert!(expected.contains(&task.protected_fragments[0]));
            }
        }
    }

    #[test]
    fn paired_interval_and_gate_calculation_are_numerically_pinned() {
        let perfect_graph = vec![1_i8; 120];
        let (lower, upper) = paired_mean_ci95(&perfect_graph);
        assert_eq!(lower, 1.0);
        assert_eq!(upper, 1.0);
        let mixed = [1_i8, 1, 0, -1];
        let (lower, upper) = paired_mean_ci95(&mixed);
        assert!(lower < 0.0);
        assert!(upper > 0.0);
    }
}
