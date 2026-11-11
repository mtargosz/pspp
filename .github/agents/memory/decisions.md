# pspp — Decision log (append-only)

## 2026-11-11 — orchestrator (planning session, user-approved)

- **Language/stack: Rust** with `ratatui` + `crossterm` (TUI, SGR mouse, truecolor), `sysinfo`
  (portable process snapshot), platform natives (`libproc` on macOS via `libc`, `/proc` on Linux),
  `clap` (CLI), `serde`/`toml` (config). Chosen over Python/Textual (interpreter dependency, refresh
  CPU cost, packaging) and Go/bubbletea (less mature braille/gradient ecosystem). *User confirmed.*
- **Platforms: macOS + Linux.** *User confirmed.*
- **Query mode = non-interactive stdout mode** (`pspp <query>`), not an in-TUI query language. *User confirmed.*
- **Aggregation: both** subtree rollup and flat direct-children grouping, runtime-toggleable. *User confirmed.*
- **Refresh: live auto-refresh**, configurable `update_ms`-style interval. *User confirmed.*
- **Memory types per platform maximized** (RSS/VSZ everywhere; footprint on macOS; PSS/USS/swap on Linux). *User delegated to agents.*
- **Multi-agent development** via reusable profiles in `.github/agents/` driven by an orchestrator;
  **persistent memory on disk** in `.github/agents/memory/` (this directory), committed to git. *User directed.*
- **Workspace**: `/Users/mtargosz/dev/projects/pspp` (branch `main`); commit per milestone; never push. *User directed.*
- Usage gradient colors adopted from btop default theme: `#77ca9b → #cbc06c → #dc4c4c`.
- Graceful degradation rule: when privileged per-process data is unreadable, fall back to RSS and
  mark approximate; never crash or omit the process.

## 2026-11-11 — orchestrator (model assignment, user-directed: "don't use Fable for everything")

Per-agent models set in profile frontmatter:

| Agent | Model | Rationale |
|---|---|---|
| orchestrator | claude-sonnet-5.5 | frequent coordination turns; balanced cost/judgment |
| planner | gpt-6.1-sol | strong structured reasoning for task breakdown |
| coder | gpt-5.3-codex | coding-specialized model |
| tester | claude-haiku-5.5 | fast/cheap for tight test-run loops |
| reviewer | claude-opus-5.5 | deepest scrutiny; different family from coder to catch family-blind-spot bugs |
| documentator | gemini-4-carbon | strong prose at low cost |
| ui-stylist | claude-sonnet-5.5 | detail-oriented visual/code fidelity work |
