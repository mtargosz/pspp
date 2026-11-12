# pspp — second review & proposed follow-up plan (2026-11-11)

You are working in the `pspp` Rust repository at `/Users/mtargosz/dev/projects/pspp` (commit on the current branch; **never push**).
`pspp` ("ps++") is an interactive btop-style TUI and scriptable one-shot CLI that aggregates process memory
across the `pid ← ppid` hierarchy. Prior specs and history live in `prompts/pspp_initial_prompt.md`,
`prompts/pspp_review_prompt.md`, `.github/copilot-instructions.md`, `.github/instructions/`, and
`.github/agents/memory/` (`requirements.md`, `architecture.md`, `decisions.md`, `progress.md`,
`conventions.md`, `btop-reference.md`).

All existing automated gates (`cargo test` — 170 tests across lib/bin/model/CLI, `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check`, `cargo doc --no-deps`, and `mandoc -Tlint doc/pspp.1`) currently pass. However, running
`pspp` interactively inside a real PTY across terminal sizes, overlays, filters, themes, and live macOS process
trees—paired with a full source audit—exposed critical rendered-UI defects, collector accuracy gaps,
state-persistence bugs, and UX/performance improvements detailed below.

---

## Part 1 — Verified findings

### A. Critical rendered-UI bugs (spotted by running the live TUI)

1. **All three overlays (`Menu`, `Options`, `Help`) bleed underlying UI text and colours straight through (`src/ui/draw.rs:463-663`).**
   - `draw()` renders `draw_main` and then `draw_overlay`, but none of `render_menu_overlay`,
     `render_options_overlay`, or `render_help_overlay` renders `ratatui::widgets::Clear` over `overlay`
     or applies `theme.main_bg` to the overlay `Block`.
   - Live capture of `Esc` (`Menu`) at 110×28:
     ```text
     ││                               ╭ menu ────────────────────────────────────╮                              ⣿││
     ││used 14.5 GiB / 18.0 GiB ■■■■■■│■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■│■■■■■ 80%  Σ FOOTPRINT 49.6 GiB││
     │╰───────────────────────────────│──────────────────────────────────────────│───────────────────────────────╯│
     │COMM                            │ ╔════════════════Options═══════════════╗ │   % CHILDREN METER             │
     │[-] launchd                     │ ║             1       0 B~    49.6 GiB~║2│5.5%      486 ■■■■■■■■■■■■■■■■■■│
     │├─ [-] Microsoft Edge           │ ╚══════════════════════════════════════╝1│0.9%       99 ■■■■■■■■■■■■■■■■■■│
     │   ├─ Microsoft Edge Helper (Ape│itif Rende 10746  943.0 MiB    943.0 MiB  │5.1%        0 ■■■■■■■■■■■■■■■■■■│
     │   ├─ Microsoft Edge Helper (Ape│i┌─────────────────Help─────────────────┐ │4.6%        0 ■■■■■■■■■■■■■■■■■■│
     │   ├─ Microsoft Edge Helper (Ape│i│if Rende 76602  722.7 MiB    722.7 MiB│ │3.9%        0 ■■■■■■■■■■■■■■■■■■│
     ```
   - In `Options`, unselected rows only write `"color_theme          Nord"`, so underlying process text merges
     directly onto option values (`"Nordf 38810 138.2 MiB..."`, `"%H:%M:%S961"`), and when `list_area.height`
     exceeds `OPTION_KEYS.len()` (e.g. 18 rows vs. 17 options), row 18 is untouched and shows a full process
     row inside the modal. In `Help`, every short wrapped line leaks meters and numbers on the right.

2. **Menu button labels render onto the top border instead of the button interior (`src/ui/draw.rs:509-519`).**
   - `render_menu_overlay` renders `button_block` into the 3-row `rect` and then renders `Paragraph::new(button.label())`
     into `rect` rather than `button_block.inner(rect)`. `Paragraph` writes to row 0 of `rect`, overwriting the
     top border (`╔════Options════╗`, `┌─────Help─────┐`, `┌─────Quit─────┐`) while leaving the middle row (`rect.y + 1`)
     empty (showing bled-through table text).

