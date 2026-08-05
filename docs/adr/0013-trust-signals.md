# ADR-0013: Trust signals — source credibility and disagreement

- Status: accepted
- Date: 2026-08-04

## Context

The only trust signal muaddib carried was ✓ / ✗: whether the URL answered an HTTP
request. That says nothing about whether a source is a peer-reviewed paper or an
anonymous blog post, how old it is, or whether it is the *only* thing holding up a
claim. Worse, when two sources flatly contradicted each other, synthesis flattened the
disagreement into confident prose — the failure mode the whole "every claim, sourced"
pitch exists to avoid.

The 2026 literature on research agents converges on the same two answers: score source
credibility, and surface contradictions rather than resolving them silently.

## Decision

Two signals, both first-class in the `Answer`, computed in opposite ways on purpose.

### Credibility is deterministic, and the model never touches it

`core/credibility.rs` classifies a source from its host through a `DOMAIN_RULES` table
(`doi.org` and `arxiv.org` → peer-reviewed, `.edu`/`.gov`/`.int` → institutional,
`wikipedia.org`/`docs.rs` → reference, wire services → press, forums and `medium.com`
→ community), with one override: a hit that came from an `Academic` engine
(OpenAlex, Crossref, Semantic Scholar) is peer-reviewed whatever its host.

Matching is on the **host**, never a substring, so `example.com/wikipedia.org/fake`
stays unclassified and `notreddit.com` does not match `reddit.com`. Subdomains do
match (`old.reddit.com`).

`Source` gained `class` and `published`, filled by `annotate_sources` after
`renumber_sources` — deliberately **outside `ANSWER_SCHEMA`**, so the model is never
asked to rate its own sources. muaddib computes them; a model that wanted to inflate
its credibility has no field to write into.

Publication years now travel as `WebHit.published: Option<u16>` instead of being
baked into the snippet string, extracted by the three academic JSON parsers
(`publication_year`, `/issued/date-parts/0/0`, `year`) and range-checked to
1000..=3000.

### Disagreement is a block, not prose

`Block::Conflict { topic, kind, positions }` where each position carries its own claim
and `source_ids`. `ConflictKind` is `Direct | Temporal | Indirect`, following the
existing tolerant-enum pattern (`from = "String"`, unknown degrades to `Direct`), which
matches the three conflict types the literature distinguishes — the temporal case
matters because two sources disagreeing about "current capacity" five years apart are
not really in conflict.

The prompt rule is deliberately restrictive: emit the block **only** when findings
genuinely disagree, never manufacture a disagreement to fill it, and never use it to
contrast a source with the model's own knowledge. `minItems: 2` on `positions` makes a
one-sided "conflict" schema-invalid.

The block is in `ANSWER_SCHEMA` but **not** `FAST_ANSWER_SCHEMA` — fast mode has one
engine call and no cross-checking, so it is exactly the mode with no business
adjudicating conflicts.

### Sole support gets a marker, not a class

`sole_support_sources` finds sources that are the only citation on at least one block.
Those get a dim `!` in the sources list. This is orthogonal to credibility: a
peer-reviewed paper holding up a claim alone is still a single point of failure.

### Model notes only where depth is the point

`ModeSpec.source_notes` is a table column, true only for `Deep`. When set, the
synthesis prompt asks for a one-clause `note` on contested or partisan sources. It is
a mode's property, not a user setting — no flag, no config key.

## Consequences

- The sources list now reads `[1] ✓ ⬢ 2024 ! Title — url (en)`: link health, source
  class, year, and sole-support warning in one line. The line count per source is
  unchanged, so `source_ranges` and the scroll addressing still hold.
- Markdown export carries the same signals as backticked labels plus the note.
- Credibility survives a `--print` and a saved session, because it lives on `Source`.
- `DOMAIN_RULES` is a judgement call encoded as data. It will be wrong at the margins
  (a `.gov` site can be propaganda; a blog can be authoritative). It is a prior, shown
  as a glyph, not a verdict — and being a table, disagreeing with it is a one-line diff.
- `WebHit` and `Source` both gained fields, so every literal in the codebase now uses
  `..Default::default()`. That makes future additions cheap and is why both types now
  derive `Default`.

## Alternatives considered

- **Asking the model to score credibility** — rejected. It adds a round-trip and cost,
  makes the score non-deterministic across runs, and asks the thing being audited to
  audit itself. The deterministic table is testable offline, which the model is not.
- **Putting `class` in `ANSWER_SCHEMA`** — rejected for the same reason: any field the
  model can write is a field it can inflate.
- **A numeric 0–100 credibility score** — rejected as false precision. Six named
  classes are honest about the granularity the evidence actually supports.
- **Rendering conflicts as a paragraph with a warning prefix** — rejected: the
  positions and their sources need structure to survive export, renumbering, and the
  citation walkers. Prose would lose all three.
- **Deriving the year by parsing it back out of the snippet string** — rejected as
  fragile; the parsers already had the value and were discarding it.
