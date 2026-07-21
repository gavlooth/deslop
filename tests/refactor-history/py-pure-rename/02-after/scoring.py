class Scorer:
    def __init__(self, model):
        self.model = model

    def choose(self, candidates):
        return max(candidates, key=lambda c: self.model.raw_score(c))

    def public_score(self, candidate):
        return self.model.raw_score(candidate)


def rank(scorer, candidates):
    chosen = scorer.choose(candidates)
    return chosen, scorer.public_score(chosen)