3. **Tree branch continuation bars (`│`) are off by one level on real process trees (`src/ui/app.rs:895-934`).**
   - `build_display_rows` pushes `visible_row.is_last_child` for **depth 0** (root nodes, which never draw a
     `├─`/`└─` connector column) into `ancestor_last[0]`, and then passes `&ancestor_last[..visible_row.depth - 1]`
     to `build_tree_prefix` instead of `&ancestor_last[1..visible_row.depth]`.
   - Live capture on macOS (`launchd` is the sole root → `ancestor_last[0] == true`, while `Microsoft Edge` at
     `depth = 1` has `is_last_child == false` with 485 siblings below it):
     ```text
     │[-] launchd                                       1        0 B    9.4 GiB  52.3%      486 ■■■■■■■■■■■■■■■■■■│
     │├─ [-] Microsoft Edge                          8017  194.4 MiB    2.4 GiB  13.5%       99 ■■■■■■■■■■■■■■■■■■│
     │   ├─ Microsoft Edge Helper (Aperitif Rendere 28174  314.6 MiB  314.6 MiB   1.7%        0 ■■■■■■■■■■■■■■■■■■│
     ```
     Column 0 under `├─ [-] Microsoft Edge` renders `"   "` instead of `"│  "`, leaving `Microsoft Edge`'s `├`
     dangling into blank space across all 99 children! At `depth = 3`, `ancestor_last[0..2]` shifts every
     ancestor's continuation state one column to the right; and when multiple roots exist (`ancestor_last[0] == false`),
     a non-last root's *last* child (`└─`) spuriously draws `"│  "` underneath `└─`.
   - *Why `draw::screens` missed it:* `synthetic_app` (`src/ui/draw.rs:791`) includes a second root `stray`
     after `launchd`, making `ancestor_last[0]`, `ancestor_last[1]`, and `ancestor_last[2]` all `false`.

4. **Selected-row meter always looks 100% full; table meters drop positional gradients and fail in TTY/plain text (`src/ui/draw.rs:383-411`, `src/ui/graph.rs:248-264`).**
   - When `selected` is true (`draw.rs:397`), `meter_line` ignores `value`/`scale` and returns `"■".repeat(width)`
     in bright `selected_fg` (`#eeeeee`). Moving the cursor onto a `0 B` (`0.0%`) leaf makes its meter jump to
     100% solid white.
   - Non-selected table rows paint all filled `■` cells in a single flat colour (`usage_color(meter_pct)`) instead
     of calling `graph::meter_spans` (used in the `mem` box for btop's smooth per-cell green → yellow → red gradient).
   - Both filled and empty meter cells use `'■'`, so in `tty` / `force_tty` mode, monochrome terminals, or the
     plain-text `README.md` capture, `0.0%` and `100%` look identical (`■■■■■■■■■■■■■■■■`).

5. **Status bar truncates `used X / Y` and renders `filter: <input>█` completely off-screen on 80–100 col terminals (`src/ui/draw.rs:74-113`).**
   - Before the filter indicator, `"739 procs  mem FOOTPRINT  agg rollup  sort agg↓  update 2000ms  used 14.5 GiB / 18.0 GiB"`
     already spans ~88 columns; `filter: <input>█` starts around column 90.
   - On a standard 80×24 terminal, pressing `/` or `f` and typing `chrome` shows **nothing on screen at all**
     because the prompt is clipped past the right border; at 68 cols, `used ...` clips mid-word (`... update 2000ms  us`).

6. **Column widths, headers, scrollbars, and overlay footers have visible polish defects (`src/ui/draw.rs`).**
   - **`OWN` and `<MEM> Σ` clip trailing `~` (`draw.rs:19, 646-648`)**: `OWN_COL_WIDTH` is `10` and `agg_len`
     floors at `10`, but `format_size` + `~` produces up to **11** chars (`"1023.4 KiB~"`, `"1019.6 MiB~"`),
     silently clipping the approximate tilde.
   - **No active-sort indicator on table headers (`draw.rs:260-289`)**: `COMM`, `PID`, `OWN`, `<MEM> Σ`, and `%`
     are click-sortable, yet none shows `↓`/`↑` or `hi_fg` styling to indicate the active sort column.
   - **Options description clips mid-word even on wide screens (`draw.rs:581-594`, `app.rs:170`)**:
     `OptionKey::UpdateMs.description()` is 79 chars (`"... adjusts by 100."`), while `options` clamps width to
     `78` (`inner.width = 76`), rendering `"... adjusts by "` on 1 row.
   - **Duplicate hint bars (`draw.rs:596-606, 659-662`)**: `Options` and `Help` each draw an internal hint line
     while the main frame's bottom hint bar repeats a slightly different hint line two rows below.
   - **Scrollbar track glyph clash (`draw.rs:568-577, 648-657`)**: `Scrollbar::new` defaults to double-line `║`
     directly beside the single-line `│` border (`║│`).
   - **Blank table when filter matches nothing (`draw.rs:291-342`)**: Zero matching rows renders an empty void
     with no `"no matching processes (press c to clear filter)"` message.
   - **Unicode display width (`query/mod.rs:265`, `draw.rs`, `help.rs`)**: Width calculations use `.chars().count()`
     instead of terminal column width (`unicode-width`), misaligning CJK/emoji process names; truncated `COMM`
     cells also cut abruptly without `…`.

---

### B. Collector accuracy, state persistence, UX & performance issues

7. **macOS unprivileged `RSS` and `VSZ` report `0 B` as exact (`approximate = false`) for ~216 system processes, and `MemSet::approximate` is dead code (`src/collect/macos.rs:106-151`, `src/collect/mod.rs:14-22`, `src/collect/linux.rs:80-83`, `src/model/tree.rs:487-492`).**
   - On macOS without root, `sysinfo` relies on `proc_pidinfo(PROC_PIDTASKINFO)`, which fails (`EPERM`) for
     other users' / root processes (216 of 737 live processes on macOS, including `launchd`, `logd`, `mds`,
     `wdavdaemon_unprivileged` at ~140 MiB RSS, `abrnotifier` at ~113 MiB RSS). `sysinfo` returns `0` for both
     `memory()` and `virtual_memory()`.
   - Because `MemSet::{rss, vsz}` are `u64` rather than `Option<u64>`, and **`MemSet::approximate` is never read
     anywhere in `model`, `query`, or `ui`** (`ProcTree::value_for` only checks `kind.get(&mem).is_none()`),
     `pspp` displays `launchd` and all 216 unreadable processes as exact `0 B` (no `~`) and omits `~` on
     `launchd`'s RSS/VSZ rollups and JSON `"approximate"`.
   - In addition, `macos.rs`:
     - iterates `system.processes()` instead of `read_kern_procs()`, dropping any `KERN_PROC_ALL` entry omitted
       by `sysinfo` (e.g. `kernel_task` pid 0);
     - calls `proc_pid_rusage` for every PID on every tick even when `kind` is `Rss` or `Vsz` (~740 wasted
       syscalls/tick);
     - constructs `System::new()` from scratch every tick instead of reusing a `System` instance on the worker
       thread (or reading `PROC_PIDTASKINFO` + `proc_name`/`proc_pidpath` directly via `libc`).

