use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use deslop_core::{Lang, revision_guard};
use deslop_parse::{SourceFile, source_parses_without_errors};
use deslop_protocol::{
    RECIPE_WORK_ORDER_SCHEMA, RecipeWorkOrder, SharedWorkOrder, WorkOrderSubject,
};
use deslop_recipes::{CandidateDisposition, TransformationCandidate, detect_rust_recipes};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

pub const RECIPE_APPLY_REPORT_SCHEMA: &str = "deslop.recipe-apply/1";

#[derive(Debug, Clone)]
pub struct RecipeApplyOptions {
    pub root: PathBuf,
    pub build_command: String,
    pub test_command: String,
    pub backup: bool,
    /// Explicit authority for controlled canary execution. Production automatic
    /// application remains disabled until real-repository enablement evidence exists.
    pub explicit_canary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeApplyStatus {
    Applied,
    Rejected,
    RolledBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecipeCheckPhase {
    Staged,
    Live,
    Rollback,
}

impl RecipeCheckPhase {
    fn environment_value(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Live => "live",
            Self::Rollback => "rollback",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeCheckResult {
    pub phase: RecipeCheckPhase,
    pub name: String,
    pub command: Option<String>,
    pub passed: bool,
    pub elapsed_millis: u128,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeApplyReport {
    pub schema: String,
    pub status: RecipeApplyStatus,
    pub workorder_ids: Vec<String>,
    pub written: Vec<PathBuf>,
    pub disk_revision: String,
    pub live_revision: String,
    pub rollback_verified: bool,
    pub protected_resources_unchanged: bool,
    pub checks: Vec<RecipeCheckResult>,
    pub reasons: Vec<String>,
}

pub fn load_recipe_work_orders(path: &Path) -> Result<Vec<RecipeWorkOrder>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    if let Ok(orders) = serde_json::from_str::<Vec<RecipeWorkOrder>>(&text) {
        return validate_order_set(orders);
    }
    if let Ok(orders) = serde_json::from_str::<Vec<SharedWorkOrder>>(&text) {
        return validate_order_set(
            orders
                .into_iter()
                .map(recipe_order_from_shared)
                .collect::<Result<Vec<_>>>()?,
        );
    }
    let mut orders = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let order = match serde_json::from_str::<RecipeWorkOrder>(line) {
            Ok(order) => order,
            Err(_) => recipe_order_from_shared(
                serde_json::from_str::<SharedWorkOrder>(line).with_context(|| {
                    format!("invalid shared or recipe work order at line {}", index + 1)
                })?,
            )?,
        };
        orders.push(order);
    }
    validate_order_set(orders)
}

fn recipe_order_from_shared(order: SharedWorkOrder) -> Result<RecipeWorkOrder> {
    match order.subject() {
        WorkOrderSubject::Transformation { candidate } => {
            RecipeWorkOrder::from_candidate((**candidate).clone())
        }
        WorkOrderSubject::FindingProposal { .. } => {
            bail!("recipe apply requires transformation work orders, not finding proposals")
        }
    }
}

pub fn apply_recipe_work_orders(
    orders: &[RecipeWorkOrder],
    options: &RecipeApplyOptions,
) -> Result<RecipeApplyReport> {
    apply_recipe_work_orders_with_hook(orders, options, || Ok(()))
}

fn apply_recipe_work_orders_with_hook(
    orders: &[RecipeWorkOrder],
    options: &RecipeApplyOptions,
    before_write: impl FnOnce() -> Result<()>,
) -> Result<RecipeApplyReport> {
    validate_options(options)?;
    let orders = validate_order_set(orders.to_vec())?;
    if orders.is_empty() {
        bail!("recipe apply requires at least one work order");
    }
    let root = options
        .root
        .canonicalize()
        .with_context(|| format!("failed to resolve recipe root {}", options.root.display()))?;
    let paths = orders
        .iter()
        .map(|order| order.target_path().clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let workorder_ids = orders
        .iter()
        .map(|order| order.id().as_str().to_string())
        .collect::<Vec<_>>();
    let mut report = RecipeApplyReport {
        schema: RECIPE_APPLY_REPORT_SCHEMA.to_string(),
        status: RecipeApplyStatus::Rejected,
        workorder_ids,
        written: Vec::new(),
        disk_revision: "unchanged".to_string(),
        live_revision: "not-rebuilt".to_string(),
        rollback_verified: true,
        protected_resources_unchanged: true,
        checks: Vec::new(),
        reasons: Vec::new(),
    };
    if !options.explicit_canary {
        report.reasons.push(
            "automatic recipe application is disabled; use explicit canary authority only for controlled validation"
                .to_string(),
        );
        return Ok(report);
    }

    let current = detect_rust_recipes(&root, &paths)?;
    if let Err(reason) = validate_current_authority(&orders, &current) {
        report.rollback_verified = validate_disk_guards(&root, &orders).is_ok();
        report.reasons.push(reason.to_string());
        return Ok(report);
    }
    let originals = read_declared_sources(&root, &orders)?;
    let patched = build_patched_sources(&orders, &originals)?;
    let protected = snapshot_protected_files(&root, &paths)?;

    let staged = TempDir::new().context("failed to create staged recipe project")?;
    super::copy_project_for_check(&root, staged.path())?;
    write_source_map(staged.path(), &patched, false)?;
    if !validate_internal(
        staged.path(),
        &paths,
        &[],
        RecipeCheckPhase::Staged,
        &mut report.checks,
    )? {
        report
            .reasons
            .push("staged parse or graph-delta validation failed".to_string());
        return Ok(report);
    }
    if !run_required_commands(
        staged.path(),
        options,
        RecipeCheckPhase::Staged,
        &mut report.checks,
    )? {
        report
            .reasons
            .push("staged build or test validation failed".to_string());
        return Ok(report);
    }

    before_write()?;
    if let Err(error) = validate_exact_originals(&root, &orders, &originals) {
        report.rollback_verified = validate_disk_guards(&root, &orders).is_ok();
        report.reasons.push(error.to_string());
        return Ok(report);
    }
    if let Err(error) = write_exact_source_map(&root, &originals, &patched, options.backup) {
        let rollback = restore_sources(&root, &originals);
        report.status = RecipeApplyStatus::RolledBack;
        report.rollback_verified = rollback.is_ok() && sources_equal(&root, &originals)?;
        report.reasons.push(format!("write failed: {error}"));
        if let Err(rollback_error) = rollback {
            report
                .reasons
                .push(format!("rollback failed: {rollback_error}"));
        }
        return Ok(report);
    }
    report.written = paths.clone();
    report.disk_revision = "candidate-applied".to_string();

    let internal_live = validate_internal(
        &root,
        &paths,
        &[],
        RecipeCheckPhase::Live,
        &mut report.checks,
    )?;
    let commands_live = if internal_live {
        run_required_commands(&root, options, RecipeCheckPhase::Live, &mut report.checks)?
    } else {
        false
    };
    let declared_live = sources_equal(&root, &patched)?;
    let protected_live = protected_files_equal(&root, &paths, &protected)?;
    report.protected_resources_unchanged = declared_live && protected_live;
    if internal_live && commands_live && declared_live && protected_live {
        report.status = RecipeApplyStatus::Applied;
        report.live_revision = "rebuilt-and-tested".to_string();
        return Ok(report);
    }

    report.status = RecipeApplyStatus::RolledBack;
    report.reasons.push(if !protected_live {
        "a required command changed an undeclared protected file".to_string()
    } else {
        "live validation failed after write".to_string()
    });
    restore_sources(&root, &originals)?;
    report.disk_revision = "original-restored".to_string();
    let bytes_restored = sources_equal(&root, &originals)?;
    let graph_restored = validate_internal(
        &root,
        &paths,
        &current,
        RecipeCheckPhase::Rollback,
        &mut report.checks,
    )?;
    let commands_restored = run_required_commands(
        &root,
        options,
        RecipeCheckPhase::Rollback,
        &mut report.checks,
    )?;
    let protected_restored = protected_files_equal(&root, &paths, &protected)?;
    report.rollback_verified =
        bytes_restored && graph_restored && commands_restored && protected_restored;
    report.live_revision = if report.rollback_verified {
        "original-rebuilt-and-tested".to_string()
    } else {
        "rollback-unverified".to_string()
    };
    if !report.rollback_verified {
        report
            .reasons
            .push("rollback validation failed".to_string());
    }
    Ok(report)
}

fn validate_order_set(mut orders: Vec<RecipeWorkOrder>) -> Result<Vec<RecipeWorkOrder>> {
    let mut ids = BTreeSet::new();
    let mut candidates = BTreeSet::new();
    for order in &orders {
        order.validate()?;
        if order.schema() != RECIPE_WORK_ORDER_SCHEMA {
            bail!("unsupported recipe work-order schema `{}`", order.schema());
        }
        if !ids.insert(order.id().as_str().to_string()) {
            bail!("duplicate recipe work order `{}`", order.id());
        }
        if !candidates.insert(order.candidate().id().as_str().to_string()) {
            bail!(
                "duplicate candidate `{}` in recipe work orders",
                order.candidate().id()
            );
        }
        if order.candidate().disposition() != CandidateDisposition::Automatic {
            bail!("recipe work order `{}` is not automatic", order.id());
        }
    }
    orders.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(orders)
}

fn validate_options(options: &RecipeApplyOptions) -> Result<()> {
    if options.build_command.trim().is_empty() || options.test_command.trim().is_empty() {
        bail!("recipe apply requires nonempty build and test commands");
    }
    Ok(())
}

fn validate_current_authority(
    orders: &[RecipeWorkOrder],
    current: &[TransformationCandidate],
) -> Result<()> {
    let expected = orders
        .iter()
        .map(|order| (order.candidate().id().as_str(), order.candidate()))
        .collect::<BTreeMap<_, _>>();
    let actual = current
        .iter()
        .map(|candidate| (candidate.id().as_str(), candidate))
        .collect::<BTreeMap<_, _>>();
    if expected.len() != actual.len() {
        bail!(
            "stale or incomplete recipe batch: {} ordered candidates but {} current candidates",
            expected.len(),
            actual.len()
        );
    }
    for (id, candidate) in expected {
        if actual.get(id).copied() != Some(candidate) {
            bail!("candidate `{id}` is stale or foreign to the current source");
        }
    }
    Ok(())
}

fn read_declared_sources(
    root: &Path,
    orders: &[RecipeWorkOrder],
) -> Result<BTreeMap<PathBuf, String>> {
    let mut originals = BTreeMap::new();
    for order in orders {
        if !originals.contains_key(order.target_path()) {
            let path = root.join(order.target_path());
            originals.insert(
                order.target_path().clone(),
                fs::read_to_string(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?,
            );
        }
    }
    Ok(originals)
}

fn build_patched_sources(
    orders: &[RecipeWorkOrder],
    originals: &BTreeMap<PathBuf, String>,
) -> Result<BTreeMap<PathBuf, String>> {
    let mut edits = BTreeMap::<PathBuf, Vec<_>>::new();
    for order in orders {
        for edit in order.candidate().edits() {
            edits
                .entry(order.target_path().clone())
                .or_default()
                .push(edit);
        }
    }
    let mut patched = BTreeMap::new();
    for (path, mut file_edits) in edits {
        let original = originals
            .get(&path)
            .with_context(|| format!("missing original source {}", path.display()))?;
        file_edits.sort_by_key(|edit| (edit.span.start_byte, edit.span.end_byte));
        if file_edits
            .windows(2)
            .any(|pair| pair[0].span.end_byte > pair[1].span.start_byte)
        {
            bail!("overlapping recipe edits for {}", path.display());
        }
        let mut text = original.clone();
        for edit in file_edits.into_iter().rev() {
            let retained = original
                .get(edit.span.start_byte..edit.span.end_byte)
                .with_context(|| format!("recipe edit is outside {}", path.display()))?;
            if retained != edit.before
                || revision_guard(&path, edit.span, retained) != edit.revision_guard
            {
                bail!("stale revision guard for {}", path.display());
            }
            text.replace_range(edit.span.start_byte..edit.span.end_byte, &edit.after);
        }
        patched.insert(path, text);
    }
    Ok(patched)
}

fn validate_exact_originals(
    root: &Path,
    orders: &[RecipeWorkOrder],
    originals: &BTreeMap<PathBuf, String>,
) -> Result<()> {
    if !sources_equal(root, originals)? {
        bail!("source changed after staged validation and before write");
    }
    validate_disk_guards(root, orders)
}

fn validate_disk_guards(root: &Path, orders: &[RecipeWorkOrder]) -> Result<()> {
    for order in orders {
        let text = fs::read_to_string(root.join(order.target_path()))?;
        for edit in order.candidate().edits() {
            let retained = text
                .get(edit.span.start_byte..edit.span.end_byte)
                .context("revision guard span is outside current source")?;
            if retained != edit.before
                || revision_guard(order.target_path(), edit.span, retained) != edit.revision_guard
            {
                bail!("stale revision guard for {}", order.target_path().display());
            }
        }
    }
    Ok(())
}

fn validate_internal(
    root: &Path,
    paths: &[PathBuf],
    expected_candidates: &[TransformationCandidate],
    phase: RecipeCheckPhase,
    checks: &mut Vec<RecipeCheckResult>,
) -> Result<bool> {
    let parse_started = Instant::now();
    let mut parse_passed = true;
    let mut parse_detail = "all declared Rust sources parsed without errors".to_string();
    for path in paths {
        let text = fs::read_to_string(root.join(path))?;
        let source = SourceFile::new_with_lang(path.clone(), text, Lang::Rust);
        if source_parses_without_errors(&source)? != Some(true) {
            parse_passed = false;
            parse_detail = format!("{} did not parse without errors", path.display());
            break;
        }
    }
    checks.push(RecipeCheckResult {
        phase,
        name: "parse".to_string(),
        command: None,
        passed: parse_passed,
        elapsed_millis: parse_started.elapsed().as_millis(),
        detail: parse_detail,
    });
    if !parse_passed {
        return Ok(false);
    }

    let graph_started = Instant::now();
    let actual = detect_rust_recipes(root, paths)?;
    let graph_passed = actual == expected_candidates;
    checks.push(RecipeCheckResult {
        phase,
        name: "graph-delta".to_string(),
        command: None,
        passed: graph_passed,
        elapsed_millis: graph_started.elapsed().as_millis(),
        detail: if graph_passed {
            format!(
                "retained detector returned {} expected candidates",
                actual.len()
            )
        } else {
            format!(
                "retained detector returned {} candidates; expected {}",
                actual.len(),
                expected_candidates.len()
            )
        },
    });
    Ok(graph_passed)
}

fn run_required_commands(
    root: &Path,
    options: &RecipeApplyOptions,
    phase: RecipeCheckPhase,
    checks: &mut Vec<RecipeCheckResult>,
) -> Result<bool> {
    let mut passed = true;
    for (name, command) in [
        ("build", options.build_command.as_str()),
        ("test", options.test_command.as_str()),
    ] {
        let started = Instant::now();
        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(root)
            .env("DESLOP_VALIDATION_PHASE", phase.environment_value())
            .output()
            .with_context(|| format!("failed to run recipe {name} command `{command}`"))?;
        let command_passed = output.status.success();
        passed &= command_passed;
        let stderr = String::from_utf8_lossy(&output.stderr);
        checks.push(RecipeCheckResult {
            phase,
            name: name.to_string(),
            command: Some(command.to_string()),
            passed: command_passed,
            elapsed_millis: started.elapsed().as_millis(),
            detail: if command_passed {
                "command passed".to_string()
            } else {
                format!("status {}: {}", output.status, stderr.trim())
            },
        });
        if !command_passed {
            break;
        }
    }
    Ok(passed)
}

fn write_source_map(root: &Path, sources: &BTreeMap<PathBuf, String>, backup: bool) -> Result<()> {
    for (path, text) in sources {
        let physical = root.join(path);
        let original = fs::read_to_string(&physical)?;
        if original != *text {
            super::write_replacement_file(&physical, &original, text.clone(), backup)?;
        }
    }
    Ok(())
}

fn write_exact_source_map(
    root: &Path,
    expected: &BTreeMap<PathBuf, String>,
    replacements: &BTreeMap<PathBuf, String>,
    backup: bool,
) -> Result<()> {
    let expected_bytes = expected
        .iter()
        .map(|(path, text)| (path.clone(), text.as_bytes().to_vec()))
        .collect::<BTreeMap<_, _>>();
    let replacement_bytes = replacements
        .iter()
        .map(|(path, text)| (path.clone(), text.as_bytes().to_vec()))
        .collect::<BTreeMap<_, _>>();
    let changed = expected_bytes
        .iter()
        .filter(|(path, bytes)| replacement_bytes.get(*path) != Some(*bytes))
        .map(|(path, bytes)| (path.clone(), bytes.clone()))
        .collect::<BTreeMap<_, _>>();
    if changed.is_empty() {
        return Ok(());
    }
    let changed_replacements = changed
        .keys()
        .map(|path| (path.clone(), replacement_bytes[path].clone()))
        .collect::<BTreeMap<_, _>>();
    super::commit_atomic_sources(
        root,
        Path::new(".deslop/undo"),
        &changed,
        &changed_replacements,
    )?;
    if backup {
        for (path, bytes) in changed {
            fs::write(
                PathBuf::from(format!("{}.deslop.bak", root.join(path).display())),
                bytes,
            )?;
        }
    }
    Ok(())
}

fn restore_sources(root: &Path, originals: &BTreeMap<PathBuf, String>) -> Result<()> {
    let current = originals
        .keys()
        .map(|path| Ok((path.clone(), fs::read(root.join(path))?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let replacements = originals
        .iter()
        .map(|(path, text)| (path.clone(), text.as_bytes().to_vec()))
        .collect::<BTreeMap<_, _>>();
    if current == replacements {
        return Ok(());
    }
    super::commit_atomic_sources(root, Path::new(".deslop/undo"), &current, &replacements)?;
    Ok(())
}

fn sources_equal(root: &Path, expected: &BTreeMap<PathBuf, String>) -> Result<bool> {
    for (path, text) in expected {
        if fs::read_to_string(root.join(path))? != *text {
            return Ok(false);
        }
    }
    Ok(true)
}

fn snapshot_protected_files(
    root: &Path,
    declared: &[PathBuf],
) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let declared = declared.iter().collect::<BTreeSet<_>>();
    let mut snapshot = BTreeMap::new();
    for entry in ignore::WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(name.as_ref(), ".deslop" | ".git" | ".jj" | "target")
        })
        .build()
    {
        let entry = entry?;
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let relative = entry.path().strip_prefix(root)?.to_path_buf();
        if declared.contains(&relative) || relative.to_string_lossy().ends_with(".deslop.bak") {
            continue;
        }
        snapshot.insert(relative, fs::read(entry.path())?);
    }
    Ok(snapshot)
}

fn protected_files_equal(
    root: &Path,
    declared: &[PathBuf],
    expected: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<bool> {
    for (path, bytes) in expected {
        if fs::read(root.join(path)).ok().as_deref() != Some(bytes.as_slice()) {
            return Ok(false);
        }
    }
    let current = snapshot_protected_files(root, declared)?;
    Ok(current == *expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_protocol::recipe_work_orders;

    fn fixture() -> (TempDir, Vec<RecipeWorkOrder>) {
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("fixture.rs"), "fn run() { return; 1; }\n").unwrap();
        fs::write(root.path().join("protected.txt"), "retained\n").unwrap();
        let candidates = detect_rust_recipes(root.path(), &[PathBuf::from("fixture.rs")]).unwrap();
        let orders = recipe_work_orders(candidates).unwrap();
        (root, orders)
    }

    fn options(root: &Path, test_command: &str) -> RecipeApplyOptions {
        RecipeApplyOptions {
            root: root.to_path_buf(),
            build_command: "true".to_string(),
            test_command: test_command.to_string(),
            backup: false,
            explicit_canary: true,
        }
    }

    #[test]
    fn guarded_recipe_apply_validates_staged_and_live_state() {
        let (root, orders) = fixture();
        let report = apply_recipe_work_orders(&orders, &options(root.path(), "true")).unwrap();
        assert_eq!(report.status, RecipeApplyStatus::Applied);
        assert_eq!(report.disk_revision, "candidate-applied");
        assert_eq!(report.live_revision, "rebuilt-and-tested");
        assert!(report.protected_resources_unchanged);
        assert!(
            !fs::read_to_string(root.path().join("fixture.rs"))
                .unwrap()
                .contains("1;")
        );
    }

    #[test]
    fn live_failure_rolls_back_exact_bytes_and_revalidates() {
        let (root, orders) = fixture();
        let original = fs::read(root.path().join("fixture.rs")).unwrap();
        let command = "test \"$DESLOP_VALIDATION_PHASE\" != live";
        let report = apply_recipe_work_orders(&orders, &options(root.path(), command)).unwrap();
        assert_eq!(report.status, RecipeApplyStatus::RolledBack);
        assert!(report.rollback_verified);
        assert_eq!(fs::read(root.path().join("fixture.rs")).unwrap(), original);
        assert_eq!(report.live_revision, "original-rebuilt-and-tested");
    }

    #[test]
    fn immediate_guard_recheck_rejects_a_race_without_overwriting_it() {
        let (root, orders) = fixture();
        let path = root.path().join("fixture.rs");
        let report =
            apply_recipe_work_orders_with_hook(&orders, &options(root.path(), "true"), || {
                fs::write(&path, "fn run() { return; 2; }\n")?;
                Ok(())
            })
            .unwrap();
        assert_eq!(report.status, RecipeApplyStatus::Rejected);
        assert!(fs::read_to_string(path).unwrap().contains("2;"));
    }

    #[test]
    fn duplicate_and_mutated_work_orders_never_reach_a_write() {
        let (root, orders) = fixture();
        assert!(
            apply_recipe_work_orders(
                &[orders[0].clone(), orders[0].clone()],
                &options(root.path(), "true")
            )
            .is_err()
        );

        let mut value = serde_json::to_value(&orders[0]).unwrap();
        value["target_path"] = serde_json::json!("foreign.rs");
        assert!(serde_json::from_value::<RecipeWorkOrder>(value).is_err());
    }

    #[test]
    fn automatic_application_is_disabled_without_explicit_canary_authority() {
        let (root, orders) = fixture();
        let mut options = options(root.path(), "true");
        options.explicit_canary = false;
        let report = apply_recipe_work_orders(&orders, &options).unwrap();
        assert_eq!(report.status, RecipeApplyStatus::Rejected);
        assert!(report.reasons[0].contains("automatic recipe application is disabled"));
        assert!(
            fs::read_to_string(root.path().join("fixture.rs"))
                .unwrap()
                .contains("1;")
        );
    }
}
