use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use deslop_eval::m9_scale::{
    M9ScaleBenchmarkConfig, run_m9_scale_benchmark, write_m9_scale_report,
};

fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let root = PathBuf::from(arguments.next().context(
        "usage: m9-scale-benchmark <repository-root> <empty-cache-dir> <output.json> [iterations] [files-per-project]",
    )?);
    let cache = PathBuf::from(arguments.next().context("missing empty cache directory")?);
    let output = PathBuf::from(arguments.next().context("missing output path")?);
    let iterations = arguments
        .next()
        .map(|value| value.to_string_lossy().parse::<usize>())
        .transpose()
        .context("iterations must be an integer")?
        .unwrap_or(5);
    let files_per_project = arguments
        .next()
        .map(|value| value.to_string_lossy().parse::<usize>())
        .transpose()
        .context("files-per-project must be an integer")?
        .unwrap_or(24);
    if arguments.next().is_some() {
        bail!("unexpected extra benchmark arguments");
    }
    let report = run_m9_scale_benchmark(
        &root,
        &cache,
        M9ScaleBenchmarkConfig {
            iterations,
            files_per_project,
        },
    )?;
    write_m9_scale_report(&output, &report)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    report.validate_terminal()?;
    Ok(())
}