8. **Toggling `save_on_exit` to `false` in Options is never saved to `pspp.conf` (`src/main.rs:278-286`).**
   - `main` only calls `cfg.save()` inside `if updated_settings.save_on_exit`. If `pspp.conf` has
     `save_on_exit = true` (the default) and the user turns `save_on_exit` **off** in Options and exits,
     `cfg.save()` is skipped—leaving `save_on_exit = true` on disk permanently.

9. **Filter input is not live, despite `README.md` and btop conventions (`src/ui/app.rs:649-671`, `README.md:93`).**
   - `on_key_filter_input` edits `self.filter_input` without rebuilding rows until `Enter`. Updating the visible
     tree live on each keystroke—while snapshotting `(filter_committed, expanded, selected_pid)` on entry so
     `Esc` cleanly restores the prior view—matches both `README.md` ("live substring filtering") and `btop`.

10. **No collapse-all / expand-all shortcut (`src/ui/app.rs:619-647`).**
    - Because `expanded` defaults to all PIDs expanded, `launchd`'s first child (`Microsoft Edge` with 99 helper
      processes) fills the entire viewport on startup, pushing `Code`, `Google Chrome`, `MSTeams`, and `Slack`
      nearly 100 rows below the fold. Users currently must collapse heavy subtrees one by one.

11. **Event loop redraws on every mouse movement and processes only one event per draw (`src/ui/mod.rs:69-88`, `src/ui/app.rs:514`).**
    - `EnableMouseCapture` reports `MouseEventKind::Moved` on every cursor pixel/cell movement; `on_mouse` writes
      to dead field `last_mouse_position` and returns, triggering a full `terminal.draw(...)` per motion event.
    - Burst trackpad scroll or window resize events are handled one event per full render instead of draining
      ready events (`event::poll(Duration::ZERO)`) before drawing.

