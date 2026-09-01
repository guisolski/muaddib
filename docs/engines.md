# Engines

muaddib reaches a model over one of two **transports**: a locally installed AI CLI
driven as a subprocess (reusing its authentication and built-in web access), or
direct HTTP to a model API. The CLI transport is the default and came first; see
[ADR-0017](adr/0017-direct-model-apis.md) for why the second one exists.

## The engine table

Every engine is one row in `ENGINES` (`src/core/engine.rs`). The row's `transport`
field selects which of the two half-specs applies:

```rust
pub enum Transport {
    Cli(&'static CliSpec),
    Api(&'static ApiSpec),
}
```

Everything outside `src/engines/` — the TUI, `main.rs`, the config layer — reads only
the transport-agnostic fields (`name`, `models`, `install_hint`, `prices`), so adding
the API transport did not spread `match` arms through the codebase.

### CLI rows

| name | binary | argv (before the prompt) | streams | parse strategy | JSON schema | fast model |
|---|---|---|---|---|---|---|
| `claude` | `claude` | `-p --output-format stream-json --verbose --allowedTools=WebSearch,WebFetch` | ✓ | `ClaudeJson` | ✓ `--json-schema` | `haiku` |
| `cursor-agent` | `cursor-agent` | `-p --output-format json --force` | — | `GenericJson` | prompt-enforced | — |
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

Only `claude` declares `web_tools` in its CLI row. Engines without built-in web
search (`cursor-agent`, `codex`, `opencode`) rely on muaddib's built-in web-search
grounding; when their sub-searches return no citable URLs, the pipeline automatically
merges web-hit snippets into the findings before synthesis.

### API rows

| name | wire | endpoint | auth header | probes models | auto-selected |
|---|---|---|---|---|---|
| `ollama` | `OllamaChat` | `$OLLAMA_HOST` or `http://localhost:11434` | — | ✓ `/api/tags` | ✓ |
| `local` | `OpenAiChat` | `$MUADDIB_LOCAL_BASE_URL` | optional bearer | ✓ `/v1/models` | ✓ |
| `openai` | `OpenAiChat` | `https://api.openai.com` | `authorization: Bearer` | — | **✗** |
| `anthropic` | `AnthropicMessages` | `https://api.anthropic.com` | `x-api-key` | — | **✗** |
| `gemini` | `GeminiGenerate` | `https://generativelanguage.googleapis.com` | `x-goog-api-key` | — | **✗** |

Every endpoint above is a default. `[engines.<name>] base_url` overrides it and wins
over the environment variable, which is how you point `local` at LM Studio, llama.cpp,
vLLM, or an OpenRouter-style gateway — and equally how you route `openai` or
`anthropic` through a proxy. The config modal's **base url** field writes that key.

The wire formats are pure functions in `src/core/api.rs` — request bodies, text and
usage extraction, headers, endpoints, retry decisions — dispatched from the `Wire`
enum. `src/engines/api.rs` owns only the HTTP.

Three details that are not obvious from the table:

- **`auto_select` is false for the billed rows.** `choose_engine` falls back to the
  first available engine when the requested one is missing; without this column, a
  stray `OPENAI_API_KEY` would silently start billing you the first time `claude` was
  not on `PATH`. Billed engines are reachable only by explicit request.
- **Anthropic requires `max_tokens`**, so its row carries `default_max_tokens`; the
  others send none unless configured. Its `content[]` may lead with `thinking` blocks,
  so extraction filters on `type == "text"` rather than taking `content[0]`, and
  `stop_reason: "refusal"` is mapped to an error instead of a blank answer.
- **Gemini takes the key in a header**, never the query string, so it cannot land in a
  log or an error URL. Its model goes in the URL path, not the body.

### Availability

CLI rows are available when the binary resolves on `PATH` (or `[engines.<name>] bin`
points at an executable). API rows are available when an endpoint resolves *and* either
the engine is keyless, or a key is found, or the encrypted vault holds one for it.

`ollama` and `local` are probed live: a `GET /api/tags` with a 1s budget both decides
availability and fills `EngineStatus.models`, which is what the config modal's model
picker reads. That is why the picker offers the models you actually pulled instead of a
hardcoded list.

The probe runs at startup and again after every config save and vault unlock, so
starting `ollama serve` and then setting a base url in the modal is enough — no
restart. Availability never gates the config modal's engine row: an engine that is
not ready is still selectable, showing why next to its name (`ollama (not running)`,
`openai (no key)`), because otherwise the engines that need configuring would be
exactly the ones you could not reach in order to configure them.

### Keys

Resolution order, first match wins:

1. `[engines.<name>] api_key_env` — a variable name you choose
2. the row's own `key_env` (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, then
   `GEMINI_API_KEY` / `GOOGLE_API_KEY`)
3. the encrypted vault, which needs the session passphrase

Step 2 is what keeps `--print` and CI zero-config. Keys never reach `config.toml`; see
[configuration.md](configuration.md#the-key-vault).

### Structured output

Every API row ships `SchemaMode::JsonObject` — the provider's "must emit valid JSON"
flag — while the answer *shape* goes through the prompt-inline path. `ANSWER_SCHEMA`
uses `$ref`/`definitions`, `oneOf`, and `minimum`, which every provider's native mode
rejects in some form. `SchemaMode::NativeSchema` is implemented and tested for all four
wires, and a table invariant test asserts no row uses it yet, so promoting a provider
is a deliberate one-row edit rather than an accident.

### Retry

429 and 5xx are retried up to `MAX_ATTEMPTS`, honouring `Retry-After`. The decision is
pure (`core::api::retry_delay`); the adapter only sleeps. The retry loop sits *inside*
`tokio::time::timeout(job.timeout, ..)`, so retries share one budget instead of
multiplying it. This matters because fan-out fires up to `max_parallel` concurrent
requests — without it, sub-queries would silently drop to empty on the first rate limit.

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

1. **Envelope layer** (`core/engine.rs`), per strategy:
   - `ClaudeJson` — parses the `{"type":"result", ...}` envelope; prefers
     `structured_output` (populated by `--json-schema`), falls back to the
     `result` string; surfaces `is_error: true` as `EngineError::Reported`.
     Stdout may be either one JSON object or a JSONL stream: the whole-buffer
     parse is tried first, then the **last** `"type":"result"` line.
   - `GenericJson` — parses stdout (or its last non-empty line) as JSON,
     surfaces `is_error: true` as `EngineError::Reported`, and probes a key
     table: `result`, `text`, `response`, `content`, `message`, `output`.
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
   case in `core/engine.rs` tests.
3. Done. Detection, the config modal, selection fallback, and the pipeline all
   read the table.

## Testing seam

`[engines.<name>] bin = "/path"` in the config doubles as the test seam: the
integration tests point `claude` at `tests/fixtures/fake-engine.sh`, which
routes on the prompt's task markers (`MUADDIB:EXPAND` / `MUADDIB:SUBSEARCH` /
`MUADDIB:SYNTH`) and answers with canned fixtures. CI never invokes a real AI CLI.
