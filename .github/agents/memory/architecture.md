# pspp — Architecture (living document)

## Module map

```
src/
  main.rs            — clap CLI entry:
                       - query mode iff PATTERN/--pid/--min/--top/--json/--query given OR stdout non-tty
                         (query mode never reads config; defaults rss/rollup/agg/binary)
                       - else (tty + no query args): load config, run ratatui loop via `ui::run_tui`,
                         then save runtime settings when `save_on_exit=true`
                       Query-only flags: [PATTERN], --pid, --min, --top, --json, --query
                       Both modes: --mem, --agg, --sort, --base10, --update-ms (clamped), -f/--filter
                       Config saved on exit = only fields changed in the TUI (diff vs. initial settings)
  collect/
    mod.rs           — Snapshot API: { procs: Vec<ProcRec { pid, ppid, comm, mem: MemSet }>, total_mem, used_mem, taken_at }
                       MemSet { rss, vsz, footprint?, pss?, uss?, swap? } (bytes, Option per platform)
    macos.rs         — pid/ppid/p_comm from sysctl(KERN_PROC_ALL) (unprivileged) + sysinfo name/RSS/VSZ +
                       proc_pid_rusage (ri_phys_footprint); graceful fallback
    linux.rs         — /proc/<pid>/stat (pid/ppid/comm/rss/vsz) + smaps_rollup (pss/uss/swap);
                       permission/malformed-input fallback to RSS/VSZ-only records
  model/
    mod.rs           — MemKind enum + parsing/labels/platform availability; model exports
    tree.rs          — pid→ppid forest (orphans/self-links/cycles → synthetic root children),
                       iterative rollup/flat aggregation, deterministic child ordering, visible flattening,
                       stable sorting helpers
  query/
    mod.rs           — pure query logic (`QueryOpts`, `run_query`, size parse/format):
                       name/pid filtering, rollup|flat aggregation, threshold/sort/top,
                       plain aligned table + JSON output wrapper { mem, agg, rows, total }
  ui/
    mod.rs           — `run_tui`: terminal lifecycle guard (alt screen/raw/mouse/cursor),
                       panic hook restoration, crossterm event loop + tick scheduling; returns
                       final `UiSettings` so main can persist them
    app.rs           — `App` state: snapshot/tree, agg mode, mem kind, update timing, base10,
                       save_on_exit, graph symbol selection, app modes
                       (Normal/FilterInput/Menu/Options/Help), tree expand/collapse state,
                       sorting + process filter state, selection+scroll, key/mouse handlers,
                       periodic refresh on ticks, row hit-test + menu hit-test caches
    draw.rs          — rounded-border frame, interactive process tree table, status+hint bars,
                       gradient-colored memory columns, btop-style meters and tree glyphs,
                       Esc menu overlay, options editor overlay, scrollable help overlay
    help.rs          — static help content (key tables, mouse actions, memory explanations),
                       wrapped/styled line builder, help scroll clamp helper
    theme.rs         — named theme colors + 101-step gradients, 256-color fallback, `TruecolorMode`,
                       btop `.theme` parser (`ThemeSpec`, `parse_btop_theme`, `discover_themes`, `load_theme_spec`)
    clock.rs         — strftime top-border clock (`format_clock_at/now`, chrono, validated formats)
    graph.rs         — `History` ring buffer, braille/block/tty graph rendering, ■ meter spans
  config.rs          — config load/save (`$PSPP_CONFIG_DIR/pspp.conf` override else
                       `~/.config/pspp/pspp.conf`), tolerant TOML reader, save_on_exit persistence
tests/
  fixtures/          — synthetic process tables (deep chains, fan-out, orphans, pid reuse)
  model_tests.rs     — aggregation/sorting unit tests
  query_cli.rs       — stdout/JSON integration tests
```

## Key data flow

collector snapshot → model::tree (aggregate per mode) → ui table/graphs (interactive)
                                              ↘ query output (non-interactive)

## Invariants

- All memory values in bytes internally; formatting (KiB vs KB per `base_10_sizes`) only at render.
- Sums use u64 with saturating adds.
- Snapshot is immutable once taken; UI never mutates it (new snapshot per tick).
- Terminal state (alt screen, mouse, cursor) restored via RAII guard + panic hook.

## `model` public API (milestone 4)

- `enum MemKind { Rss, Vsz, Footprint, Pss, Uss, Swap }`
  - `get(&MemSet) -> Option<u64>`
  - `available() -> Vec<MemKind>` (`footprint` only on macOS; `pss/uss/swap` only on Linux)
  - `label() -> &'static str`
  - `FromStr` for CLI values: `rss|vsz|footprint|pss|uss|swap`
- `ProcTree::build(&Snapshot) -> ProcTree`
  - forest from pid→ppid links; orphan/self-link/cycle-safe with synthetic root semantics