12. **Option & key-handling inconsistencies (`src/ui/app.rs`, `src/ui/clock.rs`, `src/main.rs`).**
    - **`force_tty` (`app.rs:117-124, 195`)**: `GraphSymbolSetting::resolve` only checks `force_tty` under
      `Self::Auto`, so `force_tty = true` has zero effect when `graph_symbol` is `"braille"` or `"block"`, and
      does not force square corners, ASCII meters, or 16-colour ANSI output; `NO_COLOR` is also ignored.
    - **`sort` direction (`main.rs:201-204`, `app.rs:771-774`)**: Clicking `PID`/`COMM` headers (`app.rs:804`)
      and query mode sort `pid`/`comm` ascending, but `pspp --sort comm` / `pspp --sort pid` in TUI and cycling
      `s` keep `sort_reverse = true` (`Z → A`, largest PID first), and CLI lacks `-r/--reverse`.
    - **Overlay navigation (`app.rs:673-745`)**: `Options` ignores `h`/`l` when `vim_keys = true`; `Help` lacks
      `Home`/`End`/`g`/`G`; `Menu` ignores mouse wheel scrolling.
    - **Clock timezone specifiers (`clock.rs:9-29`)**: `Local::now().naive_local()` strips timezone data, causing
      valid `strftime` specifiers `%+` (explicitly listed in `needs_second_resolution`!), `%Z`, and `%z` to fail
      formatting and render nothing.

13. **Query mode, model & documentation drift (`src/query/mod.rs`, `doc/pspp.1`, `README.md`, `.github/`).**
    - `--json` (`query/mod.rs:51-76, 286-315`) omits `own_approximate` on rows and `total_approximate` at top
      level, losing `~` information that plain mode prints.
    - `collect_subtree_pids` (`query/mod.rs:212-235`) rebuilds an ad-hoc `ppid` map instead of walking
      `tree.child_pids(pid)`, diverging from `ProcTree` on `pid == 0` and broken cycles.
    - `CHILDREN` column shows only direct `child_count` even in `rollup` mode, while `descendant_count` is never
      shown in TUI or plain query output.
    - `parse_size` (`query/mod.rs:342-349`) treats `KB`/`MB`/`GB` as `1024^n` regardless of `--base10`; passing
      both `PATTERN` and `-f/--filter` silently drops `-f`.
    - Stale docs: `doc/pspp.1:106-109` still claims macOS `ppid` fails via `proc_pidinfo` without root;
      `OptionKey::MemKind.description()` (`app.rs:172`) still claims `mem_kind` drives the memory graph;
      `help.rs:58` says `Options: Esc/q/Q` always returns to the menu (it returns to `Normal` when opened via
      `o`/`F2`); `progress.md:273` still lists deleted `ProcTree::sorted_pids`; `README.md` and `architecture.md`
      reference `.github/workflows/ci.yml` while the folder in git is `.github/_workflows/ci.yml`.

---

## Part 2 — Proposed execution plan (milestones M24–M29)

If executing these changes, follow `.github/copilot-instructions.md` and `.github/agents/memory/conventions.md`:
- Keep every gate green before each commit:
  `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check && cargo doc --no-deps && cargo check --target x86_64-unknown-linux-gnu --all-targets`
- One conventional commit per milestone; **never push**.
- Update `.github/agents/memory/{progress.md,decisions.md,architecture.md}` in the same commit, and re-sync
  generated doc blocks with `PSPP_UPDATE_DOCS=1 cargo test --lib` whenever `help.rs` or screen layout changes.

### M24 — `fix(ui): clear overlays, center menu button labels, polish scrollbars and footers`
Files: `src/ui/draw.rs`, `src/ui/app.rs`, `src/ui/help.rs`
- Render `ratatui::widgets::Clear` over `overlay` at the start of `render_menu_overlay`, `render_options_overlay`,
  and `render_help_overlay`, and apply `theme.main_bg` to each overlay `Block` when `Some`.
- In `render_menu_overlay`, render button labels into `button_block.inner(rect)` when `button_height >= 3`
  (and unbordered single-line buttons when `button_height < 3`).
- Give `Options` description 2 wrapped lines (or fit within width without mid-word clipping), eliminate the
  duplicate inner/outer hint bar when `Options` or `Help` is open, and set `.track_symbol(Some("│"))` on both
  `Scrollbar`s so `║│` never appears.
