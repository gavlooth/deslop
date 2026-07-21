def build_manifest(run):
    return emit_v2({"run_id": run.id, "metrics": {"loss": run.final_loss}})


def verify_manifest(manifest):
    required = {"run_id", "metric"}
    return required <= manifest.keys() and emit(manifest)
