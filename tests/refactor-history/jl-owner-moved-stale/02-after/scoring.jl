# Decision ownership moved to the commit-time posterior.
decide(posterior, candidates) = commit(posterior, candidates)

# Stale: still derived from the former owner.
public_score(model, c) = raw_score(model, c)
