# AGENTS.md

muaddib is an AI-powered meta-search engine for the terminal: a single Rust
binary that expands a query into sub-queries, fans them out through an
already-installed AI CLI, merges and cites the findings, and renders the
answer in a ratatui TUI. See [`README.md`](README.md) for the product view
and [`docs/architecture.md`](docs/architecture.md) for the full design.

## Layout

```
src/
├── core/       pure — zero I/O, no tokio, no ratatui (mode, plan, prompts,
│               extract, answer, citations, config, history, engine)
├── engines/    AI CLI subprocess adapters (spawn, bin resolution)
├── pipeline/   async orchestration: fan-out, merge, link validation
├── tui/        ratatui event loop, reducer, views, widgets
├── config_store.rs / history_store.rs   filesystem adapters
```

Hexagonal: `core/` holds the decisions, everything else is a thin adapter
that calls into it. New logic defaults to `core/` unless it genuinely needs
the network, filesystem, or terminal.

## Commands

```sh
make build        # cargo build
make test         # cargo test --all-features
make lint         # cargo clippy --all-targets --all-features -- -D warnings
make fmt-check    # cargo fmt --all -- --check
make ci           # fmt-check + lint + test + release — run before calling anything done
make hooks        # install pre-commit/commit-msg/pre-push hooks once per clone
```

## Conventions

**TDD.** Add or update the test in the same module's `#[cfg(test)] mod
tests` alongside the code change, not after. Tests are table-driven: a
local `struct Case { name, input, want }`, an array of cases, one loop
asserting with `case.name` in the failure message. See
`src/core/mode.rs::mode_parses_from_label_case_insensitively` for the
template to copy.

**DRY.** A behavior difference is a new row in a table or a shared helper,
never a copy-pasted match arm or a near-duplicate function.

**Table-driven design.** Model choices as a `const &[Spec]` and dispatch
with `.iter().find()` / `.find_map()` instead of branching logic — adding a
case becomes a new row, not new control flow. The house example is
`src/core/extract.rs`:

```rust
type Extractor = fn(&str) -> Option<Value>;
const EXTRACTORS: &[Extractor] = &[parse_whole_text, parse_fenced_blocks, parse_balanced_objects];
pub fn extract_json(raw: &str) -> Option<Value> {
    EXTRACTORS.iter().find_map(|extract| extract(raw))
}
```

Other tables to know: `MODES` (`core/mode.rs`), `ENGINES` (`core/engine.rs`),
`KEYMAP` (`tui/keymap.rs`), `CONFIG_FIELDS` (`tui/app.rs`).

**Functional Rust.** `src/core/` must stay pure — deterministic, no I/O, no
`tokio`/`ratatui` imports; that boundary is enforced by the crate layout,
not just convention:

- Prefer `let` over `let mut`. Local mutation while building a return value
  (filling a `Vec`, growing a `String`) is fine and still pure — it doesn't
  leak past the function.
- Prefer iterator combinators (`map`, `filter`, `find_map`, `fold`,
  `collect`) over hand-rolled loops with an external mutable accumulator —
  see `merge_sub_results` / `allowed_urls` in `src/core/citations.rs`.
- Prefer `Option`/`Result` combinators (`map`, `and_then`, `ok_or_else`,
  `?`) over `unwrap`/`panic!`/nested `if let`; let `?` or `find_map`
  short-circuit instead of manual control flow.
- Keep impurity at the edges. `engines/`, `pipeline/validate.rs`, `tui/`,
  and `config_store.rs` perform I/O and delegate every decision to a pure
  `core::` function — don't inline business logic into an adapter.

**Enforced automatically** (run `make hooks` so these fire locally):

- No comments in Rust source — a pre-commit hook rejects them. If something
  needs explaining, extract a named function or write it in `/docs`.
- Conventional commits (`feat:`, `fix:`, `chore:`, `docs:`, …).
- English everywhere: code, docs, commits, UI strings.

## Testing layers

| Layer | Where | Covers |
|---|---|---|
| unit | `#[cfg(test)]` in each module | pure core, parsing, widgets, reducer |
| integration | `tests/pipeline_integration.rs` | full pipeline over a fake-engine subprocess |
| smoke | `tests/cli_smoke.rs` | the compiled binary, `--print` mode, exit codes |

Tests never call a real AI CLI — `tests/fixtures/fake-engine.sh` answers
with fixture JSON keyed on the prompt's task marker.

## Further reading

[`docs/architecture.md`](docs/architecture.md) ·
[`docs/development.md`](docs/development.md) ·
[`docs/engines.md`](docs/engines.md) ·
[`docs/search-pipeline.md`](docs/search-pipeline.md) ·
[`docs/configuration.md`](docs/configuration.md) ·
[`docs/adr/`](docs/adr/) for the decisions behind the pure-core split.
