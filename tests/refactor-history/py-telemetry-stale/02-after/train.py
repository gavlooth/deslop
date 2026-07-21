def train_step(model):
    return controller_activity(model)


def report_health(model):
    metrics.gauge("controller_activity", read_activity(model))


def read_activity(model):
    return legacy_scalar(model)
