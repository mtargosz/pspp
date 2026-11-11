# pspp — Progress board (living document)

## Project status: follow-up milestones M15–M23 in progress (see `pspp_review_prompt.md`)

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
- [x] 9. graphs — braille history graph + meter bars (block/tty fallbacks)
- [x] 10. menus — Esc overlay, Options (General-style), config persistence
- [x] 11. help — plain-English memory explanations, filter (f, /)
- [x] 12. testing-pass — tester agent full pass
- [x] 13. review-pass — reviewer agent full pass
- [x] 14. docs — README, --help, man page
- [x] M15 fix(collect): correct ppid on macOS without root
- [x] M16 feat(cli): deterministic query mode; TUI accepts --mem/--agg/--sort/--base10
- [x] M17 feat(ui): honest memory graph, used/total status, % column
- [x] M18 fix(ui): filter honours tree state; btop-style Esc; pid-keyed double-click; header-click sort
- [ ] M19 feat(ui): mouse in Options/Help overlays; context-sensitive hint bar
- [ ] M20 feat(options): btop General parity and global keys
- [ ] M21 perf(collect): background collection; lazy smaps_rollup; tree owns snapshot
- [ ] M22 test: synthetic snapshots, fixtures, TestBackend screens, CI
- [ ] M23 refactor: single source of truth for enums; docs pass

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

### 2026-11-11 — ui-stylist
- Completed milestone 9 memory history graph + summary meters:
  - Added `src/ui/graph.rs`: `History` ring buffer (cap 600, absolute `pushed` counter so braille
    cell pairing stays parity-anchored while scrolling), `GraphSymbols {Braille, Block, Tty}`
    (`TERM=linux` → Tty; `parse`/`label` ready for milestone 10), const 5×5 `BRAILLE_UP`/`BLOCK_UP`
    tables (`[left][right]` = btop `left*5+right`) + `TTY_UP`, `row_level`/`quantize` (btop rounding,
    0.3 bias for 1 row / 0.1 otherwise), `render()` (multi-row, newest at right edge, left padded,
    rows colored by band-top pct as in btop), `meter_spans()` (per-cell gradient `■` meter).
  - `App` owns `mem_history`, `graph_symbols`; samples pushed in `new()` and every `on_tick`;
    history cleared (and re-seeded) on mem-kind change; `mem_total_used()` / `mem_graph_max()`.
  - `draw.rs`: rounded " mem " box (6 rows) under the status line: 3 graph rows + summary line
    `<KIND> total X / Y ■■■ NN%`. Hidden when inner height < 12.
  - Fixed pre-existing bug from milestone 8: body rows were rendered at the header row's `y`
    (column rects were reused unchanged), so only the header line showed row text.
  - Tests: 11 graph unit tests + 1 `TestBackend` draw test. build/clippy/fmt/test green.

### 2026-11-11 — coder
- Completed milestone 10 menus/options/config persistence:
  - Added `src/config.rs` with tolerant config load/save:
    - Path resolution: `$PSPP_CONFIG_DIR/pspp.conf` override, else `dirs::config_dir()/pspp/pspp.conf`.
    - Supported fields: `update_ms`, `mem_kind`, `agg_mode`, `graph_symbol`, `base10`,
      `save_on_exit`, `sort_key`, `sort_reverse`.
    - Missing file returns defaults; unknown keys ignored; bad values fall back per-field.
    - Save writes header-commented TOML and creates parent directories.
  - Wired `main.rs` to load config at startup, apply CLI overrides, pass `UiSettings` into TUI,
    and save runtime-edited settings on exit when `save_on_exit=true`.
  - Implemented app mode system in `ui/app.rs`:
    - `AppMode::{Normal, Menu, Options, Help}` with key routing by mode.
    - Menu navigation/activation (`↑/↓/j/k`, Enter, Esc), plus mouse click button activate /
      outside-click close behavior.
    - Options editor interactions (live apply): update interval edit buffer, toggles/cycles for
      mem kind, aggregation mode, graph symbol, unit base, save-on-exit, sort key, sort direction.
  - Extended `ui/draw.rs` overlays:
    - Centered Esc menu with stacked “big button” boxes; selected button uses double-line border.
    - Options list overlay with selected-row highlight and per-option description line.
    - Help placeholder overlay (`Help — milestone 11`).
  - Added tests:
    - `config.rs`: save/load roundtrip, missing-file defaults, garbage-file fallback.
    - `ui/app.rs`: option-cycle helper behavior (index/sort/mem-kind/graph-symbol).

