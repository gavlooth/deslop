use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use deslop_eval::pilot::{PILOT_EVAL_SCHEMA, evaluate_dir, import_bundle};

fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let mode = arguments.next().context(
        "usage: pilot-eval <import --bundle PATH --out DIR|eval --dir DIR --protocol-pin PIN>",
    )?;
    match mode.to_string_lossy().as_ref() {
        "import" => {
            let (bundle, out) = kv2(&mut arguments, "--bundle", "--out")?;
            if arguments.next().is_some() {
                bail!("unexpected extra pilot-eval import arguments");
            }
            let cases = import_bundle(&bundle, &out)?;
            println!(
                "pilot import OK: {} cases -> {}",
                cases.len(),
                out.display()
            );
            Ok(())
        }
        "eval" => {
            let (dir, pin) = kv2(&mut arguments, "--dir", "--protocol-pin")?;
            if arguments.next().is_some() {
                bail!("unexpected extra pilot-eval eval arguments");
            }
            let pin_str = pin.to_string_lossy().into_owned();
            let report = evaluate_dir(&dir, &pin_str)?;
            assert_eq!(report.schema, PILOT_EVAL_SCHEMA);
            println!(
                "pilot eval OK: {} cases, {} sealed excluded -> {}",
                report.cases_total,
                report.sealed_excluded,
                dir.join("report.json").display()
            );
            Ok(())
        }
        other => bail!("unsupported pilot-eval mode {other}"),
    }
}

/// Flag-checked two-value parser: swapped or misspelled flags fail loudly
/// instead of silently swapping paths.
fn kv2(
    arguments: &mut impl Iterator<Item = std::ffi::OsString>,
    first_name: &str,
    second_name: &str,
) -> Result<(PathBuf, PathBuf)> {
    let first_flag = arguments.next().context("missing first flag")?;
    let first_value = arguments.next().context("missing first value")?;
    let second_flag = arguments.next().context("missing second flag")?;
    let second_value = arguments.next().context("missing second value")?;
    if first_flag.to_string_lossy().as_ref() != first_name {
        bail!(
            "expected flag {first_name}, got {}",
            first_flag.to_string_lossy()
        );
    }
    if second_flag.to_string_lossy().as_ref() != second_name {
        bail!(
            "expected flag {second_name}, got {}",
            second_flag.to_string_lossy()
        );
    }
    Ok((PathBuf::from(first_value), PathBuf::from(second_value)))
}
