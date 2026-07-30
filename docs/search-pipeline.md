# Search pipeline

`pipeline/search.rs` orchestrates six stages. All planning, merging, and
citation logic is pure (`src/core/`); the pipeline only sequences engine calls
and emits events.

## Stages

### 1. Expand

One engine call (45s timeout) rates the query's complexity and asks for at most
`breadth` sub-queries as JSON:

```json
{"complexity": "simple|standard",
 "subqueries": [{"query": "...", "lang": "BCP-47", "rationale": "..."}]}
```

The prompt demands distinct facets and — for cross-language modes — at least one
sub-query in another relevant language. `plan_from_expansion` (pure) validates
each row, guarantees the original query is included first, and truncates to
breadth. **Any failure degrades, never aborts**: a per-mode fallback facet table
(`fallback_expansion`) produces a deterministic plan offline.

Breadth comes from the mode (`General` 3, `Scientific` 4, `News` 3, `Deep` 6)
unless `expansion_breadth` overrides it (1–8). A `"simple"` complexity rating —
the model judging that one direct search fully answers the query — narrows the
plan to a single sub-query, so simple questions skip the fan-out cost entirely
and reach synthesis with a small findings payload. An absent, unknown, or
malformed rating keeps the full breadth.

### 2. Fan-out

Sub-searches run through `futures::stream::buffer_unordered(max_parallel)` —
results are consumed as they land, so one slow search never blocks the others.
Each sub-search prompt demands findings with exact URLs:

```json
{"summary": "...", "findings": [{"claim": "...", "source_title": "...",
  "source_url": "https://...", "lang": "...", "image_url": "https://..."}]}
```

`image_url` is optional: the direct URL of a relevant image (photo, chart,
figure) on the consulted page. A failed sub-query is dropped (with a `SubQueryFinished { ok: false }` event);
the pipeline continues as long as at least one succeeds and produces findings.

### 3. Merge (pure)

`merge_sub_results` deduplicates findings by *(normalized URL, normalized
claim)*. URL normalization lowercases scheme and host, strips fragments and
trailing slashes, and preserves path case. Findings without an `http(s)` URL are
discarded here — they could never be cited.

### 4. Synthesize

One final engine call receives the merged findings as JSON plus the answer
schema (`core/answer.rs::ANSWER_SCHEMA` — via `--json-schema` on claude,
inlined in the prompt otherwise) and the instruction to answer entirely in the
configured language, citing `source_ids` on every block, using **only** URLs
present in the findings. The prompt favors compact visual blocks — short
paragraphs, lists, tables, charts — and asks for a `diagram` block (`flow` for
processes and causal chains, `timeline` for chronologies) that visualizes the
answer's core structure, plus an `image` block (url + caption) when a finding
carries an `image_url` worth showing.

Then two pure gates enforce the contract. `eject_unknown_images` removes every
`image` block whose normalized URL is not among the findings' `image_url`
values — the same anti-hallucination rule sources get. `renumber_sources` then:

- ejects sources whose normalized URL is absent from the findings
  (anti-hallucination gate),
- drops dangling `source_ids` and deduplicates repeats,
- renumbers sources 1..n in first-use order and prunes unused ones.

### 5. Validate links

With the `link-validation` feature (default) and `validate_links = true`, every
source URL gets an HTTP HEAD request — 8 concurrent, 8s timeout, up to 5
redirects; 403/405/501 retry as `GET` with `Range: bytes=0-0` (some servers
reject HEAD). Results stream to the UI as `LinkChecked` events: ✓, ✗ 404, or
✗ unreachable.

### 6. Fetch images

With `images = true` (default) every surviving `image` block's URL is
downloaded — 4 concurrent GETs, 5 MB cap — and streamed to the UI as
`ImageFetched` events carrying the raw bytes (or `None` on failure). The TUI
decodes and renders them with the terminal's best graphics protocol (kitty,
iTerm2, sixel) and falls back to unicode half-blocks everywhere else; a failed
fetch degrades to an "image unavailable" note. Headless `--print` runs skip
this stage — the JSON answer carries the image URLs themselves.

## Event protocol

```rust
enum SearchEvent {
    PlanReady(SearchPlan),
    SubQueryStarted { idx },
    SubQueryFinished { idx, ok },
    SynthesisStarted,
    AnswerReady(Box<Answer>),
    LinkChecked { source_id, status },
    ImageFetched { url, bytes },
    Completed,
    Failed(String),
}
```

Events flow over a `tokio::sync::mpsc` channel owned by `SearchHandle`.
Dropping the handle aborts the task and (via `kill_on_drop`) any running CLIs —
that is all Esc does.

## Failure matrix

| Failure | Behavior |
|---|---|
| Expansion call fails / returns garbage | fallback facet table, search continues |
| Some sub-queries fail | dropped; the rest proceed |
| All sub-queries fail | `Failed("every sub-query failed…")` |
| No findings with usable URLs | `Failed("…no findings with usable sources")` |
| Synthesis fails / invalid JSON | `Failed` with the reason |
| Synthesis invents a URL | source ejected, citation dropped |
| Synthesis invents an image URL | image block removed from the answer |
| Link check fails | source marked ✗, answer unaffected |
| Image fetch fails or is not an image | "image unavailable" note, answer unaffected |
