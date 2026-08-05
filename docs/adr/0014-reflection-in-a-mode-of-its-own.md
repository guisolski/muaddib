# ADR-0014: Reflection lives in a mode of its own

- Status: accepted
- Date: 2026-08-04

## Context

muaddib synthesizes once and ships. Whatever the fan-out happened to find is the
whole evidence base, and nothing ever asks whether the answer it produced actually
covers the question. The 2026 literature on research agents is consistent that a
critic pass — read the draft back, name what is missing, search only that — is the
cheapest available quality gain.

It is not free, though. It costs one extra engine round-trip minimum, and a full
extra fan-out plus a second synthesis when the critic finds anything. The first
evaluation run (`docs/eval-baseline.md`) measured `Deep` at roughly 190 seconds and
$1 per query. Bolting a reflection round onto `Deep` would have pushed both up for
every user who never asked for it.

## Decision

### A new mode, not a new phase of an existing one

`Mode::Exhaustive` — breadth 6, cross-language, `source_notes: true`, and the only
row in `MODES` with `reflect_rounds: 1`. `Deep` keeps exactly the latency it has
today.

The gate is `ModeSpec.reflect_rounds`, a table column. Not a CLI flag, not a config
key: the mode the user picked decides. Picking a mode named `Exhaustive` *is* the
opt-in to a slower search, which is the honest place for that choice to live.

`MAX_REFLECT_ROUNDS` is 1 and a table test enforces it. One round is where the
evidence for reflection actually is; round two mostly re-finds round one's gaps, and
each round multiplies both the wall clock and the bill.

### The critic reads the exported Markdown

`reflection_prompt` hands the critic the draft rendered through
`core/export.rs::to_markdown` — the same document the `e` key writes to disk. Reusing
the export renderer means the critic sees the answer as the user would, including the
citation markers and the source list, and it keeps one renderer instead of two.

The critic also receives the list of sub-queries already run and is told not to repeat
them. That instruction is advisory; `gaps_from_reflection` enforces it, dropping any
gap whose query matches an existing sub-query case- and whitespace-insensitively, and
any gap that repeats an earlier gap. The cap is `MAX_REFLECTION_GAPS` (3).

The prompt is written so that returning nothing is easy: *"An empty list is the right
answer more often than not; do not invent a gap to look thorough."* A critic that
always finds three gaps is a critic that has learned to perform diligence, and it
would triple the cost of every exhaustive search for nothing.

### No new orchestration

The gaps go back through the *existing* stages. `gather_stage` — web-search
grounding, page grounding, fan-out — is the same function the first round calls,
given a plan whose `sub_queries` are the gaps and a sub-query index `offset` so the
second round's `SubQueryStarted` events append to the TUI's progress list instead of
overwriting the first round's ticks. `merge_sub_results` already deduplicates, so
re-merging the combined findings is safe by construction.

The reflection round can therefore only *add* findings. There is no path by which it
removes evidence the first round found.

### The draft always survives

The whole round runs under one `reflection_timeout` budget: a critique, a fan-out,
and a scaled re-synthesis (≈18 minutes at the default 180s base with breadth 6). Five
things can go wrong — the budget expires, the critic call fails, the critic returns
unparsable JSON, every gap search fails, or the second synthesis fails — and all five
land in the same place: the draft ships unchanged. This is the same "degrades, never
aborts" rule as every other stage, and it is what makes an unbounded critic loop safe
to run at all.

## Consequences

- `Exhaustive` costs at least one extra engine call over `Deep`, and up to a fan-out
  plus a synthesis more. That is the deal the mode name makes.
- `ReflectionGaps` carries the gap sub-queries themselves rather than a count, so the
  searching screen can append them to the visible plan and tick them off as they run.
  A count would have left the user watching sub-queries execute that the screen never
  listed.
- `SearchState.web_hits` now accumulates across rounds instead of being overwritten,
  since a second round of grounding would otherwise hide the first round's hits.
- The reflection prompt has no `MUADDIB:REFLECT` counterpart in `FAST_ANSWER_SCHEMA`
  or fast mode. Fast mode is one call; a critic pass is the opposite of its point.
- Whether the critic's judgement is *good* is not something the test suite can prove.
  The tests prove the plumbing: gaps are parsed, deduplicated, capped, searched, and
  re-synthesized, and every failure path ships the draft. Whether a real model finds
  real gaps is a question for `make eval`, and the golden set has no case built to
  answer it yet.

## Alternatives considered

- **Adding a reflection round to `Deep`** — rejected. It taxes every existing `Deep`
  user for a feature they did not ask for, and the eval baseline had just measured
  what `Deep` costs today.
- **A `reflect = true` config key** — rejected on the project's no-flags rule. A
  setting nobody discovers is a feature nobody uses; a mode in the `Tab` cycle is
  discoverable by pressing `Tab`.
- **Letting the critic rewrite the draft directly** — rejected. It would give the
  critic authority over the answer text without giving it any new evidence, which is
  a licence to hallucinate. The critic may only propose *searches*; the evidence gate
  in `renumber_sources` still stands between the results and the answer.
- **Looping until the critic reports no gaps** — rejected as unbounded in both time
  and money, and prone to a critic that never converges. One round with a hard budget
  is a cost the user can predict.
- **Verifying individual claims instead of finding coverage gaps** — a real
  alternative, and a better one for factual accuracy. It needs a per-claim
  verification protocol rather than a sub-query list, so it does not reuse the
  existing fan-out. Deferred rather than dismissed.
