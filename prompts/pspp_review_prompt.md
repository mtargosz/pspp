# pspp — follow-up work from the 2026-11-11 review

You are working in the `pspp` Rust repo at `/Users/mtargosz/dev/projects/pspp` (branch `main`). `pspp` ("ps++") is a
btop-style TUI + one-shot query CLI that sums process memory grouped by parent process (`pid ← ppid`). The original
spec is `pspp_initial_prompt.md`. The project was built by a multi-agent workflow whose state lives in
`.github/agents/memory/` — read `requirements.md`, `decisions.md`, `architecture.md` and `progress.md` before touching
code; they are accurate except where this prompt says otherwise.

A review found the issues below. Fix them milestone by milestone, in order. Each milestone is one commit.

## Ground rules

- Use the TodoWrite/Task tools to track the milestones; mark each done only after its acceptance criteria pass.
- Gates that must be green before every commit:
  `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check && cargo doc --no-deps`
- Conventional commits (`feat:`, `fix:`, `test:`, `docs:`, `refactor:`), one per milestone. **Never push.**
- Keep memory current in the same commit: tick the milestone in `progress.md` (add a dated log entry), append
  decisions to `decisions.md` (append-only, dated), update `architecture.md` in place for API/module changes.
- No `unwrap()` outside tests; parsing of `/proc` and syscall data must be panic-free. Bytes as `u64`, saturating sums,
  format only at render. Platform code behind `#[cfg(target_os = ...)]`.
- You are on macOS. Linux code cannot be run here; keep it compiling under `cargo check --target x86_64-unknown-linux-gnu`
  (install the target with `rustup target add x86_64-unknown-linux-gnu` if missing) and unit-test its parsers under
  `#[cfg(any(target_os = "linux", test))]`.
- If a requirement here conflicts with `decisions.md`, this prompt wins; record the reversal in `decisions.md`.
- Ask before M20 about the two open questions at the bottom if the user has not answered them.

## Verified findings (context — do not re-investigate, just fix)

1. **macOS parent links are wrong without root.** 215 of 688 live processes have `ppid == 0` in pspp output because
   `sysinfo::Process::parent()` relies on `proc_pidinfo(PROC_PIDTBSDINFO)`, which fails for other users' processes.
   `ps` gets ppid unprivileged from `sysctl(CTL_KERN, KERN_PROC, KERN_PROC_ALL)` → `kinfo_proc.kp_eproc.e_ppid`.
2. **Query mode inherits TUI-saved config.** `main.rs::Cli::to_query_opts` reads `mem_kind`/`agg_mode`/`sort_key`/`base10`
   from `pspp.conf`. With default `save_on_exit = true`, a TUI session that pressed `e` persists `agg_mode = "flat"`
   and every later `pspp --json` returns flat sums (leaf rows `agg: 0`). Neither plain header nor JSON names the mode.
3. **`--mem`, `--agg`, `--sort`, `--base10` force query mode** (`Cli::is_query_mode_requested`), so the TUI can never
   be launched with them; the `cli.mem`/`cli.agg`/`cli.sort` branches in `cli_and_config_to_ui_settings` are dead.
4. **Graph scale is misleading.** `App::mem_total_used()` sums the selected kind over all processes; for RSS/footprint
   that exceeds physical RAM (43 GiB footprint on this machine) so the `mem` graph and `NN%` pin at 100.
5. Status line prints `total <RAM>` coloured by usage %; filter mode force-expands everything and ignores `expanded`;
   Esc in filter input clears instead of restoring (btop restores); `--update-ms` from the CLI is persisted by
   `save_on_exit`; Options/Help overlays have no mouse handling; btop General-options parity is thin; Linux collector
   reads `smaps_rollup` for every pid every tick and has no `parse_smaps_rollup` tests; `ProcTree::build` clones every
   `ProcRec`; double-click keyed by row index; `App::from_settings` always takes a live snapshot (UI tests are
   nondeterministic); `tests/fixtures/` is empty and `tests/model_tests.rs` does not exist although `architecture.md`
   lists both; no CI; PID column fixed at 6 cells; kind/agg/sort string mappings duplicated across `main.rs` and
   `ui/app.rs`; `ProcTree::sorted_pids` and `Snapshot::taken_at` are dead.

## Milestones

### M15 — `fix(collect): correct ppid on macOS without root`
Files: `src/collect/macos.rs`, `README.md`, `.github/agents/memory/progress.md`
- Get `pid`, `ppid` (and `p_comm` as a fallback name) for every process from
  `sysctl([CTL_KERN, KERN_PROC, KERN_PROC_ALL])` via `libc`. Size the buffer from the first call, retry on `ENOMEM`
  when the table grows between calls. Keep `sysinfo` for name/RSS/VSZ and `proc_pid_rusage` for footprint; join by pid.
