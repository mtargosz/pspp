# pspp — Progress board (living document)

## Status key: [ ] pending · [~] in progress · [x] done · [!] blocked

## Plan (dependency order)

- [x] 1. scaffold — Cargo.toml, .gitignore, module skeleton
- [x] 2. agent-profiles — all 7 profiles in `.github/agents/`
- [x] 2b. memory-store — this directory, seeded
- [ ] 3. collector — cross-platform snapshot (RSS/VSZ; macOS footprint; Linux PSS/USS/swap)
- [ ] 4. tree-model — forest, rollup + flat aggregation, sorting, unit tests
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
