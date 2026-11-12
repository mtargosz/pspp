---
name: "pspp Documentation & Man Page Sync"
description: "Use when editing README.md, doc/pspp.1, CLI --help text, in-app Help content, or generated doc blocks."
applyTo: "README.md, doc/**, src/ui/help.rs, src/docsync.rs, src/main.rs"
---

# Documentation & Man Page Guidelines

- **Synchronized generated blocks**:
  - `README.md` (`<!-- BEGIN GENERATED: screen -->` and `<!-- BEGIN GENERATED: key-tables -->`) and `doc/pspp.1` (`.\" BEGIN GENERATED: key-bindings`) are generated from `src/ui/draw.rs` and `src/ui/help.rs` and checked by `src/docsync.rs`.
  - After changing key bindings, mouse actions, help tables, or the main TUI layout, regenerate those blocks with:
    ```sh
    PSPP_UPDATE_DOCS=1 cargo test --lib
    ```
  - Never leave hand-edited drift inside generated marker blocks.
- **Plain-English memory explanations**:
  - Keep `memory_help_text` (`src/ui/help.rs`), `README.md`, and `doc/pspp.1` written in clear, plain English for every `MemKind` (`rss`, `vsz`, macOS `footprint`, Linux `pss`, `uss`, `swap`), including why per-process `Σ` can exceed physical RAM and what trailing `~` means.
- **Consistency & verification**:
  - Verify every documented CLI flag, default, config key (`pspp.conf`), and key binding against `src/main.rs`, `src/config.rs`, and `src/ui/app.rs`.
  - Lint the roff manual page with `mandoc -Tlint doc/pspp.1` and verify crate docs with `cargo doc --no-deps`.
