# Development

## Setup

```sh
git clone https://github.com/guisolski/muaddib && cd muaddib
make hooks     # install pre-commit + commit-msg + pre-push hooks
make test
```

## Make targets

| Target | What it does |
|---|---|
| `make` / `make help` | list every target (default) |
| `make build` / `make release` | debug / release build |
| `make test` | `cargo test --all-features` |
| `make lint` | clippy, all targets, `-D warnings` |
| `make fmt` / `make fmt-check` | rustfmt |
| `make precommit` | run every pre-commit hook against all files |
| `make ci` | exactly what CI runs: fmt-check + lint + test + release |
| `make install` | `cargo install --path .` |

## Conventions

- **TDD.** Tests are written with (or before) the code, per module. The pattern
  is table-driven: a `Case { name, input, want }` array and one loop asserting
  with the case name in the failure message.
- **Pure core.** `src/core/` performs no I/O and imports neither tokio nor
  ratatui. If logic can be pure, it goes there.
- **Table-driven design.** Modes, engines, keybindings, extraction strategies,
  and config fields are data tables; behavior changes are row changes.
- **No comments.** Enforced by a pre-commit hook (`scripts/check_no_comments.py`).
  When an explanation seems needed, extract a named function. Documentation
  lives in `/docs`, not in the source.
- **Conventional commits.** Enforced by the commit-msg hook (`feat:`, `fix:`,
  `chore:`, `docs:`, …).
- **English everywhere** — code, docs, commits, UI strings.

## Pre-commit hooks

`make hooks` installs, per stage:

- **pre-commit**: whitespace/EOF/YAML/TOML/JSON checks, large-file guard,
  private-key detection, shebang checks, `typos` (multilingual test data is
  whitelisted in `typos.toml`), the no-comments guard, `cargo fmt --check`,
  `cargo clippy -D warnings`
- **commit-msg**: conventional commit format
- **pre-push**: `cargo test --all-features`

## The fake engine harness

Tests never call a real AI CLI. `tests/fixtures/fake-engine.sh` inspects its
arguments for the task markers that the prompt builders embed (`MUADDIB:EXPAND`,
`MUADDIB:SUBSEARCH`, `MUADDIB:SYNTH`) and answers with the matching JSON fixture.
`fake-engine-fail-expand.sh` forces the expansion-failure path. The in-process
`FakeEngine` (in `pipeline/search.rs` tests) covers the same protocol without
subprocess overhead.

Test layers:

| Layer | Where | Covers |
|---|---|---|
| unit | `#[cfg(test)]` in each module | all pure core, parsing, widgets, reducer |
| integration | `tests/pipeline_integration.rs` | full pipeline over real subprocesses |
| smoke | `tests/cli_smoke.rs` | the compiled binary, `--print` mode, exit codes |

## Live testing

The one thing CI cannot cover — a real AI CLI:

```sh
cargo run -- --print --engine claude "rust 1.93 release highlights"
```

Expect stderr progress lines and a JSON answer on stdout with real, validated
source URLs. This is the recipe used at the project's "live checkpoint" step;
run it after touching prompts or the engine table. A `web hits: N` line with
`N > 0` confirms the web-search grounding stage reached real engines.

### Refreshing web-search fixtures

SERP markup drifts. When a `WEB_ENGINES` parser starts returning zero hits
against the live engine, re-capture its fixture with the browser User-Agent
from `pipeline/http.rs`, trim it to 2–3 results, drop it under
`tests/fixtures/websearch/`, and adjust the CSS selectors in
`core/websearch.rs` until the table-driven parse tests pass again:

```sh
curl -sS -A "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
(KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36" \
"https://www.bing.com/search?q=rust+programming+language" -o /tmp/bing.html
```

## Releasing

```sh
make ci
git tag -a vX.Y.Z -m "..."
git push origin main --tags
```
