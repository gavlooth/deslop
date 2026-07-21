def build_manifest(run):
    # Producer now nests the metric and records the seed.
    return {
        "run_id": run.id,
        "epochs": run.epochs,
        "seed": run.seed,
        "metrics": {"loss": run.final_loss},
    }


def validate_manifest(manifest):
    # Stale: still requires the retired flat "metric" field and ignores "seed".
    required = {"run_id", "epochs", "metric"}
    return required <= manifest.keys()