### 2026-11-11 — coder
- Completed milestone 11 help + process filter:
  - Replaced help placeholder with a scrollable help overlay (`↑/↓/j/k/PgUp/PgDn`, `Esc/q`) and
    moved all help text content to `src/ui/help.rs` as static strings rendered into styled lines.
  - Added required help sections: grouped key bindings, mouse actions, memory-type explanations
    (rss/vsz/footprint/pss/uss/swap), and a rollup-vs-flat summary.
  - Implemented filter input mode (`f` or `/`) with status prompt `filter: <input>█`,
    commit/clear behavior (`Enter`, `Esc`, `Backspace`, `c`), and active-filter status indicator.
  - Added model-level pure matching logic `ProcTree::filter_match_set` (case-insensitive comm
    substring; include matching nodes and ancestors of matching descendants), plus row highlighting
    (direct matches hi_fg, context ancestors inactive_fg).
  - Added focused tests:
    - `model/tree.rs`: filter match-set behavior (ancestor retention, empty matches, cleared filter).
    - `ui/help.rs`: memory-help coverage for every `MemKind` and help scroll clamp helper.
  - Validation: `cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt`,
    `cargo test` all green.

### 2026-11-11 — tester
- Milestone 12 testing pass over the whole crate.
- Test counts: before 51 unit + 4 integration; after 62 unit (+11) + 4 integration. All green.
- Gates: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all clean.
- Bugs fixed:
  - Unicode case folding: pattern, TUI filter, and comm sort used ASCII-only lowercasing, so
    `café` did not match `CAFÉ`. Switched `query/mod.rs` (pattern, comm sort) and `model/tree.rs`
    (`filter_match_set`, `compare_pid_for_sort`) to `to_lowercase()`.
  - `format_size` unit boundary: values rounding up to the base printed as `1024.0 KiB`
    (or `1000.0 KB`). Unit is now chosen after one-decimal rounding (`1.0 MiB`).
- Tests added:
  - `query/mod.rs`: `pattern_match_folds_unicode_case`, `run_query_handles_empty_and_single_process_snapshots`,
    `run_query_pid_subtree_terminates_on_cycle`, `run_query_unknown_pid_yields_no_rows`,
    `run_query_total_saturates_on_overflow`, `parse_size_edge_inputs`,
    `format_size_promotes_unit_when_rounding_reaches_base`.
  - `model/tree.rs`: `filter_match_set_folds_unicode_case`, `single_process_is_its_own_root_without_children`.
  - `ui/graph.rs`: `render_zero_size_returns_no_lines`, `render_empty_history_yields_blank_rows_of_full_width`.
- Query-mode CLI exercised (`target/debug/pspp`): `--top 5`, `--json --top 5` (structure validated:
  rows/total, eight row keys), `--agg flat`, `--mem vsz`, `--mem footprint`, `--sort own`, `--min 1G`,
  `--min 0`, `--min 1.5`, `--base10`, pattern `edge`, pattern `café`, `--pid 1`, `--pid 999999`
  (+`--json`). Invalid: `--mem bogus` exit 2 (lists kinds), `--mem pss` exit 2 (unavailable on macOS),
  `--min 12X` exit 2, `--top -1` / `--min -1G` exit 2 (clap; needs `--` form). `--top 0` exit 0.

### 2026-11-11 — reviewer (milestone 13)
- Full review of `src/` + `tests/`. Linux target not installed (`rustup target list --installed` =
  aarch64-apple-darwin only), so `collect/linux.rs` got a read-only review: stat field indexing after
  `)`, `Pss:`/`Swap:` prefix matching (skips `Pss_Anon`/`SwapPss`), USS = Private_Clean+Dirty, and cfg
  gates all look correct.
- Fixed (blocker): macOS `proc_pid_rusage(RUSAGE_INFO_V2)` wrote the 160-byte v2 struct into a
  hand-rolled 96-byte (v0-layout) buffer, a stack overflow (UB). Now uses `libc::rusage_info_v2` /
  `libc::proc_pid_rusage`. Also: `pid_to_i32` no longer allocates (`try_from(as_u32)`), and the
  snapshot refreshes only memory + process memory instead of `new_all()` + `refresh_all()` (double
  full refresh incl. CPU). Live check: 726 procs, footprint populated, root count same as before (224 vs 227, live churn).
