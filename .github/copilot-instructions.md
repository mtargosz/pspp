# pspp Project Guidelines

`pspp` ("ps++") is an interactive `ratatui`/`crossterm` terminal UI and scriptable `clap` CLI (Rust 2021, MSRV `1.88`) that aggregates process memory (`rss`, `vsz`, macOS `footprint`, Linux `pss`/`uss`/`swap`) across the `pid ← ppid` hierarchy in both subtree `rollup` and direct-children `flat` modes on **macOS** and **Linux**.

## Persistent Memory & Active Work Specs

Agents coordinate across stateless sessions through versioned files in `.github/agents/memory/` and milestone specs in `prompts/`:

- `.github/agents/memory/requirements.md` — confirmed product scope, target platforms, and non-goals.
- `.github/agents/memory/architecture.md` — module map (`src/lib.rs`, `src/main.rs`, `collect`, `model`, `query`, `ui`, `config`, `docsync`), invariants, and internal APIs. Edit in place when interfaces change.
- `.github/agents/memory/decisions.md` — **append-only** dated log of architectural, CLI, and UX decisions. Never silently override a recorded decision; if a newer user prompt supersedes one, append a dated reversal.
- `.github/agents/memory/progress.md` — ordered milestone board (`[ ]` pending, `[~]` in progress, `[x]` done, `[!]` blocked) plus an **append-only** dated work log (`### YYYY-MM-DD — <agent>`).
- `.github/agents/memory/conventions.md` — coding, testing, commit, and multi-agent coordination rules.
- `.github/agents/memory/btop-reference.md` — visual fidelity notes for gradients, braille/block/tty graphs, meters, themes, and overlays.
- `prompts/pspp_review_2_prompt.md` — active review findings and milestone specification (**M24–M29**).

## Build, Test & Verification Gates

Every milestone must pass all applicable checks before committing:

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo doc --no-deps
cargo check --target x86_64-unknown-linux-gnu --all-targets
```

When key bindings, help text, man page content, or the synthetic README screen capture change, also run:

```sh
PSPP_UPDATE_DOCS=1 cargo test --lib
mandoc -Tlint doc/pspp.1
```

## Git & Workspace Rules

- Keep all writes strictly inside this repository.
- Use Conventional Commits (`feat:`, `fix:`, `test:`, `docs:`, `refactor:`, `chore:`), one commit per completed milestone on the current branch, bundling the corresponding `.github/agents/memory/` updates in the same commit.
- Include the commit trailer: `Co-authored-by: Copilot App <223556219+Copilot@users.noreply.github.com>`.
- **Never push** (`git push` is prohibited unless explicitly requested by the user).
- Leave `.github/_workflows/ci.yml` underscored unless the user explicitly asks to enable GitHub Actions.