- Acceptance: a unit test asserts pspp's `ppid` for `std::process::id()` equals `libc::getppid()`; unprivileged
  `pspp --json | jq '[.rows[]|select(.ppid==0)]|length'` is ≤ 2 (kernel_task/launchd), not ~215.
- Remove the "run `sudo pspp`" platform note from `README.md` and the macOS entry from "Known issues" in `progress.md`.

### M16 — `feat(cli): deterministic query mode; TUI accepts --mem/--agg/--sort/--base10`
Files: `src/main.rs`, `src/query/mod.rs`, `tests/query_cli.rs`, `README.md`, `doc/pspp.1`
- Query mode never reads `pspp.conf`. Defaults: `rss`, `rollup`, sort `agg`, binary units. Record in `decisions.md`.
- Plain header shows the mode: `RSS Σ (rollup)` / `RSS Σ (flat)`. JSON becomes `{ "mem": "rss", "agg": "rollup", "rows": [...], "total": N }`.
- Mode selection: query mode iff `PATTERN`, `--pid`, `--min`, `--top`, `--json`, `--query` is given **or** stdout is not a tty.
  `--mem`, `--agg`, `--sort`, `--base10`, `--update-ms` apply to whichever mode runs. Add `--query` (force one-shot
  on a tty) and `-f/--filter <TEXT>` (pre-set the TUI filter, like btop `-f`).
- `save_on_exit` persists only values the user changed in the Options editor, never CLI overrides. Implement by
  tracking `UiSettings` dirty flags (or diffing against the loaded `Config`), with a unit test.
- Update `--help` (`long_about`, examples), README query section, man page, and integration tests
  (`jq .agg`, header contains `(rollup)`, `pspp --mem footprint` with a tty stdout is *not* query mode — test via
  `is_query_mode_requested` unit test since the integration harness has no tty).

### M17 — `feat(ui): honest memory graph, used/total status, % column`
Files: `src/collect/*`, `src/ui/app.rs`, `src/ui/draw.rs`, `src/ui/help.rs`, `src/query/mod.rs`
- Add `Snapshot::used_mem` (macOS: `sysinfo::System::used_memory()`; Linux: `MemTotal - MemAvailable` from `/proc/meminfo`).
- `mem` box graph plots `used_mem / total_mem`. Summary line: `used 12.1 GiB / 32.0 GiB ■■■■ 38%  Σ RSS 18.4 GiB`.
- Status line: `used X / Y` (gradient colour on used fraction) instead of `total X`.
- New `%` column (row aggregated ÷ total RAM, one decimal) in the TUI table and plain output; `pct` field in JSON rows.
- Help: one paragraph on why Σ of per-process values can exceed RAM (shared pages; footprint includes compressed memory).

### M18 — `fix(ui): filter honours tree state; btop-style Esc; pid-keyed double-click; header-click sort`
Files: `src/ui/app.rs`, `src/ui/draw.rs`, `src/model/tree.rs`, `src/ui/help.rs`, `README.md`
- Filtered view uses the real `expanded` set; on commit, insert all ancestors of direct matches into `expanded`.
  Space/Enter/←/→ work while filtered.
- Filter input: Esc restores the previously committed filter; `c` clears. Update help text and README key tables.
- `last_click` keyed by `pid`, not row index.
- Left click on a column header sorts by that column; clicking the active column reverses. Store header hit rects
  in `App` like the existing row hit-test cache.

### M19 — `feat(ui): mouse in Options/Help overlays; context-sensitive hint bar`
Files: `src/ui/app.rs`, `src/ui/draw.rs`
- Options: click row → select; click selected row → edit/advance; wheel → move selection; click outside → close.
- Help: wheel scrolls; click outside closes; render a `ratatui::widgets::Scrollbar` when content overflows.
- Hint bar content depends on `AppMode` (Normal / FilterInput / Menu / Options / Help) and is truncated to width.
- Extend `every_mode_renders_at_tiny_sizes_without_panicking` with mouse events at every position of a 10×3 frame.

### M20 — `feat(options): btop General parity and global keys`
Files: `src/ui/app.rs`, `src/ui/draw.rs`, `src/ui/theme.rs`, `src/ui/graph.rs`, `src/config.rs`, `src/ui/help.rs`
- New options with config keys, live apply and descriptions: `theme_background` (bool), `truecolor` (`auto|on|off`),
  `rounded_corners` (bool), `vim_keys` (bool; when off, `h/j/k/l/g/G` are inert), `disable_mouse` (bool),
  `clock_format` (strftime string rendered in the top border like btop; empty = off; use `chrono` or hand-roll
  `%H:%M:%S`), `background_update` (bool; when false, ticks pause while an overlay is open), `force_tty` (bool).
