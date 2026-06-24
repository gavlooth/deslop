# TASK 11/queue — first-run source-egress consent (don't send code to an LLM without opt-in)

`deslop fix` (and MCP fix `mode=auto`) send code regions to an external LLM (Anthropic/OpenAI) — with
NO consent today. Add an explicit-consent gate before ANY real-provider call. The mock/RecordedClient
path needs no consent (no egress). Start with `jj new` (separate change on top of quvrtxsu).

## Safety properties (the contract)
1. A REAL-provider rewrite (Anthropic/OpenAI) must NOT happen without affirmative consent. This
   applies in dry-run too (slim calls the LLM to build patches even without `--apply`).
2. `--mock`/RecordedClient → NO consent needed (nothing leaves the machine).
3. Non-interactive (no TTY, e.g. CI, and the MCP server) WITHOUT explicit consent → REFUSE with a
   clear, actionable error. NEVER hang waiting for stdin.

## Consent sources (any grants it; precedence highest-first)
- CLI `--yes` (a.k.a. `--consent`) flag.
- env `DESLOP_SLIM_CONSENT=1`.
- config `[slim] egress_consent = true` (deslop.toml).
- interactive TTY prompt: "deslop will send code regions from <N> file(s) to <provider> (<base_url>).
  Continue? [y/N]" → `y` grants for this run. (Persisting a marker across runs is OPTIONAL — if you
  do it, document the location, e.g. a state file; if not, that's fine — keep scope tight.)

## Implement
- A PURE decision function, e.g. `fn resolve_egress_consent(explicit: bool, is_interactive: bool) ->
  EgressDecision` where `EgressDecision ∈ { Granted, Prompt, DeniedNonInteractive }`. `explicit` =
  flag||env||config. Unit-test this truth table directly.
- Wire it into the CLI `fix` path: when the chosen provider is a REAL client (not mock), resolve
  consent BEFORE building/calling it. `Prompt` → read y/N from the TTY (use `std::io::IsTerminal`);
  `DeniedNonInteractive` → return a clear error naming the flag/env/config; `Granted` → proceed.
- Wire it into MCP fix `mode=auto` (feature `slim-llm`): no TTY there, so consent MUST be explicit
  (an `consent: true` arg OR `DESLOP_SLIM_CONSENT` OR config) → else return the clear error. `--mock`
  bypasses.
- Message must state WHAT goes WHERE (provider + base_url + file/region count). Never log the API key.

## Tests (deterministic, NO network/TTY)
- Decision truth table: explicit→Granted; !explicit & interactive→Prompt; !explicit & !interactive→
  DeniedNonInteractive.
- Mock path: RecordedClient run needs NO consent (existing mock e2e still passes untouched).
- Non-interactive real-provider WITHOUT consent → clear error (no hang). Test the resolver + the
  error path; don't require a live client.
- Config/env/flag each independently grant consent (resolution test).

## Constraints / gate
Don't change the rewrite/verify/gating logic. MCP default build stays network-free. Do NOT touch
`deslop/*.py`. Gate after each change:
`cargo fmt --all && cargo build --workspace && cargo build -p deslop-slim --no-default-features && cargo test --workspace && cargo clippy --workspace -- -D warnings`
(plus `cargo test -p deslop-mcp --features slim-llm` for the auto-mode consent path.)

## Report
the decision function + truth table; the three consent sources; CLI prompt + non-interactive refusal;
MCP auto-mode explicit-consent requirement; mock bypass; docs (CONFIG.md + deslop.toml.example
`egress_consent`); tests. `jj describe -m "<summary>"`. Touch `.agents/HEARTBEAT.md`. Do NOT start
queued items 12-13.