- Fixed (blocker): `update_ms` had no upper bound (config i64, CLI u64, editor digits, →), so
  `Instant + Duration` could panic. Clamped to `100..=86_400_000` (btop max) in app and config load,
  `checked_add` in `next_tick_deadline`, editor capped at 8 digits.
- Fixed (should-fix): query `TOTAL` double counted nested rollup rows (e.g. launchd + its children).
  Now each process is counted once (see decisions.md). `run_query` returns `QueryResult { output,
  row_count }`, so `main` no longer rebuilds the tree via the removed `count_matches`.
- Fixed (should-fix): Ctrl-C did not quit while a filter was active (the `c` clear-filter arm matched
  first) or in FilterInput/Menu/Options/Help (raw mode swallows SIGINT). Ctrl-C/Ctrl-Q now quit from
  every mode.
- Fixed (should-fix): `pspp | head` reported "failed to write stdout" and exit 1 on EPIPE; now success.
- Fixed (should-fix): config save truncated the file before writing; now write `.tmp` and rename.
- Fixed (should-fix): menu overlay panicked at small terminal sizes (e.g. 10x3, index outside buffer)
  because buttons were laid out in the unbordered overlay rect and not clipped. Now uses the bordered
  inner rect and clips buttons (`Rect::intersection`). Found by the new all-modes tiny-size render test.
- Fixed (nit): status line total colour summed `own` over visible rows only, so collapsing a node
  changed it; now uses `App::mem_total_used()`.
- btop fidelity: menu also closes on `q`, navigates with Tab/BackTab, activates with Space (help text
  updated).
- Dead code: removed the `#![allow(dead_code)]` in `ui/graph.rs` and `ui/theme.rs`; deleted unused
  `Theme::{graph_text, gradient_*, graph_gradient, is_truecolor, graph_rgb, graph_color}`,
  `Rgb::from_hex`, `GraphSymbols::{parse, label}`, `quantize`; `History::{is_empty, iter}` are
  `#[cfg(test)]`.
- Perf: Comm sort lowercases each name once (`sort_by_cached_key`) instead of twice per comparison.
- Decided the tester's 3 open questions (see decisions.md): `--top 0` rejected (exit 2); no matches
  exits 1; TOTAL is a deduplicated sum.
- Tests added: `update_ms_is_clamped_so_deadline_cannot_overflow`, `ctrl_c_quits_from_every_mode`,
  `menu_closes_on_q_without_quitting` (app); `update_ms_is_clamped_and_save_leaves_no_temp_file`
  (config); `run_query_total_counts_each_process_once` (query);
  `every_mode_renders_at_tiny_sizes_without_panicking` (draw);
  `comm_sort_is_case_insensitive_with_pid_tiebreak_in_both_directions` (tree);
  `top_zero_is_rejected_and_unmatched_json_exits_one` (integration). `high_min_reports_no_matches`
  now expects exit 1.
- Dependency audit: all 9 deps used (clap, crossterm, ratatui, sysinfo, libc, dirs, serde, serde_json,
  toml). Nothing removed.
- Suite: `cargo test` 68 unit + 5 integration green; `cargo clippy --all-targets -D warnings` and
  `cargo fmt --check` clean.

### 2026-11-11 — documentator (milestone 14)
- Wrote `README.md`: overview & motivation, build/install + macOS root/ppid note, TUI guide
  (exact `help.rs` key bindings & mouse tables, screenshot placeholder comment, `pspp.conf` +
  `PSPP_CONFIG_DIR` + `save_on_exit` reference), query mode guide (6 realistic examples incl.
  `--json | jq`, `--mem`/`--agg` combos, `--min`, `TOTAL` deduplication, and `0`/`1`/`2` exit
  codes), plain-English memory type guide + `~` approximate markers, and multi-agent development /
  `memory/*.md` documentation.
- Wrote `doc/pspp.1` (roff man page covering `NAME`, `SYNOPSIS`, `DESCRIPTION`, `OPTIONS`,
  `KEY BINDINGS`, `EXIT STATUS`, `FILES`, `EXAMPLES`, `SEE ALSO`); verified rendering via `man`
  and zero warnings via `mandoc -Tlint`.
- Added crate-level `//!` doc comment in `src/main.rs` and module-level `//!` one-liners in
  `src/collect/mod.rs`, `src/model/mod.rs`, `src/query/mod.rs`, `src/ui/mod.rs`, and `src/config.rs`.
