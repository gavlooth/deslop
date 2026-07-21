class Scorer:
    def __init__(self, model, posterior):
        self.model = model
        self.posterior = posterior

    def decide(self, candidates):
        return self.posterior.commit(candidates)

    def public_score(self, candidate):
        return self.model.raw_score(candidate)
