class Scorer {
  decide(candidates) {
    return this.posterior.commit(candidates);
  }

  publicScore(candidate) {
    return this.model.rawScore(candidate);
  }
}
