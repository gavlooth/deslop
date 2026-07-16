use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use deslop_protocol::SharedWorkOrder;
use deslop_recipes::ExpectedGraphDelta;
use wait_timeout::ChildExt;

use crate::{
    EvidenceOutcome, GraphReanalysisPhase, NetworkPolicy, VerificationCheck, VerificationEvidence,
    VerificationRuntime, VerifierExecutionPolicy, VerifierFailure, VerifierFailureKind,
    VerifierStage,
};

pub trait GraphDeltaOracle {
    fn observe(
        &mut self,
        root: &Path,
        order: &SharedWorkOrder,
        phase: GraphReanalysisPhase,
    ) -> std::result::Result<ExpectedGraphDelta, VerifierFailure>;
}

pub struct PolicyCommandRuntime<O> {
    oracle: O,
    sandbox_program: PathBuf,
    started: Instant,
}

impl<O> PolicyCommandRuntime<O> {
    pub fn new(oracle: O) -> Self {
        Self {
            oracle,
            sandbox_program: PathBuf::from("bwrap"),
            started: Instant::now(),
        }
    }

    pub fn with_sandbox_program(oracle: O, sandbox_program: PathBuf) -> Self {
        Self {
            oracle,
            sandbox_program,
            started: Instant::now(),
        }
    }
}

impl<O: GraphDeltaOracle> VerificationRuntime for PolicyCommandRuntime<O> {
    fn format(
        &mut self,
        staged_root: &Path,
        order: &SharedWorkOrder,
        check: &VerificationCheck,
        policy: &VerifierExecutionPolicy,
    ) -> std::result::Result<VerificationEvidence, VerifierFailure> {
        self.execute(staged_root, order, check, policy)
    }

    fn reanalyze_graph_delta(
        &mut self,
        root: &Path,
        order: &SharedWorkOrder,
        phase: GraphReanalysisPhase,
        _policy: &VerifierExecutionPolicy,
    ) -> std::result::Result<ExpectedGraphDelta, VerifierFailure> {
        self.oracle.observe(root, order, phase)
    }

    fn run_check(
        &mut self,
        staged_root: &Path,
        order: &SharedWorkOrder,
        check: &VerificationCheck,
        policy: &VerifierExecutionPolicy,
    ) -> std::result::Result<VerificationEvidence, VerifierFailure> {
        self.execute(staged_root, order, check, policy)
    }
}

