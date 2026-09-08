# M8 migration: deslop.metrics/5 to /6

deslop.metrics/6 preserves the /5 heuristic burden fields and adds:

- feature_schema describing exclusive locality and aggregation;
- functions[].features, a content-addressed deslop.readability-features/1 vector;
- eight named axes containing measured values plus estimator/sample metadata or explicit
  unknown reasons;
- readability_calibration with the frozen capture, disposition, evidence path, and label
  permission.

Clients must negotiate or accept /6 before reading the new fields. They must not:

- interpret missing axis measurements as zero;
- recompute entropy by averaging child entropies;
- treat syntax-fallback cyclomatic complexity as CFG authority;
- synthesize readability_score, health_score, or refactor confidence from the axes;
- use any M8 field to authorize a rewrite.

The current disposition is evidence_only. A UI may rank or display transparent evidence but must
not display a readability label or probability.

## Metric reference and missing-data rules

The `/7` projection retains `/6` feature semantics while making measurement boundaries explicit.
Structural mass is `cyclomatic * sqrt(nloc)`, complexity mass is `ln(1 + structural_mass)`, and
entropy is base-2 Shannon plug-in entropy normalized by `log2` of the observed positive-count
support (`0` for zero/one-category support). Counts are exclusive owned-region counts; ratios
are dimensionless, and entropy is recomputed from pooled counts rather than averaged from child
values. Exact CFG values require complete non-recovered exact edges; syntax branch counts are a
fallback with an unknown reason, not CFG evidence.

The leave-region-out bigram estimator reports bits/token only when the region has observations and
same-language peer tokens/vocabulary. Tiny, unsupported, malformed, or unavailable inputs retain
unknown evidence and are never silently represented as confident zero. Unicode identifiers are
tokenized as identifier units by the lightweight lexical fallback; quoted comment delimiters do
not terminate code tokenization. Byte entropy is bits per byte. Feature IDs remain intentionally
content-addressed, so equal vectors deduplicate; occurrence/path counts belong to the caller's
separate occurrence identity.

Mathematical reference regressions use a `1e-12` absolute tolerance for scalar floating-point
formula checks and exact equality for serialized feature/projection identity. No global score is
defined or introduced.
