class ScoreAdapter:
    """Deliberate compatibility layer: exposes the former representation
    while the new owner carries the decision. See test_scores.py for the
    invariant that binds the two representations."""

    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def legacy_score(self, candidate):
        return self.posterior.committed_score(candidate)


class Scorer:
    def __init__(self, model, posterior):
        self.posterior = posterior
        self.adapter = ScoreAdapter(model, posterior)

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.adapter.legacy_score(candidate)