- Global keys: `+`/`-` adjust `update_ms` ±100; `F1`/`?`/`h` open Help; `F2`/`o` open Options. Keep `m` = memory kind
  and note the deviation from btop in help/README.
- `graph_symbol = auto` resolves to `tty` when `force_tty` is on or `TERM` lacks colour capability (`linux`, `dumb`, no `color` in name).
- Config round-trip tests for every new key; tolerant load for missing/invalid values.

### M21 — `perf(collect): background collection; lazy smaps_rollup; tree owns snapshot`
Files: `src/collect/*`, `src/model/tree.rs`, `src/ui/mod.rs`, `src/ui/app.rs`, `src/query/mod.rs`
- Collect snapshots on a worker thread (`std::thread` + `mpsc`); the UI loop polls the channel and never blocks on I/O.
  The worker receives the current `MemKind` so Linux reads `smaps_rollup` only for `pss|uss|swap`; for `rss|vsz`
  those fields are `None` and **not** flagged approximate.
- Linux name: `/proc/<pid>/cmdline` argv[0] basename, fallback `stat` comm.
- Unit tests for `parse_smaps_rollup` (gate under `#[cfg(any(target_os = "linux", test))]`): happy path,
  `Pss_Anon:`/`SwapPss:` must not match `Pss:`/`Swap:`, missing `Private_*` → `uss = None`.
- `ProcTree::build(snapshot: Snapshot)` takes ownership; `App` and `query` read procs through `tree.proc(pid)` /
  `tree.procs()`; remove the per-tick `ProcRec` clone.
- PID column width derived from the largest pid in the snapshot (min 5).

### M22 — `test: synthetic snapshots, fixtures, TestBackend screens, CI`
Files: `src/ui/app.rs`, `src/ui/draw.rs`, `tests/`, `.github/workflows/ci.yml`, `.github/agents/memory/architecture.md`
- `App::with_snapshot(Snapshot, UiSettings)` (test-only or `pub(crate)`); `App::from_settings` delegates to it.
- `TestBackend` snapshot tests of the main screen with a 4-level synthetic tree: rollup/flat × filtered/unfiltered ×
  braille/block/tty, asserting key cells (tree glyphs, Σ values, `~` markers, meter fill).
- Populate `tests/fixtures/*.json` (deep chain, fan-out, orphans, pid reuse, cycle, duplicate pid) and add
  `tests/model_tests.rs` that loads them through a small helper and checks rollup/flat totals and root sets.
- `.github/workflows/ci.yml`: matrix `macos-latest` + `ubuntu-latest`; `cargo build`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo doc --no-deps`.
- Fix `architecture.md` so it matches the real test layout.

### M23 — `refactor: single source of truth for enums; docs pass`
Files: `src/model/mod.rs`, `src/model/tree.rs`, `src/main.rs`, `src/ui/app.rs`, `src/ui/draw.rs`, `Cargo.toml`, `README.md`, `doc/pspp.1`
- `MemKind`, `AggMode`, `SortBy`: `as_str()`, `FromStr`, `Display`, `clap::ValueEnum`. Delete `CliAgg`, `CliSort`,
  `format_available_mem_kinds`, `mem_kind_to_config`, `ui::parse_*_config`, `ui::sort_to_config`.
- Remove `ProcTree::sorted_pids` and `Snapshot::taken_at` (or use `taken_at` for a "sample age" indicator — pick one).
- `MemKind::available()` → `&'static [MemKind]`.
- Meter column width `12..=30` scaling with terminal width before COMM takes the slack.
- `Cargo.toml`: `rust-version`, `repository`, `readme`, `keywords`, `categories`.
- Docs: regenerate README key tables and man page from the final `help.rs` tables; replace the screenshot TODO with
  an actual capture instruction or image; refresh `architecture.md` module map and public APIs.

## Open questions (ask the user before M20)

1. Keep `m` = cycle memory kind (menu only via `Esc`), or follow btop (`m` = menu) and move memory-kind cycling to `t`?
2. Is a `color_theme` option that loads btop `.theme` files from `$PSPP_CONFIG_DIR/themes` wanted, or is the Default theme enough?

## Done means

All eight milestones committed on `main`, gates green, `progress.md` status shows M15–M23 done with log entries,
`decisions.md` records the query-mode defaults and any answered open questions, and `pspp` run unprivileged on macOS
shows `launchd` as the parent of user processes with a rollup Σ that is ≥ every child's Σ.
