---
name: implement-review
description: "Execute pending review milestones (defaulting to prompts/pspp_review_2_prompt.md) through the multi-agent orchestrator workflow."
disable-model-invocation: true
argument-hint: "[optional: prompt path or milestone IDs, e.g. M24 or M24-M29]"
---
Coordinate and execute the pending follow-up work using the workspace instructions and persistent memory workflow:

1. Read [prompts/pspp_review_2_prompt.md](../../../prompts/pspp_review_2_prompt.md) (unless a different spec/prompt path is specified in the user input) together with `.github/agents/memory/progress.md`, `.github/agents/memory/decisions.md`, `.github/agents/memory/architecture.md`, and `.github/agents/memory/requirements.md`.
2. Ensure all requested milestones are tracked in `#tool:todo` and `.github/agents/memory/progress.md`. If any open product question blocks a milestone you are about to execute, ask the user first and record the answer in `decisions.md`.
3. Work milestone by milestone in dependency order (or the subset specified by the user):
   - Delegate bounded implementation, UI styling, testing, documentation, and read-only review tasks to the specialist subagents (`planner`, `coder`, `ui-stylist`, `tester`, `documentator`, `reviewer`), ensuring no two write-capable subagents touch overlapping files concurrently.
   - Run all workspace verification gates (`cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo doc --no-deps`, `cargo check --target x86_64-unknown-linux-gnu --all-targets`, plus `PSPP_UPDATE_DOCS=1 cargo test --lib` and `mandoc -Tlint doc/pspp.1` when docs/UI change).
   - Update `.github/agents/memory/progress.md`, `decisions.md` (when applicable), and `architecture.md` (when APIs/layout change), and create one local Conventional Commit per completed milestone (**never push**).
