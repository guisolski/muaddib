# ADR-0003: A structured JSON answer schema as the AI-renderer contract

- Status: accepted
- Date: 2026-07-30

## Context

The answer must render rich content in a terminal (headings, lists, tables, bar
charts) and every piece of information must carry a valid source link. Free-form
markdown from a model is unverifiable: citations cannot be checked, charts
cannot be extracted, and rendering degenerates into regex parsing.

## Decision

Define one JSON document type (`core/answer.rs`) as the single contract between
the synthesis call and the renderer:

- `blocks`: a tagged union — `heading`, `paragraph`, `list`, `quote`, `table`,
  `chart` — where every content block carries `source_ids`.
- `sources`: numbered `{id, title, url, lang}` entries.
- `followups`: suggested next queries.

The compact JSON Schema (`ANSWER_SCHEMA`) is enforced by `--json-schema` on
engines that support it and inlined into the prompt otherwise. Unknown block
types deserialize to a `Unknown` variant and are skipped, so newer engines can
emit newer blocks without breaking older faro binaries.

## Consequences

- Rendering is a pure data transformation (`blocks_to_lines`) that is unit
  tested per block type — no markdown parsing, no heuristics.
- Citation integrity is mechanical: `renumber_sources` can eject sources whose
  URLs never appeared in the findings and renumber the rest deterministically.
- Charts are first-class data (`labels`/`values`), rendered by a pure widget.
- Cost: models occasionally produce near-miss JSON; mitigated by the extraction
  strategy table and schema enforcement where available.

## Alternatives considered

- **Markdown answers** — human-friendly but unverifiable and unrenderable as
  structured widgets; rejected on the every-claim-cited requirement.
- **Per-engine bespoke formats** — multiplies parsers and couples the renderer
  to engines; rejected as a DRY violation.
