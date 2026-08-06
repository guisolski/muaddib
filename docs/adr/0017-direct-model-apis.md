# ADR-0017: Direct model APIs as a second engine transport

- Status: accepted
- Date: 2026-08-05
- Supersedes, in part: [ADR-0002](0002-ai-clis-as-search-engine-backends.md)

## Context

ADR-0002 chose vendor CLIs as the only engine backend and explicitly rejected
direct model APIs, on the grounds that they "require users to provision keys per
provider". `README.md` advertised the consequence as a feature: **"No API keys."**

That decision bought real things — no key management, no billing surprises, and
the CLIs' own web search and auth for free — and none of them are given up here.
What it cost is two capabilities that turned out to matter:

- muaddib is unusable without a vendor CLI installed and logged in. A machine with
  a working `ollama` and no `claude` gets nothing.
- It cannot talk to a local model server at all, so a workflow where **nothing
  leaves the machine** was impossible to express.

The second is the motivating one. A local model is not a downgrade of a hosted
model for this use case; it is a different guarantee.

## Decision

**Add a second transport to the existing `ENGINES` table rather than a second
table.** `EngineSpec` gains a `transport` field:

```rust
pub enum Transport {
    Cli(&'static CliSpec),
    Api(&'static ApiSpec),
}
```

The alternative — a separate `PROVIDERS` table — was rejected on the evidence:
CLI-only fields are read at 4 sites, all inside `engines/`, while
transport-agnostic fields (`name`, `models`, `install_hint`) are read at 15 sites,
all outside it. Forking the table would have spread `match` arms through the TUI,
`main.rs`, and the config layer to re-unify what was already unified.

`trait Engine` was already the seam. `src/pipeline/` holds only `Arc<dyn Engine>`
and did not change by a single line.

### Five new rows

`ollama`, `local`, `openai`, `anthropic`, `gemini`. The wire formats live in the
pure core (`core/api.rs`), dispatched from a `Wire` enum; the adapter
(`engines/api.rs`) owns only the HTTP.

### Local models are probed, not assumed

`ollama` availability is a live `GET /api/tags` with a 1s budget, and the same
response populates `EngineStatus.models`. The config modal's model picker reads
that list, so it offers the models you have actually pulled instead of a
hardcoded guess. `local` covers any other OpenAI-compatible server (LM Studio,
llama.cpp, vLLM) with `base_url` from config, following the `searxng` pattern.

### Configuring an engine does not require it to already work

The config modal's engine row walks the whole table, and the field rows below it are
filtered by a `FieldVisibility` column on `CONFIG_FIELDS` — `api key` appears only for
engines that authenticate, `base url` only for engines reached over HTTP.

Both halves of that are load-bearing together. An earlier build showed only *available*
engines in the picker, which deadlocked the thing the picker exists for: `openai` is
unavailable precisely because it has no key, so the row where you would type the key was
unreachable. Only `claude` and `cursor-agent` ever appeared. Unavailable engines are now
selectable and render why (`ollama (not running)`, `openai (no key)`); the money guard
stays where it belongs, on `auto_select`, not on what the user is allowed to look at.

Filtering the rows is what keeps that honest — without it every CLI engine would carry a
dead `api key: n/a` line. `ConfigForm.field_idx` indexes the *visible* list, so the same
row position means different fields for different engines, and both the modal height and
the `↑↓` wrap follow the visible count.

### Paid engines are never auto-selected

`choose_engine` falls back to the first available engine when the requested one is
missing. With hosted APIs in the same table, a stray `OPENAI_API_KEY` in the
environment would have silently started billing the user the first time `claude`
was not on `PATH`. `EngineSpec.auto_select` is `false` for `openai`, `anthropic`,
and `gemini`: they are reachable only by explicit request.

### Structured output goes through the prompt, for now

`ANSWER_SCHEMA` uses `$ref`/`definitions`, `oneOf`, `minimum`, and `maxItems`, and
declares no `additionalProperties`. Every provider's native structured-output mode
rejects some of that: OpenAI's `strict: true` needs `additionalProperties: false`
and `$defs`; Anthropic and Gemini reject `$ref` recursion outright.

