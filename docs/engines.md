# Engines

muaddib does not talk to model APIs. It drives locally installed AI CLIs as
subprocesses, reusing their authentication and their built-in web access.

## The engine table

Every engine is one row in `ENGINES` (`src/engines/mod.rs`):

| name | binary | argv (before the prompt) | streams | parse strategy | JSON schema | fast model |
|---|---|---|---|---|---|---|
| `claude` | `claude` | `-p --output-format stream-json --verbose --allowedTools=WebSearch,WebFetch` | ✓ | `ClaudeJson` | ✓ `--json-schema` | `haiku` |
| `cursor-agent` | `cursor-agent` | `-p --output-format json` | — | `GenericJson` | prompt-enforced | — |
| `codex` | `codex` | `exec --skip-git-repo-check` | — | `RawText` | prompt-enforced | — |
| `opencode` | `opencode` | `run` | — | `RawText` | prompt-enforced | — |

`build_args` appends the prompt as the **last argv element** — never through a
shell, so there is no injection surface. When a model is configured (config
modal, `[engines.<name>] model`, or `--model`), the engine's `model_flag`
(`--model` for all current engines) and the value are inserted before the
prompt; each spec also carries a curated `models` list that feeds the config
modal's choices. For engines with `supports_json_schema`,
the synthesis call also appends `--json-schema <schema>` so the CLI itself
validates the structured output; other engines get the schema inlined in the
prompt text instead.

The `fast_model` column is the model fast mode picks by default. Only `claude`
ships one, because it is the only engine whose model lineup has an unambiguous
"small and quick" tier; the others keep their normal model unless you set
`[engines.<name>] fast_model` yourself. Resolution order in fast mode:
`fast_model` from the config → the table's `fast_model` → the configured `model`
→ the CLI's own default.

> `--allowedTools` must use the `=` form: the flag is variadic in the claude CLI
> and would otherwise swallow the trailing prompt argument (found the hard way,
> during the live checkpoint).

## Streaming

The `streams` column says whether stdout arrives as a line stream muaddib can
narrate while the call is still running. Only `claude` sets it, and a table test
enforces that it is set exactly when the argv asks for `stream-json`.

`engines/cli.rs` spawns the child and reads stdout line by line rather than
buffering to completion. Each line goes through `core/stream.rs::activities_in`
(pure), which reports only the tools in the `STREAM_TOOLS` table — `WebSearch` →
"searching" with its query, `WebFetch` → "reading" with its URL. Every other
line, including the model's own thinking and the `StructuredOutput` call that
carries the answer, is collected but never narrated.

Activity is cosmetic, so it is sent with `try_send` and dropped rather than
queued when the UI falls behind: a chatty engine can never throttle the pipeline
to the render loop's pace.

The structured-output contract is unaffected — `--json-schema` coexists with
`--output-format stream-json`, and the final `result` line still carries
`structured_output`, `total_cost_usd`, and `usage` (verified live before this
was built; see ADR-0015).

## Output parsing: two tolerant layers

1. **Envelope layer** (`engines/parse.rs`), per strategy:
   - `ClaudeJson` — parses the `{"type":"result", ...}` envelope; prefers
     `structured_output` (populated by `--json-schema`), falls back to the
     `result` string; surfaces `is_error: true` as `EngineError::Reported`.
     Stdout may be either one JSON object or a JSONL stream: the whole-buffer
     parse is tried first, then the **last** `"type":"result"` line.
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
one and surfaces a notice. If none are installed, muaddib prints each engine's
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
routes on the prompt's task markers (`MUADDIB:EXPAND` / `MUADDIB:SUBSEARCH` /
`MUADDIB:SYNTH`) and answers with canned fixtures. CI never invokes a real AI CLI.