- `enum AggMode { Rollup, Flat }`
- `ProcTree::{aggregate, rollup, flat}`
  - return per-pid `AggregateRow { own, own_approximate, aggregated, aggregated_approximate, child_count, descendant_count }`
- `ProcTree::sorted_pids(..., SortBy::{Aggregated, Own, Pid, Comm}) -> Vec<i32>` (stable)
- `ProcTree::flatten_visible(&HashSet<i32>) -> Vec<VisibleRow>`
  - depth-first visible rows: `{ pid, depth, has_children, is_last_child }`; synthetic root not emitted
- `ProcTree::flatten_visible_sorted(&HashSet<i32>, &HashMap<i32, AggregateRow>, SortBy, descending)`
  - same shape as `flatten_visible`, but sibling/root order follows the selected UI sort key+direction
  - tie-breaker is PID for deterministic order

## Query-mode API (milestone 5)

- `query::QueryOpts`
  - `pattern` (case-insensitive substring on `comm`; `!` regex mode intentionally unsupported for now)
  - `pid` subtree filter
  - `mem_kind`, `agg_mode`, `min_bytes`, `sort_by`, `top`, `json`, `base10`
- `query::run_query(&Snapshot, &QueryOpts) -> QueryResult { output: String, row_count: usize }`
  - plain table columns: `PID`, `PARENT`, `COMM`, `OWN`, `<MEM> Σ`, `CHILDREN` + final `TOTAL`
  - `TOTAL`/`total` counts each covered process once: rollup sums only returned rows with no returned
    ancestor; flat rows (direct children) are disjoint so all are summed. Saturating.
  - `main` exit status: 0 rows printed, 1 no matches (plain also prints `no matches` to stderr),
    2 usage/runtime error; EPIPE on stdout is success. `--top` must be ≥ 1.
  - JSON object: `{ mem, agg, rows: [{ pid, ppid, comm, own, agg, children, descendants, approximate }], total }`
- `query::{parse_size, format_size}`
  - parse `512K`, `100M`, `1.5G`, bytes; format binary (`KiB`) or decimal (`KB`) units

## `ui::theme` public API (milestone 7)

- `Rgb(u8,u8,u8)`: `grey(v)`.
- `build_gradient(start, mid, end) -> [Rgb; 101]` — two 50-step linear passes, index = percent.
- `truecolor_from_env(Option<&str>)` — `COLORTERM` = `truecolor`|`24bit`.
- `Theme::{detect(), new(truecolor: bool)}` (the latter for tests/overrides); `App.theme` holds it.
  - Fields: `main_bg: Option<Rgb>` (None = transparent), `main_fg`, `title`, `hi_fg`, `selected_bg`,
    `selected_fg`, `inactive_fg`, `meter_bg`, `div_line`.
  - `color(Rgb) -> Color` (truecolor `Color::Rgb`, else `Color::Indexed` via grey ramp / 6×6×6 cube).
  - `usage_rgb/usage_color(pct)` (pct clamped to 100); the graph uses the same gradient (btop default).

## Tree table UI (milestone 8)

- Expansion model: `HashSet<pid>` of expanded nodes; default is **all nodes expanded** (btop-like tree mode).
  - On refresh, existing collapsed/expanded choices are preserved for surviving pids.
  - New pids default to expanded.
- Selection model: remembered `selected_pid` + `(cursor, scroll)` indices over visible rows.
  - Refresh and sort/mode/kind changes re-locate cursor by `selected_pid` when still visible.
- Sorting model: key (`agg|own|pid|comm`) + direction, applied per sibling/root group while preserving tree structure.
- Mouse hit-test model: draw stores current body rect + rendered-row toggle marker ranges (`[+]/[-]`) in `App`.
  - Wheel: cursor ±3 rows.
  - Left click a sortable column header (COMM/PID/OWN/Σ/%): sort by it; active column reverses.
    Hit rects are stored per frame via `App::set_header_hit_test`.
  - Left click row: select.
  - Left click marker or double-click row (keyed by pid): toggle expand/collapse.
- Meter scale decision: row meter fill uses `row.aggregated / max(visible aggregated)` (not system total).
  - OWN/AGG text colors still use `% of system total`.

## Graphs (milestone 9)

- `ui::graph::History` (cap 600) holds system memory in use (`Snapshot::used_mem`: macOS `used_memory()`,
  Linux `MemTotal - MemAvailable`), pushed once per tick by `App`; independent of the selected `MemKind`.
  `App::mem_kind_sum()` is Σ of the selected kind (may exceed RAM) and is only displayed, never a scale. `pushed` counts samples
  ever added so braille cell pairing is anchored to absolute sample parity (no shimmer).
- `graph::render(&History, width, height, max, GraphSymbols, &Theme) -> Vec<Line>` (top row first).
  - Braille: 2 samples/cell, 4 levels/row via const `BRAILLE_UP[left][right]`; newest sample is the right
    column of the rightmost cell (an odd trailing sample repeats in both columns); left padded with blanks.
  - Block/tty: 1 sample/cell (`BLOCK_UP` diagonal, `TTY_UP` = ` ░░▒█`).
  - Row color = usage gradient at the row band's top pct (btop); height 1 colors each cell by its max sample.
  - Scale `max` = system total memory (`App::mem_graph_max`); values clamp.
