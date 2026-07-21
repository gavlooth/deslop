def decide(model, candidates):
    return raw_score(model, candidates)


def public_score(model, handler_name):
    return getattr(model, handler_name)(raw_score)
