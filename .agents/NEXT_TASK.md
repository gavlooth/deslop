# TASK 4/queue — LSP server (deslop-lsp): diagnostics + safe-auto code actions

Editor integration over the analyzer. MVP: live diagnostics + code actions limited to the
fix-safety lattice (only SafeAuto/AnalyzerConfirmed get an auto-fix). Start with `jj new` (separate
change on top of wnyosyly). This is a bigger feature — multiple rounds are fine; gate each round.

## Deps (use the SYNC stack — matches deslop's no-tokio design)
Add `lsp-server` and `lsp-types` (rust-analyzer's crates: synchronous JSON-RPC over stdio +
protocol types). Do NOT use tower-lsp/tokio. These are justified new deps (LSP can't be done with
the existing stack; these are the minimal maintained sync option).

## Build (new crate crates/deslop-lsp: lib + bin `deslop-lsp`)
1. **Server loop**: stdio JSON-RPC via `lsp-server`. Initialize with capabilities:
   `text_document_sync = FULL`, `code_action_provider = true`. Handle shutdown/exit cleanly.
2. **Diagnostics**: on `didOpen`/`didChange`/`didSave`, take the in-memory text, infer `Lang` from
   the URI extension, run the analyzer over a `SourceFile` built from that text (reuse the same scan
   path the CLI/MCP use — find the public analyze entry in deslop-analyzer; do NOT duplicate rule
   logic), map each `Finding` → `lsp_types::Diagnostic`:
   - range from `Finding.span` (0-based lines `start_line-1..=end_line-1`; columns 0..end-of-line is
     acceptable for the MVP — note precise UTF-16 columns as a follow-up),
   - severity: Major→ERROR, Minor→WARNING, Info→HINT (pick a sane fixed mapping),
   - `source = "deslop"`, `code = rule`, `message = finding.message`.
   Publish via `textDocument/publishDiagnostics`. Keep a simple in-memory doc map (uri → text).
3. **Code actions** (`textDocument/codeAction`): for findings overlapping the requested range
   whose `safety` is `SafeAuto | AnalyzerConfirmed`, offer a `quickfix` "deslop: apply safe fix"
   returning a `WorkspaceEdit`. Compute the edit by running
   `deslop_fix::apply_findings_to_text(text, &[finding])` and diffing to a TextEdit (replace the
   whole document, or the minimal changed range). For findings with any OTHER safety class
   (RiskySuggest/LlmOnly/SafeWithPrecondition/NeverAuto), DO NOT offer an auto-fix (optionally a
   non-editing informational action). This enforces the lattice.

## Tests (deterministic, NO editor / NO full RPC loop required)
- Unit-test the PURE mapping functions:
  - findings → diagnostics: a known fixture yields a diagnostic with the right range/severity/
    source/code/message.
  - code-action gating: a SafeAuto finding yields a quickfix with a non-empty WorkspaceEdit; an
    LlmOnly finding yields NO quickfix.
- (Optional) a minimal init/handshake smoke test if cheap. Don't block on full server I/O tests.

## Constraints / gate
Keep the analyzer/fix/verify surfaces unchanged (consume them, don't modify). MCP stays
network-free; don't add deslop-lsp deps to other crates. Do NOT touch `deslop/*.py`. Gate after
each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Report
crate layout + bin name; deps added (lsp-server/lsp-types versions); capabilities; the
finding→diagnostic mapping; the safe-auto-only code-action gating; test outcomes; what's deferred
(incremental sync, precise UTF-16 columns, workspace-wide scan, multi-fix actions). `jj describe -m
"<summary>"`. Touch `.agents/HEARTBEAT.md`. Do NOT start queued items 5-6.
