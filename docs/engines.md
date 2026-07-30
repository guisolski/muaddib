# Engines

faro does not talk to model APIs. It drives locally installed AI CLIs as
subprocesses, reusing their authentication and their built-in web access.

## The engine table

Every engine is one row in `ENGINES` (`src/engines/mod.rs`):

| name | binary | argv (before the prompt) | parse strategy | JSON schema |
|---|---|---|---|---|
| `claude` | `claude` | `-p --output-format json --allowedTools=WebSearch,WebFetch` | `ClaudeJson` | ✓ `--json-schema` |
| `cursor-agent` | `cursor-agent` | `-p --output-format json` | `GenericJson` | prompt-enforced |
| `codex` | `codex` | `exec --skip-git-repo-check` | `RawText` | prompt-enforced |
| `opencode` | `opencode` | `run` | `RawText` | prompt-enforced |

`build_args` appends the prompt as the **last argv element** — never through a
shell, so there is no injection surface. When a model is configured (config
modal, `[engines.<name>] model`, or `--model`), the engine's `model_flag`
(`--model` for all current engines) and the value are inserted before the
prompt; each spec also carries a curated `models` list that feeds the config
modal's choices. For engines with `supports_json_schema`,
the synthesis call also appends `--json-schema <schema>` so the CLI itself
validates the structured output; other engines get the schema inlined in the
prompt text instead.

> `--allowedTools` must use the `=` form: the flag is variadic in the claude CLI
> and would otherwise swallow the trailing prompt argument (found the hard way,
> during the live checkpoint).

## Output parsing: two tolerant layers

1. **Envelope layer** (`engines/parse.rs`), per strategy:
   - `ClaudeJson` — parses the `{"type":"result", ...}` envelope; prefers
     `structured_output` (populated by `--json-schema`), falls back to the
     `result` string; surfaces `is_error: true` as `EngineError::Reported`.
   - `GenericJson` — parses stdout (or its last non-empty line) as JSON and
     probes a key table: `result`, `text`, `response`, `content`, `message`,
     `output`.
   - `RawText` — passes stdout through unchanged.
   Every strategy degrades to raw text rather than failing.
2. **Extraction layer** (`core/extract.rs`), always applied to the inner text:
   direct parse → fenced ```json block → first balanced `{...}` scan
   (string-and-escape aware). This makes chatty CLIs tolerable.

## Detection and selection

At startup `detect_engines` walks the table and checks each binary on `PATH`
(honoring per-engine `bin` overrides from the config). Unavailable engines show
as "(not installed)" in the config modal and cannot be selected. If the
configured engine is missing, `choose_engine` falls back to the first available
one and surfaces a notice. If none are installed, faro prints each engine's
install hint.

## Execution

`CliEngine` (`engines/cli.rs`) runs the binary with `tokio::process::Command`:

- `stdin` null, stdout/stderr captured
- wrapped in `tokio::time::timeout` (`engine_timeout_secs`, default 180s)
- `kill_on_drop(true)` — cancelling a search (Esc) reaps every child CLI
- non-zero exit becomes `EngineError::Failed { status, stderr_tail }`

## Adding an engine

1. Add one row to `ENGINES` with the argv and the closest parse strategy.
2. If the CLI has a custom envelope, add a fixture under `tests/fixtures/` and a
   case in `engines/parse.rs` tests.
3. Done. Detection, the config modal, selection fallback, and the pipeline all
   read the table.

## Testing seam

`[engines.<name>] bin = "/path"` in the config doubles as the test seam: the
integration tests point `claude` at `tests/fixtures/fake-engine.sh`, which
routes on the prompt's task markers (`FARO:EXPAND` / `FARO:SUBSEARCH` /
`FARO:SYNTH`) and answers with canned fixtures. CI never invokes a real AI CLI.
