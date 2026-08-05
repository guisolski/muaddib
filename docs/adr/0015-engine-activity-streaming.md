# ADR-0015: Streaming what the engine is doing, not what it is writing

- Status: accepted
- Date: 2026-08-04

## Context

A standard search takes minutes, and for most of that time muaddib showed a
spinner and a list of sub-queries with no indication that anything was happening.
The engine was doing plenty — running web searches, fetching pages — and saying
so on stdout, but `engines/cli.rs` called `Command::output()`, which buffers the
child's stdout until the process exits. Nothing could be shown before everything
was known.

The obvious fix, streaming, carried a specific risk. The claude CLI's structured
output rides on `--json-schema`, documented alongside `--output-format json`,
while streaming needs `--output-format stream-json --verbose`. If those two were
mutually exclusive, switching to streaming would trade the anti-hallucination
contract (ADR-0003) for a progress indicator — a bad trade at any speed.

## Decision

### The checkpoint came before the code

Before any of this was written, the combination was run live against the real
CLI. It works: `--json-schema` coexists with `--output-format stream-json`, and
the final `{"type":"result"}` line still carries `structured_output`,
`total_cost_usd`, and `usage` — the same envelope the buffered path already
parsed. The kill condition did not trigger, so streaming applies to **every**
call, synthesis included, not just the schema-free ones.

### Activity, not tokens

muaddib narrates *what the engine is doing*, not *what it is writing*.

The `assistant` lines of the stream carry complete `tool_use` blocks with their
full `input` — one well-formed JSON object per line, no reassembly. That makes
`--include-partial-messages` unnecessary, which in turn keeps the stream an order
of magnitude smaller than token-level streaming would.

`core/stream.rs::activities_in` is pure and table-driven: `STREAM_TOOLS` maps
`WebSearch` → "searching" (target: its query) and `WebFetch` → "reading" (target:
its URL). Anything not in that table is collected and never narrated — including
the model's thinking blocks and the `StructuredOutput` call that carries the
answer itself. Narrating a tool muaddib cannot name would be noise; narrating the
answer's own tool call would leak the answer before the gates ran on it.

### Cosmetic events are droppable by construction

The pipeline's event channel holds 64 slots and every other producer uses
`send().await`. Activity uses `try_send` and discards on `Full`. A chatty engine
must never be able to throttle the search to the render loop's pace, and a lost
progress line costs nothing. The per-call activity channel holds 8.

### The gate is the engine, not the user

`EngineSpec.streams` is a table column. There is no `--stream` flag and no
config key: an engine either emits a line stream muaddib can read or it does
not, and that is a property of the engine, not a preference. A table test
asserts `streams` is true exactly when the argv asks for `stream-json`, so
flipping the boolean without changing the invocation fails the build.

`ParseStrategy::ClaudeJson` now accepts both shapes — whole-buffer JSON first,
then the last `"type":"result"` line — so every existing envelope fixture stays
valid and a non-streaming claude would still parse.

### Token-level answer streaming was not built

The checkpoint cleared it, and it is still not built. Reaching the answer as it
is written means turning on `--include-partial-messages` and reassembling the
`input_json_delta` fragments of the `StructuredOutput` call into a JSON document
that is only partially present — an incremental parser that must decide when a
block is complete enough to render. That is a real parser with real failure modes
(a half-parsed block shown to the user), and it would multiply the volume of the
highest-volume call in the pipeline.

The payoff is small. Synthesis is one of roughly five engine calls, and the
fan-out — which activity streaming now narrates — dominates the wall clock.
`tui/anim.rs::revealed_blocks` already reveals the answer block by block on
arrival; token streaming would stretch that reveal over the last stretch of the
wait rather than introduce it. Deferred, with the checkpoint's result recorded
here so the question does not need re-litigating.

## Consequences

- `run_cli` spawns and reads instead of calling `.output()`. `kill_on_drop(true)`
  and the outer `tokio::time::timeout` are unchanged, so cancellation still reaps
  children. stderr is drained concurrently with stdout — a child that fills the
  stderr pipe while muaddib reads stdout would otherwise deadlock.
- The `Engine` trait gained `run_reporting(job, sink)` with a default
  implementation that drops the sink and calls `run`. Every fake engine in the
  test suite kept working untouched, and non-streaming engines cost nothing.
- Headless `--print` narrates to stderr, which keeps stdout a clean JSON
  contract. The TUI keeps the last three activity lines under the sub-query list,
  deduplicating consecutive repeats.
- Activity is global, not per-sub-query: three concurrent sub-searches interleave
  their lines. Attributing each line to its sub-query would mean threading an
  index through the engine trait for a cosmetic gain.
- claude's stdout is now much larger per call (every assistant message, not just
  the result). It is still accumulated in full, as before, because the envelope
  parse needs the result line and `--print` needs nothing else.

## Alternatives considered

- **Passing the pipeline's `Sender<SearchEvent>` into the engine** — rejected: it
  would make `engines/` depend on `pipeline/`, which depends on `engines/`. The
  sink carries `core::stream::EngineActivity` instead and `pipeline/search.rs`
  translates, keeping the dependency arrow pointing one way.
- **Keeping `args` for JSON and adding a `stream_args` column** (the original
  plan) — rejected once written out: claude would then carry two argv lists, one
  of them permanently dead. One argv plus one boolean says the same thing with
  nothing unreachable.
- **Parsing `content_block_start` events for tool names** — rejected. They carry
  the tool name but not its input, which arrives as `input_json_delta` fragments.
  The `assistant` line has both, already assembled.
- **Narrating every tool the engine calls** — rejected. muaddib cannot describe a
  tool it does not know, and `STREAM_TOOLS` is the honest boundary of what it can
  say. An unknown tool is silence, not a guess.
