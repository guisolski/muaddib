# ADR-0009: Page-content grounding

- Status: accepted
- Date: 2026-08-04

## Context

Web-search grounding (ADR-0007) hands the AI title + URL + snippet and tells
it to verify candidates with its own built-in web search. That works, but the
verification cost lives inside the AI CLI call — the dominant latency the
fast-mode measurements exposed (ADR-0006) — and a 300-char SERP snippet is a
thin evidence base for Scientific mode, where the answer's value hangs on what
the cited page actually says. Perplexica-class engines fetch and read the top
result pages instead of trusting snippets.

The constraints stand: single binary, no keys, no new dependencies, silent
degradation.

## Decision

Fetch the top-ranked hit pages and feed extracted text into the sub-search
prompts, by default only in the modes where depth beats latency.

- A new pipeline stage (`pipeline/pages.rs`) runs between web-search grounding
  and the fan-out when the mode is listed in `[websearch] ground_modes`
  (default `["scientific", "deep"]`; empty list disables). It fetches the top
  `ground_top_n` (default 3, 1..=5) reranked hits per sub-query — each unique
  URL once — with a 4s per-request timeout, `text/html`/`application/xhtml`
  only, a 2 MiB body cap, 4 concurrent fetches, and a 10s stage deadline that
  keeps whatever finished.
- Extraction is a readability-lite pass in pure core (`core/readability.rs`)
  using the `scraper` dependency the websearch feature already carries: first
  matching content root from `article` → `main` → `[role=main]` → `body`,
  noise subtrees (`script`, `style`, `nav`, `header`, `footer`, `aside`,
  `form`, `noscript`) dropped, whitespace normalized, excerpt truncated to
  `ground_page_chars` (default 4000, 500..=20000).
- The excerpts join each sub-search prompt as a "fetched page content" block
  after the candidate-sources block; an empty page list leaves the prompt
  byte-identical to the ungrounded one. `PageFetched { url, ok }` events
  stream progress to the TUI and `--print` stderr.
- `WebFetcher` gained a `fetch_page` method with a default `None`
  implementation, so test fakes and the no-op fetcher stay untouched.

## Consequences

- Sub-searches in Scientific and Deep modes reason over actual page text, not
  snippets, and cite URLs whose content was really seen — better findings and
  fewer verification round-trips inside the AI call.
- Up to `ground_top_n × ground_page_chars` (~12 KB by default) more prompt per
  sub-search, and up to 10s more wall clock before the fan-out in grounded
  modes. General and News keep today's latency.
- Fetching pages is bounded scraping of arbitrary sites: capped, typed, and
  deadline-boxed, with every failure silent — the stage can only add, never
  break.

## Alternatives considered

- **Grounding all modes by default** — rejected: General and News are
  latency-sensitive; the mode list keeps the tradeoff explicit and
  configurable.
- **A readability crate (`readability`, `html2text`, `dom_smoothie`)** —
  rejected: new dependencies for a pass that `scraper` already enables in a
  few dozen lines.
- **Fetching inside the web-search waterfall** — rejected: page fetches would
  starve the 5s per-sub-query hit deadline; a separate stage gets its own
  budget.
- **Feeding page text only into synthesis** — rejected: the sub-searches are
  where findings are born; grounding them improves everything downstream.
