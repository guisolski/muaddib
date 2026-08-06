# ADR-0016: Diff-scoped mutation testing

- Status: accepted
- Date: 2026-08-05

## Context

The suite is large — 426 tests across 44 modules — and the gate around it is
strict: `-D warnings`, no comments, conventional commits, TDD-first. What none of
that measures is whether the tests *assert* anything. A test that calls a function
and checks only that it did not panic still counts toward the total, and the
table-driven style this project favors makes that failure mode easy to reach: a
new row in a `const &[Spec]` table looks tested because the loop covers it, even
when the `want` field is never compared against anything load-bearing.

ADR-0012 drew the same distinction for answer quality — `cargo test` proves the
pipeline *behaves*, `make eval` asks whether the answers are *good*. This ADR
covers the third question: whether the tests would notice if the code were wrong.

The constraint that shapes everything here is cost. `cargo mutants` rebuilds and
re-runs the suite once per mutant. Over 22,015 lines in 60 files that is thousands
of mutants and hours of wall clock — far too slow for a push gate and too slow for
a PR.

## Decision

**Scope every run to the diff.** Both entry points pass `--in-diff`, so only
mutants on lines the change touched are tested:

- a `cargo-mutants` hook at the **pre-push** stage, beside the existing
  `cargo-test` hook, diffing against the merge-base with `origin/main`
- a **`mutants` CI job** gated to `pull_request`, diffing against the PR base

Both block. A surviving mutant fails the push and fails the build.

This puts the pressure on new code at the moment it is written, which is when the
missing assertion is cheap to add, and leaves the existing 22k lines alone.

**Generate the local diff with a single ref.** `cargo mutants --in-diff` exits 5
when the new side of the diff does not match the text in the tree it mutates. The
pre-commit framework does not stash unstaged work at the pre-push stage, so a
two-commit `git diff A..B` would go stale the moment the tree is dirty. Diffing
with one ref and no `..` — `git diff <merge-base> -- src` — compares against the
*working tree*, so the new side matches by construction. CI needs no equivalent
because `actions/checkout` leaves a clean tree.

**Exclude the untested adapters** (`.cargo/mutants.toml`). Thirteen files, ~1,163
lines: `src/main.rs`, the filesystem and HTTP shims (`config_store.rs`,
`history_store.rs`, `pipeline/{mod,http,images,validate}.rs`), and the render-only
views (`tui/{mod,event,theme}.rs`, `tui/view/{config_modal,followup,tree}.rs`).
Every one has zero unit tests, so every mutant in them is a guaranteed survivor —
keeping them in scope would make a blocking gate unusable rather than informative.

The exclusion is by *file*, not by directory. `src/tui/view/` keeps six of its nine
files in scope because they do have tests, and `src/tui/update.rs` stays in scope
despite living under the adapter tree: it is a pure reducer with 48 tests, and its
`Command` is the side-effect descriptor from `tui/event.rs`, not
`std::process::Command`. It is one of the highest-value targets in the crate.

**Raise the timeout floor to 60s.** The default is `max(20s, 5 × baseline)`, and
the baseline here is roughly 0.3s, so the floor governs. But
`tests/pipeline_integration.rs` sets `engine_timeout` and `fast_timeout` to 20
seconds; a mutant that neutralizes a timeout constant would be killed at the floor
and reported as a TIMEOUT rather than being allowed to hit its own deadline and
fail cleanly as CAUGHT. Sixty seconds lets those classify correctly.

**Do not parallelize.** `cargo mutants` tests one mutant at a time by default and
stays that way: `tests/cli_smoke.rs` writes to a fixed, non-unique path under
`std::env::temp_dir()`, which concurrent mutant jobs would collide on, producing
false CAUGHT results — the worst possible failure for a tool whose only output is
a verdict.

## Consequences

- A change that adds a branch without adding the assertion that covers it is now
  rejected at push time, with `mutants.out/missed.txt` naming the function.
- `make hooks` now installs `cargo-mutants` as well, so the once-per-clone setup
  step got slower. The hook hard-fails when the binary is absent rather than
  skipping silently — a blocking gate that quietly no-ops is worse than none.
- `make ci` no longer means "exactly what CI runs": mutation is a separate
  PR-only job, because push-to-main has no diff base to scope against.
- The diff view can miss regressions elsewhere. `--in-diff` matches only code
  under test, never test code, so a change that *deletes* an assertion produces no
  mutants at all. This gate raises the floor on new code; it does not audit the
  suite.
- PRs get ~5-15 minutes slower. cargo-mutants copies `target/` into its scratch
  tree, so `Swatinem/rust-cache` still pays off across the baseline build.
- Escape hatches are `SKIP=cargo-mutants git push` locally and an `exclude_globs`
  or `exclude_re` entry in `.cargo/mutants.toml` for a permanent one.

## Alternatives considered

- **A full run on a schedule** — deferred, not rejected. It is the natural
  complement once the survivor rate on new code settles, and would catch the drift
  the diff view structurally cannot. It needs sharding or a long timeout first.
- **Advisory rather than blocking** — rejected for consistency. Every other gate
  in this project is blocking; a warning nobody has to act on trains people to
  scroll past it, which is the same argument ADR-0012 makes for committing the
  eval baseline instead of just printing it.
- **Mutating the whole crate with a committed survivor baseline** — rejected: the
  baseline file becomes a second thing to keep current, and the untested adapters
  it would enumerate are excluded for a reason that will not change until they get
  tests.
- **Narrowing to `src/core/` and `tui/update.rs` only** — rejected as too timid.
  `pipeline/search.rs` and `engines/cli.rs` carry real orchestration logic and 43
  tests between them; they belong under the same pressure.
- **A pre-commit-stage hook** — rejected: mutation is strictly slower than
  `cargo test`, which already sits at pre-push. Running it on every commit would
  invert the project's fast-checks-early ordering.
