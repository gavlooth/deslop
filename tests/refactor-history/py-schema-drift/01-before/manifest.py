def build_manifest(run):
    return {"run_id": run.id, "epochs": run.epochs, "metric": run.final_loss}


def validate_manifest(manifest):
    required = {"run_id", "epochs", "metric"}
    return required <= manifest.keys()
