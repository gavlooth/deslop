# NEXT TASK — SARIF output (goal item #5, final)

Add SARIF 2.1.0 output so deslop findings drop into GitHub/GitLab code-scanning. Deterministic,
fully testable locally. Keep all prior work.

**STEP 0:** `cargo build --workspace` + `cargo test --workspace`, report.

## Deliver
- Add `sarif` to `scan --format` (alongside `text|json|agent`): emit a valid **SARIF 2.1.0**
  document.
- Mapping:
  - `runs[].tool.driver`: name `deslop`, version, and `rules[]` with each rule's id +
    shortDescription + the safety class as a property.
  - each finding → `runs[].results[]` with `ruleId`, `level` (Major→`error`, Minor→`warning`,
    Info→`note`), `message.text`, and `locations[].physicalLocation`
    (`artifactLocation.uri` = path, `region.startLine`/`endLine`).
- Render it in `deslop-report` next to the existing JSON renderer; reuse the finding data.

## Tests
- `scan --format sarif` on a fixture parses as valid JSON and has
  `version=="2.1.0"`, `$schema`, `runs[0].tool.driver.name=="deslop"`.
- `results` count equals the finding count; severity→level mapping is correct.
- Each result has a physicalLocation with the right uri + startLine.

## Report
Status; the SARIF renderer + `--format sarif`; the tests with the validated fields. Keep
`fmt`/`build`/`test`/`clippy -D warnings` green. `jj describe`. Do NOT touch `deslop/*.py`.

(This is the last roadmap/goal item — after this, the open-deferred list is cleared except
the explicitly optional `slim` consumer and `LSP`.)
