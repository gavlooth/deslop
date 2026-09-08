use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
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
        let started = Instant::now();
        let remaining = Duration::from_millis(policy.maximum_total_millis)
            .saturating_sub(self.started.elapsed());
        if remaining.is_zero() {
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
        let (status, stdout, stderr) = run_resource_scoped_command(
            staged_root,
            command,
            &self.sandbox_program,
            policy,
            started + timeout,
        )
        .map_err(|mut error| {
            error.check = Some(check.id.clone());
            error
        })?;
        if !status.success() {
            let detail = String::from_utf8_lossy(&stderr);
            return Err(failure(
                VerifierFailureKind::CommandFailed,
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

pub(crate) fn sandbox_arguments(
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
        "--json-status-fd".into(),
        "3".into(),
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
    args.extend([
        "--bind".into(),
        root.into(),
        root.into(),
        "--chdir".into(),
        root.into(),
        "--clearenv".into(),
    ]);
    for key in &policy.environment_allowlist {
        // An environment allowlist is not permission to mount host homes.
        // Toolchains/caches must be in the supported system or staged roots.
        let value = match key.as_str() {
            "HOME" => Some("/tmp/deslop-home".to_string()),
            "CARGO_HOME" => Some("/tmp/deslop-cargo".to_string()),
            "RUSTUP_HOME" => Some("/tmp/deslop-rustup".to_string()),
            "TMPDIR" => Some("/tmp".to_string()),
            _ => std::env::var(key).ok(),
        };
        if let Some(value) = value {
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
        // Limit workload files, not Bubblewrap's trusted startup-status record.
        "/usr/bin/prlimit".into(),
        format!("--fsize={}", policy.maximum_file_bytes),
        "--".into(),
        "/bin/sh".into(),
        "-c".into(),
        command.into(),
    ]);
    Ok(args)
}

// No untrusted command starts until its own cgroup's kernel limit files and
// writable cgroup.kill have been checked. Keep the gate outside the sandbox.
const SCOPE_GATE: &str = r#"set -eu
/usr/bin/cat /proc/self/cgroup > "$1"
while [ ! -e "$2" ]; do /usr/bin/sleep 0.01; done
exec 3>"$3"
shift 3
exec "$@"
"#;
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

struct ResourceScope {
    unit: String,
    child: Child,
    cgroup: Option<PathBuf>,
    kill: Option<File>,
}

impl ResourceScope {
    fn terminate(&mut self) -> std::io::Result<()> {
        use std::io::Write;

        let deadline = Instant::now() + CLEANUP_TIMEOUT;
        let mut cleanup_error = None;
        // cgroup.kill atomically kills the subtree, including descendants that
        // changed process groups or retained output handles. Collected, empty
        // scopes need no kill (their open controller descriptor may be stale).
        let killed = match (&self.cgroup, &mut self.kill) {
            (Some(cgroup), Some(kill)) => match cgroup_populated(cgroup) {
                Ok(false) => true,
                Ok(true) => match kill.write_all(b"1") {
                    Ok(()) => true,
                    Err(error) => {
                        cleanup_error = Some(error);
                        false
                    }
                },
                Err(error) => {
                    cleanup_error = Some(error);
                    false
                }
            },
            _ => false,
        };
        if !killed {
            // Before admission only the trusted gate can run. Kill the
            // launcher before stopping its uniquely named scope; never release
            // the gate when controller verification or startup failed.
            let _ = self.child.kill();
            let stop = manager_command("/usr/bin/systemctl")
                .args(["--user", "--no-ask-password", "stop", &self.unit])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            match stop {
                Ok(mut stop) => {
                    match stop.wait_timeout(deadline.saturating_duration_since(Instant::now())) {
                        Ok(Some(status)) if status.success() => cleanup_error = None,
                        Ok(Some(_)) if self.cgroup.is_none() => {}
                        Ok(Some(_)) => {
                            cleanup_error =
                                Some(std::io::Error::other("failed stopping the verifier scope"))
                        }
                        result => {
                            let _ = stop.kill();
                            let _ = stop
                                .wait_timeout(deadline.saturating_duration_since(Instant::now()));
                            cleanup_error = Some(result.err().unwrap_or_else(|| {
                                std::io::Error::new(
                                    std::io::ErrorKind::TimedOut,
                                    "timed out stopping the verifier scope",
                                )
                            }));
                        }
                    }
                }
                Err(error) => cleanup_error = Some(error),
            }
        }
        let _ = self.child.kill();
        if self
            .child
            .wait_timeout(deadline.saturating_duration_since(Instant::now()))?
            .is_none()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out reaping the verifier launcher",
            ));
        }
        if let Some(cgroup) = &self.cgroup {
            while cgroup_populated(cgroup)? {
                if Instant::now() >= deadline {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "verifier cgroup remained populated after termination",
                    ));
                }
                thread::sleep(Duration::from_millis(5));
            }
            // Empty kernel cgroup state is authoritative even if a manager
            // request failed because --collect already removed the unit.
            cleanup_error = None;
        }
        match cleanup_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn cgroup_populated(cgroup: &Path) -> std::io::Result<bool> {
    match fs::read_to_string(cgroup.join("cgroup.events")) {
        Ok(events) if events.lines().any(|line| line == "populated 0") => Ok(false),
        Ok(events) if events.lines().any(|line| line == "populated 1") => Ok(true),
        Ok(_) => Err(std::io::Error::other("missing cgroup population state")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn manager_command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.env_clear().env("PATH", "/usr/bin:/bin");
    for key in ["XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
}

fn resource_scoped_command(
    sandbox: &Path,
    args: &[String],
    policy: &VerifierExecutionPolicy,
    unit: &str,
    control: &Path,
    timeout: Duration,
) -> Command {
    let mut command = manager_command("/usr/bin/systemd-run");
    command
        .args([
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            "--no-ask-password",
            "--expand-environment=no",
        ])
        .arg(format!("--unit={unit}"))
        .arg(format!(
            "--property=MemoryMax={}",
            policy.maximum_memory_bytes
        ))
        .arg("--property=MemorySwapMax=0")
        .arg(format!("--property=TasksMax={}", policy.maximum_processes))
        .arg(format!(
            "--property=RuntimeMaxSec={}ms",
            timeout.as_nanos().div_ceil(1_000_000)
        ))
        .args([
            "--property=KillMode=control-group",
            "--property=KillSignal=SIGKILL",
            "--property=TimeoutStopSec=1s",
            "--",
            "/bin/sh",
            "-c",
            SCOPE_GATE,
            "deslop-scope-gate",
        ])
        .arg(control.join("cgroup"))
        .arg(control.join("release"))
        .arg(control.join("status"))
        .arg(sandbox)
        .args(args);
    command
}

fn checked_cgroup(membership: &str, unit: &str) -> std::io::Result<PathBuf> {
    let relative = membership
        .trim()
        .strip_prefix("0::/")
        .ok_or_else(|| std::io::Error::other("a unified cgroup v2 hierarchy is required"))?;
    let relative = Path::new(relative);
    if relative.file_name().and_then(|name| name.to_str()) != Some(unit)
        || !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(std::io::Error::other(
            "launcher did not enter its unique verifier scope",
        ));
    }
    Ok(Path::new("/sys/fs/cgroup").join(relative))
}

fn enforce_cgroup_limits(cgroup: &Path, policy: &VerifierExecutionPolicy) -> std::io::Result<()> {
    for (property, bound) in [
        ("memory.max", policy.maximum_memory_bytes),
        ("memory.swap.max", 0),
        ("pids.max", u64::from(policy.maximum_processes)),
    ] {
        let value = fs::read_to_string(cgroup.join(property))?;
        let enforced = value.trim().parse::<u64>().map_err(|_| {
            std::io::Error::other(format!("verifier controller {property} has no hard limit"))
        })?;
        if enforced > bound {
            return Err(std::io::Error::other(format!(
                "verifier controller {property} exceeds its required hard limit"
            )));
        }
    }
    Ok(())
}

fn run_resource_scoped_command(
    staged_root: &Path,
    command: &str,
    sandbox: &Path,
    policy: &VerifierExecutionPolicy,
    deadline: Instant,
) -> std::result::Result<(ExitStatus, Vec<u8>, Vec<u8>), VerifierFailure> {
    let policy_error = |error| failure(VerifierFailureKind::PolicyViolation, None, error, false);
    let io_error =
        |error: std::io::Error| failure(VerifierFailureKind::Crash, None, error.to_string(), true);
    let check_workspace = || {
        let metrics = workspace_metrics(staged_root).map_err(|error| {
            failure(
                VerifierFailureKind::FilesystemViolation,
                None,
                error.to_string(),
                false,
            )
        })?;
        if metrics.files > policy.maximum_files
            || metrics.maximum_file_bytes > policy.maximum_file_bytes
        {
            return Err(failure(
                VerifierFailureKind::FilesystemViolation,
                None,
                "sandbox workspace exceeded policy resource budget",
                false,
            ));
        }
        Ok(())
    };
    check_workspace()?;
    let args = sandbox_arguments(staged_root, command, policy).map_err(policy_error)?;
    let control = tempfile::Builder::new()
        .prefix("deslop-verify-")
        .tempdir()
        .map_err(io_error)?;
    let unit = format!(
        "{}.scope",
        control.path().file_name().unwrap().to_string_lossy()
    );
    // Regular files cannot keep a reader blocked after a failed command. They
    // are outside the mounted workspace; output is polled and read only to cap.
    let mut stdout = tempfile::tempfile().map_err(io_error)?;
    let mut stderr = tempfile::tempfile().map_err(io_error)?;
    let timeout = deadline.saturating_duration_since(Instant::now());
    if timeout.is_zero() {
        return Err(failure(
            VerifierFailureKind::Timeout,
            None,
            "sandbox command deadline elapsed before launch",
            true,
        ));
    }
    let child = resource_scoped_command(sandbox, &args, policy, &unit, control.path(), timeout)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().map_err(io_error)?)
        .stderr(stderr.try_clone().map_err(io_error)?)
        .spawn()
        .map_err(|error| policy_error(format!("verifier scope backend is unavailable: {error}")))?;
    let mut scope = ResourceScope {
        unit,
        child,
        cgroup: None,
        kill: None,
    };
    let outcome = (|| {
        loop {
            if Instant::now() >= deadline {
                return Err(failure(
                    VerifierFailureKind::Timeout,
                    None,
                    "sandbox command exceeded policy timeout",
                    true,
                ));
            }
            if scope.cgroup.is_none() {
                match fs::read_to_string(control.path().join("cgroup")) {
                    Ok(membership) if membership.ends_with('\n') => {
                        let cgroup = checked_cgroup(&membership, &scope.unit).map_err(|error| {
                            policy_error(format!("verifier scope identity is unavailable: {error}"))
                        })?;
                        scope.cgroup = Some(cgroup);
                        let cgroup = scope.cgroup.as_ref().expect("verified scope cgroup");
                        scope.kill = Some(
                            OpenOptions::new()
                                .write(true)
                                .open(cgroup.join("cgroup.kill"))
                                .map_err(|error| {
                                    policy_error(format!(
                                        "verifier scope termination is unavailable: {error}"
                                    ))
                                })?,
                        );
                        enforce_cgroup_limits(cgroup, policy).map_err(|error| {
                            policy_error(format!(
                                "verifier resource enforcement is unavailable: {error}"
                            ))
                        })?;
                        fs::write(control.path().join("release"), b"1").map_err(io_error)?;
                    }
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                        return Err(io_error(error));
                    }
                    _ => {}
                }
            }
            check_workspace()?;
            if stdout
                .metadata()
                .map_err(io_error)?
                .len()
                .saturating_add(stderr.metadata().map_err(io_error)?.len())
                > policy.maximum_output_bytes as u64
            {
                return Err(failure(
                    VerifierFailureKind::OutputLimit,
                    None,
                    "sandbox command exceeded output limit",
                    false,
                ));
            }
            if let Some(status) = scope.child.try_wait().map_err(io_error)? {
                let started = File::open(control.path().join("status"))
                    .and_then(|file| read_bounded(file, 4096))
                    .is_ok_and(|bytes| {
                        bytes.split(|byte| *byte == b'\n').any(|line| {
                            serde_json::from_slice::<serde_json::Value>(line)
                                .ok()
                                .and_then(|value| value["child-pid"].as_u64())
                                .is_some_and(|pid| pid > 0)
                        })
                    });
                if !started {
                    stderr.rewind().map_err(io_error)?;
                    let diagnostic =
                        read_bounded(&mut stderr, policy.maximum_output_bytes).map_err(io_error)?;
                    return Err(policy_error(format!(
                        "verifier sandbox backend did not start the command: {}",
                        String::from_utf8_lossy(&diagnostic).trim(),
                    )));
                }
                return Ok(status);
            }
            thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(10)),
            );
        }
    })();
    // Cleanup also runs for successful launchers: background children are not
    // allowed to outlive evidence production. RuntimeMaxSec independently
    // bounds the scope if the manager or this verifier becomes unavailable.
    let cleanup = scope.terminate();
    if let Err(error) = cleanup {
        return Err(match outcome {
            Err(mut primary) => {
                primary
                    .detail
                    .push_str(&format!("; verifier scope cleanup failed: {error}"));
                primary
            }
            Ok(_) => io_error(error),
        });
    }
    let status = outcome?;
    if Instant::now() >= deadline {
        return Err(failure(
            VerifierFailureKind::Timeout,
            None,
            "sandbox scope exceeded policy timeout during cleanup",
            true,
        ));
    }
    check_workspace()?;
    stdout.rewind().map_err(io_error)?;
    stderr.rewind().map_err(io_error)?;
    let cap = policy.maximum_output_bytes.saturating_add(1);
    let stdout = read_bounded(stdout, cap).map_err(io_error)?;
    let stderr = read_bounded(stderr, cap).map_err(io_error)?;
    if stdout.len().saturating_add(stderr.len()) > policy.maximum_output_bytes {
        return Err(failure(
            VerifierFailureKind::OutputLimit,
            None,
            "sandbox command exceeded output limit",
            false,
        ));
    }
    Ok((status, stdout, stderr))
}

