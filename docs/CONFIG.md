# Configuration

deslop reads `./deslop.toml` by default. Use `--config <path>` to choose another file.
If the file is absent, deslop uses the same built-in defaults as an unconfigured run.

Precedence is:

1. Explicit CLI flag
2. Environment variable, where one exists
3. `deslop.toml`
4. Built-in default

API keys are env-only. Do not put provider secrets in `deslop.toml`; deslop does not read
`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, or `DESLOP_SLIM_API_KEY` from config files.

## scan

```toml
[scan]
fail_on = "major"              # info | minor | major
baseline = "deslop-baseline.json"
```

Feeds `deslop scan --fail-on` and `deslop scan --baseline`.

## fix

```toml
[fix]
check_cmd = "cargo test --workspace"
coverage = "disabled"
allow_unverified = false
```

Feeds `deslop fix`. `coverage` uses the same parser as the CLI:
`disabled`, `auto`, `auto:<cmd>`, `lcov:<path>`, `cloverage:<path>`,
`julia-cov:<path>`, or `coverage-py:<path>`.

`allow_unverified` can be overridden with `--allow-unverified` or
`--allow-unverified=false`.

## slim

```toml
[slim]
provider = "anthropic"         # anthropic | openai
model = "claude-sonnet-4-6"
base_url = "https://api.openai.com/v1"
egress_consent = false
```

Feeds the bundled `deslop fix` LLM consumer. `DESLOP_SLIM_MODEL` overrides the config
model when `--model` is not supplied. Provider API keys stay in environment variables.

Real-provider `deslop fix` calls send selected code regions to the configured provider, even
in dry-run. Consent is required through one of:

- `deslop fix --yes` or `deslop fix --consent`
- `DESLOP_SLIM_CONSENT=1`
- `[slim] egress_consent = true`
- an interactive `y` response to the TTY prompt

`--mock` uses a recorded local response and does not require consent. Non-interactive runs
without consent fail before constructing a provider client or reading an API key.

`deslop fix` prints progress to STDERR when STDERR is a TTY. Use `--quiet` to suppress it.
When STDERR is not a TTY, progress is silent by default so CI logs and pipes stay clean.
STDOUT remains only the final report.

MCP `fix mode=auto` is non-interactive. With `deslop-mcp --features slim-llm`, real-provider
auto mode requires `consent: true`, `DESLOP_SLIM_CONSENT=1`, or a config file containing
`[slim] egress_consent = true`; mock auto runs bypass consent.

## external

```toml
[external]
clippy = "off"                 # off | on
julia_analyzer = "off"         # off | staticlint | jet
julia_project = "julia-env"
```

Keeps the existing external analyzer defaults for `deslop scan` and `deslop propose`.
CLI flags such as `--rust-external`, `--julia-external`, and `--julia-project` override
these values.

## analyzer

```toml
[analyzer]
min_duplication_tokens = 24
long_method_nloc = 40
min_meaningful_tokens = 8

[analyzer.rust]
long_method_nloc = 45

[analyzer.python]
long_method_nloc = 35

[analyzer.typescript]
long_method_nloc = 45
```

`min_duplication_tokens` controls duplicate-window size. `long_method_nloc` controls the
non-comment line threshold for `long-method`. `min_meaningful_tokens` controls the minimum
meaningful-token count required before token duplication findings are emitted.

Per-language analyzer tables can override `long_method_nloc` for `rust`, `clojure`,
`julia`, `python`, `javascript`, `typescript`, or `generic` without changing the global fallback.

Unknown keys under `[analyzer]` are rejected, not silently ignored. A typo such as
`ignore_comments = true` is a hard parse error rather than a no-op.

### Suppression

Token thresholds are global and blunt. Suppression is the scalpel: it filters findings
*after* they are produced, by rule name and by path. It applies uniformly to every
analyzer pack and to external-analyzer findings.

```toml
[analyzer]
# Drop these rules everywhere. Each name must be a known deslop rule (see `deslop rules`);
# an unknown name is an error so typos never silently do nothing.
disabled_rules = ["magic-number", "narrating-comment"]

# Path globs skipped for every rule. Globs match the scanned path; a leading "./" is ignored.
ignore_paths = ["**/generated/**", "vendor/**"]

# Per-rule controls, keyed by rule name.
[analyzer.rules.long-method]
enabled = false                 # same as adding "long-method" to disabled_rules

[analyzer.rules.duplicate-block]
ignore_paths = ["**/tests/**"]  # skip duplicate-block only under test directories
```

- `disabled_rules` removes a rule regardless of path.
- `ignore_paths` (under `[analyzer]`) skips a path for all rules.
- `[analyzer.rules.<rule>].enabled = false` disables one rule.
- `[analyzer.rules.<rule>].ignore_paths` skips a path for that one rule only.

Globs use [`globset`](https://docs.rs/globset) syntax (`*`, `**`, `?`, `[...]`). Invalid
globs and unknown rule names are reported as errors.

MCP `scan`, `propose`, and prompt-mode `fix` accept the same optional `config` path and an
inline `analyzer` object. The inline object supports `disabled_rules`, `ignore_paths`, and
`rules` in addition to the threshold keys. Inline analyzer values override values loaded
from `deslop.toml` for that tool call; suppression from both sources is merged.
