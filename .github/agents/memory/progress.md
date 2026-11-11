# pspp — Progress board (living document)

## Status key: [ ] pending · [~] in progress · [x] done · [!] blocked

## Plan (dependency order)

- [x] 1. scaffold — Cargo.toml, .gitignore, module skeleton
- [x] 2. agent-profiles — all 7 profiles in `.github/agents/`
- [x] 2b. memory-store — this directory, seeded
- [x] 3. collector — cross-platform snapshot (RSS/VSZ; macOS footprint; Linux PSS/USS/swap)
- [x] 4. tree-model — forest, rollup + flat aggregation, sorting, unit tests
- [x] 5. query-mode — pspp <query> stdout mode (filters, --json)
- [x] 6. tui-skeleton — ratatui shell: alt screen, mouse, tick loop, resize, quit
- [x] 7. theme-gradients — theme struct, 101-step gradients, 256-color fallback
- [x] 8. tree-table-ui — expand/collapse, kb+mouse nav, gradient meters, aggregation toggle
- [ ] 9. graphs — braille history graph + meter bars (block/tty fallbacks)
- [ ] 10. menus — Esc overlay, Options (General-style), config persistence
- [ ] 11. help — plain-English memory explanations, filter (f, /)
- [ ] 12. testing-pass — tester agent full pass
- [ ] 13. review-pass — reviewer agent full pass
- [ ] 14. docs — README, --help, man page

## Log

### 2026-11-11 — orchestrator (planning session)
- Plan approved by user (Rust/ratatui; macOS+Linux; both aggregation modes; live refresh;
  non-interactive query mode; 7 agents; this persistent memory).
- btop research completed and persisted to `btop-reference.md`.
- Workspace moved to `/Users/mtargosz/dev/projects/pspp` on user direction; commit per
  milestone, never push.
- Scaffold, agent profiles, and memory store seeded (milestone 1+2).

### 2026-11-11 — coder
- Implemented milestone 3 collector scaffolding: `collect::MemSet`, `ProcRec`, `Snapshot`,
  and `collect::take_snapshot()` platform dispatch.
- Added macOS collector using `sysinfo` process enumeration and `libproc` `proc_pid_rusage`
  (`RUSAGE_INFO_V2`) to capture `ri_phys_footprint`, with graceful fallback (`approximate=true`).
- Added Linux collector parsing `/proc/<pid>/stat` for pid/ppid/comm/rss/vsz and
  `/proc/<pid>/smaps_rollup` for PSS/USS/swap with permission-safe fallback.
- Added parser unit tests (including comm containing `) (`) and a live snapshot invariant test.
- Added temporary `main.rs` stub that snapshots and prints process count + total RSS.

### 2026-11-11 — coder
- Completed milestone 4 model layer: added `src/model/mod.rs` + `src/model/tree.rs` and wired
  `mod model;` in `main.rs`.
- Implemented `MemKind` (`get`, `available`, `label`, `FromStr`) and tree construction from
  `Snapshot` with deterministic ordering, synthetic-root semantics for orphan/self-link roots,
  and cycle breaking/visited guards for traversal safety.
- Added iterative aggregation in both modes (`AggMode::Rollup` and `AggMode::Flat`) with
  saturating arithmetic, per-pid own/aggregate values, approximate flags, and child/descendant counts.
- Added stable sorting helpers (`aggregated`, `own`, `pid`, `comm`) and
  `flatten_visible(expanded)` depth-first row generation.
- Added focused unit tests for deep-chain rollup/flat behavior, wide/orphan/self/cycle handling,
  saturating addition, missing-value approximation, flattening collapse/expand behavior, and sorting.
- Kept `main.rs` as a compiling smoke stub, now printing top rollup RSS parents.

### 2026-11-11 — coder
- Completed milestone 5 query-mode + real CLI entry:
  - Replaced `main.rs` stub with derive-`clap` CLI and mode switch:
    - query mode when query args are present or stdout is non-tty
    - tty + no query args currently prints `TUI not yet implemented` (placeholder for milestone 6)
  - Added `src/query/mod.rs` pure logic:
    - `QueryOpts`, filtering (`PATTERN`, `--pid` subtree), aggregation mode (`rollup|flat`),
      sorting (`agg|own|pid|comm`), `--min`, `--top`, plain table renderer, and JSON renderer
    - size parse/format utilities (`512K`, `1.5G`, binary vs decimal units) with unit tests
  - Added integration tests `tests/query_cli.rs` that execute the real binary for JSON validity,
    top-row limiting, no-match stderr behavior, and invalid `--mem` diagnostics.
  - Updated architecture memory docs with final milestone 5 CLI/query API details.

  ### 2026-11-11 — coder
  - Completed milestone 6 TUI skeleton:
    - Added `src/ui/mod.rs` with `run_tui(update_ms)` event/tick loop using crossterm poll timeout,
      terminal RAII lifecycle (alternate screen/raw mode/cursor/mouse), and panic-hook restoration.
    - Added `src/ui/app.rs` app state and refresh logic (`Snapshot` + `ProcTree` rebuild on ticks),
      quit key handling (`q`, `Ctrl-C`, `Ctrl-Q`), mouse position tracking, and timing helpers.
    - Added `src/ui/draw.rs` minimal rounded-border frame titled `pspp` with status + hint lines.
    - Wired `main.rs` to route tty+no-query-args into TUI mode and added `--update-ms` global flag
      (default 2000, clamped to minimum 100) without changing query-mode behavior.
    - Added unit tests for update interval clamp and deadline/timeout helper logic.

### 2026-11-11 — ui-stylist
- Completed milestone 7 theme + gradients:
  - Added `src/ui/theme.rs`: `Rgb`, `Theme` (btop Default colors, `main_bg: None`), 101-step
    usage/graph gradients (two 50-step passes #77ca9b → #cbc06c → #dc4c4c), `usage_color`,
    `color()` applying truecolor-vs-256-color policy decided once from `COLORTERM`.
  - `App` now owns a `Theme`; `draw.rs` colors border/title/status/hint from the theme and the
    "total" figure via `usage_color(sum(selected mem kind) / system memory)`.
  - 7 theme unit tests + 1 draw helper test; build, clippy `-D warnings`, fmt, tests green.
  - Deviation: grey (r==g==b) colors use btop's 232+ ramp (<8→16, >248→231) instead of the cube;
    non-grey use the cube formula. `theme.rs` has module-level `allow(dead_code)` until later
    milestones consume the full API.

### 2026-11-11 — coder
- Completed milestone 8 interactive tree table UI:
  - Expanded `App` state with expanded-node set, cursor/scroll, sort key+direction, remembered
    selected pid, and draw-provided row hit-test metadata for mouse interactions.
  - Added key handling: `↑/↓/j/k`, `PgUp/PgDn`, `Home/End`, `g/G`, `Space/Enter`, `←/→`,
    `e` (agg mode), `m` (mem kind cycle), `s` (sort cycle), `S`/`r` (reverse).
  - Added mouse handling: wheel scroll (±3 rows), row click select, marker click and double-click
    row expand/collapse.
  - Reworked draw path to render a full process tree table with btop-style tree glyphs/markers,
    COMM/PID/OWN/AGG/CHILDREN columns, per-row `■` meter, selected-row full highlight, and status+hint bars.
  - Added pure helper tests for tree prefix glyphs, meter fill computation, cursor/scroll clamp,
    and selection relocation logic.
- Decision captured in architecture: expanded-set model (default all expanded), meter scaling against
  largest visible aggregate, and table hit-test cache in `App`.
