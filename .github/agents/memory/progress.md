# pspp — Progress board (living document)

## Status key: [ ] pending · [~] in progress · [x] done · [!] blocked

## Plan (dependency order)

- [x] 1. scaffold — Cargo.toml, .gitignore, module skeleton
- [x] 2. agent-profiles — all 7 profiles in `.github/agents/`
- [x] 2b. memory-store — this directory, seeded
- [x] 3. collector — cross-platform snapshot (RSS/VSZ; macOS footprint; Linux PSS/USS/swap)
- [x] 4. tree-model — forest, rollup + flat aggregation, sorting, unit tests
- [ ] 5. query-mode — pspp <query> stdout mode (filters, --json)
- [ ] 6. tui-skeleton — ratatui shell: alt screen, mouse, tick loop, resize, quit
- [ ] 7. theme-gradients — theme struct, 101-step gradients, 256-color fallback
- [ ] 8. tree-table-ui — expand/collapse, kb+mouse nav, gradient meters, aggregation toggle
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
