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
| `make test` | `cargo test` |
| `make mutants` | mutation-test the lines changed against `origin/main` |
| `make lint` | clippy, all targets, `-D warnings` |
| `make fmt` / `make fmt-check` | rustfmt |
| `make precommit` | run every pre-commit hook against all files |
| `make eval` | score the golden queries against a **real** AI CLI and rewrite `docs/eval-baseline.md` |
| `make ci` | fmt-check + lint + test + release (mutation runs as its own PR-only job) |
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
- **pre-push**: `cargo test`, then `cargo mutants` over the
  changed lines

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
| mutation | `make mutants` | whether those tests actually assert |

## Mutation testing

`cargo test` tells you the tests pass. It cannot tell you they would *fail* if the
code were wrong — a test that calls a function and only checks it did not panic
counts toward the total like any other. `make mutants` closes that gap (ADR-0016):
it perturbs the source (flips a comparison, returns a default, drops a branch) and
reports every mutant the suite failed to catch.

```sh
make mutants     # the same thing the pre-push hook and PR CI run
```

**It only mutates what you changed.** A full run over 22k lines takes hours, so
both the hook and the CI job pass `--in-diff` and test only mutants on lines your
change touched. That is fast enough to gate a push — a typical change is a few
minutes — and it puts the pressure on new code, where the missing assertion is
cheap to add.

The local diff is taken with a **single ref and no `..`** (`git diff <merge-base>
-- src`), which compares against the working tree rather than a commit. That
matters: `cargo mutants --in-diff` exits 5 when the diff's new side does not match
the tree it mutates, and the pre-commit framework does not stash unstaged work at
the pre-push stage. Diffing against the tree makes the two match by construction,
dirty or clean.

**When a mutant survives**, `mutants.out/missed.txt` names the function and the
mutation that went unnoticed. Almost always the right fix is the assertion you
skipped — a new row in the module's `Case` table whose `want` actually pins the
behavior the mutant changed. Reach for an `exclude_re` entry in
`.cargo/mutants.toml` only when the mutant is genuinely unobservable, and say why.

`.cargo/mutants.toml` already excludes 13 files with zero unit tests — `main.rs`,
the filesystem and HTTP shims, and the render-only views. Every mutant in them
would survive, which would drown the signal. The exclusion is per *file*, not per
directory: six of the nine files in `src/tui/view/` are tested and stay in scope,
and so does `tui/update.rs`, which despite its location is a pure reducer with 48
tests and one of the best targets in the crate.

Two settings worth knowing before you change them. The timeout floor is raised to
60s because `tests/pipeline_integration.rs` uses 20s internal deadlines — at the
default 20s floor, a mutant that neutralizes a timeout gets killed and misreported
as TIMEOUT instead of the CAUGHT it really is. And runs stay single-threaded
because `tests/cli_smoke.rs` writes to a fixed path under `std::env::temp_dir()`
that parallel mutant jobs would collide on, producing false CAUGHT results.

To push past a survivor you have decided not to fix: `SKIP=cargo-mutants git push`.

## Measuring answer quality

`cargo test` proves the pipeline behaves. It cannot prove the answers are good,
because the fake engine returns fixed fixtures. That is what `make eval` is for
(ADR-0012):

```sh
make eval        # runs tests/fixtures/eval/cases.toml against your configured engine
```

It drives the same `spawn_search` the binary uses, scores each answer with the pure
metrics in `core/eval.rs`, and rewrites `docs/eval-baseline.md`.

| Metric | What a regression looks like |
|---|---|
| uncited blocks | the model started asserting things without a source |
| broken links | the anti-hallucination gate is letting invented URLs through |
| expected-domain coverage | grounding stopped reaching the obvious authority |
| expected-mention coverage | the answer stopped containing the actual answer |
| wall clock / cost | a prompt got fatter without paying for itself |

**Commit the baseline.** The regression signal is the diff in a tracked file during
review; an uncommitted score changes nobody's mind. Run it before and after any change
to prompts, `ANSWER_SCHEMA`, or the grounding stages.

CI never runs this — it needs a real CLI, real money, and real time.

> **It is not cheap.** The first recorded run took **20 minutes and $5.32** for five
> queries against `claude` on its default model. Roughly a dollar per query, and the
> Scientific case alone took 5.5 minutes. Budget for it; do not wire it into a watch
> loop. Setting `[engines.claude] model = "haiku"` for the run trades some answer
> quality for a much smaller bill.

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

A tag drives everything. `.github/workflows/release.yml` fires on `v[0-9]+.[0-9]+.[0-9]+*`:

```sh
make ci
# bump `version` in Cargo.toml, commit it
git tag -a vX.Y.Z -m "..."
git push origin main --tags
```

Four jobs then run:

| Job | What it produces |
|---|---|
| `create-release` | the GitHub release for the tag |
| `upload-assets` | `muaddib-<target>.tar.gz` + `.sha256` for macOS arm64/x86_64 and Linux x86_64/arm64 |
| `publish-crate` | `cargo publish` to crates.io |
| `homebrew-tap` | rewrites `Formula/muaddib.rb` in `guisolski/homebrew-muaddib` and pushes it |

Cross-compilation, archive naming, and checksums come from
`taiki-e/upload-rust-binary-action`; the formula is rendered from the published
`.sha256` files, so the tap can never drift from the binaries.

### Secrets the workflow needs

| Secret | Used by | Notes |
|---|---|---|
| `GITHUB_TOKEN` | create-release, upload-assets | provided automatically |
| `CARGO_REGISTRY_TOKEN` | publish-crate | crates.io API token |
| `HOMEBREW_TAP_TOKEN` | homebrew-tap | PAT with write access to the tap repo |

`scripts/install.sh` is the `curl | sh` path: it maps `uname` to a target triple,
resolves the latest tag through the GitHub API, verifies the `.sha256` when `shasum`
is available, and installs to `$MUADDIB_INSTALL_DIR` (default `~/.local/bin`).