- Consistency audit & `--help` polish:
  - Filled in 6 blank/incomplete clap `help` descriptions in `src/main.rs` (`--mem`, `--agg`,
    `--min`, `--sort`, `--top`, `--json`), clarified TUI vs query mode selection in `long_about`,
    and added CLI examples (`after_long_help`).
  - Documented `dirs::config_dir()` platform difference (`~/.config/pspp/pspp.conf` on Linux vs
    `~/Library/Application Support/pspp/pspp.conf` on macOS, plus `$PSPP_CONFIG_DIR` override) and
    `Shift-Tab` menu navigation alongside `help.rs` tables.
- Verified `cargo build`, `cargo test` (68 unit + 5 integration), `cargo clippy --all-targets -- -D warnings`,
  `cargo fmt --check`, and `cargo doc --no-deps` all clean. Project status: COMPLETE.

### 2026-11-11 — M15 (fix(collect): correct ppid on macOS without root)
- macOS collector now takes `pid`/`ppid`/`p_comm` from `sysctl(CTL_KERN, KERN_PROC, KERN_PROC_ALL)`
  (unprivileged, same source as `ps`); buffer sized from a first call with headroom, retried on `ENOMEM`.
  libc has no `kinfo_proc`, so the 648-byte entry is parsed by fixed byte offsets (pid 40, comm 243,
  e_ppid 560) with bounds-checked reads. sysinfo still supplies name/RSS/VSZ; `proc_pid_rusage` footprint.
- Live check unprivileged: 723 procs, 1 with ppid 0 (launchd) — was ~215.
- Tests: own ppid == `getppid()`, launchd present, ≤4 parentless, partial-trailing-entry parse.

### 2026-11-11 — M16 (feat(cli): deterministic query mode; TUI accepts --mem/--agg/--sort/--base10)
- Query mode ignores `pspp.conf`; header `RSS Σ (rollup)`; JSON gains `mem` and `agg`.
- Mode selection per the new rule; added `--query`, `-f/--filter` (`App::set_filter`, `run_tui(settings, filter)`).
- Config persistence diffs initial vs final settings (`apply_changed_settings_to_config`).
- Tests: `is_query_mode_requested` unit tests (incl. `--mem footprint` is not query mode), config-diff test,
  3 integration tests (JSON `mem`/`agg`, header, `PSPP_CONFIG_DIR` isolation). README, man page, `--help` updated.

### 2026-11-11 — M17 (feat(ui): honest memory graph, used/total status, % column)
- `Snapshot::used_mem` (macOS `used_memory()`, Linux `MemTotal-MemAvailable` via tested `parse_meminfo`).
- `mem` box plots `used_mem / total_mem`; summary `used X / Y ■■■ NN%  Σ RSS Z`; status line `used X / Y`.
- `%` column in TUI + plain query output (`format_pct`) and `pct` in JSON rows; plain renderer rewritten
  column-generic (also fixes width miscount of `Σ`). Help gained a "why totals can exceed RAM" section.
- Linux target installed; `cargo check --target x86_64-unknown-linux-gnu` clean.

### 2026-11-11 — M18 (fix(ui): filter honours tree state; btop-style Esc; pid-keyed double-click; header-click sort)
- Filtered rows use `expanded`; commit/`set_filter` expands ancestors of direct matches.
- Filter-input Esc restores the committed filter; help + README + man key tables updated.
- `last_click` is `(pid, Instant)`; header hit rects stored in `App` (`set_header_hit_test`) and sorted on click.
- `App::with_snapshot` introduced (needed for deterministic tests; M22 builds on it).
- 6 new app tests (ancestor expansion, collapse/toggle while filtered, Esc restore, pid double-click, header sort).

## Known issues
- Resolved 2026-11-11 (reviewer): `--top 0`, no-match exit status, TOTAL semantics. See decisions.md.

## Review notes (deferred, 2026-11-11 reviewer)
- `ProcTree::build` clones every `ProcRec` while `App` also keeps the snapshot (≈2× process memory per
  tick). Could borrow or move the snapshot into the tree.
- Config save rewrites the file: user comments and unknown keys are dropped.
- Menu is a simplified btop menu (no ASCII-art banner, no 80x24 minimum-size message).
- No mouse handling in the Options/Help overlays.
- `ProcTree::sorted_pids` (Comm arm case-sensitive) is only used by tests; `pub mod model` in
  `main.rs` keeps it from being flagged as dead. Consider removing or aligning with the UI sort.
- Query mode would print duplicate rows if a snapshot ever contained duplicate pids (theoretical).