- Acceptance: `TestBackend` tests render `Menu`, `Options` (including `Nord` `main_bg`), and `Help` on top of a
  populated tree and assert zero underlying table characters bleed into `overlay` and `"Options"` sits on the
  interior row of its button rather than its top border.

### M25 — `fix(ui): fix ancestor tree connectors, meter styling, column widths, and responsive status/filter bar`
Files: `src/ui/app.rs`, `src/ui/draw.rs`, `src/ui/graph.rs`, `Cargo.toml`, `README.md`
- Fix `build_display_rows` to pass `&ancestor_last[1..visible_row.depth]` (for `depth > 1`, else `&[]`) to
  `build_tree_prefix`. Add unit + `TestBackend` tests covering a single-root 4-level tree (`launchd` → multiple
  children → grandchildren) and a multi-root tree whose non-last root has a `└─` child with grandchildren.
- Fix `meter_line` / `graph::meter_spans`:
  - Selected rows keep `selected_bg` across the meter cell range while preserving filled vs. empty distinction
    (never painting a `0%` row as 100% solid white).
  - Non-selected table rows use per-cell gradient `graph::meter_spans` (matching the `mem` box).
  - When `resolved_graph_symbols() == GraphSymbols::Tty` (or `NO_COLOR` is set), use distinct filled vs. empty
    glyphs (e.g. `#` and `-` or `█` and `·`) so meters remain readable without colour.
- Widen `OWN_COL_WIDTH` and minimum `agg_len` from `10` to `11` so `"1023.4 KiB~"` never clips `~`.
- Show sort direction (`↓`/`↑`) and `hi_fg` styling on the active sortable column header.
- Make the status bar width-aware: keep `filter: <input>█` visible at all widths (dropping lower-priority status
  segments like `update` / `sort` on narrow screens rather than clipping mid-word), fix the double space in
  `render_mem_box` when `meter_width == 0`, render a centered `"no matching processes (press c to clear)"` notice
  when `rows` is empty, and account for Unicode display width (`unicode-width`) with `…` truncation on `COMM`.

### M26 — `fix(collect): accurate macOS RSS/VSZ approximation, lazy rusage, and unified Option<u64> MemSet`
Files: `src/collect/mod.rs`, `src/collect/macos.rs`, `src/collect/linux.rs`, `src/collect/worker.rs`, `src/model/mod.rs`, `src/model/tree.rs`, `tests/model_tests.rs`
- Replace `MemSet::{rss: u64, vsz: u64, approximate: bool}` with `rss: Option<u64>`, `vsz: Option<u64>` and
  remove the dead `MemSet::approximate` field so every `MemKind` uniformly signals unreadability via `None` →
  `(0, true)` (`0 B~`).
- On macOS:
  - Build the process list from `read_kern_procs()` (`KERN_PROC_ALL`) joined with `libc::proc_pidinfo(pid, PROC_PIDTASKINFO, ...)`
    (or `sysinfo` verified against `PROC_PIDTASKINFO`), setting `rss`/`vsz` to `None` whenever `PROC_PIDTASKINFO`
    fails (`EPERM`) instead of `Some(0)`.
  - Call `proc_pid_rusage` only when `kind == MemKind::Footprint`.
  - Reuse collector state across worker requests rather than re-allocating `System::new()` on every tick.
- Acceptance: unprivileged `pspp --query --top 5` on macOS shows `0 B~` and `~` on `launchd` in `rss` and `vsz`
  modes just as in `footprint` mode; `cargo check --target x86_64-unknown-linux-gnu --all-targets` and all
  Linux parser tests stay green.

### M27 — `fix(config,cli): persist save_on_exit=false, align sort direction, enrich JSON approx fields`
Files: `src/main.rs`, `src/query/mod.rs`, `tests/query_cli.rs`
- When `initial.save_on_exit && !updated_settings.save_on_exit`, persist `cfg.save_on_exit = false` to `pspp.conf`
  (without writing other transient session changes) so users can actually turn `save_on_exit` off from Options.
- Add `-r, --reverse` CLI flag; when `--sort <key>` is passed without `--reverse` (or when `s` cycles to a new
  sort key in the TUI), use the natural default direction for that key (`agg`/`own` descending; `pid`/`comm`
  ascending), matching header-click and query mode. Mark `PATTERN` as `conflicts_with = "filter"` in clap.
