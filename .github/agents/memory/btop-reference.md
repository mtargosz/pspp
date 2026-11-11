# btop reference — research findings for pspp UI fidelity

Researched 2026-11-11 from `aristocratos/btop` `main` (line numbers may drift).

## UI style and shading

- **Rounded borders** (`btop_draw.cpp:57-75, 289-294`): lines `─ │`; square corners `┌ ┐ └ ┘`;
  rounded `╭ ╮ ╰ ╯` when `rounded_corners` on and not TTY. Title tabs `┐ ┌` (top) / `┘ └` (bottom);
  dividers `├ ┤ ┬ ┴`; superscript digits for box numbers.
- **Default theme** (`btop_theme.cpp:52-102`): greys `main_bg #00`, `main_fg #cc`, `title #ee`,
  `inactive_fg #40`, `meter_bg #40`, `div_line #30`; accents `hi_fg #b54040`, `selected_bg #6a2f2f`,
  `selected_fg #ee`; muted box outlines (`proc_box #805252`, `mem_box #6c6c4b`…). Usage gradient:
  `cpu_start #77ca9b`, `cpu_mid #cbc06c`, `cpu_end #dc4c4c` (green→yellow→red).
- **Gradients** (`btop_theme.cpp:306-362`): 101 precomputed escape strings indexed 0–100; linear RGB
  interpolation, two 50-step passes when a mid color exists. Used as `Theme::g("cpu").at(percent)`.
- **Escapes**: truecolor `ESC[38;2;r;g;bm` / `48;2;…`; lowcolor → `38;5;n`.
- **Row selection** (`btop_draw.cpp:2080`): `selected_bg` + `selected_fg` on the selected row.
- **theme_background=false** skips emitting `main_bg` (terminal transparency shows through).
- **Meters** (`btop_draw.cpp:87,396-418`): glyph `■`; filled cells colored at their gradient
  position; remainder `meter_bg`; disabled `inactive_fg`.

## Braille / block / tty graphs

- **Symbol tables** (`btop_draw.cpp:89-127`): per style (`braille`, `block`, `tty`) with `_up` and
  `_down` variants; each a 5×5 grid (25 entries) indexed `left*5 + right`, values 0–4 dot-rows.
  `braille_up`: `" ⢀⢠⢰⢸ ⡀⣀… ⡇⣇⣧⣷⣿"` (left column fills first). `_down` mirrors from top.
- **Resolution** (`btop_draw.cpp:422-470`): one cell = two consecutive samples ({last, current}),
  one per braille column → 2 time-samples/cell horizontal, 4 levels vertical. Values scaled 0–100,
  clamped to row's `cur_low/cur_high`, mapped to 0–4 via `round((v-low)*4/(high-low) + mod)`
  (mod 0.3 for height 1, else 0.1). Two toggled buffers carry sample pairing between frames.
  `no_zero` forces min 1 to keep a visible baseline.
- **Coloring**: height>1 → row i colored at `100 - (i-1)*100/height` (inverted: `i*100/height`);
  height 1 → per-cell color `max(last, current)`.
- **Fallbacks**: `block` quadrants `▗▖▄▟▐▌▙█` (half resolution); `tty` `░▒█`.

## Main menu (Esc or `m`)

- Triggers (`btop_input.cpp:222-240`): Esc/`m` main; F1/`?`/`h` help; F2/`o` options; `q` quit.
- Rendered into a `Global::overlay` composited over live UI; UI keeps updating if
  `background_update` true. Banner centered at `Term::height/2 - 10`; three 3-line ASCII-art
  buttons (Options, Help, Quit). Selected button uses double-line `╔═╗╚═╝`; normal buttons
  single-line with `#CC/#AA/#80` fading per line. Min size 80×24.
- Keyboard: down/tab/`j`/scroll-down = next (wraps); up/shift-tab/`k`/scroll-up = prev;
  enter/space activates; Esc/`q`/`m`/outside-click closes.
- Mouse: each button registers hit rect `{row,col,3,width}`; click on selected button activates,
  click on another selects it first.

## Options menu — General tab (btop_menu.cpp:224-400; defaults btop_config.cpp:274-342)

1. `color_theme` — theme file picker. 2. `theme_background` — show theme bg (off = transparency).
3. `truecolor` — 24-bit; off → 256-color 6×6×6 cube conversion. 4. `force_tty` — 16-color TTY mode.
5. `vim_keys` — h/j/k/l nav (help→`H`, kill→`K`). 6. `disable_mouse`. 7. `disable_presets`.
8. `presets` — box layouts. 9. `shown_boxes`. 10. `update_ms` — min 100, max 86400000.
11. `rounded_corners`. 12. `terminal_sync` — synchronized output (`?2026h/l`) anti-flicker.
13. `graph_symbol` — braille|block|tty. 14. `clock_format` — strftime + `/host`,`/user`,`/uptime`.
15. `base_10_sizes` — KB=1000 vs KiB=1024. 16. `background_update`. 17. `show_battery`.
18. `selected_battery`. 19. `show_battery_watts`. 20. `log_level`. 21. `save_config_on_exit`.

Editing: bool/list options change with left/right (h/l vim); string/int edited via enter/`e`;
up/down select, page keys page; Esc/`q`/`o`/backspace closes. Layout: 78 wide, left list 30 cols,
right help pane, `├─┬─┤` dividers. Click outside closes; click item selects; click selected edits.

**pspp mirrors the applicable subset** plus its own: `memory_type`, `aggregation_mode`.

## Mouse support

- Enable: `ESC[?1002h ?1015h ?1006h` (button-event tracking + urxvt + SGR extended); matching `l`
  set to disable. Alternate screen `?1049h`; hide cursor `?25l`; sync output `?2026h/l`.
- Parsing (`btop_input.cpp:131-173`): SGR `ESC[<code;col;row M|m` — code 0 M=click/m=release,
  32=drag, 64=scroll-up, 65=scroll-down.
- Hit testing: map key→`Mouse_loc{row,col,height,width}`; active menu's map when a menu is open;
  a click inside a rect is translated into that key press (buttons behave like keys).
- Process box: click selects row; wheel scrolls; scrollbar dragging supported.

## Keyboard model

- Global: Esc/`m` menu; F1/`?`/`h` help; F2/`o` options; `q`/ctrl-c quit; `+`/`-` adjust update_ms by 100.
- List: Up/Down/PgUp/PgDn/Home/End navigate; Left/Right change sort column; `r` reverse;
  `e` toggle tree view; Enter details; Space expand/collapse node.
- Vim keys (opt-in): h/l = left/right; j/k/g/G = down/up/top/bottom.
- Filter: `f` or `/` toggles filter edit; Enter/Down commits; Esc restores previous; `!`-prefix =
  regex; Delete clears; title shows `↵` while filtering; also `-f/--filter` CLI flag.

## Terminal requirements

- Best: truecolor + UTF-8 locale + font with Braille Patterns U+2800–U+28FF.
- Fallbacks: 256-color conversion — greyscale→232+ when R≈G≈B else cube
  `round(r/51)*36 + round(g/51)*6 + round(b/51) + 16`; TTY mode (16 colors, tty graphs).
- Needs a real tty on stdin; uses alternate screen; enforces minimum terminal size.
