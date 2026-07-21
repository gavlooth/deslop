def test_rank_documents():
    assert rank_documents([fixture_doc()]) == [expected_result()]
