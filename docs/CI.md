# CI and Pre-commit

deslop already exposes the CI primitives through the CLI:

- `deslop scan --format sarif` writes SARIF to stdout.
- `deslop scan --changed[=<ref>]` limits scanning to files changed since a Git ref
  (`HEAD` when no ref is supplied).
- `deslop scan --fail-on <severity>` exits non-zero when a finding meets or exceeds
  `info`, `minor`, or `major`.
- `deslop baseline write` creates a baseline, `deslop baseline update` ratchets it to
  the current accepted finding set, and `deslop scan --baseline <file>` suppresses known
  findings so only regressions gate.

## GitHub Actions

This repository includes a reusable composite action at `action.yml`. It installs the local
CLI, optionally writes `deslop.sarif`, then runs the fail-on gate.

```yaml
name: deslop

on:
  pull_request:

permissions:
  contents: read
  security-events: write

jobs:
  scan:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - id: deslop
        uses: ./
        with:
          paths: .
          fail-on: major
          sarif: "true"
          # baseline: deslop-baseline.json
      - uses: github/codeql-action/upload-sarif@v3
        if: always()
        with:
          sarif_file: ${{ steps.deslop.outputs.sarif-file }}
```

The included `.github/workflows/deslop.yml` is the same pattern as a ready-to-edit example.
`upload-sarif` publishes findings to GitHub code scanning. The fail-on step still controls
whether the job passes.

For a direct workflow without the composite action:

```yaml
name: deslop

on:
  pull_request:

permissions:
  contents: read
  security-events: write

jobs:
  scan:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install --path crates/deslop-cli
      - run: deslop scan --changed=origin/${{ github.base_ref }} --baseline deslop-baseline.json --format sarif > deslop.sarif
      - uses: github/codeql-action/upload-sarif@v3
        if: always()
        with:
          sarif_file: deslop.sarif
      - run: deslop scan --changed=origin/${{ github.base_ref }} --baseline deslop-baseline.json --fail-on major
```

## Baseline Ratchet

Use a baseline when adopting deslop on an existing codebase:

```sh
deslop baseline write . -o deslop-baseline.json
deslop scan --baseline deslop-baseline.json --fail-on major .
```

Commit the baseline file. Future CI runs suppress those known fingerprints and fail only when
new findings meet the threshold. Refresh the baseline only after reviewing and accepting the
current finding set:

```sh
deslop baseline update . -o deslop-baseline.json
```

## Pre-commit

This repository exposes a pre-commit hook in `.pre-commit-hooks.yaml`. Consumers can add:

```yaml
repos:
  - repo: https://github.com/OWNER/REPO
    rev: vX.Y.Z
    hooks:
      - id: deslop
```

For local development in this checkout:

```yaml
repos:
  - repo: local
    hooks:
      - id: deslop
        name: deslop scan
        entry: deslop scan --fail-on major
        language: system
        pass_filenames: true
        types: [text]
```

The hook uses `language: system`, so `deslop` must be on `PATH`:

```sh
cargo install --path crates/deslop-cli
```

The hook passes changed filenames to `deslop scan`. For Git-native local checks, use
`deslop scan --changed --fail-on major` to scan files changed since `HEAD`.

## Feedback

When a reviewed finding is a false positive, add it to the eval corpus before lowering trust
in the rule:

```sh
deslop feedback <fingerprint> --false-positive --corpus tests/corpus .
deslop eval tests/corpus
```

The feedback command copies the source into `tests/corpus/feedback/` and appends a clean
expectation, so precision regressions are measured by the normal eval gate.
