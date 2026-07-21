class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        committed = self.posterior.commit(candidates)
        return committed

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
