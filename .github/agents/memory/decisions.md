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

## 2026-11-11 — M17

- **Graph and status plot real system memory in use**, not Σ of per-process values: `Snapshot::used_mem`
  (macOS `used_memory()`; Linux `MemTotal - MemAvailable`). Σ of the selected kind is shown separately
  (`Σ RSS 18.4 GiB`) because it can exceed RAM (shared pages per process; footprint includes compressed
  memory). Consequence: the history graph no longer resets when the memory kind changes.
- **`%` column = row aggregated ÷ total RAM**, one decimal, deliberately not capped at 100 (a rollup of
  footprint can read 137%); JSON `pct` is the same value as a number; the TUI column caps display at 999.9%.
- `collect::linux` compiles under `cfg(test)` on every platform so its parsers are unit-testable on macOS.

## 2026-11-11 — M18

- **Filtered view uses the real `expanded` set** (previously force-expanded). Committing a filter inserts
  every ancestor of each direct match into `expanded`, so matches start visible but can be collapsed;
  Space/Enter/←/→ work while filtered. Reverses the milestone-11 "filter ignores expanded" behaviour.
- **Esc in filter input restores the committed filter** (btop); `c` in Normal mode clears. Reverses the
  milestone-11/13 "Esc clears" decision.
- **Header click sort**: COMM/PID/OWN/Σ/% headers are click targets; clicking the active key reverses,
  a new key starts ascending for COMM/PID and descending for OWN/Σ (memory columns largest-first).
  `%` sorts by the aggregated value. CHILDREN/METER are not sortable.
- `App::with_snapshot` (pub(crate)) pulled forward from M22 so these behaviours test deterministically.

## 2026-11-11 — M19

- **Mouse in overlays**: Options — click row selects, click selected row activates (same as Enter), wheel
  moves selection (clamped, no wrap), any click cancels an `update_ms` edit; Help — wheel scrolls ±3.
  A click outside the Options or Help box closes it to the main view (`Normal`), whereas Esc steps back to
  the menu (dismiss gesture = leave overlays entirely).
- **Hint bar** is chosen by `AppMode` and drops whole segments that do not fit the width (never wraps).
- Help reserves its right column for a `ratatui::widgets::Scrollbar`, drawn only when the text overflows.

## 2026-11-11 — M20 (user answered the open questions: keep `m`; load btop themes)

- **`m` stays "cycle memory kind"; the menu stays on `Esc`.** Deviation from btop (where `m` = menu) is
  noted in help, README and man page. *User confirmed.*
