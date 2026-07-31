# ADR-0002: AI CLIs as subprocess search backends

- Status: accepted
- Date: 2026-07-30

## Context

muaddib needs web search plus analysis and synthesis. The obvious implementations
are: call model HTTP APIs directly, scrape search engines, or drive the AI CLIs
users already have (`claude`, `cursor-agent`, `codex`, `opencode`).

## Decision

Drive locally installed AI CLIs as subprocesses. Each engine is one declarative
row in the `ENGINES` table (binary, argv, parse strategy, capabilities); a
single `CliEngine` adapter executes any row. The prompt is always passed as one
argv element — no shell interpolation. Availability is detected at startup and
degrades gracefully.

## Consequences

- **Zero API-key management.** muaddib reuses the user's existing CLI auth and
  billing; nothing is stored or proxied.
- **Web search for free.** Claude Code's WebSearch/WebFetch tools give real
  browsing without muaddib implementing a crawler.
- **Adding an engine is one table row** plus, at most, an envelope fixture and
  a parse test.
- Cost: CLI output is only semi-structured; robustness comes from two tolerant
  layers (envelope parsing, then strategy-table JSON extraction) instead of a
  typed API.
- Cost: subprocess latency per call. Mitigated by parallel fan-out and by
  `kill_on_drop` cancellation.
- Risk accepted: CLI flag surfaces drift. The live checkpoint recipe in
  docs/development.md exists precisely to catch this (it already caught the
  variadic `--allowedTools` swallowing the prompt).

## Alternatives considered

- **Direct model APIs (HTTP)** — faster and typed, but requires users to
  provision keys per provider and reimplements web search.
- **Scraping search engines** — brittle, ToS-hostile, and provides no synthesis.
- **A single hardcoded engine** — simpler, but the product explicitly demands a
  user-selectable engine.
