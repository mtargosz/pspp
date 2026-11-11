# pspp — Architecture (living document)

## Module map

```
src/
  main.rs            — clap CLI entry: interactive TUI (default) vs non-interactive query mode
  collect/
    mod.rs           — Snapshot API: Vec<ProcRec { pid, ppid, comm, mem: MemSet }>
                       MemSet { rss, vsz, footprint?, pss?, uss?, swap? } (bytes, Option per platform)
    macos.rs         — sysinfo base + libproc proc_pid_rusage (ri_phys_footprint); graceful fallback
    linux.rs         — /proc/<pid>/stat + status (rss/vsz) + smaps_rollup (pss/uss/swap); fallback to stat
  model/
    tree.rs          — pid→ppid forest (orphans → synthetic root); subtree rollup sums; flat
                       direct-children sums; sorting by any memory column; stable across refreshes
  query/
    mod.rs           — non-interactive mode: name/pid filter, --mem type, --min threshold,
                       plain-table and --json output
  ui/
    app.rs           — event loop (crossterm events + tick), AppState, update_ms, mode toggles
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