So each API row ships `SchemaMode::JsonObject` — the provider's "must emit valid
JSON" flag — while the *shape* contract goes through the prompt-inline path that
already works for `codex` and `opencode`. `SchemaMode::NativeSchema` is
implemented and unit-tested for all four wires, so promoting a provider is a
one-row edit once a schema-translation pass exists. A table invariant test locks
the current state so the promotion cannot happen by accident.

### Keys are never written to `config.toml`

`config_store::save` rewrites the entire config file, world-readable, on every TUI
save. Putting key material in `Config` would have meant a secret leaking into that
file on an unrelated settings change.

Resolution order is: the provider's environment variable, then an `api_key_env`
override pointing at a different variable, then an encrypted vault. The first step
keeps `--print` and CI zero-config.

The vault is `$XDG_STATE_HOME/muaddib/keys.enc`, mode `0600`, written temp-file
and renamed:

```
magic "MUADDIB1" | version | argon2id params | salt | nonce | names_len | names || ciphertext+tag
```

- **Argon2id** (19456 KiB, t=2, p=1 — the OWASP floor) derives the key
- **XChaCha20-Poly1305** seals it, with the **entire header as associated data**,
  so the KDF parameters cannot be downgraded and the name list cannot be edited
  without detection
- The **plaintext name list** exists because `detect_engines` runs at startup,
  long before a passphrase is available. Without it, availability would degrade to
  "the vault file exists", showing engines as available that have no key. It leaks
  which providers you configured, never key material, and it is inside the AAD.

Core stays pure: `core/vault.rs::seal` takes salt and nonce as arguments and
`open` is fully deterministic, so the round-trip, wrong-passphrase, and tamper
cases are ordinary unit tests. `vault_store.rs` owns the randomness and the I/O.
Because `panic = "abort"` is set in the release profile, every vault operation
returns `Result` and none of them `unwrap`.

Secrets carry masked `Debug` and no `Display`/`Serialize`: `ApiKey` prints
`ApiKey(***)`, `Passphrase` prints `Passphrase(***)`, and `ConfigForm` prints
`key_input: "***"`. This is asserted by tests, not by convention — including one
that drives the whole config modal with a key typed into it and asserts the
serialized `config.toml` does not contain it.

## Consequences

- `README.md`'s promise becomes "**No API keys required**", not "No API keys".
- muaddib runs with nothing installed but `ollama`, and answers without any part
  of the query leaving the machine.
- `EngineStatus` gained `endpoint`, `models`, and `key_from_env`; `detect_engines`
  now touches the network for probed engines and reads the vault header.
- New dependencies, all pure-Rust RustCrypto: `argon2`, `chacha20poly1305`,
  `zeroize`, `getrandom`. No new HTTP dependency — `reqwest` was already present.
- The Cargo feature system was removed rather than extended. `api-engines` would
  have been the third feature gating the same `reqwest` dependency, so no single
  one of them could drop it; only turning all three off shed `reqwest` and
  `scraper`, and what remained was a muaddib with no web search, no link
  validation, no images and no API engines. Nothing built that configuration — not
  CI, not the install script, not a documented flag — and it had been broken on
  `main` for long enough that six compile errors went unnoticed. The 32 `cfg`
  sites and six do-nothing stubs that supported it are gone, and there is now one
  build.
- Retry on 429/5xx honours `Retry-After` and is bounded, nested *inside* the job
  timeout so retries share one budget rather than multiplying it. Without it,
  fan-out at `max_parallel = 8` would silently drop sub-queries to empty on the
  first rate limit.
- Per-model cost estimation is table-driven from `EngineSpec.prices`, matched by
  longest model prefix, so a dated snapshot (`gpt-5-2026-03-01`) still prices as
  its family. Unpriced models — every local one — report zero rather than a guess.

## Alternatives considered

- **A separate `PROVIDERS` table.** Rejected on the read-site count above.
- **An OS keychain** (`security`, `libsecret`, DPAPI). Rejected: three platform
  backends, a subprocess or C dependency on each, and untestable without a
  session bus. The encrypted file is one format, portable, and unit-testable.
- **Keys in `config.toml` with the file mode tightened.** Rejected: the whole-file
  rewrite means one unrelated TUI save is enough to leak a key into a backup, a
  dotfile repo, or a synced directory.
- **A key-encryption key with no passphrase**, stored beside the vault. Rejected —
  it is obfuscation, not encryption, and it reads as a security guarantee it does
  not provide.
