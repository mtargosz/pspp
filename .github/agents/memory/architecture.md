# pspp — Architecture (living document)

## Module map

```
src/
  main.rs            — clap CLI entry:
                       - if query args present OR stdout is non-tty: run query mode
                       - else (tty + no query args): run ratatui loop via `ui::run_tui`
                       Query flags: [PATTERN], --pid, --mem, --agg, --min, --sort, --top, --json, --base10
                       Global UI flag: --update-ms (min-clamped to 100; default 2000)
  collect/
    mod.rs           — Snapshot API: Vec<ProcRec { pid, ppid, comm, mem: MemSet }>
                       MemSet { rss, vsz, footprint?, pss?, uss?, swap? } (bytes, Option per platform)
    macos.rs         — sysinfo base + libproc proc_pid_rusage (ri_phys_footprint); graceful fallback
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
                       plain aligned table + JSON output wrapper { rows, total }
  ui/
    mod.rs           — `run_tui`: terminal lifecycle guard (alt screen/raw/mouse/cursor),
                       panic hook restoration, crossterm event loop + tick scheduling
    app.rs           — `App` state: snapshot/tree, agg mode, mem kind, update timing,
                       key/mouse handlers, periodic refresh on ticks
    draw.rs          — milestone-6 minimal frame draw function with rounded border + status lines
    theme.rs         — named theme colors + 101-step gradients, 256-color fallback
    graph.rs         — braille/block/tty history graph + ■ meter widgets (ratatui custom widgets)
    table.rs         — process tree table: expand/collapse, selection, gradient-colored mem columns
    menu.rs          — Esc overlay (Options/Help/Quit), Options editor, Help screens
  config.rs          — ~/.config/pspp/pspp.conf (TOML), save_on_exit
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

## Query-mode API (milestone 5)

- `query::QueryOpts`
  - `pattern` (case-insensitive substring on `comm`; `!` regex mode intentionally unsupported for now)
  - `pid` subtree filter
  - `mem_kind`, `agg_mode`, `min_bytes`, `sort_by`, `top`, `json`, `base10`
- `query::run_query(&Snapshot, &QueryOpts) -> String`
  - plain table columns: `PID`, `PARENT`, `COMM`, `OWN`, `<MEM> Σ`, `CHILDREN` + final `TOTAL`
  - JSON object: `{ rows: [{ pid, ppid, comm, own, agg, children, descendants, approximate }], total }`
- `query::{parse_size, format_size}`
  - parse `512K`, `100M`, `1.5G`, bytes; format binary (`KiB`) or decimal (`KB`) units