impl<O> PolicyCommandRuntime<O> {
    fn execute(
        &self,
        staged_root: &Path,
        order: &SharedWorkOrder,
        check: &VerificationCheck,
        policy: &VerifierExecutionPolicy,
    ) -> std::result::Result<VerificationEvidence, VerifierFailure> {
        validate_runtime_policy(policy, check)?;
        let Some(command) = check.command.as_deref() else {
            return Err(failure(
                VerifierFailureKind::InvalidInput,
                Some(check.id.clone()),
                "external verification check has no command",
                false,
            ));
        };
        let before = workspace_metrics(staged_root).map_err(|error| {
            failure(
                VerifierFailureKind::FilesystemViolation,
                Some(check.id.clone()),
                error.to_string(),
                false,
            )
        })?;
        if before.files > policy.maximum_files
            || before.maximum_file_bytes > policy.maximum_file_bytes
        {
            return Err(failure(
                VerifierFailureKind::FilesystemViolation,
                Some(check.id.clone()),
                "staged workspace exceeds verifier file limits before command execution",
                false,
            ));
        }
        let args = sandbox_arguments(staged_root, command, policy).map_err(|detail| {
            failure(
                VerifierFailureKind::PolicyViolation,
                Some(check.id.clone()),
                detail,
                false,
            )
        })?;
        let mut child = Command::new(&self.sandbox_program)
            .args(&args)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                failure(
                    VerifierFailureKind::PolicyViolation,
                    Some(check.id.clone()),
                    format!(
                        "sandbox `{}` is unavailable: {error}",
                        self.sandbox_program.display()
                    ),
                    false,
                )
            })?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let cap = policy.maximum_output_bytes.saturating_add(1);
        let stdout_reader = thread::spawn(move || read_bounded(stdout, cap));
        let stderr_reader = thread::spawn(move || read_bounded(stderr, cap));
        let started = Instant::now();
        let total = Duration::from_millis(policy.maximum_total_millis);
        let remaining = total.saturating_sub(self.started.elapsed());
        if remaining.is_zero() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(failure(
                VerifierFailureKind::Timeout,
                Some(check.id.clone()),
                format!(
                    "verification transaction exceeded {} ms",
                    policy.maximum_total_millis
                ),
                false,
            ));
        }
        let timeout = Duration::from_millis(policy.maximum_command_millis).min(remaining);
        let status = match child.wait_timeout(timeout) {
            Ok(Some(status)) => status,
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(failure(
                    VerifierFailureKind::Timeout,
                    Some(check.id.clone()),
                    format!("command exceeded {} ms", policy.maximum_command_millis),
                    true,
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(failure(
                    VerifierFailureKind::Crash,
                    Some(check.id.clone()),
                    format!("failed waiting for sandbox command: {error}"),
                    true,
                ));
            }
        };
        let stdout = join_reader(stdout_reader, &check.id)?;
        let stderr = join_reader(stderr_reader, &check.id)?;
        if stdout.len().saturating_add(stderr.len()) > policy.maximum_output_bytes {
            return Err(failure(
                VerifierFailureKind::OutputLimit,
                Some(check.id.clone()),
                format!(
                    "command exceeded {} output bytes",
                    policy.maximum_output_bytes
                ),
                false,
            ));
        }
        let after = workspace_metrics(staged_root).map_err(|error| {
            failure(
                VerifierFailureKind::FilesystemViolation,
                Some(check.id.clone()),
                error.to_string(),
                false,
            )
        })?;
        if after.files > policy.maximum_files
            || after.maximum_file_bytes > policy.maximum_file_bytes
        {
            return Err(failure(
                VerifierFailureKind::FilesystemViolation,
                Some(check.id.clone()),
                "command exceeded verifier file limits",
                false,
            ));
        }
        if !status.success() {
            let detail = String::from_utf8_lossy(&stderr);
            let sandbox_denied = detail.contains("Operation not permitted")
                || detail.contains("No permissions to create new namespace");
            return Err(failure(
                if sandbox_denied {
                    VerifierFailureKind::PolicyViolation
                } else {
                    VerifierFailureKind::CommandFailed
                },
                Some(check.id.clone()),
                format!(
                    "command exited {status} after {} ms{}",
                    started.elapsed().as_millis(),
                    if detail.trim().is_empty() {
                        String::new()
                    } else {
                        format!(": {}", detail.trim())
                    }
                ),
                false,
            ));
        }
        VerificationEvidence::new(
            check.id.clone(),
            check.kind.into(),
            order
                .provenance()
                .project_snapshot
                .clone()
                .unwrap_or_else(|| "missing-snapshot".into()),
            output_artifact(&stdout, &stderr),
            EvidenceOutcome::Passed,
            format!(
                "sandboxed command passed in {} ms",
                started.elapsed().as_millis()
            ),
        )
        .map_err(|error| {
            failure(
                VerifierFailureKind::InvalidInput,
                Some(check.id.clone()),
                error.to_string(),
                false,
            )
        })
    }
}

fn validate_runtime_policy(
    policy: &VerifierExecutionPolicy,
    check: &VerificationCheck,
) -> std::result::Result<(), VerifierFailure> {
    if let Err(error) = policy.validate() {
        return Err(failure(
            VerifierFailureKind::PolicyViolation,
            Some(check.id.clone()),
            error.to_string(),
            false,
        ));
    }
    if policy.network == NetworkPolicy::AllowListed {
        return Err(failure(
            VerifierFailureKind::NetworkViolation,
            Some(check.id.clone()),
            "generic command runtime cannot enforce host-level network allowlists",
            false,
        ));
    }
    if policy.readable_roots != [PathBuf::from(".")]
        || policy.writable_roots != [PathBuf::from(".")]
    {
        return Err(failure(
            VerifierFailureKind::FilesystemViolation,
            Some(check.id.clone()),
            "generic command runtime supports only an exact workspace read/write root",
            false,
        ));
    }
    Ok(())
}

