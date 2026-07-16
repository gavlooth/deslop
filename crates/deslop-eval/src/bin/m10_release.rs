use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use deslop_eval::m10_release::{
    assemble_release_evidence, verify_release_evidence, write_release_evidence,
};

fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let mode = arguments
        .next()
        .context("usage: m10-release <assemble|verify> <root> <release.json>")?;
    let root = PathBuf::from(arguments.next().context("missing root")?);
    let output = PathBuf::from(arguments.next().context("missing release evidence path")?);
    if arguments.next().is_some() {
        bail!("unexpected release-evidence argument");
    }
    match mode.to_string_lossy().as_ref() {
        "assemble" => {
            let evidence = assemble_release_evidence(&root)?;
            write_release_evidence(&output, &evidence)?;
            println!("assembled {}", evidence.release_id);
        }
        "verify" => {
            let evidence = verify_release_evidence(&root, &output)?;
            println!("verified {}", evidence.release_id);
        }
        other => bail!("unsupported release-evidence mode {other}"),
    }
    Ok(())
}
