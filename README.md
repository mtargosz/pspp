# pspp (`ps++`)

`pspp` ("ps plus plus") is an interactive terminal UI and scriptable CLI that sums process memory **grouped by parent process** — following the self-referencing `pid ← ppid` hierarchy from `ps ax -o pid,ppid,rss,comm` so every parent shows the true weight of the processes underneath it.

## Why `pspp`?

Standard tools like `ps` and `top` list processes flatly, making heavy multi-process workloads look deceptively small. Modern browsers, Electron/WebView apps, IDE language-server trees, build orchestrators, and worker supervisors routinely split work across dozens of child processes whose individual RSS looks harmless while their combined subtree quietly consumes gigabytes. `pspp` reconstructs the live `pid → ppid` forest and computes both full-subtree **rollup** totals and direct-children **flat** totals—either interactively in a [btop](https://github.com/aristocratos/btop)-inspired TUI with braille history graphs and usage gradients, or non-interactively from scripts and pipes.

## Install & Build

Requires Rust 1.88 or newer (`rustc` / `cargo`, 2021 edition) on **macOS** or **Linux**.

```sh
cargo build --release
# Binary is written to ./target/release/pspp

# Or install into ~/.cargo/bin:
cargo install --path .
```

### Platform notes

- **macOS & Linux** are supported out of the box (`RSS` and `VSZ` on both; `Footprint` on macOS; `PSS`, `USS`, and `Swap` on Linux).
- **Unprivileged hierarchy & `sudo` precision**: on macOS, `sysctl(KERN_PROC_ALL)` preserves the full `pid ← ppid` parent/child hierarchy without root, while unreadable per-process memory counters (`rss`, `vsz`, `footprint` on root or other users' processes) report `0 B~` and mark ancestor totals approximate (`~`); running `sudo pspp` unlocks exact figures for every process. On Linux, `rss` and `vsz` come from `/proc/<pid>/stat`, while privileged `/proc/<pid>/smaps_rollup` metrics (`pss`, `uss`, `swap`) likewise degrade to `0 B~` without root.

---

## Interactive TUI

Run `pspp` on an interactive terminal with no query flags (`PATTERN`, `--pid`, `--min`, `--top`, `--json`, `--query`) to launch the live TUI:

```sh
pspp
pspp --update-ms 1000           # 1-second refresh interval
pspp --mem footprint --agg flat # start with footprint memory, flat aggregation
pspp --sort comm                # sort siblings A → Z (-r/--reverse inverts to Z → A)
pspp -f chrome                  # pre-set the process filter (like btop -f)
```

`--mem`, `--agg`, `--sort`, `-r`/`--reverse`, `--base10`, and `--update-ms` apply to the TUI too, for that run only: CLI overrides are never written to `pspp.conf`. With `save_on_exit = true`, only settings you change inside the TUI are saved (and turning `save_on_exit` off in Options persists `save_on_exit = false` without writing other transient session changes).

<!-- BEGIN GENERATED: screen -->
A plain-text capture of the TUI, rendered by the test suite from a synthetic process tree (100×24; the green→red usage gradient and the selected-row highlight are colours and do not show up in text). `launchd` is the selected row.

```text
╭ pspp ────────────────────────────────────────────────────────────────────────────────────────────╮
│7 procs  mem FOOTPRINT  agg rollup  sort agg↓  update 2000ms  used 2.0 GiB / 4.0 GiB              │
│╭ mem ───────────────────────────────────────────────────────────────────────────────────────────╮│
││                                                                                            ⢰⡇ ⣤││
││                                                                                          ⡄⢠⣿⣷⡀⣿││
││                                                                                          ⣇⣿⣿⣿⣧⣿││
││used 2.0 GiB / 4.0 GiB ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■ 50%  Σ FOOTPRINT 1.2 GiB││
│╰────────────────────────────────────────────────────────────────────────────────────────────────╯│
│COMM                                PID         OWN FOOTPRINT Σ~↓      % CHILDREN METER           │
│[-] launchd                           1   100.0 MiB      1.1 GiB~  28.1%        2 ■■■■■■■■■■■■■■■■│
│├─ [-] shell                          2   200.0 MiB    550.0 MiB~  13.4%        2 ■■■■■■■■■■■■■■■■│
││  ├─ [-] editor                      3   300.0 MiB    300.0 MiB~   7.3%        1 ■■■■■■■■■■■■■■■■│
││  │  └─ plugin                       4        0 B~          0 B~   0.0%        0 ■■■■■■■■■■■■■■■■│
││  └─ build                           5    50.0 MiB      50.0 MiB   1.2%        0 ■■■■■■■■■■■■■■■■│
│└─ daemon                             6   500.0 MiB     500.0 MiB  12.2%        0 ■■■■■■■■■■■■■■■■│
│stray                                 7    60.0 MiB      60.0 MiB   1.5%        0 ■■■■■■■■■■■■■■■■│
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│                                                                                                  │
│↑/↓ j/k PgUp/PgDn  ←/→  Space/Enter  z/Z fold  e agg  m mem  s sort  S/r rev  f// filter          │
╰──────────────────────────────────────────────────────────────────────────────────────────────────╯
```

To capture your own: run `pspp` in a terminal about 120 columns wide and use the terminal's screenshot tool (macOS: `Cmd-Shift-4`, then `Space` and click the window).
<!-- END GENERATED: screen -->

The TUI renders a rounded btop-style frame with:

- **Status bar** showing process count, active memory metric, aggregation mode, sort order, refresh interval, `used X / Y` system memory (gradient-coloured by used fraction), and active filter indicator.
- **Live memory history graph** (`mem` box, automatically hidden on short terminals) of real system memory in use (`used / total`; summary line `used 12.1 GiB / 32.0 GiB ■■■■ 38%  Σ RSS 18.4 GiB`), using UTF-8 braille (`braille`), block (`block`), or ASCII (`tty`) glyphs and a 101-step grass-green → yellow → bloody-red usage gradient (`#77ca9b → #cbc06c → #dc4c4c`), with automatic 256-color fallback when `COLORTERM` is not `truecolor`/`24bit`.
- **Context-sensitive hint bar** that follows the active mode (table, filter input, menu, options, help) and truncates to the terminal width.
- **Expandable process tree table** with per-row positional gradient `■` usage meters (or ASCII `#`/`-` meters in `tty`/`force_tty`/`NO_COLOR` mode), `OWN`, aggregated (`<MEM> Σ`) and `%` (Σ ÷ total RAM, can exceed 100% because shared pages are counted per process) columns, child counts, and live-as-you-type substring filtering that preserves ancestor context.

### Key Bindings

Every operation is accessible from the keyboard (matching the in-app **Help** overlay via `Esc` → `Help`):

<!-- BEGIN GENERATED: key-tables -->
#### Navigation

| Key | Action |
|---|---|
| `↑/↓, j/k` | Move selection one row (j/k need vim_keys). |
| `PgUp/PgDn` | Move by one page. |
| `Home/End, g/G` | Jump to first/last row (g/G need vim_keys). |
| `h, ?, F1` | Open Help (h always opens Help, even with vim_keys off). |
| `o, F2` | Open Options. |
| `+ / -` | Increase / decrease the refresh interval by 100 ms. |
| `m vs btop` | m cycles the memory type here; btop uses m for its menu (pspp's menu is Esc). |
| `q/Q, Ctrl-C, Ctrl-Q` | Quit pspp (Ctrl-C/Ctrl-Q from any mode). |
| `f/F or /` | Start filter input. |
| `c/C` | Clear the committed filter. |
| `Esc` | Open main menu. |

#### Tree

| Key | Action |
|---|---|
| `Space or Enter` | Toggle expand/collapse on selected node (also while filtering). |
| `←` | Collapse selected node, or select parent. |
| `→, l` | Expand selected node (l needs vim_keys). |
| `z / [` | Collapse all non-root subtrees (again: roots too). |
| `Z / ]` | Expand all nodes. |

#### Display & Filter

| Key | Action |
|---|---|
| `e/E` | Toggle aggregation mode: rollup/flat. |
| `m/M` | Cycle memory type. |
| `s` | Cycle sort key. |
| `S or r/R` | Reverse sort direction. |
| `Filter input` | Type any characters (including spaces) to filter live and expand match ancestors. |
| `Backspace` | Delete one character in filter input. |
| `Delete / Ctrl-U` | Clear the filter input. |
| `Ctrl-W` | Delete previous word in filter input. |
| `Enter` | Keep the filter and return to the table. |
| `Esc` | Cancel filter edit and restore the prior filter and tree. |

#### Menu / Overlays

| Key | Action |
|---|---|
| `Menu: ↑/↓, j/k, Tab` | Select Options / Help / Quit. |
| `Menu: Enter/Space` | Activate selected menu button. |
| `Menu: Esc/q/Q` | Close menu. |
| `Options: ↑/↓, j/k` | Select option row. |
| `Options: ← / →, h / l` | Change/toggle selected option (h/l need vim_keys). |
| `Options: Enter` | Edit update_ms/clock_format or advance value. |
| `Options: Esc/q/Q` | Close options and return to previous view. |
| `update_ms / clock_format editor` | Type to edit; Backspace deletes; Enter commits; Esc cancels. |
| `Help: ↑/↓, j/k, PgUp/PgDn` | Scroll help text (j/k need vim_keys). |
| `Help: Home / End, g / G` | Jump to top/bottom of help (g/G need vim_keys). Esc/q/Q closes. |

### Mouse Support

| Mouse Action | Effect |
|---|---|
| `Wheel up/down` | Scroll process rows. |
| `Left click row` | Select row. |
| `Left click [+]/[-]` | Expand/collapse node. |
| `Double click row` | Expand/collapse node. |
| `Click column header` | Sort by that column; click the active column again to reverse. |
| `Menu: click / wheel` | Click button to activate; wheel cycles menu buttons; click outside to close menu. |
| `Options: click / wheel` | Click row to select, click the selected row to edit/advance; wheel moves; click outside closes. |
| `Help: wheel / click` | Wheel scrolls; click outside closes. |
<!-- END GENERATED: key-tables -->

### Options & Configuration (`pspp.conf`)

`pspp` loads settings at startup and—when `save_on_exit = true` (the default)—atomically persists any changes made in `Options` (`o`/`F2` or `Esc` → `Options`) when exiting the TUI. Turning `save_on_exit` off in `Options` saves `save_on_exit = false` to `pspp.conf` without writing other transient session edits.

Configuration path resolution:

1. `$PSPP_CONFIG_DIR/pspp.conf` when `PSPP_CONFIG_DIR` is set and non-empty.
2. Platform config directory via `dirs::config_dir()`:
   - **Linux**: `~/.config/pspp/pspp.conf` (or `$XDG_CONFIG_HOME/pspp/pspp.conf`)
   - **macOS**: `~/Library/Application Support/pspp/pspp.conf`

Missing files, unknown keys, or invalid values fall back cleanly to built-in defaults:

```toml
# pspp configuration
update_ms = 2000          # Refresh interval in ms (clamped to 100..=86400000)
mem_kind = "rss"          # rss | vsz | footprint (macOS) | pss | uss | swap (Linux)
agg_mode = "rollup"       # rollup | flat
graph_symbol = "auto"     # auto | braille | block | tty
base10 = false            # false = KiB/MiB/GiB (1024), true = KB/MB/GB (1000)
save_on_exit = true       # Save TUI changes on exit (turning off writes save_on_exit = false)
sort_key = "agg"          # agg | own | pid | comm
sort_reverse = true       # true = descending, false = ascending
color_theme = "Default"   # Default | name of a btop .theme file in <config dir>/themes
theme_background = true   # paint the theme's background colour (false = terminal background)
truecolor = "auto"        # auto (COLORTERM) | on | off (256-colour palette)
rounded_corners = true    # ╭╮╰╯ corners; false = ┌┐└┘
vim_keys = true           # j/k/l/g/G (plus h/l in Options); false makes them inert (h opens Help in table)
disable_mouse = false     # true = do not capture the mouse (terminal selection works)
clock_format = ""         # strftime text in the top border, e.g. "%H:%M:%S"; empty = off
background_update = true  # false pauses refresh while Menu/Options/Help is open
force_tty = false         # true forces 16-colour TTY mode (#/- meters, tty graphs, square borders;
                          # overrides graph_symbol, rounded_corners, truecolor, and custom themes)
```

All of these are editable live in **Options** (`o`/`F2`). `color_theme` cycles through the built-in `Default` theme and every `*.theme` file found in `<config dir>/themes` (`$PSPP_CONFIG_DIR/themes` when set, otherwise next to `pspp.conf`). Theme files use btop's format (`theme[main_fg]="#cc"`; `#RRGGBB`, `#RR` greys and `R G B` decimals). pspp reads `main_bg`, `main_fg`, `title`, `hi_fg`, `selected_bg`, `selected_fg`, `inactive_fg`, `meter_bg`, `div_line` and the usage gradient `cpu_start`/`cpu_mid`/`cpu_end`; every other key is ignored and anything missing keeps the Default colour.

Low-colour and monochrome terminal support:

- **`force_tty = true`** forces 16-colour ANSI TTY mode: `#`/`-` ASCII meters, `tty` graph glyphs, and square box borders (`┌┐└┘`), overriding `graph_symbol`, `rounded_corners`, `truecolor`, and `.theme` files (while `graph_symbol = "auto"` also picks `tty` glyphs on `TERM=linux`/`dumb` or a `TERM` without `"color"` when `COLORTERM` is unset).
- **`NO_COLOR`** (non-empty environment variable) disables all ANSI colours in the TUI, selects `tty` graph glyphs, `#`/`-` ASCII meters, square borders, and reverse-video selection highlights without modifying `pspp.conf`.

---

## Query Mode (Non-Interactive CLI)

`pspp` runs in one-shot **query mode** when `PATTERN`, `--pid`, `--min`, `--top`, `--json` or `--query` is supplied (`--query` forces one-shot output on a terminal) **or** whenever `stdout` is piped/redirected to a non-TTY. Query mode is deterministic: it **never reads `pspp.conf`**. Defaults are `--mem rss --agg rollup --sort agg` with binary units; the plain header and the JSON output both name the mode in effect (`RSS Σ (rollup)`, `"agg": "rollup"`).

### CLI Options

| Argument / Flag | Description |
|---|---|
| `[PATTERN]` | Case-insensitive substring match on `COMM` (conflicts with `-f`/`--filter`, exit `2`; `!regex` prefix is not yet supported). |
| `--pid <PID>` | Restrict evaluation to `<PID>` and its descendant subtree. |
| `--mem <KIND>` | Memory metric: `rss`, `vsz`, `footprint` (macOS), `pss`, `uss`, `swap` (Linux). |
| `--agg <MODE>` | Aggregation mode: `rollup` (self + all descendants) or `flat` (direct children sum). |
| `--min <SIZE>` | Keep only rows whose aggregated memory is `>= SIZE` (e.g. `512K`, `100M`, `1.5G`, or bytes). |
| `--sort <FIELD>` | Sort rows by `agg` (default desc), `own` (default desc), `pid` (default asc), or `comm` (default asc); applies to TUI and query mode. |
| `-r, --reverse` | Invert the sort direction in both TUI and query mode (reverses `--sort`'s natural order, or flips `sort_reverse` from `pspp.conf` in the TUI). |
| `--top <N>` | Emit at most the first `N` rows after sorting (`N >= 1`). |
| `--json` | Emit pretty JSON (`{ "mem", "agg", "rows", "total", "total_approximate" }`) with raw byte counts. |
| `--query` | Force one-shot query mode even when `stdout` is a terminal. |
| `-f, --filter <TEXT>` | Pre-set the TUI process filter (like `btop -f`); used as `PATTERN` in query mode when none is given (conflicts with `PATTERN`, exit `2`). |
| `--base10` | Format human sizes in decimal (`KB`/`MB`/`GB`) instead of binary (`KiB`/`MiB`/`GiB`). |
| `--update-ms <MS>` | TUI refresh interval in milliseconds (`100` to `86400000`). |

> **How `TOTAL` is calculated:** `TOTAL` (and JSON `"total"`) counts every process covered by the returned rows **once**. Because `rollup` rows nest (`launchd`'s `Σ` already includes `Microsoft Edge`'s `Σ`), rollup `TOTAL` sums only returned rows that have no returned ancestor; `flat` rows sum direct children only and are already disjoint.

### Examples

#### 1. Top memory subtrees (default `rollup` + `rss`)

```console
$ pspp --top 5
PID    PARENT      COMM                  OWN  RSS Σ (rollup)      %  CHILDREN
-----------------------------------------------------------------------------
1      -           launchd              0 B~       11.7 GiB~  65.1%       501
8017   1(launchd)  Microsoft Edge  273.2 MiB         3.9 GiB  21.5%        98
38372  1(launchd)  Google Chrome   178.6 MiB         1.4 GiB   7.9%        28
52183  1(launchd)  claude           93.4 MiB       911.9 MiB   4.9%         4
46189  1(launchd)  Claude          174.1 MiB       463.9 MiB   2.5%        12
                   TOTAL                           11.7 GiB~  65.1%
```

#### 2. Filter by process name (`PATTERN`)

```console
$ pspp zsh
PID    PARENT             COMM         OWN  RSS Σ (rollup)     %  CHILDREN
--------------------------------------------------------------------------
60888  50784(herdr-core)  zsh      1.8 MiB       353.6 MiB  1.9%         1
9602   9601(login)        zsh      1.2 MiB       192.9 MiB  1.0%         1
84378  61037(claude)      zsh      2.9 MiB        25.7 MiB  0.1%         1
97242  97241(login)       zsh    656.0 KiB        13.8 MiB  0.1%         1
55005  50784(herdr-core)  zsh      1.9 MiB         1.9 MiB  0.0%         0
49660  50784(herdr-core)  zsh      1.4 MiB         1.4 MiB  0.0%         0
40164  40163(login)       zsh      1.3 MiB         1.3 MiB  0.0%         0
18361  23042(github)      zsh      1.2 MiB         1.2 MiB  0.0%         0
66235  58240(herdr)       zsh    656.0 KiB       656.0 KiB  0.0%         0
                          TOTAL                  566.9 MiB  3.1%
```

#### 3. Combine `--mem` and `--agg` (direct-children `flat` sum)

```console
$ pspp --mem footprint --agg flat --top 4
PID    PARENT          COMM                           OWN  Footprint Σ (flat)       %  CHILDREN
-----------------------------------------------------------------------------------------------
8017   1(launchd)      Microsoft Edge           824.5 MiB            24.7 GiB  137.0%        98
1      -               launchd                       0 B~            9.0 GiB~   49.9%       501
38372  1(launchd)      Google Chrome            368.3 MiB             2.1 GiB   11.9%        28
88752  88701(MSTeams)  Microsoft Teams WebView  134.2 MiB             1.9 GiB   10.8%         7
                       TOTAL                                        37.7 GiB~  209.6%
```

*(On Linux, try `pspp --mem pss --agg rollup --top 5` for proportional shared-memory rollups.)*

#### 4. Threshold filtering (`--min`) and sorting (`--sort`)

```console
$ pspp --min 500M --sort own --top 3
PID    PARENT      COMM                  OWN  RSS Σ (rollup)      %  CHILDREN
-----------------------------------------------------------------------------
8017   1(launchd)  Microsoft Edge  273.2 MiB         3.9 GiB  21.5%        98
38372  1(launchd)  Google Chrome   178.6 MiB         1.4 GiB   7.9%        28
52183  1(launchd)  claude           93.4 MiB       911.9 MiB   4.9%         4
                   TOTAL                             6.2 GiB  34.3%
```

#### 5. JSON output (`--json`) piped to `jq`

```console
$ pspp --json --min 1G | jq '.rows[] | {pid, comm, agg_gib: (.agg / 1073741824 * 10 | round / 10)}'
{
  "pid": 1,
  "comm": "launchd",
  "agg_gib": 11.2
}
{
  "pid": 8017,
  "comm": "Microsoft Edge",
  "agg_gib": 4
}
```

Each JSON object in `rows` contains `{ pid, ppid, comm, own, own_approximate, agg, pct, children, descendants, approximate }` with raw byte integers (`pct` = `agg` ÷ total RAM as a percentage with one decimal; `own_approximate` and `approximate` correspond to trailing `~` on `OWN` and `Σ`) alongside top-level `mem`, `agg` (the aggregation mode: `rollup` or `flat`), `total`, and `total_approximate`.

#### 6. Grep-style exit codes in shell scripts

Query mode uses `grep`/`pgrep`-compatible exit statuses:

- `0` — at least one matching row was printed (piping into `head` / `EPIPE` also exits `0`).
- `1` — no processes matched (plain mode writes `no matches` to `stderr`; `--json` still prints `{"mem":"rss","agg":"rollup","rows":[],"total":0,"total_approximate":false}`).
- `2` — CLI usage error (e.g. passing both `PATTERN` and `-f`/`--filter`, `--top 0`, invalid `--min` suffix, unavailable `--mem` kind) or runtime I/O error.

```sh
if pspp --pid "$WORKER_PID" --min 2G >/dev/null 2>&1; then
    echo "worker subtree exceeded 2 GiB"
fi
```

---

## Memory Types Explained

`pspp` exposes every per-process memory counter supported by the host OS. Press `m` in the TUI or pass `--mem <KIND>` on the CLI:

| Kind | Platform | Plain-English Meaning |
|---|---|---|
| **RSS** (`rss`) | macOS, Linux | RAM the process is using right now, including its share of memory also used by others. Good quick signal, though shared libraries/pages are counted in every process that maps them. |
| **VSZ** (`vsz`) | macOS, Linux | All address space the process has reserved — mostly not real RAM; usually scary-looking and mostly harmless. Treat it as a reservation figure rather than actual RAM pressure. |
| **Footprint** (`footprint`) | macOS only | Apple's own "how heavy is this process" number (`ri_phys_footprint` via `proc_pid_rusage`), matching Activity Monitor's **Memory** column. |
| **PSS** (`pss`) | Linux only | RAM used, with shared memory split fairly between the processes sharing it (`/proc/<pid>/smaps_rollup`). Best answer to *"who is really using my RAM?"* |
| **USS** (`uss`) | Linux only | RAM only this process uses (`Private_Clean + Private_Dirty`) — roughly what you would get back immediately if you killed it. |
| **Swap** (`swap`) | Linux only | Memory moved out of RAM onto disk because RAM was full (`/proc/<pid>/smaps_rollup`). |

### Approximate marker (`~`)

When a selected metric cannot be read for a process (for example, `proc_pidinfo(PROC_PIDTASKINFO)` for `rss`/`vsz` or `proc_pid_rusage` for `footprint` on macOS, or `/proc/<pid>/smaps_rollup` on Linux for another user's or system process without `sudo`), `pspp` keeps the process in its `pid ← ppid` hierarchy, shows its own value as `0 B~`, and marks any parent rollup/flat sum that includes it with a trailing tilde (such as `1.2 GiB~`, or `"own_approximate": true` / `"approximate": true` / `"total_approximate": true` in `--json`). Running `sudo pspp` unlocks exact figures across all processes.

---

## Development

`pspp` uses a **multi-agent workflow** with generic role agents in `.github/agents/*.agent.md`, workspace instructions in `.github/copilot-instructions.md` and `.github/instructions/*.instructions.md`, and reusable task prompts in `.github/prompts/` (such as `/implement-review` to run milestone specs in `prompts/`). An `orchestrator` agent coordinates specialized role agents across sequential milestones while ensuring no two write-capable agents edit overlapping files concurrently:

| Agent Profile | Model | Role |
|---|---|---|
| `orchestrator` | `Gemini 4 Carbon` | Reads project memory & review specs, selects ready milestones, delegates to specialists, verifies gates, commits. |
| `planner` | `Gemini 4 Carbon` | Read-only decomposition of milestones into ordered tasks with concrete interfaces and testable acceptance criteria. |
| `coder` | `Gemini 4 Carbon` | Implements scoped Rust modules and unit tests with zero clippy warnings. |
| `tester` | `Gemini 4 Carbon` | Expands unit, fixture, CLI integration, and `TestBackend` TUI tests. |
| `reviewer` | `Gemini 4 Carbon` | Read-only audit of correctness, unsafe/FFI boundaries, render-loop cost, and terminal-state hygiene. |
| `documentator` | `Gemini 4 Carbon` | Maintains `README.md`, CLI `--help`, in-app Help explanations, and `doc/pspp.1`. |
| `ui-stylist` | `Gemini 4 Carbon` | Implements btop visual fidelity: 101-step gradients, braille graphs, meters, and overlays. |

### Persistent Agent Memory (`.github/agents/memory/`)

Agents stay stateless across invocations and share durable context exclusively through versioned Markdown files in `.github/agents/memory/`:

- `requirements.md` — confirmed product scope, target platforms, and non-goals.
- `architecture.md` — living module map, data-flow invariants, and internal APIs.
- `decisions.md` — append-only log of architectural, UX, and CLI behavior decisions.
- `progress.md` — milestone checklist, chronological agent work log, and known issues.
- `conventions.md` — Rust coding, testing, commit, and memory hygiene rules.
- `btop-reference.md` — visual reference notes for gradients, braille tables, and menu chrome.

### Checks & Tests

```sh
cargo test                                # unit, fixture-driven model, TestBackend screen, and CLI integration tests
cargo clippy --all-targets -- -D warnings # lint gate
cargo fmt --check                         # formatting check
cargo doc --no-deps                       # verify rustdoc builds cleanly
cargo check --target x86_64-unknown-linux-gnu --all-targets # cross-target Linux check
mandoc -Tlint doc/pspp.1                  # lint the roff manual page
man ./doc/pspp.1                          # preview the manual page
PSPP_UPDATE_DOCS=1 cargo test --lib       # regenerate the README key tables/screen capture and the man page key bindings
```

The key-binding tables in this README and in `doc/pspp.1` (and the screen capture above) are generated from `src/ui/help.rs` and the renderer; `cargo test` fails if they drift, and the command above rewrites them. The minimum supported Rust version is 1.88 (`rust-version` in `Cargo.toml`). The GitHub Actions workflow template lives at `.github/_workflows/ci.yml` (`macos-latest` and `ubuntu-latest`) and is kept underscored so Actions remain disabled on GitHub unless moved to `.github/workflows/ci.yml`.
