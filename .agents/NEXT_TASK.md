# TASK 5/queue — CI / pre-commit packaging

The gating primitives already exist in the CLI: `deslop scan --fail-on <severity>` (exits 1 when a
finding meets/exceeds the threshold), `deslop scan --format sarif`, and a baseline ratchet
(`--baseline deslop-baseline.json` + `deslop baseline`). This task PACKAGES them for CI / pre-commit
— mostly config + docs, minimal/no Rust. Start with `jj new` (separate change on top of wvzwxyuw).

## Do
1. **GitHub Action (reusable) + example workflow**
   - `action.yml` (composite action, repo root or `.github/actions/deslop/action.yml`) with inputs:
     `paths` (default `.`), `fail-on` (default `major`), `sarif` (bool, default true),
     `baseline` (optional path). Steps: build/install deslop (`cargo install --path
     crates/deslop-cli` or use the workspace binary), run `deslop scan` producing SARIF, and run the
     `--fail-on` gate. Note: `scan` writes to stdout (no `--output` flag) — redirect
     `deslop scan --format sarif <paths> > deslop.sarif`.
   - `.github/workflows/deslop.yml` example: checkout → rust toolchain → run the composite action →
     upload SARIF via `github/codeql-action/upload-sarif@v3` (so findings show in GitHub code
     scanning) → the `--fail-on major` gate fails the build on Major findings. If `baseline` is set,
     pass `--baseline` so only NEW findings gate.
2. **pre-commit hook**
   - `.pre-commit-hooks.yaml` at repo root defining a `deslop` hook: `id: deslop`,
     `name: deslop scan`, `entry: deslop scan --fail-on major`, `language: system` (deslop must be on
     PATH), `pass_filenames: true` (or false with `.` — pick what works with the CLI's path args),
     `types: [text]`. Document adding it to a consumer `.pre-commit-config.yaml`.
3. **Docs** — a `docs/CI.md` (or a README "CI & pre-commit" section): the Action usage, pre-commit
   setup, the exit-code contract (`--fail-on`), SARIF → GitHub code scanning, and the baseline
   ratchet workflow (`deslop baseline` to snapshot, `--baseline` to gate only regressions).

## Verify (this task is config-heavy — verification = well-formed + the gate works)
- YAML validity: parse every new YAML (`python3 -c "import yaml,sys; yaml.safe_load(open(f))"` for
  action.yml, the workflow, `.pre-commit-hooks.yaml`) — all must load. Show the commands.
- Exit-code contract: add/confirm a CLI test that `deslop scan --fail-on major` exits NON-zero on a
  sloppy fixture and ZERO on a clean fixture (reuse tests/corpus or tests/fixtures). If such a test
  already exists, cite it.
- SARIF still schema-valid (cite the existing SARIF test; don't duplicate).
- Do NOT push, tag, or trigger anything. These are files only.

## Constraints / gate
No analyzer/CLI behavior change beyond (if needed) a test; reuse existing flags. MCP stays
network-free. Do NOT touch `deslop/*.py`. Gate after each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Report
files added (action.yml, workflow, .pre-commit-hooks.yaml, docs); YAML-parse proof; the exit-code
test (new or cited); how SARIF upload + baseline ratchet are wired; SPEC.md note. `jj describe -m
"<summary>"`. Touch `.agents/HEARTBEAT.md`. Do NOT start queued item 6 (config file).
