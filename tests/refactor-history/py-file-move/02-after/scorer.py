def decide(model, candidates):
    return max(candidates, key=lambda c: raw_score(model, c))


def public_score(model, candidate):
    return raw_score(model, candidate)
