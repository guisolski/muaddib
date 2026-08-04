# ADR-0010: Research tree and sessions

- Status: accepted
- Date: 2026-08-04

## Context

muaddib was strictly one-shot: each search wiped the previous answer, and a
follow-up question started from zero. Scientific research is iterative — a
question spawns better questions, and the value of the third search depends on
what the first two established. The requirement: follow-up searches that carry
conversational context, organized behind the scenes as a **research tree** the
user can navigate and branch from, recording how the investigation progressed
(queries, answers, sub-queries, pages consulted). Constraints unchanged:
single binary, TUI-first, pure core.

## Decision

Every completed search becomes a node in an in-memory research tree; follow-ups
branch from nodes and thread ancestor context into the engine prompts; the tree
is saved to disk only on explicit request.

- **Tree** (`core/tree.rs`, pure, fully serde): `ResearchNode { id, parent,
  query, mode, fast, timestamps, answer, sub_queries, web_urls }` in a
  `ResearchTree` forest with a `current` pointer. Findings are deliberately not
  stored — the answer's cited sources are the curated evidence, and `web_urls`
  (capped at 30) records the pages the grounding stage consulted.
- **Context** (`core/context.rs`, pure): a follow-up's `ResearchContext` is the
  ancestor path root→parent, capped at 4 steps (root + last 3, with an elision
  marker), each step carrying the query, a ≤500-char answer digest, and up to 5
  cited source URLs. `context_prompt_block` renders it into the expansion,
  synthesis, and fast prompts (empty block for a fresh search), and the step
  URLs join the synthesis allowlist so ancestors' sources stay re-citable.
- **TUI**: on `AnswerReady` the reducer captures the node (parent =
  `pending_parent`, set by whichever action started the search: home submit →
  new root; `f` follow-up overlay and answer follow-up suggestions → child of
  the current/selected node). `t` opens a `Screen::Tree` view (unicode branch
  prefixes via the pure `widgets/tree.rs`); `Enter` projects any node's answer
  back into Results; `f` branches from the selected node. The tree owns the
  answers — `SearchState`'s single-answer wipe is now lossless.
- **Sessions**: in-memory by default; `s` saves the tree as versioned JSON
  (`tree_store.rs`, XDG state dir `muaddib/sessions/session-<unix>.json`,
  `MUADDIB_SESSIONS` override) and remembers the path so later saves overwrite
  it. `--session <file>` reopens a saved tree in the TUI. `--print` is
  untouched: stdout stays a bare `Answer`.

## Consequences

- Follow-ups stop repeating ground already covered and can cite earlier
  sources; the tree shows how the research progressed and allows revisiting
  and branching from any point.
- Prompt growth is bounded (~3 KB context block).
- Historical nodes re-render without link ticks or refetched images (accepted
  v1 limitation; the data to redo both is in the node).
- Node timestamps come from a per-frame clock stamp (`clock_unix`) so the
  reducer stays free of system-time reads.

## Alternatives considered

- **Flat chat history (Perplexica-style)** — rejected: a linear thread cannot
  represent branching investigations; the tree is strictly more expressive and
  renders naturally in a TUI.
- **Auto-persisting every search** — rejected: the user chose explicit save;
  research is often throwaway and silent disk writes are surprising.
- **Storing MergedFindings per node** — rejected: bloats session files and
  requires new serde derives; the cited sources plus `web_urls` cover the
  provenance story.
- **A knowledge graph with typed edges** — rejected for v1: the user's own
  conclusion was that the practical structure is a research tree; a graph adds
  modeling cost with no navigation story in a terminal.
