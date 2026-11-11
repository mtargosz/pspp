# pspp — Decision log (append-only)

## 2026-11-11 — orchestrator (planning session, user-approved)

- **Language/stack: Rust** with `ratatui` + `crossterm` (TUI, SGR mouse, truecolor), `sysinfo`
  (portable process snapshot), platform natives (`libproc` on macOS via `libc`, `/proc` on Linux),
  `clap` (CLI), `serde`/`toml` (config). Chosen over Python/Textual (interpreter dependency, refresh
  CPU cost, packaging) and Go/bubbletea (less mature braille/gradient ecosystem). *User confirmed.*
- **Platforms: macOS + Linux.** *User confirmed.*
- **Query mode = non-interactive stdout mode** (`pspp <query>`), not an in-TUI query language. *User confirmed.*
- **Aggregation: both** subtree rollup and flat direct-children grouping, runtime-toggleable. *User confirmed.*
- **Refresh: live auto-refresh**, configurable `update_ms`-style interval. *User confirmed.*
- **Memory types per platform maximized** (RSS/VSZ everywhere; footprint on macOS; PSS/USS/swap on Linux). *User delegated to agents.*
- **Multi-agent development** via reusable profiles in `.github/agents/` driven by an orchestrator;
  **persistent memory on disk** in `.github/agents/memory/` (this directory), committed to git. *User directed.*
- **Workspace**: `/Users/mtargosz/dev/projects/pspp` (branch `main`); commit per milestone; never push. *User directed.*
- Usage gradient colors adopted from btop default theme: `#77ca9b → #cbc06c → #dc4c4c`.
- Graceful degradation rule: when privileged per-process data is unreadable, fall back to RSS and
  mark approximate; never crash or omit the process.

## 2026-11-11 — orchestrator (model assignment, user-directed: "don't use Fable for everything")

Per-agent models set in profile frontmatter:

| Agent | Model | Rationale |
|---|---|---|
| orchestrator | claude-sonnet-5.5 | frequent coordination turns; balanced cost/judgment |
| planner | gpt-6.1-sol | strong structured reasoning for task breakdown |
| coder | gpt-5.3-codex | coding-specialized model |
| tester | claude-haiku-5.5 | fast/cheap for tight test-run loops |
| reviewer | claude-opus-5.5 | deepest scrutiny; different family from coder to catch family-blind-spot bugs |
| documentator | gemini-4-carbon | strong prose at low cost |
| ui-stylist | claude-sonnet-5.5 | detail-oriented visual/code fidelity work |

## 2026-11-11 — reviewer (milestone 13)

- **`--top 0` is a usage error** (clap validation, exit 2: "must be at least 1"). Zero rows is never
  useful, and silently treating 0 as "no limit" would be surprising; omit `--top` for no limit.
- **Query exit status is grep/pgrep-style**: 0 = at least one row printed, 1 = no matching rows
  (plain mode also prints `no matches` to stderr; JSON still prints `{"rows":[],"total":0}`),
  2 = usage error (clap) or runtime error (e.g. stdout write failure). A broken pipe on stdout
  (`pspp | head`) is treated as success. Lets scripts write `if pspp --pid 123 >/dev/null; then`.
- **TOTAL covers the returned rows (after filters and `--top`), with each process counted once.**
  Rollup Σ values nest, so only returned rows without a returned ancestor contribute; flat Σ values
  (direct children) are disjoint and are all summed. Saturating. Documented in `--help`.
- **`update_ms` range is 100..=86_400_000 ms** (btop's limits) in the TUI, options editor, and config
  load; prevents `Instant + Duration` overflow panics.
- **Ctrl-C / Ctrl-Q quit from every TUI mode** (raw mode means no SIGINT), taking precedence over
  filter-clear `c`.
- **Config is saved atomically** (write `pspp.conf.tmp`, rename).


## 2026-11-11 — M16 (review follow-up; reverses earlier "query inherits config defaults")

- **Query mode never reads `pspp.conf`.** Defaults: `--mem rss`, `--agg rollup`, `--sort agg`, binary
  units. Reason: a TUI session that toggled `e` persisted `agg_mode = "flat"` and silently changed every
  later `pspp --json`. Reverses the milestone-5 behaviour of inheriting config defaults.
- **Plain header and JSON name the mode**: `RSS Σ (rollup)`; JSON is `{ "mem", "agg", "rows", "total" }`.
- **Mode selection**: query mode iff `PATTERN`, `--pid`, `--min`, `--top`, `--json` or `--query` is given,
  or stdout is not a tty. `--mem/--agg/--sort/--base10/--update-ms` apply to whichever mode runs (they no
  longer force query mode). New `--query` and `-f/--filter` (TUI filter preset; `PATTERN` fallback in query mode).
- **`save_on_exit` persists only what the user changed in the TUI**: `main` diffs the settings the TUI
  started with (config + CLI overrides) against the final settings and writes only differing fields, so CLI
  overrides are never saved. Caveat: changing a value and changing it back to the CLI-overridden value is
  indistinguishable from no change. This also reverses the reviewer's "`--update-ms` is persisted" note.
