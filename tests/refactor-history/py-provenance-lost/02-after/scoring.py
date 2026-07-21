def decide(model, candidates):
    return posterior_commit(model, candidates)


def public_score(model, candidate):
    committed = posterior_commit_index(model, candidate)
    return round(reconstruct_score(committed), 3)


def debug_dump(model, candidate):
    return best_by_raw_score(model, candidate)
