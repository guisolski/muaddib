# Architecture

muaddib is a single binary crate with a `lib.rs`, structured hexagonally: a pure
functional core surrounded by thin adapters for subprocesses, HTTP, and the
terminal.

## Layers

```mermaid
flowchart TB
    subgraph adapters [Adapters — I/O at the edges]
        TUI[tui/ — ratatui event loop and views]
        ENG[engines/ — CLI subprocess execution]
        VAL[pipeline/validate — HTTP link checks]
        WEB[pipeline/websearch — HTTP search grounding]
        PAGES[pipeline/pages — HTTP page-content grounding]
        CFG[config_store — filesystem]
        TREES[tree_store — session files]
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
        EXPORT[core/export — markdown, OSC 52]
        CRED[core/credibility — source classes]
        CIT[core/citations — merge and renumber]
        WEBT[core/websearch — WEB_ENGINES table and parsers]
        RANK[core/rank — BM25 hit reranking]
        READ[core/readability — page text extraction]
        TREE[core/tree — research tree and sessions]
        CTX[core/context — follow-up context]
        CONF[core/config — parse and clamp]
    end
    TUI --> PIPE
    PIPE --> ENG
    PIPE --> VAL
    PIPE --> WEB
    PIPE --> PAGES
    PIPE --> core
    TUI --> core
    ENG --> core
    WEB --> WEBT
    WEB --> RANK
    PAGES --> READ
    CFG --> CONF
    TREES --> TREE
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
| `WEB_ENGINES` | `core/websearch.rs` | web/academic search engines, request shape, hit parsers |
| `CONTENT_SELECTORS` / `NOISE_TAGS` | `core/readability.rs` | page-content extraction roots and excluded subtrees |
| `EXTRACTORS` | `core/extract.rs` | JSON extraction strategies, tried in order |
| `KEYMAP` | `tui/keymap.rs` | keybindings, and the Ctrl+G help screen |
| `CONFIG_FIELDS` | `tui/app.rs` | config modal fields |
| `CLIPBOARD_COMMANDS` | `tui/mod.rs` | clipboard binaries tried before OSC 52 |
| `SOURCE_CLASSES` / `DOMAIN_RULES` | `core/credibility.rs` | source credibility classes and the hosts that map to them |
| `FRAMES` | `tui/widgets/spinner.rs` | spinner animation |
| `SLEEPING` / `WORM` | `tui/widgets/mascot.rs` | mascot frames: sleeping breath, hop, Shai-Hulud pass |
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
├── config_store.rs   config file resolution and persistence (MUADDIB_CONFIG, XDG)
├── history_store.rs  search history file: append, load, clear (MUADDIB_HISTORY, XDG state)
├── tree_store.rs     research session files: save, load (MUADDIB_SESSIONS, XDG state)
├── core/             pure: mode, plan, prompts, extract, answer, citations, config, history, websearch, rank, readability, tree, context
├── engines/          EngineSpec table, Engine trait, CliEngine, output parsing
├── pipeline/         SearchEvent protocol, stage orchestration, web-search grounding, page fetching, link validation
└── tui/              App state, reducer, keymap, theme, views, widgets
```
