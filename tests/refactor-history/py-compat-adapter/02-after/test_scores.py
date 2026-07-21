def test_adapter_matches_new_owner(posterior, model, candidate):
    from scoring import ScoreAdapter

    adapter = ScoreAdapter(model, posterior)
    # Conversion invariant: the compatibility layer is provably identical
    # to the new owner's representation over the scored domain.
    assert adapter.legacy_score(candidate) == posterior.committed_score(candidate)
