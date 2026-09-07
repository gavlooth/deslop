use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use deslop_eval::research::{
    check_inventory, check_live_metrics_shape, live_metrics_report_json, load_bundled_registry,
    load_registry, render_claims_markdown, render_inventory_markdown,
};

fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let mode = arguments.next().context(
        "usage: research-registry [--registry PATH] <check|inventory|claims> [metrics-dir]",
    )?;
    let (registry_path, mode) = if mode.to_string_lossy().as_ref() == "--registry" {
        let path = PathBuf::from(arguments.next().context("missing path after --registry")?);
        let mode = arguments
            .next()
            .context("missing mode after --registry PATH")?;
        (Some(path), mode)
    } else {
        (None, mode)
    };
    let extra = arguments.next().map(PathBuf::from);
    if arguments.next().is_some() {
        bail!("unexpected extra research-registry arguments");
    }
    let registry = match registry_path {
        Some(path) => load_registry(&path)?,
        None => load_bundled_registry()?,
    };
    match mode.to_string_lossy().as_ref() {
        "check" => {
            let check = check_inventory(&registry)?;
            let live = live_metrics_report_json(extra.as_deref())?;
            let metrics_json = live.metrics_json;
            let slop_json = live.slop_json;
            check_live_metrics_shape(&registry, &metrics_json, slop_json.as_ref())?;
            println!(
                "research registry OK: {} rules, {} recipes, {} metric fields, {} claims (live metrics shape checked)",
                check.rules, check.recipes, check.metric_fields, check.claims
            );
        }
        "inventory" => {
            print!("{}", render_inventory_markdown(&registry));
        }
        "claims" => {
            print!("{}", render_claims_markdown(&registry));
        }
        other => bail!("unsupported research-registry mode {other}"),
    }
    Ok(())
}
