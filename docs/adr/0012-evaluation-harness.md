# ADR-0012: Evaluation harness and quality baseline

- Status: accepted
- Date: 2026-08-04

## Context

The test suite proved the pipeline *worked* — stages fire in order, hallucinated
sources are ejected, fast mode collapses to one call — but nothing measured whether
the answers were any *good*. Every prompt edit was therefore unfalsifiable: the fake
engine returns fixed fixtures, so a prompt change that quietly halved citation
coverage would pass CI green.

The constraint that shapes everything here: real quality measurement needs a real AI
CLI, which costs money, takes minutes, and is non-deterministic. CI must never invoke
one (ADR-0002's whole premise is that muaddib drives the user's own CLI).

## Decision

Split the harness in two, along the purity boundary the project already has.

- **Metrics are pure** (`core/eval.rs`). `score_answer` takes an `Answer`, an
  `EvalCase`, a wall-clock reading, and optional usage, and returns a `CaseReport`.
  Nothing in it does I/O, so the scoring itself is unit-tested offline in the ordinary
  table-driven style and runs in CI on every commit.
- **The run is a live, ignored test** (`tests/eval_live.rs`, `#[ignore]`). It drives
  the same `spawn_search` the binary uses, over a golden suite in
  `tests/fixtures/eval/cases.toml`, and is reached only through `make eval`.
  `cargo test` skips it by default, so CI stays keyless and free.
- **Only objective metrics.** Uncited blocks, broken links, source count, coverage of
  expected domains and expected mentions, wall clock, cost. Every one is checkable
  without a human judging prose. An LLM-as-judge score was rejected: it would make the
  regression signal itself non-deterministic and cost a second round of engine calls
  per case.
- **The baseline is committed** (`docs/eval-baseline.md`, written by the run). A
  quality regression then shows up as a *diff in a tracked file* during review, which
  is the entire point — an uncommitted score nobody reads changes no decisions.

`EvalCase.expect_domains` and `expect_mentions` are deliberately loose: they assert
that a search about Peru's capital mentions Lima and that a Tokio question cites
`tokio.rs`, not that any particular sentence appears. Tight expectations would break
on every legitimate rewording and train the reader to ignore the report.

## Consequences

- Prompt, schema, and grounding changes now have a before/after to point at. The
  workflow is: run `make eval` before the change, commit the baseline, make the
  change, run it again, and read the diff.
- The baseline is only as current as the last person who ran it. It carries a header
  saying so, and re-generating costs a few minutes and a few cents.
- Coverage of an empty expectation list is defined as 1.0, so a case can assert
  nothing and still contribute latency and cost numbers.
- `block_source_id_slots` became `pub` in `core/citations.rs` so uncited-block
  counting reuses the same walker that renumbering uses, rather than a second match
  over `Block` that could drift from it.

## Alternatives considered

- **A separate `muaddib-eval` binary** — rejected: the project's pitch is a single
  small binary, and an ignored integration test needs no new artifact, no new `[[bin]]`
  entry, and no dead weight in the release build.
- **Running the eval in CI against a real CLI** — rejected: needs credentials in CI,
  costs money per push, and would make the build flaky on a rate limit.
- **Scoring against the fake engine so it could run in CI** — rejected as
  circular: the fixtures are fixed, so every metric would be a constant and the report
  would only ever restate what the fixtures already say.
- **LLM-as-judge answer grading** — rejected for v1; see above. It is the natural
  extension once the objective floor is in place.
