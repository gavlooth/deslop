decide(model, candidates) = argmax(c -> raw_score(model, c), candidates)

public_score(model, c) = raw_score(model, c)
