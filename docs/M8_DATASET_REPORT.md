# M8 dataset and collection report

The strict registry is
crates/deslop-eval/evaluation/m8/dataset_registry.json. The frozen normalized corpus is
crates/deslop-eval/evaluation/m8/corpus.json.

## Imported sources

- Themis-CodePreference revision 7c366b23590cc9ff8d372bb47280fcd474536344,
  Apache-2.0. Imported 300 deterministic short rows from the first pinned shard: JavaScript
  65, Python 65, Ruby 65, Java 51, C# 24, Go 20, C++ 5, and C 5. The task is broad
  readability-and-maintainability preference over pre-2019 merged commits selected with
  learned classifiers and multi-model consensus. Published rows omit project identity.
- Bergum et al. AoC-FRP Zenodo 14229849, CC-BY-4.0. Imported two condition aggregates from
  1,727 Java callable trials by 24 participants. Ambiguous: n=863, mean time 12,204.055 ms,
  correctness 0.702202. Unambiguous: n=864, mean time 11,238.246 ms, correctness 0.884259.
  This target is timed/correct atoms-of-confusion comprehension.
- Deslop controlled pilot, MIT. Combined 160 published human-commit cleanup pairs, 40
  LLM-assisted controlled cleanup pairs, and 40 unsafe near-misses, yielding 240 fixed tasks.
  The common row schema carries no authorship field.

## Rejected source

The observed Dorn dataset mirror was not imported because its dataset card provided no
explicit redistribution licence. Public availability was not treated as permission.

## Limits preserved

The preference and comprehension targets are not merged. The corpus cannot support a true
leave-project-out claim because Themis does not publish project identity. It also lacks
surprisal, clone, dependence-cohesion, impact, and verifier evidence for the published
snippets. These facts are decision failures, not imputed values.
