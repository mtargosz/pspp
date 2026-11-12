---
name: "pspp TUI & btop Visual Fidelity"
description: "Use when editing ratatui rendering, overlays, tree connectors, gradients, braille/block/tty graphs, meters, themes, or keyboard/mouse handling."
applyTo: "src/ui/**/*.rs"
---

# TUI Rendering & btop Fidelity Guidelines

Consult `.github/agents/memory/btop-reference.md` for detailed visual specifications.

- **Modal & overlay compositing (`Menu`, `Options`, `Help`)**:
  - Always render `ratatui::widgets::Clear` over the overlay `Rect` before rendering its `Block` so underlying table text, borders, and `■` meters never bleed through.
  - Apply `theme.main_bg` to overlay blocks when `theme_background` is enabled.
  - Render bordered button labels inside `button_block.inner(rect)` when height permits, never overwriting the button's top border.
  - Use single-line scrollbar tracks (`.track_symbol(Some("│"))`) rather than double-line `║` next to single-line box borders, and avoid duplicate inner/outer hint bars.
- **Tree connectors & table columns**:
  - Depth-0 roots have no `├─`/`└─` connector column; pass `&ancestor_last[1..depth]` (only depths `>= 1`) when building continuation bars (`│  ` vs `   `).
  - Keep `OWN` and `<MEM> Σ` wide enough (minimum 11 cells) so values like `1023.4 KiB~` never clip the approximate `~` marker.
  - Indicate active sort column and direction (`↓`/`↑`) on clickable column headers.
- **Gradients, graphs, meters & low-colour modes**:
  - Default usage gradient uses 101 precomputed steps (`#77ca9b → #cbc06c → #dc4c4c`), falling back to the 256-colour cube/grey ramp when truecolor is off.
  - Selected rows must preserve filled vs. empty meter proportions (never painting a `0%` row as 100% solid); non-selected rows use positional gradient `graph::meter_spans`.
  - When `force_tty` is enabled or `NO_COLOR` is set, override `graph_symbol` to `Tty`, use square corners, and use distinct filled vs. empty meter glyphs so meters remain legible without colour.
- **Responsive layout & tiny-frame safety**:
  - Every `AppMode` (`Normal`, `FilterInput`, `Menu`, `Options`, `Help`) must render and process mouse/key events without panicking down to `1×1` and `10×3` viewports.
  - Keep active input prompts (`filter: <input>█`) visible on 80-column terminals by dropping lower-priority status segments before clipping the prompt.
  - Measure terminal display width accurately for Unicode process names and truncate overflowing `COMM` cells cleanly.
- **Terminal hygiene & event loop**:
  - Always restore raw mode, alternate screen, mouse capture, and cursor visibility via RAII guard + panic hook.
  - Never block the UI thread on process collection (poll `collect::Collector` via channel), ignore no-op mouse motion events, and coalesce queued zero-timeout input events before redrawing.
- **Headless verification**: Test both character cells and `fg`/`bg` `Color` styles using `ratatui::backend::TestBackend` (`ui::draw::screens` and unit tests).
