class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        # Decision ownership moved to the commit-time posterior.
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        # Adoption complete: the exposed score follows the new owner.
        return self.posterior.committed_score(candidate)
