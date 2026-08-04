# ADR-0008: Lexical rerank of grounding hits

- Status: accepted
- Date: 2026-08-04

## Context

The web-search grounding stage (ADR-0007) walks engines as a waterfall and
stops at `max_hits_per_query` deduplicated hits. Ordering is purely positional:
engine-table priority first, then the SERP's own row order. Two `take()` points
discard candidates with no quality signal — the per-engine parser truncation
and the cross-engine budget cap. Engines typically return ~10 rows, so with the
default budget of 5 roughly half of every SERP is thrown away unseen, and a
highly relevant hit from a lower-priority engine can lose its slot to filler
from a higher-priority one.

Perplexica-class engines solve this with embedding similarity, which would
require either a model API (violates keyless) or a bundled embedding model
(violates small-single-binary).

## Decision

Rank the pooled hits lexically with BM25 in pure core, then truncate to the
budget.

- The waterfall over-fetches: it targets `max_hits_per_query ×
  RANK_POOL_FACTOR (3)` deduplicated hits instead of the budget, still inside
  the unchanged 5s per-sub-query deadline.
- `core/rank.rs` scores each hit against the sub-query with BM25 (k1 = 1.2,
  b = 0.75) over a token bag of title (counted twice, a positionless title
  boost) plus snippet. Tokenization is lowercase + split on non-alphanumeric —
  no stemming, no stopword lists, so it works for any language that delimits
  words. Document frequency and average length come from the pool itself.
- Scores stay out-of-band: `WebHit` keeps `PartialEq`/`Eq`; a stable sort over
  `(score, original index)` preserves engine-priority order on ties.
- Every degenerate input (empty query tokens, pool of 0 or 1) returns the
  input order truncated to the budget.

## Consequences

- The 5 hits handed to sub-search prompts are the best of ~15 candidates
  instead of the first 5 encountered, at zero added dependencies and
  microseconds of CPU.
- More engine requests per sub-query (the waterfall keeps walking until the
  pool target or the deadline), still keyless and mailto-polite.
- CJK and other unsegmented scripts degrade to long tokens and effectively
  keep input order — accepted; the fallback is the old behavior, never worse.

## Alternatives considered

- **Embedding rerank (API)** — rejected: needs an API key, violating the core
  keyless constraint.
- **Bundled embedding model** — rejected: tens of MB of weights and an
  inference dependency in a small Rust binary.
- **Simple term-overlap count** — rejected: no length normalization or rarity
  weighting; BM25 costs a handful of extra lines for substantially better
  ordering.
- **Score field on `WebHit`** — rejected: breaks `Eq` and every test literal;
  scores are transient and belong out-of-band.
