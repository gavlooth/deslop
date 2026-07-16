use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use deslop_eval::m10_dogfood::{
    assemble_dogfood_report, evaluate_dogfood_partition, verify_dogfood_report,
    write_dogfood_partition, write_dogfood_report,
};

fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let mode = arguments
        .next()
        .context("usage: m10-dogfood <assemble|verify> <root> <report.json>")?;
    match mode.to_string_lossy().as_ref() {
        "assemble" => {
            let root = PathBuf::from(arguments.next().context("missing root")?);
            let report = PathBuf::from(arguments.next().context("missing report path")?);
            if arguments.next().is_some() {
                bail!("unexpected dogfood assemble argument");
            }
            let evidence = assemble_dogfood_report(&root)?;
            write_dogfood_report(&report, &evidence)?;
            println!(
                "dogfood {} findings and {} recipe candidates in {}",
                evidence.findings.len(),
                evidence.recipe_candidates.len(),
                evidence.report_id
            );
        }
        "verify" => {
            let root = PathBuf::from(arguments.next().context("missing root")?);
            let report = PathBuf::from(arguments.next().context("missing report path")?);
            if arguments.next().is_some() {
                bail!("unexpected dogfood verify argument");
            }
            let evidence = verify_dogfood_report(&root, &report)?;
            println!("verified {}", evidence.report_id);
        }
        "internal-partition" => {
            let root = PathBuf::from(arguments.next().context("missing root")?);
            let path = PathBuf::from(arguments.next().context("missing partition path")?);
            let output = PathBuf::from(arguments.next().context("missing partition output")?);
            if arguments.next().is_some() {
                bail!("unexpected dogfood partition argument");
            }
            let evidence = evaluate_dogfood_partition(&root, &path)?;
            write_dogfood_partition(&output, &evidence)?;
        }
        other => bail!("unsupported dogfood mode {other}"),
    }
    Ok(())
}
