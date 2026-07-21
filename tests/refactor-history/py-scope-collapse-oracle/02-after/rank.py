def rank_documents(docs):
    merged = flatten(docs)
    return score_batch(merged)
