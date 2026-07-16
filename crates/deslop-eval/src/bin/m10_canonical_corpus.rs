use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use deslop_eval::m10_canonical::{
    assemble_canonical_corpus, verify_canonical_corpus, write_canonical_corpus,
};

fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let mode = arguments
        .next()
        .context("usage: m10-canonical-corpus <assemble|verify> <root> <manifest.json>")?;
    let root = PathBuf::from(arguments.next().context("missing root")?);
    let manifest = PathBuf::from(arguments.next().context("missing manifest")?);
    if arguments.next().is_some() {
        bail!("unexpected extra canonical-corpus arguments");
    }
    match mode.to_string_lossy().as_ref() {
        "assemble" => {
            let corpus = assemble_canonical_corpus(&root)?;
            write_canonical_corpus(&manifest, &corpus)?;
            println!("{}", serde_json::to_string_pretty(&corpus)?);
        }
        "verify" => {
            let corpus = verify_canonical_corpus(&root, &manifest)?;
            println!(
                "verified {} cases in {}",
                corpus.total_cases, corpus.corpus_id
            );
        }
        other => bail!("unsupported canonical-corpus mode {other}"),
    }
    Ok(())
}
