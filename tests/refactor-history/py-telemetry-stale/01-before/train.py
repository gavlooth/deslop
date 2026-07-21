def train_step(model):
    return legacy_scalar(model)


def report_health(model):
    metrics.gauge("controller_activity", read_activity(model))


def read_activity(model):
    return legacy_scalar(model)
