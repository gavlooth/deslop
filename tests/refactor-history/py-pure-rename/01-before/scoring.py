class Scorer:
    def __init__(self, model):
        self.model = model

    def decide(self, candidates):
        return max(candidates, key=lambda c: self.model.raw_score(c))

    def public_score(self, candidate):
        return self.model.raw_score(candidate)


def rank(scorer, candidates):
    chosen = scorer.decide(candidates)
    return chosen, scorer.public_score(chosen)
