from scoring import Scorer


def test_public_score(scorer, candidate):
    assert scorer.public_score(candidate) == model.raw_score(candidate)
