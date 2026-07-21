def test_rank_documents_partition_independence():
    fixed = fixture_doc()
    alone = rank_documents([fixed])
    packed = rank_documents([fixed, companion_doc()])
    assert alone[0] == packed[0]
