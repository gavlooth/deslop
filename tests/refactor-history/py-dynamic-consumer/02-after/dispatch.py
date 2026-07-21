def decide(model, candidates):
    return posterior_commit(model, candidates)


def public_score(model, handler_name):
    return getattr(model, handler_name)(raw_score)
