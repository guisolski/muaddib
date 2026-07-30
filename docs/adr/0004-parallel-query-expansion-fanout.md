# ADR-0004: Parallel query expansion with bounded fan-out and pure fallback

- Status: accepted
- Date: 2026-07-30

## Context

faro's differentiator is that it does not search only the literal query text: it
expands into related facets and other languages. Engine calls take tens of
seconds each; running expanded sub-queries sequentially would make the product
unusably slow, and depending on a model call for expansion would add a hard
failure point.

## Decision

- **Expansion is one engine call** producing sub-queries as JSON; the plan
  always includes the original query first and is truncated to a per-mode
  breadth (`MODES` table; `Deep` = 6, others 3–4).
- **If expansion fails in any way**, a deterministic per-mode facet table
  (`fallback_expansion`) produces the plan offline. The search degrades, never
  dies.
- **Sub-searches fan out concurrently** through
  `futures::stream::buffer_unordered(max_parallel)` (config-clamped 1–8):
  results are consumed in completion order, so wall-clock time approaches the
  slowest single search, not the sum.
- **Failures are per-item.** A failed sub-query emits a failure event and is
  dropped; the pipeline proceeds with whatever succeeded.
- **Cancellation is structural.** The whole search lives in one tokio task
  behind a `SearchHandle`; dropping it aborts the task and `kill_on_drop`
  reaps every child CLI.

## Consequences

- A Deep search with 6 sub-queries costs roughly one sub-search of wall-clock
  time beyond expansion and synthesis.
- The offline fallback makes the pipeline testable and demo-able with no
  network and keeps behavior deterministic under failure.
- Cost: bounded concurrency multiplies CLI usage (and any per-call billing) by
  the breadth; the bound and breadth are both user-configurable.

## Alternatives considered

- **Sequential sub-searches** — simplest, unacceptably slow.
- **Unbounded parallelism** — spawning N CLIs at once invites rate limits and
  memory spikes; a config-clamped bound is safer.
- **Expansion always via model with no fallback** — one flaky call would kill
  the whole search; rejected.
