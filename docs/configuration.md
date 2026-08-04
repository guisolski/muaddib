# Configuration

## File locations

muaddib keeps two files apart: hand-edited settings under the XDG *config* dir, and
machine-appended search history under the XDG *state* dir.

### Config

Resolution order (first hit wins):

1. `$MUADDIB_CONFIG` — explicit path (also the test seam)
2. `$XDG_CONFIG_HOME/muaddib/config.toml`
3. `~/.config/muaddib/config.toml`

A missing file means defaults; a malformed file means defaults plus a visible
notice (muaddib never refuses to start over config).

### Search history

Resolution order (first hit wins):

1. `$MUADDIB_HISTORY` — explicit path (also the test seam)
2. `$XDG_STATE_HOME/muaddib/history.jsonl`
3. `~/.local/state/muaddib/history.jsonl`

`$XDG_STATE_HOME` is where the base-directory spec puts "actions history", which
is exactly what this is — not user-editable configuration.

The format is JSON Lines: one object per search, appended, never rewritten in the
common case.

```jsonl
{"query":"rust async runtimes","mode":"deep","fast":false,"at":1753876543}
{"query":"capital of peru","mode":"general","fast":true,"at":1753876600}
```

A line that fails to parse is skipped, not fatal. The file is capped at 500
entries and compacted on load when it grows past that. A search identical to the
most recent one is not appended again (shell `ignoredups` behavior), though it
still moves to the front of the in-session recall list.

See [Search history](#search-history-1) for the keys that drive it.

## Keys

```toml
language = "pt-BR"          # answer language, BCP-47 tag
engine = "claude"           # claude | cursor-agent | codex | opencode
max_parallel = 4            # concurrent sub-searches, clamped to 1..=8
expansion_breadth = 0       # 0 = use the mode default, otherwise clamped to 1..=8
validate_links = true       # HTTP HEAD validation of every source
images = true               # fetch and render answer images in the terminal
animations = true           # staggered block reveal, chart growth, jump pulses
engine_timeout_secs = 180   # per engine call
fast_timeout_secs = 45      # hard ceiling for the single fast-mode call, clamped to 5..=120

[websearch]                 # built-in web-search grounding (ADR-0007)
enabled = true              # query conventional engines before the AI fan-out; off in fast mode
merge_snippets = false      # also merge hit snippets into the findings, not just the prompts
max_hits_per_query = 5      # deduplicated hits per sub-query, clamped to 1..=10
engines = []                # empty = mode defaults; else an allowlist of engine names
                            # (ddg, ddg-lite, bing, mojeek, google, openalex, crossref, s2)
mailto = ""                 # optional email for the OpenAlex/Crossref polite pools

[engines.claude]            # optional, one block per engine
bin = "/custom/path/claude" # binary override (also used by the test suite)
model = "sonnet"            # model passed to the CLI; any value the CLI accepts
fast_model = "haiku"        # model used in fast mode; falls back to the engine's curated one
```

Unknown keys are tolerated (forward compatibility). Out-of-range numbers are
clamped, not rejected.

## Precedence

CLI flags > config file > defaults:

| Setting | CLI flag | Config key | Default |
|---|---|---|---|
| answer language | `--lang` | `language` | `pt-BR` |
| engine | `--engine` | `engine` | `claude` |
| model | `--model` | `[engines.<name>] model` | engine default |
| search mode | `--mode` | — | `general` |
| fast mode | `--fast` | — | off (`Ctrl+F` toggles it in the TUI) |
| web search | `--no-websearch` (disables) | `[websearch] enabled` | on |

The config modal (`Ctrl+O`) edits and persists the file; `--lang`/`--engine`/
`--model` apply to the current run only.

## Fast mode

`Ctrl+F` (or `--fast`) is orthogonal to the search mode: `fast` + `scientific` is
a valid combination. It replaces the three serial engine calls with a single one,
so it trades breadth and cross-checking for latency.

| | Standard | Fast |
|---|---|---|
| engine calls | 3 (expand → sub-searches → synthesize) | 1 |
| sub-queries | 3–6 by mode | the literal query only |
| model | `[engines.<name>] model` | `fast_model`, else the engine's curated fast model (`haiku` for claude) |
| timeout | `engine_timeout_secs` | `fast_timeout_secs` |
| answer blocks | headings, prose, lists, quotes, tables, charts, diagrams, images | headings, prose, lists |
| source cross-check | synthesized URLs must appear in the sub-search findings | the answer's own declared sources, plus link validation |
| images | per `images` | always off |
| link validation | per `validate_links` | per `validate_links` |
| web-search grounding | per `[websearch]` | always off |

Only `claude` ships a curated fast model. Other engines reuse their normal model
unless you set `fast_model` yourself.

### What to actually expect

Measured against `claude` + `haiku`, wall clock, warm CLI:

| Query | Standard | Fast |
|---|---|---|
| `capital of peru` (rated `simple`, so standard already ran one sub-search) | ~29s | ~19s |
| `rust async runtime tradeoffs` (3 sub-searches) | ~166s | ~31s |

**Five seconds is a target, not a contract, and in practice it is not reached.**
The floor is not muaddib — it is the engine CLI's agent loop: process start, a large
agent system prompt, a thinking block, the web search round-trip, and a second
thinking block before the structured answer. That is roughly 15–30s for `haiku`,
whatever the prompt asks for.

`FAST_TARGET_SECS` (5s) therefore only controls when the elapsed counter on the
searching screen turns yellow. Nothing is aborted at 5s; `fast_timeout_secs`
(45s) is the only hard ceiling, set high enough that a normal fast search never
trips it.

## Search history

| Key | Effect |
|---|---|
| `$MUADDIB_HISTORY` | overrides the history file path entirely |
| `--clear-history` | erases the file, reports the count, exits |

In the TUI, `Up`/`Down` walk the history from the search bar and `Ctrl+L` clears
it — the first press asks, the second one deletes.

`Ctrl+F` and `Ctrl+L` shadow `tui-input`'s emacs bindings for forward-char and
(unused) clear-line on the Home screen. `Right` still moves the cursor.

## The config modal

| Field | Values |
|---|---|
| language | cycles `en`, `pt-BR`, `es`, `fr`, `de`, `it`, `ja`, `zh` (any BCP-47 tag works via `--lang` or the file) |
| engine | cycles installed engines; uninstalled ones are shown but not selectable |
| model | cycles `default` plus a curated list per engine; a custom model set in the file appears as an extra option |
| validate links | on / off |
| web search | on / off |
| max parallel | 1–8 |

`Enter` saves to the config file; `Esc` discards.
