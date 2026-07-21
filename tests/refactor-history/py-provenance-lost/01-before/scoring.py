def decide(model, candidates):
    return best_by_raw_score(model, candidates)


def public_score(model, candidate):
    return best_by_raw_score(model, candidate)


def debug_dump(model, candidate):
    return best_by_raw_score(model, candidate)
