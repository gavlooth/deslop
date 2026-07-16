# M8 calibration pilot protocol

Version: deslop.m8-pilot/1

This protocol freezes the evidence-only M8 pilot before evaluation. It does not define a
readability label and is not training data for a shipped model.

## Published preference import

Source: Themis-CodePreference, revision
7c366b23590cc9ff8d372bb47280fcd474536344, Apache-2.0. The exact imported artifact is
data/train-00000-of-00024.parquet, SHA-256
8ea455812b1e8470934fa6f52005a3cb46673ba01c57cda8fd70ac6591aba414.

Selection is deterministic: retain rows whose aspect is Readability and Maintainability,
source is COMMITPREFS_READABILITY, and both chosen and rejected strings contain at most
2,500 Unicode scalar values; sort by source row idx; take the first quota for each language:
JavaScript 65, Python 65, Ruby 65, Java 51, C# 24, Go 20, C++ 5, and C 5. The resulting 300
pairs use the low bit of the BLAKE3 row-id digest to swap chosen/rejected display sides, preventing
position from revealing the label, and are blinded to row-level authorship. They remain broad readability-and-maintainability
preferences derived from merged pre-2019 commits, classifier selection, and multi-model
consensus—not direct human perceived-readability ratings. Project identity is unavailable in
the published row schema and must remain the explicit unknown-project stratum.

The first 160 selected human-commit pairs form cleanup tasks. Forty project-owned cleanup pairs
are produced in the controlled LLM-assisted calibration pass, and forty project-owned unsafe
near-misses cover literal, operator, semantic, and public-API changes. The shared row schema
intentionally contains no human/LLM authorship field.

## Published comprehension import

Source: the CC-BY-4.0 figure-data archive for Bergum et al., “Fixation-related potentials
reveal that confusing program code elicits a late frontal positivity,” Zenodo record
10.5281/zenodo.14229849, archive SHA-256
8c986528fd24f7622ba02ed2964d8828037c44322f37265360d6faecff1c51d1.

Import Data Table1 behavioral data.csv and aggregate only by SnippetCondition. Retain sample
count, arithmetic mean ComprehensionTime converted from seconds to milliseconds, and
arithmetic mean binary AnswerCorrectness. The data contain 1,727 Java callable observations
from 24 participants: 863 ambiguous and 864 unambiguous trials. This is timed/correct
comprehension evidence for Java atoms of confusion, not a general readability label.

## Frozen evaluation

Each candidate is reduced once to a content-addressed feature vector. The same capture feeds
the portable challenger, size baseline, NLOC/complexity baseline, lexical baseline, eight
leave-one-axis-out ablations, size-controlled subset, project/language holdouts, Wilson 95%
accuracy intervals, Brier score, and ten-bin expected calibration error.

The portable model gate is the policy recorded in .agents/PLAN.md. The expected pilot decision
is evidence-only because the preference target is broader than perceived readability, four
axes are unavailable in the published rows, and Themis does not expose project identity. Those
are measured decision inputs, not exceptions to the gate.
