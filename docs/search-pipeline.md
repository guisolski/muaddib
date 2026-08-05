# Search pipeline

`pipeline/search.rs` orchestrates eight stages, plus a ninth that only
`Exhaustive` mode pays for. All planning, merging, and citation logic is pure
(`src/core/`); the pipeline only sequences engine calls and emits events.

`request.fast` selects a second, much shorter path — see
[Fast mode](#fast-mode-one-call) below.

## Stages

### Follow-up context

A follow-up search (branched from a research-tree node — ADR-0010) carries a
`ResearchContext` on the request: the ancestor path root→parent, capped at 4
steps, each with the query, a ≤500-char answer digest, and up to 5 cited
source URLs (`core/context.rs`). `context_prompt_block` injects it into the
expansion, synthesis, and fast prompts — an empty context leaves every prompt
byte-identical to a fresh search — and the step URLs join the synthesis
allowlist so the new answer may re-cite the ancestors' sources.

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

Breadth comes from the mode (`General` 3, `Scientific` 4, `News` 3, `Code` 3,
`Forums` 3, `Deep` 6, `Exhaustive` 6)
unless `expansion_breadth` overrides it (1–8). A `"simple"` complexity rating —
the model judging that one direct search fully answers the query — narrows the
plan to a single sub-query, so synthesis gets a small findings payload. An
absent, unknown, or malformed rating keeps the full breadth.

Note what this rating does *not* save: sub-searches already run concurrently, so
collapsing N of them to one leaves the three serial engine round-trips intact.
Cutting round-trips is what fast mode is for.

### 2. Web-search grounding

With `[websearch] enabled = true` (default; forced off in fast mode and by
`--no-websearch`), each sub-query is first run against conventional search
engines and — in Scientific mode — scholarly APIs, all declared as rows in the
`WEB_ENGINES` table (`core/websearch.rs`). Engines are walked as a waterfall
per sub-query (a configured SearXNG instance leads; DuckDuckGo falls back to its
lite endpoint; academic engines come first in Scientific mode; an engine whose
URL cannot be resolved is skipped), over-fetching a pool of `max_hits_per_query ×
3` deduplicated hits, with 2 sub-queries in flight, a 3s per-request timeout,
and a 5s per-sub-query deadline that keeps whatever partial hits arrived. The
pool is then ranked against the sub-query with BM25 (`core/rank.rs` — pure
lexical scoring over title-doubled-plus-snippet token bags, stable ties keep
engine order) and truncated to `max_hits_per_query`, so the surviving hits are
the most relevant of the pool rather than the first encountered (ADR-0008).
One `WebHits { count }` event reports the total.

Modes that declare `site_hints` (`Code`, `Forums`) send a widened query to the web
engines only — `web_query` appends `(site:a OR site:b)` — while the AI sub-search
prompt and the BM25 reranking both keep the plain query, so the operators steer
retrieval without polluting either the model's prompt or the lexical score.

The hits ground the next stage: each sub-search prompt gains a "candidate
sources" block (title, URL, snippet) the AI is told to verify before citing,
and hit URLs join the synthesis allowlist so a cited candidate survives the
anti-hallucination gate. With `merge_snippets = true` the hits are also merged
into the findings themselves (claim = snippet). **Every failure is silent**:
a blocked engine, drifted markup, or timeout contributes zero hits and the
pipeline continues exactly as an AI-only search — this stage can only add,
never break.

### 3. Page-content grounding

With the current mode listed in `[websearch] ground_modes` (default:
`scientific`, `deep`, and `exhaustive`), the top `ground_top_n` reranked hits of each
sub-query have their pages fetched (`pipeline/pages.rs`) — GET with a 4s
per-request timeout, `text/html`/`application/xhtml` only, 2 MiB cap, 4
concurrent fetches, all under a 10s stage deadline that keeps whatever
finished. Each page is fetched once even when several sub-queries share it.
A readability-lite pass (`core/readability.rs`) picks the first matching
content root (`article`, `main`, `[role=main]`, `body`), drops noise subtrees
(`script`, `style`, `nav`, `header`, `footer`, `aside`, `form`, `noscript`),
normalizes whitespace, and truncates to `ground_page_chars`. The excerpts
join the matching sub-search prompts as a "fetched page content" block after
the candidate sources. One `PageFetched { url, ok }` event streams per
attempt. **Every failure is silent**: a dead page, a non-HTML response, an
oversized body, or the deadline just means no page block for that URL
(ADR-0009).

### 4. Fan-out

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

An engine whose `EngineSpec.streams` is set (only `claude` today) is read line by
line rather than buffered to completion, so its own tool calls surface while the
call is still running: `EngineActivity { label, target }` events report each
`WebSearch` and `WebFetch` the engine makes (ADR-0015). They are cosmetic and
sent with `try_send`, so a chatty engine is throttled by dropping lines rather
than by stalling the pipeline. This applies to every stage's engine call, not
just the fan-out.

### 5. Merge (pure)

`merge_sub_results` deduplicates findings by *(normalized URL, normalized
claim)*. URL normalization lowercases scheme and host, strips fragments and
trailing slashes, and preserves path case. Findings without an `http(s)` URL are
discarded here — they could never be cited.

### 6. Synthesize

One final engine call receives the merged findings as JSON plus the answer
schema. Its timeout is **not** the flat `engine_timeout_secs`: synthesis is the one
call whose input grows with the plan, so `synthesis_timeout` multiplies the base
budget by one unit per three sub-queries, capped at 3×. A `Deep` search (breadth 6)
therefore gets 360s where a `General` one gets 180s. The first evaluation run found
this the hard way — `Deep` reliably timed out at a flat 180s while every narrower
mode fit comfortably. Sub-searches keep the flat budget: each one is the same size
regardless of how many there are.

The schema (`core/answer.rs::ANSWER_SCHEMA`) travels via `--json-schema` on claude and
inlined in the prompt otherwise, together with the instruction to answer entirely in the
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

`annotate_sources` then fills each surviving source's `class` and `published` from the
grounding hits (ADR-0013). Those fields are deliberately absent from `ANSWER_SCHEMA`:
muaddib computes credibility, the model never declares it.

A `conflict` block is available to synthesis but restricted by prompt to findings that
genuinely disagree, with at least two positions each carrying their own `source_ids`.
It is absent from `FAST_ANSWER_SCHEMA` — one engine call has nothing to cross-check
against, so fast mode has no business adjudicating a conflict.

### 6.5 Reflect (Exhaustive only)

`ModeSpec.reflect_rounds` is 0 for every mode but `Exhaustive`, where it is 1.
When it is non-zero the answer produced by stage 6 is a **draft**, and one critic
call reads it back.

The critic receives the draft rendered through `core/export.rs::to_markdown` —
the same Markdown a user gets from `e` — plus the list of sub-queries already
searched, and returns coverage gaps in the sub-query shape the planner already
validates:

```json
{"gaps": [{"query": "...", "lang": "BCP-47", "rationale": "..."}]}
```

`core/reflect.rs::gaps_from_reflection` (pure) validates each row, drops any gap
that repeats a sub-query already searched (case- and whitespace-insensitive), and
caps the list at `MAX_REFLECTION_GAPS` (3). An empty list is the expected answer
and the prompt says so: the critic is told not to invent a gap to look thorough.

Surviving gaps go back through the **existing** stages 2–5 — web-search
grounding, page grounding, fan-out, merge — and stage 6 runs again over the
combined findings. No new orchestration exists: `gather_stage` is the same
function the first round calls, with a sub-query index `offset` so the second
round's `SubQueryStarted` events append to the progress list instead of
overwriting it.

**The draft always survives.** The whole round runs under one
`reflection_timeout` budget (a critique + a fan-out + a scaled re-synthesis, so
~18 min at the default 180s base with breadth 6). If the budget expires, the
critic call fails, the gap searches all fail, or the second synthesis fails, the
draft ships unchanged — the same "degrades, never aborts" rule as every other
stage. A reflection round can only add findings, never subtract them.

### 7. Validate links

With the `link-validation` feature (default) and `validate_links = true`, every
source URL gets an HTTP HEAD request — 8 concurrent, 8s timeout, up to 5
redirects; 403/405/501 retry as `GET` with `Range: bytes=0-0` (some servers
reject HEAD). Results stream to the UI as `LinkChecked` events: ✓, ✗ 404, or
✗ unreachable.

### 8. Fetch images

With `images = true` (default) every surviving `image` block's URL is
downloaded — 4 concurrent GETs, 5 MB cap — and streamed to the UI as
`ImageFetched` events carrying the raw bytes (or `None` on failure). The TUI
decodes and renders them with the terminal's best graphics protocol (kitty,
iTerm2, sixel) and falls back to unicode half-blocks everywhere else; a failed
fetch degrades to an "image unavailable" note. Headless `--print` runs skip
this stage — the JSON answer carries the image URLs themselves.

