# Configuration

## File location

Resolution order (first hit wins):

1. `$FARO_CONFIG` — explicit path (also the test seam)
2. `$XDG_CONFIG_HOME/faro/config.toml`
3. `~/.config/faro/config.toml`

A missing file means defaults; a malformed file means defaults plus a visible
notice (faro never refuses to start over config).

## Keys

```toml
language = "pt-BR"          # answer language, BCP-47 tag
engine = "claude"           # claude | cursor-agent | codex | opencode
max_parallel = 4            # concurrent sub-searches, clamped to 1..=8
expansion_breadth = 0       # 0 = use the mode default, otherwise clamped to 1..=8
validate_links = true       # HTTP HEAD validation of every source
engine_timeout_secs = 180   # per engine call

[engines.claude]            # optional, one block per engine
bin = "/custom/path/claude" # binary override (also used by the test suite)
```

Unknown keys are tolerated (forward compatibility). Out-of-range numbers are
clamped, not rejected.

## Precedence

CLI flags > config file > defaults:

| Setting | CLI flag | Config key | Default |
|---|---|---|---|
| answer language | `--lang` | `language` | `pt-BR` |
| engine | `--engine` | `engine` | `claude` |
| search mode | `--mode` | — | `general` |

The config modal (F2) edits and persists the file; `--lang`/`--engine` apply to
the current run only.

## The config modal

| Field | Values |
|---|---|
| language | cycles `en`, `pt-BR`, `es`, `fr`, `de`, `it`, `ja`, `zh` (any BCP-47 tag works via `--lang` or the file) |
| engine | cycles installed engines; uninstalled ones are shown but not selectable |
| validate links | on / off |
| max parallel | 1–8 |

`Enter` saves to the config file; `Esc` discards.
