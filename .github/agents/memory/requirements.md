# pspp — Requirements

Source: user's `pspp_initial_prompt.md` + clarifications (2026-11-11 planning session).

## Core

`pspp` ("ps plus plus" / ps++) sums memory used by processes **grouped by their parent process** —
the `pid ← ppid` self-referencing relationship from `ps ax -o pid,ppid,rss,comm`.

## Confirmed requirements

1. **Interactive TUI** with mouse support; keyboard remains the primary way to do everything.
2. **Non-interactive query mode** *(clarified)*: `pspp <query>` prints results to stdout and exits, for scripts/pipes.
3. **Aggregation — both modes, toggleable at runtime** *(clarified)*:
   - Tree rollup: each parent shows its entire descendant subtree's sum; expandable/collapsible tree.
   - Flat grouping: sum of direct children per parent.
4. **Live auto-refresh** with configurable interval, btop `update_ms` style *(clarified)*.
5. **Colors**: usage gradient from grass green (low/good) to bloody red (high/bad).
6. **btop look & feel**: gentle shading, UTF-8 braille chars for graphs/bars, Esc overlay main menu
   with Options / Help / Quit, Options modeled on btop Main Menu → Options → General.
7. **Memory types — maximize per platform** *(user delegated choice)*:
   - Both OSes: RSS, VSZ
   - macOS: physical footprint (libproc `proc_pid_rusage` → `ri_phys_footprint`)
   - Linux: PSS, USS, swap (`/proc/<pid>/smaps_rollup`)
   - Help explains each type in simple English (hard requirement).
8. **Platforms** *(clarified)*: macOS + Linux.

## Workflow directives (user)

- Workspace is `/Users/mtargosz/dev/projects/pspp`; never write outside it.
- Commit every milestone; **never push**.

## Out of scope (unless requested later)

- Kill/signal/renice functionality
- BSD/Windows support
- CPU/network/disk monitoring (memory only)
