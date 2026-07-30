# ADR-0005: Hexagonal architecture with a pure functional core

- Status: accepted
- Date: 2026-07-30

## Context

The project mandates TDD, DRY, table-driven design, and pure functions. AI CLI
subprocesses, HTTP checks, and a terminal UI are all side-effect-heavy; without
deliberate structure, logic and I/O interleave and testing requires mocking the
world.

## Decision

Split the crate into a **pure core** (`src/core/`) and **thin adapters**:

- Core modules (mode, plan, prompts, extract, answer, citations, config) import
  neither tokio nor ratatui and perform no I/O. They are deterministic
  functions over data, each covered by table-driven unit tests.
- Adapters (engines, pipeline, tui, config_store) execute effects but delegate
  every decision to core functions. The TUI follows the Elm shape: a pure
  reducer (`update`) folds events into state and returns `Command`s that only
  the event loop executes.
- Dispatch is table-driven everywhere behavior varies: `MODES`, `ENGINES`,
  `EXTRACTORS`, `KEYMAP`, `CONFIG_FIELDS`, spinner frames, fallback facets.
- A `lib.rs` exposes everything so integration tests exercise the same code the
  binary runs.

## Consequences

- The overwhelming majority of tests need no async runtime, no filesystem, no
  subprocesses — they run in milliseconds and never flake.
- The subprocess boundary is covered by one fake shell script; the reducer is
  tested by feeding synthetic key and search events.
- The F1 help screen renders from `KEYMAP` itself, so documentation cannot
  drift from behavior (DRY applied to UX).
- Cost: some ceremony — events and commands instead of direct calls — accepted
  for testability.

## Alternatives considered

- **Conventional layered MVC with mocks** — mock-heavy tests couple to
  implementation details and rot; rejected.
- **A cargo workspace with separate core crate** — stronger enforcement of the
  purity boundary, but overkill for a single binary; module discipline plus
  review suffices at this size.
