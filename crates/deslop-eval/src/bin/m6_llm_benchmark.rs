use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use deslop_eval::m6_benchmark::{score_batch, verify_report_assets, write_batch_assets};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("generate") => {
            let manifest = path(&mut args, "manifest path")?;
            let requests = path(&mut args, "requests path")?;
            no_more(args)?;
            write_batch_assets(&manifest, &requests)
        }
        Some("score") => {
            let manifest = path(&mut args, "manifest path")?;
            let output = path(&mut args, "batch output path")?;
            let report = path(&mut args, "report path")?;
            no_more(args)?;
            let scored = score_batch(&manifest, &output)?;
            fs::write(&report, serde_json::to_vec_pretty(&scored)?)
                .with_context(|| format!("failed to write {}", report.display()))?;
            println!("{}", serde_json::to_string_pretty(&scored)?);
            if !scored.passed {
                bail!("M6 LLM benchmark gates failed");
            }
            Ok(())
        }
        Some("verify") => {
            let manifest = path(&mut args, "manifest path")?;
            let report = path(&mut args, "report path")?;
            no_more(args)?;
            let verified = verify_report_assets(&manifest, &report)?;
            println!(
                "verified {} paired tasks; accepted-patch delta {:.2} percentage points",
                verified.paired_tasks,
                verified.accepted_patch_delta * 100.0
            );
            Ok(())
        }
        _ => bail!(
            "usage: m6-llm-benchmark generate MANIFEST REQUESTS | score MANIFEST BATCH_OUTPUT REPORT | verify MANIFEST REPORT"
        ),
    }
}

fn path(args: &mut impl Iterator<Item = String>, label: &str) -> Result<PathBuf> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("missing {label}"))
}

fn no_more(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("unexpected extra arguments");
    }
    Ok(())
}
