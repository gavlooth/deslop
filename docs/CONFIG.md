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
```

Feeds the bundled `deslop fix` LLM consumer. `DESLOP_SLIM_MODEL` overrides the config
model when `--model` is not supplied. Provider API keys stay in environment variables.

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
```

Only `min_duplication_tokens` is exposed because it is the only threshold currently
represented in `AnalyzerConfig`. Long-method thresholds and similar constants are not
configurable until the analyzer owns those knobs directly.
