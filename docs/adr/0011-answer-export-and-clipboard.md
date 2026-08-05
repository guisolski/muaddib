# ADR-0011: Answer export and clipboard

- Status: accepted
- Date: 2026-08-04

## Context

The answer only existed in two places: painted in the TUI, or as `--print` JSON on
stdout. Neither is what a reader wants after a good search — they want to paste the
result into a note, an issue, or a PR. Every comparable tool (GPT Researcher exports
PDF/Word/Markdown/JSON/CSV) treats getting the answer *out* as a first-class feature;
muaddib had no path at all, and a repo-wide grep for `clipboard|export|to_markdown`
returned nothing.

Two constraints shaped the design. The project takes no new flags: a feature is
reached by a key, a mode, or an engine capability, never by a CLI argument or an
on/off config key. And `--print` is a scripting contract that `cli_smoke.rs` asserts
on, so its stdout shape cannot move.

## Decision

Export is Markdown, produced by a pure `core/export.rs`, and reached by two keys in
the Results screen. `--print` is untouched.

- **One format, no table.** `to_markdown(answer, context)` is the whole surface.
  A `FORMATS` dispatch table was rejected as speculative: with no flag, nothing would
  ever select HTML or JSON, so the extra rows would be dead code. The project's rule
  is that a table exists where there are real rows; a second format with a real
  trigger can extract one mechanically.
- **Mermaid for diagrams.** `Block::Diagram` renders as a fenced ` ```mermaid ` block
  (`flowchart LR` / `timeline`). GitHub and Obsidian render it natively, and the
  README already uses mermaid, so the choice is consistent with the project's own docs.
  Labels are sanitized (`"` → `'`, `:` and newlines → space) because both keywords
  break the diagram grammar.
- **Keys, not flags.** `y` copies, `e` exports to `muaddib-<slug>.md` in the working
  directory. Both are free keys in `Scope::Results`; `Ctrl+C` is the global quit and
  was left alone. The help screen is generated from `KEYMAP`, so it picked both up
  with no edit.
- **OSC 52 as the fallback, not the primary.** A local clipboard command
  (`pbcopy` / `wl-copy` / `xclip` / `xsel`, walked as a table) reports success through
  an exit status, so it is tried first. When none exists — most importantly over SSH —
  the OSC 52 escape sequence carries the text to the *client's* clipboard. The base64
  encoder is a pure ~15-line function in `core/`, tested against the RFC 4648 vectors,
  so no dependency was added. Payloads over `OSC52_MAX_BYTES` are refused rather than
  silently truncated by the terminal.
- **`status` moved into `Source`.** Link validation results lived only in a
  `HashMap<u32, LinkStatus>` in TUI state, so the exported document, the `--print`
  JSON, and saved sessions all lost them. `Source` now carries
  `Option<LinkStatus>` (`#[serde(default)]`, `skip_serializing_if`), written by the
  reducer as `LinkChecked` events arrive.

## Consequences

- The exported document reports broken links (`~~404~~`) because the status travels
  with the source instead of beside it.
- ADR-0010 listed as a known limitation that *"historical nodes re-render without link
  ticks"*. `ResearchTree::set_source_status` closes that: a re-opened session shows the
  ticks it had when it was saved.
- `--print` JSON gained a `status` field per source. It is additive and skipped when
  absent, so existing consumers are unaffected — and `cli_smoke.rs` needed no change.
- Any keypress now dismisses the current notice, so the Results footer can borrow its
  line for feedback without permanently hiding the key hints.
- Export writes to the working directory. That is the least surprising place for a
  file the user just asked for, but it does mean `e` in a read-only directory reports
  a failure notice rather than falling back elsewhere.

## Alternatives considered

- **`--format md` / `--out FILE` on `--print`** — rejected: the project takes no new
  flags, and the JSON contract already covers scripting. The pure renderer means a
  headless path is a few lines away if that ever changes.
- **A `FORMATS` table with Markdown, HTML, and JSON** — rejected as dead rows; see above.
- **A clipboard crate (`arboard`, `copypasta`)** — rejected: pulls X11/Wayland/Windows
  system dependencies into a single-binary project to replace roughly forty lines, and
  none of them work over SSH, which OSC 52 does.
- **OSC 52 first, shell command as fallback** — rejected: OSC 52 gives no success
  signal, so the notice would lie whenever the terminal has it disabled. Trying the
  command first means a truthful notice wherever a clipboard actually exists.
- **Keeping link status in the TUI map only, and passing the map into the renderer** —
  rejected: it would have kept `--print` and saved sessions lossy, which is the actual
  defect.