- **`color_theme` loads btop `.theme` files** from `<config dir>/themes` (`$PSPP_CONFIG_DIR/themes` when
  set). `Default` is built in. Parsed tolerantly (`#RRGGBB`, `#RR`, `R G B`); recognised keys: main_bg,
  main_fg, title, hi_fg, selected_bg, selected_fg, inactive_fg, meter_bg, div_line and the usage gradient
  `cpu_start/mid/end` (btop's green→red usage gradient); everything else is ignored. Theme names are
  validated (no path separators, no leading dot) because they become file names. *User confirmed.*
- **`h` always opens Help.** The brief lists `h` as a global Help key and also says `vim_keys` off makes
  `h` inert; those conflict. Resolution: `h` = Help regardless; `vim_keys` governs `j/k/l/g/G`
  (new: `l` expands). `vim_keys` defaults to **true** to keep the existing j/k/g/G behaviour (btop's
  default is false).
- **`theme_background` defaults to true** (btop) — no visible change with the Default theme, whose
  `main_bg` is unset; loaded themes paint their `main_bg` unless it is turned off.
- **`graph_symbol = auto`** → `tty` when `force_tty` or the terminal lacks colour: `TERM` of `linux`/`dumb`,
  or a `TERM` without "color" **and** no `COLORTERM`. The COLORTERM exception keeps alacritty/kitty/wezterm
  on braille. Unset `TERM` counts as colour-capable.
- **Overlay navigation**: Help/Options opened via a global key (`h ? F1 o F2`) return to the main view on
  Esc; opened through the menu they return to the menu (`overlay_parent`). F1/F2 work in every mode except
  while typing in a text field; `+`/`-`/`h`/`o` are Normal-mode keys (plain text while filtering/editing).
- **`background_update = false`** pauses ticks while Menu/Options/Help is open (FilterInput keeps updating);
  the loop still wakes every 250 ms. A clock with seconds caps the poll timeout at 1 s.
- **`disable_mouse`** drops mouse capture live (loop syncs it each iteration) and ignores mouse events.
- **`clock_format`** uses `chrono` (clock+std features only), validated with `StrftimeItems` so bad
  specifiers render nothing instead of panicking. Default is empty (off).
- Options list grew to 17 rows and now scrolls (with a scrollbar); hit rects carry the option index.

## 2026-11-11 — M21

- **Collection runs on a worker thread** (`collect::Collector`: `std::thread` + `mpsc`). One request in
  flight at a time (extra requests are dropped, never queued); the UI loop calls `App::poll_snapshot()`
  every iteration and shortens its poll timeout to 50 ms while a request is outstanding. The *first*
  snapshot is still taken synchronously in `App::from_settings` so the first frame is populated.
- **`take_snapshot(kind)`**: the worker receives the current `MemKind`. Linux reads `smaps_rollup` only
  for `pss|uss|swap`; for `rss|vsz` those fields are `None` and the process is **not** approximate
  (approximate = smaps wanted and unreadable/partial). Changing the kind requests a fresh snapshot; a
  result for a stale kind triggers an immediate follow-up request.
- **Linux process name** = basename of `argv[0]` from `/proc/<pid>/cmdline` (path-like first token only;
  titles like `nginx: worker process` stay whole), falling back to `stat` comm (kernel threads).
- **`ProcTree::build(Snapshot)` takes ownership**; the tree is the single owner of process records and of
  `total_mem`/`used_mem`. `App` no longer stores a `Snapshot`; `query::run_query` also takes `Snapshot` by value.
- **`Snapshot::taken_at` removed** (was dead; chosen over a "sample age" indicator — M23's choice is made here).
- PID column width = digits of the largest pid present, min 5.

## 2026-11-11 — M22

- **Library + binary split**: `src/lib.rs` now owns all modules; `main.rs` is the CLI shell. Needed so
  `tests/model_tests.rs` can drive the model through the public API (a bin-only crate cannot be imported
  by integration tests). Side effect: public items are no longer dead-code-checked as aggressively.
- **Filtered view recomputes `is_last_child` among visible siblings** (`relabel_last_children`) so hidden later
  siblings no longer leave dangling `├─` glyphs (found while writing the filtered screen tests).
- **CI** runs `build`, `test`, `clippy --all-targets -D warnings`, `fmt --check`, `doc --no-deps` on
  macos-latest and ubuntu-latest with `RUSTFLAGS`/`RUSTDOCFLAGS=-D warnings`.
- Screen tests assert meter fill by cell colour (filled vs `meter_bg`), because filled and empty cells are
  both `■`.

## 2026-11-11 — M23

- **One definition per enum**: `MemKind`, `AggMode`, `SortBy` own `as_str`/`Display`/`FromStr`/`ValueEnum`;
  `CliAgg`, `CliSort`, `format_available_mem_kinds`, `mem_kind_to_config`, `ui::parse_*_config`,
  `ui::sort_to_config` and the draw-side label helpers are gone. `MemKind`'s `ValueEnum` lists only
  `available()`, so `--mem pss` on macOS is a clap usage error (exit 2) reading
  `invalid value 'pss' … [possible values: rss, vsz, footprint]`. This replaces the earlier custom
  "available kinds: …" message.
- **`ProcTree::sorted_pids` removed** (case-sensitive Comm arm, test-only). `Snapshot::taken_at` was already
  removed in M21 (decision: no "sample age" indicator).
- **`MemKind::available()` returns `&'static [MemKind]`.**
- **Meter column = `width / 6` clamped to 12..=30 cells**, shrunk (to 0 if needed) before COMM drops below
  12 cells; extra width beyond the meter's cap goes to COMM.
- **MSRV 1.88** (`rust-version`): the newest `rust_version` among locked dependencies (darling/instability,
  pulled in by ratatui). Not CI-verified on an older toolchain; CI uses stable.
- **Docs are generated where they derive from code**: README key tables, man-page KEY BINDINGS and the README
  screen capture are produced from `help::key_sections()` and the TestBackend renderer; `cargo test --lib`
  fails on drift and `PSPP_UPDATE_DOCS=1 cargo test --lib` rewrites them. The screenshot TODO became an
  inline plain-text capture plus capture instructions (no binary image committed).

## 2026-11-12 — agent customization & Gemini 4 Carbon standardization (user-confirmed)

- **All custom agents standardized on `Gemini 4 Carbon`**: supersedes the 2026-11-11 multi-family model
  table (`claude-sonnet-5.5`, `gpt-6.1-sol`, `gpt-5.3-codex`, `claude-haiku-5.5`, `claude-opus-5.5`).
  Frontmatter uses `model: "Gemini 4 Carbon (copilot)"` so both
  VS Code Copilot and CLI runners select Gemini 4 Carbon cleanly. *User confirmed.*
- **Generic `.agent.md` roles + standard instruction separation**:
  - Renamed `.github/agents/*.md` → `.github/agents/*.agent.md` (VS Code Custom Agent convention) and
    removed hardcoded `pspp` assumptions from agent bodies, adding explicit `tools` / `agents` boundaries
    (`planner` = `[read, search]`, `reviewer` = `[read, search, execute]`, `orchestrator` = `[read, search, edit, execute, agent, todo]`
    scoped to `[planner, coder, tester, reviewer, documentator, ui-stylist]`).
  - Moved `pspp`-specific context into `.github/copilot-instructions.md` (always-on stack, memory/specs,
    verification gates, git rules) plus file-scoped `.github/instructions/{rust-platform,tui-btop,docs-sync}.instructions.md`.
  - Added `.github/prompts/implement-review.prompt.md` (`/implement-review`) and seeded M24–M29 from
    `prompts/pspp_review_2_prompt.md` into `progress.md` for immediate pickup. *User confirmed.*
- **`.github/_workflows/ci.yml` kept underscored** (disabled on GitHub) per user preference.