- Add `own_approximate: bool` on JSON rows and `total_approximate: bool` on the top-level JSON object; simplify
  `collect_subtree_pids` to traverse `tree.child_pids(pid)` so `--pid` always matches `ProcTree` semantics.

### M28 — `feat(ui): live filter-as-you-type, collapse/expand-all keys, force_tty fidelity, and event-loop coalescing`
Files: `src/ui/mod.rs`, `src/ui/app.rs`, `src/ui/clock.rs`, `src/ui/theme.rs`, `src/ui/help.rs`
- Live filtering in `FilterInput`: snapshot `(filter_committed, expanded, selected_pid)` on `enter_filter_input()`,
  recompute matches + expand match ancestors on every edit (`Char`, `Backspace`, `Delete`, `Ctrl-U`, `Ctrl-W`),
  and restore the snapshot on `Esc`.
- Add `z` / `Z` (or `[` / `]`) in Normal mode to collapse all non-root subtrees (collapse to top-level children
  of roots / roots) and expand all nodes, so heavy browser/IDE helper trees can be folded in one keystroke.
- Make `force_tty = true` override `GraphSymbolSetting` regardless of `auto`/`braille`/`block`, force square
  borders, and honour `NO_COLOR`.
- In `ui::run_tui`: ignore no-op `MouseEventKind::Moved`/`Drag`/`Up` events without redrawing, remove dead
  `App::last_mouse_position`, and drain queued `Duration::ZERO` events before calling `terminal.draw(...)`.
- Support `h`/`l` in `Options` (when `vim_keys` is on and not editing text), `Home`/`End`/`g`/`G` in `Help`,
  mouse wheel in `Menu`, and format `DateTime<Local>` directly in `clock.rs` so `%+`, `%z`, `%Z` work.

### M29 — `docs(all): fix stale man page, option descriptions, CI path docs, and regenerate tables`
Files: `doc/pspp.1`, `README.md`, `src/ui/app.rs`, `src/ui/help.rs`, `.github/agents/memory/{architecture.md,progress.md,decisions.md}`
- Remove the obsolete `proc_pidinfo` root-orphan claim from `doc/pspp.1:106-109`.
- Fix `OptionKey::MemKind.description()` (`mem_kind` affects table and `Σ`, not the `used` history graph) and
  clarify `Options: Esc/q/Q` help text (`returns to previous view`).
- Keep `.github/_workflows/ci.yml` underscored (disabled on GitHub per `decisions.md`) and update `README.md`
  and `architecture.md` references to match; clean stale `sorted_pids` note from `progress.md`.
- Run `PSPP_UPDATE_DOCS=1 cargo test --lib` and `mandoc -Tlint doc/pspp.1`.

---

## Part 3 — Optional product enhancements / open questions

1. **Show descendant count alongside direct children (`CHILDREN` vs. `DESC` / `PROCS`)**:
   In `rollup` mode, `Code` (`2.1 GiB`) displays `CHILDREN = 11` even when 24 total descendant processes
   contribute to that `2.1 GiB` sum (`descendant_count` is already computed in `ProcTree` and emitted in
   `--json`, just hidden in TUI and plain tables). Should `rollup` display `DESC` (total descendants) while
   `flat` displays `CHILDREN` (direct children), or show `11/24`?
2. **Process detail / command-line inspect pane (`Enter` or `i`)**:
   `btop` opens a small detail sub-box showing full executable path / argv, UID/user, all platform memory
   counters side-by-side (`RSS`, `VSZ`, `Footprint` or `PSS`/`USS`/`Swap`), and PID/PPID chain for the selected
   process. Since `Enter` currently toggles expand/collapse (same as `Space`), `i` could toggle a 4-line detail
   footer/pane.
3. **`!regex` pattern support in filter & query mode**:
   CLI `--help`, `README.md`, and `doc/pspp.1` all carry the placeholder note *"!regex is not yet supported"*.
   Adding `regex-lite` (tiny, zero-unsafe crate) to support `!<pattern>` in both `ProcTree::filter_match_set`
   and `query` would complete btop filter parity at minimal binary-size cost.
4. **Decimal vs. binary suffixes in `--min`**:
   Should `--min 500MB` (or `--min 500M --base10`) use `1000^2` while `--min 500MiB` / `--min 500M` uses
   `1024^2`, or keep `--min` strictly binary (`1024^n`) and document that explicitly in `--help` and `doc/pspp.1`?
