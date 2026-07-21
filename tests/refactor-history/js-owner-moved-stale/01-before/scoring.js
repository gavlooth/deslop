class Scorer {
  decide(candidates) {
    return candidates.sort((a, b) => this.model.rawScore(b) - this.model.rawScore(a))[0];
  }

  publicScore(candidate) {
    return this.model.rawScore(candidate);
  }
}
