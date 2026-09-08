use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use deslop_eval::trajectory::{
    LicenseGate, MinimizationBudget, Snapshot, generate_candidates, import_opencode_export_file,
    import_opencode_export_file_with_root, import_trajectory, replay_trajectory, write_trajectory,
};

fn path(value: Option<std::ffi::OsString>, label: &str) -> Result<PathBuf> {
    value
        .map(PathBuf::from)
        .with_context(|| format!("missing {label}"))
}

fn read_snapshot(path: &Path) -> Result<Snapshot> {
    let bytes = fs::read(path).with_context(|| format!("read snapshot {}", path.display()))?;
    let snapshot: Snapshot = serde_json::from_slice(&bytes).context("parse snapshot JSON")?;
    snapshot.validate("submitted snapshot")?;
    Ok(snapshot)
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let mode = args
        .next()
        .context("usage: trajectory-eval <replay|candidates|adapt-opencode> ...")?;
    match mode.to_string_lossy().as_ref() {
        "replay" => {
            let input = path(args.next(), "trajectory path")?;
            if args.next().is_some() {
                bail!("unexpected replay argument");
            }
            let trajectory = import_trajectory(
                &fs::read(&input).with_context(|| format!("read {}", input.display()))?,
            )?;
            let evidence = replay_trajectory(&trajectory)?;
            println!(
                "trajectory replay OK: {} events, final {}",
                evidence.applied_events.len(),
                evidence.final_hash
            );
        }
        "candidates" => {
            let input = path(args.next(), "trajectory path")?;
            if args.next().is_some() {
                bail!("unexpected candidates argument");
            }
            let trajectory = import_trajectory(
                &fs::read(&input).with_context(|| format!("read {}", input.display()))?,
            )?;
            let candidates = generate_candidates(&trajectory, &MinimizationBudget::default())?;
            println!("{}", serde_json::to_string_pretty(&candidates)?);
        }
        "adapt-opencode" => {
            let input = path(args.next(), "OpenCode export path")?;
            let base_path = path(args.next(), "base snapshot path")?;
            let final_path = path(args.next(), "final snapshot path")?;
            let output = path(args.next(), "trajectory output path")?;
            let spdx = args
                .next()
                .context("missing SPDX license identifier")?
                .to_string_lossy()
                .into_owned();
            let workspace_root = args.next().map(PathBuf::from);
            if args.next().is_some() {
                bail!("unexpected adapt-opencode argument");
            }
            let base = read_snapshot(&base_path)?;
            let final_snapshot = read_snapshot(&final_path)?;
            let license = LicenseGate {
                approved: true,
                spdx: Some(spdx),
                source_uri: Some("opencode export".to_string()),
            };
            let trajectory = if let Some(root) = workspace_root.as_deref() {
                import_opencode_export_file_with_root(&input, base, final_snapshot, license, root)?
            } else {
                import_opencode_export_file(&input, base, final_snapshot, license)?
            };
            write_trajectory(&output, &trajectory)?;
            println!(
                "OpenCode trajectory OK: {} events -> {}",
                trajectory.events.len(),
                output.display()
            );
        }
        other => bail!("unsupported trajectory-eval mode `{other}`"),
    }
    Ok(())
}
