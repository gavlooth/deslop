def build_manifest(run):
    return emit({"run_id": run.id, "metric": run.final_loss})


def verify_manifest(manifest):
    required = {"run_id", "metric"}
    return required <= manifest.keys() and emit(manifest)