## Fast mode: one call

`Ctrl+F` in the TUI, `--fast` on the CLI. `run_stages` branches into
`run_fast_stages`, which collapses stages 1–6 into a single engine call
(web-search grounding is skipped — latency first):

1. **Plan locally.** `literal_plan` (pure) wraps the query as the one and only
   sub-query. No engine call, so `PlanReady` reaches the UI in the first frame.
2. **One call.** `fast_prompt` asks for one web search, at most 4 consulted
   pages, at most two short paragraphs or one list, and *only* heading, paragraph,
   and list blocks. The contract is `FAST_ANSWER_SCHEMA` — the same `Answer`
   type, with chart, diagram, image, quote, and table stripped out. It is under
   a third the size of `ANSWER_SCHEMA`, which matters twice: it is inlined into
   the prompt for engines without `--json-schema`, and it constrains claude's
   structured output more tightly.
3. **Guard the output.** `strip_image_blocks` removes any image the model emitted
   anyway; `renumber_sources` then runs against `self_declared_urls(&answer)` —
   the answer's own `sources`, filtered to real `http(s)` URLs.
4. **Validate links** as usual (stage 7). Stage 8 never runs: `from_config`
   forces `fetch_images = false` whenever `fast` is set.

The model comes from `[engines.<name>] fast_model`, else the engine table's
`fast_model` (only `claude` has one: `haiku`), else the normal configured model.
The timeout is `fast_timeout_secs` (default 20s, clamped 5..=120).

