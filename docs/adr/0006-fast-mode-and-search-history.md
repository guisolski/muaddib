# ADR-0006: Fast mode as one engine call, search history as JSON Lines

- Status: accepted
- Date: 2026-07-30

## Context

Two unrelated needs landed together, both about the cost of asking a question.

**Latency.** Every search — a definition lookup as much as a research question —
paid three strictly serial CLI round-trips: expand, fan out, synthesize. The
existing "model-decided fast path" (ADR-0004's `"complexity": "simple"` rating)
only collapsed N sub-queries to one, but those already ran concurrently under
`buffer_unordered`, so it never touched the dominant term. Each round-trip means
a fresh subprocess, its own model warm-up, and its own web searches; three of
them put a trivial question in the same latency class as a deep one. The goal
was an answer in about five seconds.

**Recall.** Nothing was remembered between runs. Re-running or tweaking a
previous query meant retyping it. `Up` and `Down` were unbound on the Home
screen, so shell-style recall was an unclaimed affordance.

## Decision

### Fast mode is one engine call, toggled orthogonally

`run_stages` branches on `request.fast` into `run_fast_stages`: the plan is built
locally by the pure `literal_plan`, and a single call carries a trimmed prompt
(`fast_prompt`) and a trimmed contract (`FAST_ANSWER_SCHEMA` — heading,
paragraph, and list only, under a third the size of `ANSWER_SCHEMA`). The model
resolves to `fast_model` (only `claude` has a curated one: `haiku`), the timeout
to `fast_timeout_secs`, and image fetching is forced off.

Fast is a **toggle over** the search mode, not a fifth mode. `fast` +
`scientific` is meaningful; the four existing modes describe a *domain*, while
fast describes the *pipeline shape*. Making it a `MODES` row would have been
cheaper — `--mode fast` and Tab cycling for free — but would have made the two
concepts mutually exclusive for no good reason.

**Five seconds is a UI threshold, not a deadline.** `FAST_TARGET_SECS` only
decides when the searching screen's elapsed counter turns yellow. A hard abort at
5s would discard answers arriving at 5.5s to satisfy a number; the only hard
ceiling is `fast_timeout_secs` (20s by default).

### Search history is JSON Lines under `$XDG_STATE_HOME`

`~/.local/state/muaddib/history.jsonl`, one object per search, append-only, capped
at 500 entries and compacted on load. `$MUADDIB_HISTORY` overrides the path and is
the test seam, mirroring `$MUADDIB_CONFIG`.

`Up`/`Down` walk it from the search bar; `Ctrl+L` clears it, arming on the first
press and deleting on the second.

## Consequences

- Fast mode goes from three serial round-trips to one, and the plan renders in
  the first frame instead of after a 45s-budget expansion call. Measured on
  `claude` + `haiku`: `rust async runtime tradeoffs` went from ~166s to ~31s
  (5.3x); `capital of peru` from ~29s to ~19s.
- **The five-second goal was not met, and cannot be met here.** Live measurement
  showed the cost is not muaddib's: one `claude -p` call spends ~2s on process start
  and 15–30s inside its own agent loop (large system prompt, thinking block, web
  search, second thinking block), regardless of prompt size. Hitting 5s would
  require calling a model API directly rather than driving an agent CLI —
  precisely the tradeoff ADR-0002 accepted in exchange for needing no API keys.
  `FAST_TARGET_SECS` was kept as a 5s *display* threshold and
  `fast_timeout_secs` raised to 45s so ordinary fast searches never trip it.
- Building this surfaced a latency bug in the **existing** synthesis stage:
  `output_contract` told schema-capable engines to "reply with ONLY the JSON
  answer object" while `--json-schema` requires calling the `StructuredOutput`
  tool. The model satisfied both — writing the answer as text, being told to use
  the tool, then generating the whole answer again. Both contracts now ask for
  the tool call directly: 4 turns and 18.9s became 3 turns and 15.0s on a fast
  search, and every standard synthesis call gets the same saving.
- **Fast mode gives up the URL cross-check.** Standard mode only keeps a source
  if it appears in findings the sub-searches actually returned — two independent
  calls have to agree. With one call there is no second set, so
  `renumber_sources` runs against the answer's own declared sources and the
  safety net narrows to the prompt rule plus post-render link validation. This is
  the reason fast mode is opt-in rather than the default.
- JSON Lines adds **zero dependencies** (`serde_json` is already direct), keeps
  appends `O(1)`, lets one corrupt line be skipped instead of failing the file,
  and carries mode and timestamp that plain text could not.
- Two config paths now exist, config vs. state. Slightly more surface, but it
  keeps machine-appended history out of the hand-edited `config.toml`.
- `Ctrl+F` and `Ctrl+L` shadow `tui-input`'s emacs forward-char and clear-line on
  Home. `Right` still moves the cursor; the loss is acceptable for two
  discoverable, help-listed bindings.

## Alternatives considered

- **Keep three calls, just shrink the prompts and use a small model** — leaves
  three subprocess spawns and three model warm-ups on the clock; the round-trips,
  not the token counts, are the floor.
- **Abort fast searches at exactly 5s** — a crisp contract that throws away work
  finishing moments later, and the wasted call still cost the same time.
- **A fifth `Mode::Fast` row** — cheapest to implement (`--mode fast` and Tab
  cycling come free from the `MODES` table) but conflates a domain axis with a
  pipeline axis and makes "fast and scientific" unexpressible.
- **SQLite via `rusqlite` for history** (Atuin's choice) — right for cross-machine
  sync and rich queries; a heavy C dependency for a capped list of 500 strings.
- **Plain newline-delimited text** (bash, `rustyline::FileHistory`) — simplest
  possible, but carries no mode or timestamp and corrupts on an embedded newline.
- **A `[history]` table in `config.toml`** — mixes machine-appended state into a
  hand-edited file and forces a full rewrite on every search.
