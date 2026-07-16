use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use deslop_eval::m10_external::{
    default_external_manifest, evaluate_external_projects, read_external_report,
    verify_external_checkouts, write_external_manifest, write_external_report,
};

fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let mode = arguments.next().context(
        "usage: m10-external-eval <init|evaluate|verify> <manifest.json> [checkout-root] [report.json]",
    )?;
    let manifest = PathBuf::from(arguments.next().context("missing manifest path")?);
    match mode.to_string_lossy().as_ref() {
        "init" => {
            if arguments.next().is_some() {
                bail!("unexpected external-eval init argument");
            }
            let frozen = default_external_manifest()?;
            write_external_manifest(&manifest, &frozen)?;
            println!("initialized {}", frozen.manifest_id);
        }
        "evaluate" => {
            let checkout_root = PathBuf::from(arguments.next().context("missing checkout root")?);
            let report = PathBuf::from(arguments.next().context("missing report path")?);
            if arguments.next().is_some() {
                bail!("unexpected external-eval evaluate argument");
            }
            let result = evaluate_external_projects(&manifest, &checkout_root, true)?;
            write_external_report(&report, &result)?;
            println!(
                "evaluated {} pinned projects in {}",
                result.projects.len(),
                result.report_id
            );
        }
        "verify" => {
            let checkout_root = PathBuf::from(arguments.next().context("missing checkout root")?);
            let report = PathBuf::from(arguments.next().context("missing report path")?);
            if arguments.next().is_some() {
                bail!("unexpected external-eval verify argument");
            }
            let pinned = verify_external_checkouts(&manifest, &checkout_root)?;
            let result = read_external_report(&report)?;
            result.validate()?;
            if result.manifest_id != pinned.manifest_id {
                bail!("external report does not belong to the pinned manifest");
            }
            println!(
                "verified {} checkouts and {}",
                pinned.projects.len(),
                result.report_id
            );
        }
        other => bail!("unsupported external-eval mode {other}"),
    }
    Ok(())
}