- `GraphSymbols::{detect(), from_term}`; `App.graph_symbol` stores the user choice
  (`auto|braille|block|tty`) and resolves to a concrete renderer at draw time.
- `graph::meter_spans(theme, width, value, scale)`: per-cell gradient `■` filled part + `meter_bg` rest.
- Table columns: COMM, PID, OWN, `<MEM> Σ`, `%` (Σ ÷ total RAM, 1 decimal), CHILDREN, METER.
- Layout: status line, then rounded " mem " box (6 rows: 3 graph + 1 summary line, hidden when the inner
  area is < 12 rows), table header, table body, hint bar.

## Menus + config (milestone 10)

- App mode enum: `AppMode::{Normal, Menu, Options, Help}`.
  - `Esc` toggles menu from Normal.
  - Mouse: Options rows/overlay and Help overlay rects are cached per frame (`set_options_hit_test`,
    `set_help_hit_test`, `clear_overlay_hit_tests`); see decisions.md (M19) for semantics.
  - Menu keys: `↑/↓/j/k/Tab/BackTab`, `Enter/Space`, `Esc/q`; mouse click on button activates, click
    outside closes. Buttons are clipped to the bordered inner area on tiny terminals.
  - `Ctrl-C`/`Ctrl-Q` quit from every mode (checked in `App::on_key` before mode dispatch).
- Menu buttons (centered overlay): `Options`, `Help`, `Quit`.
  - Selected button uses double-line border + `hi_fg`; unselected uses single-line + `main_fg`.
- Options overlay:
  - Settings list: `update_ms`, `mem_kind`, `agg_mode`, `graph_symbol`, `base10`,
    `save_on_exit`, `sort_key`, `sort_reverse`, `color_theme`, `theme_background`, `truecolor`,
    `rounded_corners`, `vim_keys`, `disable_mouse`, `clock_format`, `background_update`, `force_tty`.
  - `update_ms`: Enter-to-edit digit buffer (max 8 digits; `Enter` commit, `Esc` cancel), or `←/→` ±100.
    Clamped to `100..=86_400_000` everywhere (app, config load) so tick deadlines cannot overflow.
  - Other rows: `←/→` cycles/toggles; effects apply live to app rendering/sorting/refresh behavior.
  - Bottom line shows per-option description text.
- Config schema (`serde` + TOML):
  - `update_ms`, `mem_kind`, `agg_mode`, `graph_symbol`, `base10`, `save_on_exit`,
    `sort_key`, `sort_reverse`, plus (M20) `color_theme`, `theme_background`, `truecolor`,
    `rounded_corners`, `vim_keys`, `disable_mouse`, `clock_format`, `background_update`, `force_tty`.
  - Options overlay lists all 17 (scrolls); themes dir = `<config dir>/themes` (`config::themes_dir`).
  - `load()` is tolerant: missing file => defaults; unknown keys ignored; invalid values fallback.
  - `save()` creates parent dir and writes TOML with a header comment to `pspp.conf.tmp`, then renames
    over `pspp.conf` (atomic on the same filesystem).

## Help + filter (milestone 11)

- App mode enum now includes `FilterInput` (`AppMode::{Normal, FilterInput, Menu, Options, Help}`).
- Help overlay:
  - centered rounded box titled `" help "` with scrollable content
  - scroll keys: `↑/↓/j/k/PgUp/PgDn`/wheel; close: `Esc`/`q`/click outside; scrollbar when overflowing
  - sections rendered from `ui/help.rs` as styled `Line`s:
    - key bindings grouped by navigation/tree/display/menu
    - mouse actions
    - memory types explained (`rss`, `vsz`, `footprint`, `pss`, `uss`, `swap`) with platform
      availability and approximate (`~`) notes
    - rollup vs flat aggregation summary
- Filter behavior:
  - `f` or `/` enters input mode with status prompt `filter: <input>█`
  - input accepts printable characters (including spaces), `Backspace`, `Enter`, `Esc`
  - `Enter` commits (and expands ancestors of matches); `Esc` restores the previously committed filter;
    active filters show `filter: ...`; `c` clears
  - the filtered tree uses the real `expanded` set (collapse/expand work while filtering)
  - matching is case-insensitive substring on `comm`
  - retained rows are matching nodes plus ancestors of matching descendants (context preserved)
  - matching `comm` is highlighted (`hi_fg`); non-matching ancestors are dimmed (`inactive_fg`)
- Pure match-set logic:
  - `ProcTree::filter_match_set(&str) -> FilterMatchSet { visible, direct }` in `model/tree.rs`
  - unit tests cover: leaf match keeps ancestors, no matches => empty, empty filter => full set
