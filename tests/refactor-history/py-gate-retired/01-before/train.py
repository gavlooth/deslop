def step(trainer):
    return gate_scalar_update(trainer)


def release_check(model):
    value = read_gate(model)
    assert value > 0


def read_gate(model):
    return gate_scalar_update(model)
