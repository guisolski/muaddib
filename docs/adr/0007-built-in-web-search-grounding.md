# ADR-0007: Built-in web-search grounding

- Status: accepted
- Date: 2026-08-03

## Context

ADR-0002 made AI CLIs the only search backend and rejected scraping search
engines outright. That leaves result coverage hostage to whatever the CLI's
built-in search chooses to consult, and the Scientific mode has no direct line
to scholarly indexes. The requirement that emerged: also use the indexes of
conventional engines (Google-class web search) and of academic sources, under a
hard constraint — everything runs inside the muaddib binary. No Docker, no
sidecar service, no API keys.

The 2026 API landscape forecloses the official route: Google's Custom Search
JSON API is closed to new customers and is EOL on 2027-01-01, Microsoft retired
the Bing Search API, and DuckDuckGo and Google Scholar never had official
web-results APIs. Paid SERP proxies (SerpApi and friends) and metasearch
sidecars (SearXNG) exist, but both violate the constraint.

## Decision

Add an in-binary web-search layer that is used **only as grounding input**,
never as the reasoning backend — amending ADR-0002's blanket rejection of
engine scraping.

- Web engines are the keyless server-rendered HTML endpoints (DuckDuckGo
  html/lite, Bing, Mojeek; Google exists as an explicit opt-in best effort).
  Academic sources are official keyless JSON APIs (OpenAlex, Crossref,
  Semantic Scholar) with polite-pool `mailto` support. Every engine is one row
  in the `WEB_ENGINES` table (`core/websearch.rs`) with a pure parser; the
  mode→engine mapping is also a table.
- Hits (title, URL, snippet) are injected into each sub-search prompt as
  candidate leads the AI must verify before citing, and hit URLs join the
  citation allowlist. `merge_snippets = true` additionally merges them as
  findings. The AI CLI remains the only synthesizer.
- Enabled by default with graceful degradation: an HTTP failure, bot
  challenge, markup drift, or timeout yields zero hits and the AI-only
  pipeline continues unchanged. `--no-websearch`, `[websearch] enabled =
  false`, and the config-modal toggle turn it off; fast mode never uses it.

## Consequences

- **"No API keys" stays true.** Every endpoint is keyless; `mailto` is an
  optional courtesy for the academic polite pools.
- Scientific mode now reaches scholarly indexes (which also cover arXiv and
  Google-Scholar-class material) directly, with DOIs as URLs.
- Latency: typically 1–3s before fan-out, hard-capped at 5s per sub-query —
  noise next to 20–60s engine calls.
- Risk accepted: SERP markup drift breaks a parser silently (zero hits, search
  continues). Fixtures under `tests/fixtures/websearch/` are refreshed against
  live responses when a drift is suspected.
- ToS posture, stated rather than hidden: HTML endpoints are queried at
  personal-use volumes — the same approach SearXNG and ddgs take — with the
  browser User-Agent, at most `breadth` requests per engine per search, and
  concurrency 2. Google stays out of the defaults because consent walls and
  captchas make it fail more often than succeed.
- arXiv's own API is deferred: it is Atom XML and would cost an XML dependency
  for one engine already indexed by OpenAlex/Crossref/Semantic Scholar.

## Alternatives considered

- **SearXNG sidecar** — one JSON API aggregating 70+ engines, but a Python
  service (Docker or venv): violates the in-binary, no-external-dependency
  constraint.
- **Websurfx** — metasearch in Rust, but a standalone AGPL-3.0 application,
  not an embeddable library, and license-incompatible for code reuse.
- **Official search APIs** — Google CSE closed to new customers, Bing retired,
  SERP proxies paid; all would also reintroduce key management.
- **Prompting the AI CLI harder** — no guarantee any particular index is
  consulted; that is exactly the status quo this ADR improves on.
