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

The current disposition is evidence_only. A UI may rank or display transparent evidence but
must not display a readability label or probability.
