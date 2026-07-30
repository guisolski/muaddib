# ADR-0001: Rust with ratatui/crossterm for the TUI

- Status: accepted
- Date: 2026-07-30

## Context

faro must be fast, light, and run many concurrent subprocess searches while
keeping a responsive terminal interface. The product requirement named Rust
explicitly and asked for maximum performance and parallel queries.

## Decision

Build a single Rust binary using **ratatui 0.30** over **crossterm 0.29** for
rendering, and **tokio** as the async runtime. The event loop multiplexes three
sources with `tokio::select!`: crossterm's `EventStream`, a 100ms tick for
animation, and the search pipeline's mpsc channel. The release profile enables
`lto = "thin"`, `codegen-units = 1`, `strip = true`, and `panic = "abort"` for a
small, fast binary.

## Consequences

- One static binary, instant startup, single-digit-MB footprint, no runtime
  dependencies beyond the AI CLIs themselves.
- ratatui's immediate-mode drawing pairs naturally with a pure
  `state + event -> state` reducer, which serves the pure-functions methodology.
- tokio's `process`, `sync`, and `time` features cover subprocess fan-out,
  channels, and timeouts without extra crates.
- Cost: terminal graphics (images) require extra protocols (kitty/sixel) and are
  deferred to the roadmap.

## Alternatives considered

- **Textual (Python)** — richer widgets, but a heavyweight runtime and no story
  for the required performance profile.
- **Bubble Tea (Go)** — good model, but the requirement named Rust.
- **egui/iced (GUI)** — not a terminal application; faro is explicitly a TUI.
- **Direct crossterm without ratatui** — more control, far more code for
  layout, diffing, and widgets that ratatui provides for free.
