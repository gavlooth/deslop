class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        # Decision ownership moved to the commit-time posterior.
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        # Stale: still derived from the former owner.
        return self.model.raw_score(candidate)
