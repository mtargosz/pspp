# pspp — Conventions

## Code

- Rust 2021, `cargo fmt` default style; `cargo clippy` clean on changed code (no new warnings).
- Errors: `Result` + lightweight custom error enum; no `unwrap()` outside tests; parsing of
  `/proc`/syscall data must be panic-free on malformed input.
- Platform gates: `#[cfg(target_os = "macos")]` / `#[cfg(target_os = "linux")]`; shared logic platform-free.
- Memory values: u64 bytes internally; saturating arithmetic for sums; format only at render.
- TUI testing: ratatui `TestBackend` snapshots; pure logic unit-tested next to the module
  (`#[cfg(test)] mod tests`), cross-module tests in `tests/`.

## Commits

- Conventional commits: `feat:`, `fix:`, `test:`, `docs:`, `chore:`, `refactor:` + short imperative subject.
- One commit per milestone; memory file updates ride along with the milestone commit.
- **Never push** — commits stay local unless the user explicitly asks.
- Trailer on every commit:
  `Co-authored-by: Copilot App <223556219+Copilot@users.noreply.github.com>`

## Agents & memory

- Every agent reads `requirements.md` + `progress.md` + task-relevant memory before acting,
  and writes outcomes after acting (dated, attributed: `### YYYY-MM-DD — <agent>`).
- `decisions.md` and progress log entries are append-only; `architecture.md` and the progress
  board are edited in place.
- Only the orchestrator assigns tasks; no two agents touch overlapping files concurrently.
