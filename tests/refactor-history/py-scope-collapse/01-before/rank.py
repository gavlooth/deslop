def rank_documents(docs):
    results = []
    for doc in docs:
        results.append(score_document(doc))
    return results