fn sandbox_arguments(
    staged_root: &Path,
    command: &str,
    policy: &VerifierExecutionPolicy,
) -> std::result::Result<Vec<String>, String> {
    let root = staged_root
        .canonicalize()
        .map_err(|error| format!("failed to resolve staged root: {error}"))?;
    let root = root
        .to_str()
        .ok_or_else(|| "staged root is not UTF-8".to_string())?;
    let mut args = vec![
        "--die-with-parent".into(),
        "--new-session".into(),
        "--unshare-pid".into(),
        "--unshare-ipc".into(),
        "--unshare-uts".into(),
        "--unshare-net".into(),
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
        "--tmpfs".into(),
        "/tmp".into(),
    ];
    for path in ["/bin", "/lib", "/lib64", "/usr"] {
        if Path::new(path).exists() {
            args.extend(["--ro-bind".into(), path.into(), path.into()]);
        }
    }
    for key in ["CARGO_HOME", "RUSTUP_HOME"] {
        if policy
            .environment_allowlist
            .iter()
            .any(|allowed| allowed == key)
            && let Ok(value) = std::env::var(key)
            && Path::new(&value).exists()
        {
            args.extend(["--ro-bind".into(), value.clone(), value]);
        }
    }
    args.extend([
        "--bind".into(),
        root.into(),
        root.into(),
        "--chdir".into(),
        root.into(),
        "--clearenv".into(),
    ]);
    for key in &policy.environment_allowlist {
        if let Ok(value) = std::env::var(key) {
            args.extend(["--setenv".into(), key.clone(), value]);
        }
    }
    args.extend([
        "--setenv".into(),
        "CARGO_NET_OFFLINE".into(),
        "true".into(),
        "--setenv".into(),
        "DESLOP_NETWORK".into(),
        "denied".into(),
        "--".into(),
        "/bin/sh".into(),
        "-c".into(),
        command.into(),
    ]);
    Ok(args)
}

#[derive(Debug, Clone, Copy)]
struct WorkspaceMetrics {
    files: usize,
    maximum_file_bytes: usize,
}

fn workspace_metrics(root: &Path) -> std::io::Result<WorkspaceMetrics> {
    let mut metrics = WorkspaceMetrics {
        files: 0,
        maximum_file_bytes: 0,
    };
    for entry in ignore::WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(name.as_ref(), ".deslop" | ".git" | ".jj")
        })
        .build()
    {
        let entry = entry.map_err(std::io::Error::other)?;
        if entry.file_type().is_some_and(|kind| kind.is_file()) {
            metrics.files += 1;
            metrics.maximum_file_bytes = metrics
                .maximum_file_bytes
                .max(fs::metadata(entry.path())?.len() as usize);
        }
    }
    Ok(metrics)
}

fn read_bounded(mut reader: impl Read, maximum: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(maximum as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn join_reader(
    handle: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    check: &str,
) -> std::result::Result<Vec<u8>, VerifierFailure> {
    match handle.join() {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(error)) => Err(failure(
            VerifierFailureKind::Crash,
            Some(check.into()),
            format!("failed reading command output: {error}"),
            true,
        )),
        Err(_) => Err(failure(
            VerifierFailureKind::Crash,
            Some(check.into()),
            "command output reader panicked",
            true,
        )),
    }
}

fn output_artifact(stdout: &[u8], stderr: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"deslop verifier command output v1\0");
    hasher.update(&(stdout.len() as u64).to_le_bytes());
    hasher.update(stdout);
    hasher.update(&(stderr.len() as u64).to_le_bytes());
    hasher.update(stderr);
    format!("vo1_{}", hasher.finalize().to_hex())
}

fn failure(
    kind: VerifierFailureKind,
    check: Option<String>,
    detail: impl Into<String>,
    retryable: bool,
) -> VerifierFailure {
    VerifierFailure {
        stage: VerifierStage::Command,
        kind,
        check,
        detail: detail.into(),
        retryable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VerificationCheckKind;

    #[test]
    fn sandbox_arguments_deny_network_clear_environment_and_bind_only_staging_writable() {
        let root = tempfile::TempDir::new().unwrap();
        let policy = VerifierExecutionPolicy::hermetic_workspace();
        let args = sandbox_arguments(root.path(), "true", &policy).unwrap();
        assert!(args.windows(1).any(|arg| arg == ["--unshare-net"]));
        assert!(args.windows(1).any(|arg| arg == ["--clearenv"]));
        let root = root
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(args.windows(3).any(|arg| arg == ["--bind", &root, &root]));
    }

    #[test]
    fn allowlisted_network_and_missing_sandbox_fail_structured() {
        let mut policy = VerifierExecutionPolicy::hermetic_workspace();
        policy.network = NetworkPolicy::AllowListed;
        policy.allowed_network_hosts = vec!["example.com".into()];
        let check = VerificationCheck {
            id: "test".into(),
            kind: VerificationCheckKind::TargetedTest,
            command: Some("true".into()),
            covers: Vec::new(),
            dependencies: Vec::new(),
            authority: Vec::new(),
            always_required: true,
        };
        assert_eq!(
            validate_runtime_policy(&policy, &check).unwrap_err().kind,
            VerifierFailureKind::NetworkViolation
        );
    }
}