### Measured latency, and why 5s is out of reach

Against `claude` + `haiku`, wall clock:

| Query | Standard | Fast |
|---|---|---|
| `capital of peru` (rated `simple`) | ~29s | ~19s |
| `rust async runtime tradeoffs` (3 sub-searches) | ~166s | ~31s |

A 5.3x improvement on a real query — but **not the "under five seconds" this mode
was aimed at, and the gap is structural.** muaddib's own overhead is negligible; the
cost lives inside one `claude -p` invocation: process start (~2s), a large agent
system prompt, a thinking block, the `WebSearch` round-trip, and a second
thinking block before the structured answer. That is 15–30s regardless of how
short the prompt is. Reaching 5s would mean calling a model API directly instead
of driving an agent CLI — which is exactly the tradeoff ADR-0002 rejected.

`FAST_TARGET_SECS` (5s) is therefore only a UI threshold: it decides when the
elapsed counter turns yellow. Nothing is aborted at 5s, and `fast_timeout_secs`
defaults to 45s so a normal fast search never trips it.

### The double-generation trap

Both `output_contract` and `fast_output_contract` tell schema-capable engines to
**call the `StructuredOutput` tool**, never to "reply with only JSON". Those two
instructions look equivalent and are not: asking for text while `--json-schema`
is active makes the model write the whole answer as a fenced block, get told it
must use the tool, and then generate the entire answer a second time. Measured on
a fast search, that was 4 turns and 18.9s instead of 3 turns and 15.0s — and it
was silently costing every standard synthesis call too.

### What fast mode gives up

Standard mode cross-checks synthesized URLs against URLs the sub-searches
actually returned — two independent engine calls have to agree before a source
survives. Fast mode has only one call, so there is no second set to check
against: it relies on the prompt's "never invent or guess a URL" rule plus the
post-render link validation to flag anything dead. Trading that cross-check for
latency is the whole point of the mode, and it is why fast mode is a deliberate
opt-in rather than the default.

## Event protocol

```rust
enum SearchEvent {
    PlanReady(SearchPlan),
    WebHits { count },
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
| Web engine blocked, captcha, or markup drift | zero hits from that engine, waterfall tries the next one |
| All web engines fail or time out | no grounding block in the prompts, AI-only search continues |
| Some sub-queries fail | dropped; the rest proceed |
| All sub-queries fail | `Failed("every sub-query failed…")` |
| No findings with usable URLs | `Failed("…no findings with usable sources")` |
| Synthesis fails / invalid JSON | `Failed` with the reason |
| Fast call fails, times out, or returns invalid JSON | `Failed("fast search …")`; nothing to degrade to |
| Fast answer cites a source it never declared | citation dropped, source ejected |
| Synthesis invents a URL | source ejected, citation dropped |
| Synthesis invents an image URL | image block removed from the answer |
| Link check fails | source marked ✗, answer unaffected |
| Image fetch fails or is not an image | "image unavailable" note, answer unaffected |
