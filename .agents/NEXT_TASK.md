# TASK 13/queue (final) — LSP edges: real columns, fix-all, a true RPC test, (then incremental/workspace)

The LSP MVP (deslop-lsp) does FULL sync, whole-line diagnostic ranges, per-open-file scan, safe-auto
quickfixes, and only PURE-function unit tests. Sharpen it. Multiple rounds OK; gate each round. Start
with `jj new` (separate change on top of oszlxpvn). Do these in priority order; if you run low on
runway, ship the high-priority ones and HONESTLY defer the rest with reasons.

## P1 — precise UTF-16 columns (biggest quality win)
Replace whole-line diagnostic ranges with precise ranges from `Finding.span`: map byte offsets within
each line to LSP `Position.character` in **UTF-16 code units** (LSP's required encoding). Handle
multi-byte UTF-8 correctly (a byte offset → the UTF-16 column on that line). Unit-test with a fixture
containing non-ASCII so the column math is exercised (not just ASCII).

## P2 — fix-all source action + keep per-finding quickfixes
Add a `source.fixAll` (kind `CodeActionKind::SOURCE_FIX_ALL`) "deslop: fix all safe findings in file"
action that applies ALL `SafeAuto`/`AnalyzerConfirmed` fixes via `deslop_fix::apply_findings_to_text`
in one `WorkspaceEdit`. Keep the per-finding quickfixes. Still NEVER offer auto-fixes for riskier
safety classes. Unit-test: a file with 2 safe findings yields a fix-all editing both; a file with only
LlmOnly findings yields no fix-all.

## P3 — a real JSON-RPC loop integration test
Drive the actual server over in-memory pipes (lsp-server supports a Connection over channels):
`initialize` → `initialized` → `textDocument/didOpen` (a sloppy fixture) → assert a
`textDocument/publishDiagnostics` with the expected diagnostic → `textDocument/codeAction` → assert
the quickfix/fix-all → `shutdown`/`exit`. This proves the server works end-to-end (the MVP only tested
pure fns). Keep it deterministic and fast.

## P4 (if runway allows; else defer with reasons)
- Incremental document sync (`TextDocumentSyncKind::INCREMENTAL`): apply ranged `didChange` edits to
  the in-memory doc instead of FULL replacement.
- Workspace-wide scan: on `initialize`/`didOpen`, scan the workspace folder (respect the LangPack
  extensions) and publish diagnostics for files, not only open ones. Be mindful of cost.

## Constraints / gate
Reuse the analyzer/fix surfaces unchanged. LSP deps stay isolated to deslop-lsp (don't leak into other
crates). MCP default build stays network-free. Do NOT touch `deslop/*.py`. Gate after each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Report
P1 column mapping + the non-ASCII test; P2 fix-all + tests; P3 the end-to-end RPC test; P4 done-or-
deferred (with reasons); SPEC.md LSP update. `jj describe -m "<summary>"`. Touch `.agents/HEARTBEAT.md`.
This is the LAST queued item — state the queue is complete in your report.