pub(crate) fn run_bounded_sandbox_command(
    staged_root: &Path,
    command: &str,
    policy: &VerifierExecutionPolicy,
) -> std::io::Result<(ExitStatus, Vec<u8>, Vec<u8>)> {
    policy.validate().map_err(std::io::Error::other)?;
    let timeout = Duration::from_millis(
        policy
            .maximum_command_millis
            .min(policy.maximum_total_millis),
    );
    run_resource_scoped_command(
        staged_root,
        command,
        Path::new("/usr/bin/bwrap"),
        policy,
        Instant::now() + timeout,
    )
    .map_err(|error| {
        let kind = match error.kind {
            VerifierFailureKind::Timeout => std::io::ErrorKind::TimedOut,
            VerifierFailureKind::FilesystemViolation | VerifierFailureKind::OutputLimit => {
                std::io::ErrorKind::FileTooLarge
            }
            VerifierFailureKind::PolicyViolation => std::io::ErrorKind::PermissionDenied,
            _ => std::io::ErrorKind::Other,
        };
        std::io::Error::new(kind, error.detail)
    })
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
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(name.as_ref(), ".git" | ".jj")
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

    fn available_sandbox(root: &Path, policy: &VerifierExecutionPolicy) -> bool {
        match run_bounded_sandbox_command(root, "printf admitted", policy) {
            Ok((status, stdout, _)) => {
                assert!(status.success());
                assert_eq!(stdout, b"admitted");
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!(
                    "sandbox enforcement unavailable; live regression not exercised: {error}"
                );
                false
            }
            Err(error) => panic!("sandbox preflight unexpectedly failed: {error}"),
        }
    }

    #[test]
    fn sandbox_preflight_requires_kernel_memory_and_task_limits() {
        let root = tempfile::tempdir().unwrap();
        let policy = VerifierExecutionPolicy::hermetic_workspace();
        fs::write(
            root.path().join("memory.max"),
            policy.maximum_memory_bytes.to_string(),
        )
        .unwrap();
        fs::write(root.path().join("memory.swap.max"), "0").unwrap();
        fs::write(
            root.path().join("pids.max"),
            policy.maximum_processes.to_string(),
        )
        .unwrap();
        enforce_cgroup_limits(root.path(), &policy).unwrap();

        fs::write(root.path().join("memory.max"), "max").unwrap();
        assert!(enforce_cgroup_limits(root.path(), &policy).is_err());
        fs::write(
            root.path().join("memory.max"),
            policy.maximum_memory_bytes.to_string(),
        )
        .unwrap();
        fs::write(
            root.path().join("pids.max"),
            (policy.maximum_processes + 1).to_string(),
        )
        .unwrap();
        assert!(enforce_cgroup_limits(root.path(), &policy).is_err());
        fs::remove_file(root.path().join("pids.max")).unwrap();
        assert!(enforce_cgroup_limits(root.path(), &policy).is_err());
    }

    #[test]
    fn sandbox_live_preflight_verifies_hard_limits_without_resource_stress() {
        let root = tempfile::tempdir().unwrap();
        let mut policy = VerifierExecutionPolicy::hermetic_workspace();
        policy.maximum_memory_bytes = 64 * 1024 * 1024;
        policy.maximum_processes = 16;
        policy.maximum_command_millis = 2_000;
        // Admission reads the actual scope's memory.max, memory.swap.max and
        // pids.max, not just the manager's requested configuration properties.
        available_sandbox(root.path(), &policy);
    }

    #[test]
    fn sandbox_timeout_terminates_descendants_and_returns_without_reader_joins() {
        let root = tempfile::tempdir().unwrap();
        let mut policy = VerifierExecutionPolicy::hermetic_workspace();
        policy.maximum_command_millis = 2_000;
        if !available_sandbox(root.path(), &policy) {
            return;
        }
        policy.maximum_command_millis = 500;
        let started = Instant::now();
        let result = run_bounded_sandbox_command(
            root.path(),
            "/bin/sh -c 'trap \"\" TERM; while :; do printf x >> heartbeat; sleep 0.02; done' & wait",
            &policy,
        );
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(3));
        let heartbeat = root.path().join("heartbeat");
        let stopped_size = fs::metadata(&heartbeat)
            .expect("descendant ran before deadline")
            .len();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(
            fs::metadata(heartbeat).unwrap().len(),
            stopped_size,
            "descendant remained live after timeout"
        );
    }

    #[test]
    fn sandbox_filesystem_failure_terminates_descendants() {
        let root = tempfile::tempdir().unwrap();
        let mut policy = VerifierExecutionPolicy::hermetic_workspace();
        policy.maximum_command_millis = 2_000;
        if !available_sandbox(root.path(), &policy) {
            return;
        }
        policy.maximum_files = 1;
        let result = run_bounded_sandbox_command(
            root.path(),
            "/bin/sh -c 'trap \"\" TERM; while :; do printf x >> heartbeat; sleep 0.02; done' & while [ ! -e heartbeat ]; do sleep 0.01; done; touch overflow; wait",
            &policy,
        );
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::FileTooLarge);
        let heartbeat = root.path().join("heartbeat");
        let stopped_size = fs::metadata(&heartbeat).unwrap().len();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(
            fs::metadata(heartbeat).unwrap().len(),
            stopped_size,
            "descendant remained live after filesystem failure"
        );
    }

    #[test]
    fn unavailable_sandbox_fails_closed_before_workload_execution() {
        let root = tempfile::tempdir().unwrap();
        let policy = VerifierExecutionPolicy::hermetic_workspace();
        let error = run_resource_scoped_command(
            root.path(),
            "touch workload-ran",
            &root.path().join("missing-bwrap"),
            &policy,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap_err();
        assert_eq!(error.kind, VerifierFailureKind::PolicyViolation);
        assert!(!root.path().join("workload-ran").exists());
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

    #[test]
    fn sandbox_rejects_completed_command_exceeding_file_budget() {
        let root = tempfile::tempdir().unwrap();
        let mut policy = VerifierExecutionPolicy::hermetic_workspace();
        policy.maximum_command_millis = 2_000;
        if !available_sandbox(root.path(), &policy) {
            return;
        }
        policy.maximum_file_bytes = 1;
        let result = run_bounded_sandbox_command(root.path(), "printf ok > result.txt", &policy);
        match result {
            Ok((status, _, _)) => assert!(
                !status.success(),
                "a successful command exceeded its file budget"
            ),
            Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::FileTooLarge),
        }
        if root.path().join("result.txt").exists() {
            assert!(fs::metadata(root.path().join("result.txt")).unwrap().len() <= 1);
        }
    }
}
