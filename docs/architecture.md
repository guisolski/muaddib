# Architecture

faro is a single binary crate with a `lib.rs`, structured hexagonally: a pure
functional core surrounded by thin adapters for subprocesses, HTTP, and the
terminal.

## Layers

```mermaid
flowchart TB
    subgraph adapters [Adapters — I/O at the edges]
        TUI[tui/ — ratatui event loop and views]
        ENG[engines/ — CLI subprocess execution]
        VAL[pipeline/validate — HTTP link checks]
        CFG[config_store — filesystem]
    end
    subgraph orchestration [Orchestration]
        PIPE[pipeline/search — async stage runner]
    end
    subgraph core [Pure core — zero tokio, zero ratatui, zero I/O]
        MODE[core/mode — MODES table]
        PLAN[core/plan — expansion planning]
        PROMPT[core/prompts — prompt builders]
        EXTRACT[core/extract — JSON extraction]
        ANSWER[core/answer — answer schema]
        CIT[core/citations — merge and renumber]
        CONF[core/config — parse and clamp]
    end
    TUI --> PIPE
    PIPE --> ENG
    PIPE --> VAL
    PIPE --> core
    TUI --> core
    ENG --> core
    CFG --> CONF
```

## Purity rules

- `src/core/` imports neither `tokio` nor `ratatui` nor performs any I/O. Every
  function is deterministic: same inputs, same outputs.
- Adapters are thin. `engines/cli.rs` spawns processes; `pipeline/validate.rs`
  makes HTTP requests; `tui/` draws. Each delegates every decision to core
  functions.
- The TUI reducer (`tui/update.rs`) is a pure state machine: it receives an
  `AppEvent`, mutates `App`, and returns an optional `Command`. All side effects
  (spawning searches, opening URLs, saving config) are executed by the event loop
  in `tui/mod.rs`.

## Table-driven dispatch

Behavior is data wherever possible. Adding a row changes behavior; no branching
logic needs to be touched:

| Table | Location | Drives |
|---|---|---|
| `MODES` | `core/mode.rs` | search modes, breadth, prompt instructions |
| `ENGINES` | `engines/mod.rs` | engine binaries, argv, parse strategy |
| `EXTRACTORS` | `core/extract.rs` | JSON extraction strategies, tried in order |
| `KEYMAP` | `tui/keymap.rs` | keybindings, and the Ctrl+G help screen |
| `CONFIG_FIELDS` | `tui/app.rs` | config modal fields |
| `FRAMES` | `tui/widgets/spinner.rs` | spinner animation |
| `GENERIC_TEXT_KEYS` | `engines/parse.rs` | envelope key probing |
| fallback facet tables | `core/plan.rs` | offline query expansion |

## Data flow of one search

```mermaid
sequenceDiagram
    participant U as User
    participant T as TUI event loop
    participant P as pipeline::search (tokio task)
    participant E as engine CLI (subprocess)

    U->>T: Enter (query)
    T->>P: spawn_search(engine, request)
    P->>E: expansion prompt
    E-->>P: sub-queries JSON
    P-->>T: PlanReady
    par bounded fan-out
        P->>E: sub-search prompt 1..N
        E-->>P: findings JSON 1..N
        P-->>T: SubQueryStarted / SubQueryFinished
    end
    P->>P: merge + dedupe (pure)
    P-->>T: SynthesisStarted
    P->>E: synthesis prompt + answer JSON schema
    E-->>P: answer JSON
    P->>P: renumber + eject hallucinated URLs (pure)
    P-->>T: AnswerReady
    P-->>T: LinkChecked (per source, parallel HEAD)
    P-->>T: Completed
```

Communication is one-way: the pipeline task emits `SearchEvent`s over an mpsc
channel; the TUI folds them into `App` state via the reducer. Cancellation drops
the `SearchHandle`, which aborts the task; `kill_on_drop(true)` reaps any child
CLI processes.

## Module map

```
src/
├── main.rs           clap CLI, headless --print mode, TUI launch
├── lib.rs            public modules (integration tests build against this)
├── config_store.rs   config file resolution and persistence (FARO_CONFIG, XDG)
├── history_store.rs  search history file: append, load, clear (FARO_HISTORY, XDG state)
├── core/             pure: mode, plan, prompts, extract, answer, citations, config, history
├── engines/          EngineSpec table, Engine trait, CliEngine, output parsing
├── pipeline/         SearchEvent protocol, stage orchestration, link validation
└── tui/              App state, reducer, keymap, theme, views, widgets
```
