use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::time::{Duration, Instant};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::collect::{self, Snapshot};
use crate::model::{AggMode, AggregateRow, MemKind, ProcTree, SortBy, VisibleRow};
use crate::ui::graph::{GraphSymbols, History};
use crate::ui::help;
use crate::ui::theme::Theme;

pub const DEFAULT_UPDATE_MS: u64 = 2_000;
pub const MIN_UPDATE_MS: u64 = 100;
/// btop's upper bound (24h); also keeps `Instant + Duration` from overflowing.
pub const MAX_UPDATE_MS: u64 = 86_400_000;

const MOUSE_SCROLL_STEP: isize = 3;
const DOUBLE_CLICK_MS: u64 = 350;
const SORT_ORDER: [SortBy; 4] = [SortBy::Aggregated, SortBy::Own, SortBy::Pid, SortBy::Comm];
const MENU_BUTTONS: [MenuButton; 3] = [MenuButton::Options, MenuButton::Help, MenuButton::Quit];
const OPTION_KEYS: [OptionKey; 8] = [
    OptionKey::UpdateMs,
    OptionKey::MemKind,
    OptionKey::AggMode,
    OptionKey::GraphSymbol,
    OptionKey::Base10,
    OptionKey::SaveOnExit,
    OptionKey::SortKey,
    OptionKey::SortReverse,
];

pub fn clamp_update_ms(update_ms: u64) -> u64 {
    update_ms.clamp(MIN_UPDATE_MS, MAX_UPDATE_MS)
}

pub fn next_tick_deadline(last_tick_at: Instant, update_ms: u64) -> Instant {
    let interval = Duration::from_millis(clamp_update_ms(update_ms));
    last_tick_at.checked_add(interval).unwrap_or(last_tick_at)
}

pub fn remaining_until_tick(now: Instant, next_tick_at: Instant) -> Duration {
    next_tick_at.saturating_duration_since(now)
}

pub fn cycle_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let len = len as isize;
    ((current as isize + delta).rem_euclid(len)) as usize
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppMode {
    Normal,
    FilterInput,
    Menu,
    Options,
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuButton {
    Options,
    Help,
    Quit,
}

impl MenuButton {
    pub fn label(self) -> &'static str {
        match self {
            Self::Options => "Options",
            Self::Help => "Help",
            Self::Quit => "Quit",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphSymbolSetting {
    Auto,
    Braille,
    Block,
    Tty,
}

impl GraphSymbolSetting {
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Self::Auto,
            "braille" => Self::Braille,
            "block" => Self::Block,
            "tty" => Self::Tty,
            _ => Self::Auto,
        }
    }

    pub fn as_config(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Braille => "braille",
            Self::Block => "block",
            Self::Tty => "tty",
        }
    }

    pub fn label(self) -> &'static str {
        self.as_config()
    }

    pub fn resolve(self) -> GraphSymbols {
        match self {
            Self::Auto => GraphSymbols::detect(),
            Self::Braille => GraphSymbols::Braille,
            Self::Block => GraphSymbols::Block,
            Self::Tty => GraphSymbols::Tty,
        }
    }

    pub fn cycle(self, delta: isize) -> Self {
        const ORDER: [GraphSymbolSetting; 4] = [
            GraphSymbolSetting::Auto,
            GraphSymbolSetting::Braille,
            GraphSymbolSetting::Block,
            GraphSymbolSetting::Tty,
        ];

        let current = ORDER.iter().position(|value| *value == self).unwrap_or(0);
        ORDER[cycle_index(current, ORDER.len(), delta)]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionKey {
    UpdateMs,
    MemKind,
    AggMode,
    GraphSymbol,
    Base10,
    SaveOnExit,
    SortKey,
    SortReverse,
}

impl OptionKey {
    pub fn label(self) -> &'static str {
        match self {
            Self::UpdateMs => "update_ms",
            Self::MemKind => "mem_kind",
            Self::AggMode => "agg_mode",
            Self::GraphSymbol => "graph_symbol",
            Self::Base10 => "base10",
            Self::SaveOnExit => "save_on_exit",
            Self::SortKey => "sort_key",
            Self::SortReverse => "sort_reverse",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::UpdateMs => {
                "Refresh interval in milliseconds (minimum 100). Enter edits, ←/→ adjusts by 100."
            }
            Self::MemKind => "Memory field used by table and graph (platform-dependent kinds).",
            Self::AggMode => "rollup sums descendants; flat sums direct children only.",
            Self::GraphSymbol => "Graph glyph style: auto, braille, block, or tty.",
            Self::Base10 => "Size units: false = KiB/MiB (1024), true = KB/MB (1000).",
            Self::SaveOnExit => "Persist current options to pspp.conf when leaving TUI.",
            Self::SortKey => "Primary sort key inside each tree sibling group.",
            Self::SortReverse => "Sort direction: true = descending, false = ascending.",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiSettings {
    pub update_ms: u64,
    pub mem_kind: MemKind,
    pub agg_mode: AggMode,
    pub graph_symbol: GraphSymbolSetting,
    pub base10: bool,
    pub save_on_exit: bool,
    pub sort_by: SortBy,
    pub sort_reverse: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            update_ms: DEFAULT_UPDATE_MS,
            mem_kind: MemKind::Rss,
            agg_mode: AggMode::Rollup,
            graph_symbol: GraphSymbolSetting::Auto,
            base10: false,
            save_on_exit: true,
            sort_by: SortBy::Aggregated,
            sort_reverse: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DisplayRow {
    pub pid: i32,
    pub comm: String,
    pub own: u64,
    pub own_approximate: bool,
    pub aggregated: u64,
    pub aggregated_approximate: bool,
    pub child_count: usize,
    pub tree_prefix: String,
    pub toggle_marker: Option<Range<usize>>,
}

pub struct App {
    pub snapshot: Snapshot,
    pub tree: ProcTree,
    pub agg_mode: AggMode,
    pub mem_kind: MemKind,
    pub sort_by: SortBy,
    pub sort_desc: bool,
    pub update_ms: u64,
    pub base10: bool,
    pub save_on_exit: bool,
    pub theme: Theme,
    pub mem_history: History,
    pub graph_symbol: GraphSymbolSetting,
    pub mode: AppMode,
    pub should_quit: bool,
    pub last_tick_at: Instant,
    pub next_tick_at: Instant,
    pub last_mouse_position: Option<(u16, u16)>,

    expanded: HashSet<i32>,
    rows: Vec<DisplayRow>,
    cursor: usize,
    scroll: usize,
    selected_pid: Option<i32>,
    viewport_height: usize,

    table_body_rect: Option<Rect>,
    comm_col_x: u16,
    rendered_start: usize,
    rendered_toggle_zones: Vec<Option<Range<u16>>>,
    last_click: Option<(usize, Instant)>,

    menu_selected: MenuButton,
    menu_overlay_rect: Option<Rect>,
    menu_button_rects: [Option<Rect>; 3],

    option_selected: usize,
    update_ms_editor: Option<String>,

    help_scroll: usize,
    help_content_lines: usize,
    help_viewport_lines: usize,

    filter_input: String,
    filter_committed: String,
    filter_visible: HashSet<i32>,
    filter_direct: HashSet<i32>,
}

impl App {
    #[cfg(test)]
    pub fn new(update_ms: u64) -> Self {
        let settings = UiSettings {
            update_ms,
            ..UiSettings::default()
        };
        Self::from_settings(settings)
    }

    pub fn from_settings(settings: UiSettings) -> Self {
        let update_ms = clamp_update_ms(settings.update_ms);
        let snapshot = collect::take_snapshot();
        let tree = ProcTree::build(&snapshot);
        let now = Instant::now();
        let expanded: HashSet<i32> = snapshot.procs.iter().map(|proc_rec| proc_rec.pid).collect();

        let mut app = Self {
            snapshot,
            tree,
            agg_mode: settings.agg_mode,
            mem_kind: settings.mem_kind,
            sort_by: settings.sort_by,
            sort_desc: settings.sort_reverse,
            update_ms,
            base10: settings.base10,
            save_on_exit: settings.save_on_exit,
            theme: Theme::detect(),
            mem_history: History::default(),
            graph_symbol: settings.graph_symbol,
            mode: AppMode::Normal,
            should_quit: false,
            last_tick_at: now,
            next_tick_at: next_tick_deadline(now, update_ms),
            last_mouse_position: None,
            expanded,
            rows: Vec::new(),
            cursor: 0,
            scroll: 0,
            selected_pid: None,
            viewport_height: 1,
            table_body_rect: None,
            comm_col_x: 0,
            rendered_start: 0,
            rendered_toggle_zones: Vec::new(),
            last_click: None,
            menu_selected: MenuButton::Options,
            menu_overlay_rect: None,
            menu_button_rects: [None, None, None],
            option_selected: 0,
            update_ms_editor: None,
            help_scroll: 0,
            help_content_lines: 0,
            help_viewport_lines: 1,
            filter_input: String::new(),
            filter_committed: String::new(),
            filter_visible: HashSet::new(),
            filter_direct: HashSet::new(),
        };
        app.record_mem_sample();
        app.rebuild_rows();
        app
    }

    pub fn ui_settings(&self) -> UiSettings {
        UiSettings {
            update_ms: self.update_ms,
            mem_kind: self.mem_kind,
            agg_mode: self.agg_mode,
            graph_symbol: self.graph_symbol,
            base10: self.base10,
            save_on_exit: self.save_on_exit,
            sort_by: self.sort_by,
            sort_reverse: self.sort_desc,
        }
    }

    pub fn resolved_graph_symbols(&self) -> GraphSymbols {
        self.graph_symbol.resolve()
    }

    /// System-wide total of the selected memory kind across all processes.
    pub fn mem_total_used(&self) -> u64 {
        self.snapshot
            .procs
            .iter()
            .filter_map(|proc_rec| self.mem_kind.get(&proc_rec.mem))
            .fold(0_u64, u64::saturating_add)
    }

    /// Graph scale: total physical memory.
    pub fn mem_graph_max(&self) -> u64 {
        self.snapshot.total_mem
    }

    pub fn menu_buttons() -> &'static [MenuButton] {
        &MENU_BUTTONS
    }

    pub fn menu_selected(&self) -> MenuButton {
        self.menu_selected
    }

    pub fn option_keys() -> &'static [OptionKey] {
        &OPTION_KEYS
    }

    pub fn option_selected_index(&self) -> usize {
        self.option_selected
    }

    pub fn selected_option_key(&self) -> OptionKey {
        OPTION_KEYS
            .get(self.option_selected)
            .copied()
            .unwrap_or(OptionKey::UpdateMs)
    }

    pub fn selected_option_description(&self) -> &'static str {
        self.selected_option_key().description()
    }

    pub fn help_scroll(&self) -> usize {
        self.help_scroll
    }

    pub fn set_help_scroll_metrics(&mut self, content_lines: usize, viewport_lines: usize) {
        self.help_content_lines = content_lines;
        self.help_viewport_lines = viewport_lines.max(1);
        self.help_scroll = help::clamp_scroll_offset(
            self.help_scroll,
            self.help_content_lines,
            self.help_viewport_lines,
        );
    }

    pub fn is_filter_active(&self) -> bool {
        !self.filter_committed.is_empty()
    }

    pub fn filter_indicator(&self) -> Option<String> {
        if self.mode == AppMode::FilterInput {
            return Some(format!("filter: {}█", self.filter_input));
        }
        if self.filter_committed.is_empty() {
            None
        } else {
            Some(format!("filter: {}", self.filter_committed))
        }
    }

    pub fn is_direct_filter_match(&self, pid: i32) -> bool {
        self.is_filter_active() && self.filter_direct.contains(&pid)
    }

    pub fn is_context_filter_match(&self, pid: i32) -> bool {
        self.is_filter_active()
            && !self.filter_direct.contains(&pid)
            && self.filter_visible.contains(&pid)
    }

    pub fn option_value(&self, key: OptionKey) -> String {
        match key {
            OptionKey::UpdateMs => {
                if self.selected_option_key() == OptionKey::UpdateMs {
                    if let Some(buffer) = &self.update_ms_editor {
                        return format!("{buffer}_");
                    }
                }
                self.update_ms.to_string()
            }
            OptionKey::MemKind => self.mem_kind.label().to_ascii_lowercase(),
            OptionKey::AggMode => match self.agg_mode {
                AggMode::Rollup => "rollup".to_owned(),
                AggMode::Flat => "flat".to_owned(),
            },
            OptionKey::GraphSymbol => self.graph_symbol.label().to_owned(),
            OptionKey::Base10 => bool_label(self.base10).to_owned(),
            OptionKey::SaveOnExit => bool_label(self.save_on_exit).to_owned(),
            OptionKey::SortKey => sort_to_config(self.sort_by).to_owned(),
            OptionKey::SortReverse => bool_label(self.sort_desc).to_owned(),
        }
    }

    fn record_mem_sample(&mut self) {
        let used = self.mem_total_used();
        self.mem_history.push(used);
    }

    pub fn poll_timeout(&self, now: Instant) -> Duration {
        remaining_until_tick(now, self.next_tick_at)
    }

    pub fn should_tick(&self, now: Instant) -> bool {
        now >= self.next_tick_at
    }

    pub fn on_tick(&mut self) {
        let old_pids: HashSet<i32> = self
            .snapshot
            .procs
            .iter()
            .map(|proc_rec| proc_rec.pid)
            .collect();

        self.snapshot = collect::take_snapshot();
        self.tree = ProcTree::build(&self.snapshot);

        let new_pids: HashSet<i32> = self
            .snapshot
            .procs
            .iter()
            .map(|proc_rec| proc_rec.pid)
            .collect();
        self.expanded = remap_expanded_set(&self.expanded, &old_pids, &new_pids);

        self.record_mem_sample();
        self.rebuild_rows();
        self.last_tick_at = Instant::now();
        self.next_tick_at = next_tick_deadline(self.last_tick_at, self.update_ms);
    }

    pub fn on_key(&mut self, key_event: KeyEvent) {
        if key_event.kind != KeyEventKind::Press {
            return;
        }
        // Raw mode swallows SIGINT, so Ctrl-C/Ctrl-Q must quit from every mode.
        if key_event.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key_event.code, KeyCode::Char('c' | 'C' | 'q' | 'Q'))
        {
            self.should_quit = true;
            return;
        }

        match self.mode {
            AppMode::Normal => self.on_key_normal(key_event),
            AppMode::FilterInput => self.on_key_filter_input(key_event),
            AppMode::Menu => self.on_key_menu(key_event),
            AppMode::Options => self.on_key_options(key_event),
            AppMode::Help => self.on_key_help(key_event),
        }
    }

    pub fn on_mouse(&mut self, mouse_event: MouseEvent) {
        self.last_mouse_position = Some((mouse_event.column, mouse_event.row));

        match self.mode {
            AppMode::Normal => match mouse_event.kind {
                MouseEventKind::ScrollUp => self.move_cursor_by(-MOUSE_SCROLL_STEP),
                MouseEventKind::ScrollDown => self.move_cursor_by(MOUSE_SCROLL_STEP),
                MouseEventKind::Down(MouseButton::Left) => {
                    self.handle_left_click(mouse_event.column, mouse_event.row)
                }
                _ => {}
            },
            AppMode::FilterInput => {}
            AppMode::Menu => {
                if matches!(mouse_event.kind, MouseEventKind::Down(MouseButton::Left)) {
                    self.handle_menu_click(mouse_event.column, mouse_event.row);
                }
            }
            AppMode::Options | AppMode::Help => {}
        }
    }

    pub fn rows(&self) -> &[DisplayRow] {
        &self.rows
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn set_viewport_height(&mut self, height: usize) {
        self.viewport_height = height.max(1);
        self.apply_cursor_scroll_clamp();
    }

    pub fn set_table_hit_test(
        &mut self,
        body_rect: Rect,
        comm_col_x: u16,
        rendered_start: usize,
        toggle_zones: Vec<Option<Range<u16>>>,
    ) {
        self.table_body_rect = Some(body_rect);
        self.comm_col_x = comm_col_x;
        self.rendered_start = rendered_start;
        self.rendered_toggle_zones = toggle_zones;
    }

    pub fn clear_table_hit_test(&mut self) {
        self.table_body_rect = None;
        self.comm_col_x = 0;
        self.rendered_start = 0;
        self.rendered_toggle_zones.clear();
    }

    pub fn set_menu_hit_test(&mut self, overlay_rect: Rect, button_rects: [Rect; 3]) {
        self.menu_overlay_rect = Some(overlay_rect);
        self.menu_button_rects = [
            Some(button_rects[0]),
            Some(button_rects[1]),
            Some(button_rects[2]),
        ];
    }

    pub fn clear_menu_hit_test(&mut self) {
        self.menu_overlay_rect = None;
        self.menu_button_rects = [None, None, None];
    }

    fn on_key_normal(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Esc => self.open_menu(),
            KeyCode::Char('f') | KeyCode::Char('F') | KeyCode::Char('/') => {
                self.enter_filter_input();
            }
            KeyCode::Char('c') | KeyCode::Char('C') if self.is_filter_active() => {
                self.clear_filter();
            }
            KeyCode::Char(ch) if is_quit_key(ch, key_event.modifiers) => {
                self.should_quit = true;
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor_by(1),
            KeyCode::PageUp => self.move_cursor_by(-(self.page_step() as isize)),
            KeyCode::PageDown => self.move_cursor_by(self.page_step() as isize),
            KeyCode::Home | KeyCode::Char('g') => self.go_home(),
            KeyCode::End | KeyCode::Char('G') => self.go_end(),
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle_selected_expand(),
            KeyCode::Left => self.collapse_or_select_parent(),
            KeyCode::Right => self.expand_selected(),
            KeyCode::Char('e') | KeyCode::Char('E') => self.toggle_agg_mode(),
            KeyCode::Char('m') | KeyCode::Char('M') => self.cycle_mem_kind(1),
            KeyCode::Char('s') => self.cycle_sort_key(1),
            KeyCode::Char('S') | KeyCode::Char('r') | KeyCode::Char('R') => self.reverse_sort(),
            _ => {}
        }
    }

    fn on_key_filter_input(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Esc => {
                self.clear_filter();
                self.mode = AppMode::Normal;
            }
            KeyCode::Enter => {
                self.commit_filter_input();
                self.mode = AppMode::Normal;
            }
            KeyCode::Backspace => {
                self.filter_input.pop();
            }
            KeyCode::Char(ch)
                if !key_event.modifiers.contains(KeyModifiers::CONTROL)
                    && !key_event.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.filter_input.push(ch);
            }
            _ => {}
        }
    }

    fn on_key_menu(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => self.mode = AppMode::Normal,
            KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => self.move_menu_selection(-1),
            KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => self.move_menu_selection(1),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate_menu_button(self.menu_selected),
            _ => {}
        }
    }

    fn on_key_options(&mut self, key_event: KeyEvent) {
        if self.update_ms_editor.is_some() {
            self.on_key_options_editing_update_ms(key_event);
            return;
        }

        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => self.mode = AppMode::Menu,
            KeyCode::Up | KeyCode::Char('k') => self.move_option_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_option_selection(1),
            KeyCode::Left => self.adjust_selected_option(-1),
            KeyCode::Right => self.adjust_selected_option(1),
            KeyCode::Enter => {
                if self.selected_option_key() == OptionKey::UpdateMs {
                    self.update_ms_editor = Some(self.update_ms.to_string());
                } else {
                    self.adjust_selected_option(1);
                }
            }
            _ => {}
        }
    }

    fn on_key_options_editing_update_ms(&mut self, key_event: KeyEvent) {
        let Some(editor) = self.update_ms_editor.as_mut() else {
            return;
        };

        match key_event.code {
            KeyCode::Esc => self.update_ms_editor = None,
            KeyCode::Enter => {
                if let Ok(parsed) = editor.parse::<u64>() {
                    self.set_update_ms(parsed);
                }
                self.update_ms_editor = None;
            }
            KeyCode::Backspace => {
                editor.pop();
            }
            KeyCode::Char(ch) if ch.is_ascii_digit() && editor.len() < 8 => {
                editor.push(ch);
            }
            _ => {}
        }
    }

    fn on_key_help(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_help_scroll_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_help_scroll_by(1),
            KeyCode::PageUp => self.move_help_scroll_by(-(self.help_page_step() as isize)),
            KeyCode::PageDown => self.move_help_scroll_by(self.help_page_step() as isize),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => self.mode = AppMode::Menu,
            _ => {}
        }
    }

    fn open_menu(&mut self) {
        self.mode = AppMode::Menu;
        self.update_ms_editor = None;
    }

    fn move_menu_selection(&mut self, delta: isize) {
        let idx = MENU_BUTTONS
            .iter()
            .position(|button| *button == self.menu_selected)
            .unwrap_or(0);
        self.menu_selected = MENU_BUTTONS[cycle_index(idx, MENU_BUTTONS.len(), delta)];
    }

    fn activate_menu_button(&mut self, button: MenuButton) {
        match button {
            MenuButton::Options => {
                self.mode = AppMode::Options;
                self.update_ms_editor = None;
            }
            MenuButton::Help => {
                self.mode = AppMode::Help;
                self.update_ms_editor = None;
                self.help_scroll = 0;
            }
            MenuButton::Quit => self.should_quit = true,
        }
    }

    fn move_option_selection(&mut self, delta: isize) {
        self.option_selected = cycle_index(self.option_selected, OPTION_KEYS.len(), delta);
    }

    fn adjust_selected_option(&mut self, delta: isize) {
        match self.selected_option_key() {
            OptionKey::UpdateMs => {
                let step = 100_u64;
                let mut value = self.update_ms;
                let count = delta.unsigned_abs();
                if delta >= 0 {
                    for _ in 0..count {
                        value = value.saturating_add(step);
                    }
                } else {
                    for _ in 0..count {
                        value = value.saturating_sub(step);
                    }
                }
                self.set_update_ms(value);
            }
            OptionKey::MemKind => self.cycle_mem_kind(delta),
            OptionKey::AggMode => self.toggle_agg_mode(),
            OptionKey::GraphSymbol => {
                self.graph_symbol = self.graph_symbol.cycle(delta);
            }
            OptionKey::Base10 => {
                self.base10 = !self.base10;
            }
            OptionKey::SaveOnExit => {
                self.save_on_exit = !self.save_on_exit;
            }
            OptionKey::SortKey => self.cycle_sort_key(delta),
            OptionKey::SortReverse => self.reverse_sort(),
        }
    }

    fn set_update_ms(&mut self, value: u64) {
        self.update_ms = clamp_update_ms(value);
        self.next_tick_at = next_tick_deadline(self.last_tick_at, self.update_ms);
    }

    fn handle_menu_click(&mut self, column: u16, row: u16) {
        for (idx, button_rect) in self.menu_button_rects.iter().enumerate() {
            if let Some(rect) = button_rect {
                if point_in_rect(*rect, column, row) {
                    let button = MENU_BUTTONS[idx];
                    self.menu_selected = button;
                    self.activate_menu_button(button);
                    return;
                }
            }
        }

        if let Some(rect) = self.menu_overlay_rect {
            if !point_in_rect(rect, column, row) {
                self.mode = AppMode::Normal;
            }
        }
    }

    fn page_step(&self) -> usize {
        self.viewport_height.saturating_sub(1).max(1)
    }

    fn help_page_step(&self) -> usize {
        self.help_viewport_lines.saturating_sub(1).max(1)
    }

    fn rebuild_rows(&mut self) {
        let aggregates = self.tree.aggregate(self.mem_kind, self.agg_mode);
        if self.filter_committed.is_empty() {
            self.filter_visible.clear();
            self.filter_direct.clear();
            let visible = self.tree.flatten_visible_sorted(
                &self.expanded,
                &aggregates,
                self.sort_by,
                self.sort_desc,
            );
            self.rows = build_display_rows(&self.tree, &aggregates, &visible, &self.expanded);
        } else {
            let match_set = self.tree.filter_match_set(&self.filter_committed);
            self.filter_visible = match_set.visible.clone();
            self.filter_direct = match_set.direct;
            let expanded_all: HashSet<i32> = self
                .snapshot
                .procs
                .iter()
                .map(|proc_rec| proc_rec.pid)
                .collect();
            let visible_all = self.tree.flatten_visible_sorted(
                &expanded_all,
                &aggregates,
                self.sort_by,
                self.sort_desc,
            );
            let visible: Vec<VisibleRow> = visible_all
                .into_iter()
                .filter(|row| self.filter_visible.contains(&row.pid))
                .collect();
            self.rows = build_display_rows(&self.tree, &aggregates, &visible, &expanded_all);
        }
        self.cursor = relocate_cursor_to_selected(self.selected_pid, &self.rows, self.cursor);
        self.apply_cursor_scroll_clamp();
        self.selected_pid = self.rows.get(self.cursor).map(|row| row.pid);
        self.rendered_toggle_zones.clear();
    }

    fn apply_cursor_scroll_clamp(&mut self) {
        let (cursor, scroll) = clamp_cursor_scroll(
            self.cursor,
            self.scroll,
            self.rows.len(),
            self.viewport_height,
        );
        self.cursor = cursor;
        self.scroll = scroll;
    }

    fn move_cursor_by(&mut self, delta: isize) {
        if self.rows.is_empty() {
            self.cursor = 0;
            self.scroll = 0;
            self.selected_pid = None;
            return;
        }

        let max = self.rows.len().saturating_sub(1) as isize;
        let next = (self.cursor as isize + delta).clamp(0, max) as usize;
        self.cursor = next;
        self.selected_pid = self.rows.get(self.cursor).map(|row| row.pid);
        self.apply_cursor_scroll_clamp();
    }

    fn go_home(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.cursor = 0;
        self.selected_pid = self.rows.first().map(|row| row.pid);
        self.apply_cursor_scroll_clamp();
    }

    fn go_end(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.cursor = self.rows.len().saturating_sub(1);
        self.selected_pid = self.rows.get(self.cursor).map(|row| row.pid);
        self.apply_cursor_scroll_clamp();
    }

    fn toggle_selected_expand(&mut self) {
        if let Some(pid) = self.selected_pid {
            self.toggle_expand_for_pid(pid);
        }
    }

    fn collapse_or_select_parent(&mut self) {
        let Some(pid) = self.selected_pid else {
            return;
        };

        let has_children = !self.tree.child_pids(pid).is_empty();
        if has_children && self.expanded.contains(&pid) {
            self.expanded.remove(&pid);
            self.rebuild_rows();
            return;
        }

        if let Some(parent_pid) = self.tree.parent_pid(pid) {
            self.select_pid(parent_pid);
        }
    }

    fn expand_selected(&mut self) {
        let Some(pid) = self.selected_pid else {
            return;
        };

        if self.tree.child_pids(pid).is_empty() || self.expanded.contains(&pid) {
            return;
        }

        self.expanded.insert(pid);
        self.rebuild_rows();
    }

    fn toggle_agg_mode(&mut self) {
        self.agg_mode = match self.agg_mode {
            AggMode::Rollup => AggMode::Flat,
            AggMode::Flat => AggMode::Rollup,
        };
        self.rebuild_rows();
    }

    fn cycle_mem_kind(&mut self, delta: isize) {
        let available = MemKind::available();
        if available.is_empty() {
            return;
        }

        self.mem_kind = cycle_mem_kind_value(self.mem_kind, delta, &available);
        self.mem_history.clear();
        self.record_mem_sample();
        self.rebuild_rows();
    }

    fn cycle_sort_key(&mut self, delta: isize) {
        self.sort_by = cycle_sort_key_value(self.sort_by, delta);
        self.rebuild_rows();
    }

    fn reverse_sort(&mut self) {
        self.sort_desc = !self.sort_desc;
        self.rebuild_rows();
    }

    fn select_pid(&mut self, pid: i32) {
        if let Some(index) = self.rows.iter().position(|row| row.pid == pid) {
            self.cursor = index;
            self.selected_pid = Some(pid);
            self.apply_cursor_scroll_clamp();
        }
    }

    fn toggle_expand_for_pid(&mut self, pid: i32) {
        if self.tree.child_pids(pid).is_empty() {
            return;
        }

        if self.expanded.contains(&pid) {
            self.expanded.remove(&pid);
        } else {
            self.expanded.insert(pid);
        }
        self.rebuild_rows();
    }

    fn handle_left_click(&mut self, column: u16, row: u16) {
        let Some((row_index, local_row)) = self.hit_test_row(row) else {
            self.last_click = None;
            return;
        };

        self.cursor = row_index;
        self.selected_pid = self.rows.get(row_index).map(|entry| entry.pid);
        self.apply_cursor_scroll_clamp();

        let marker_hit = self.hit_toggle_zone(local_row, column);
        let now = Instant::now();
        let double_click = self
            .last_click
            .map(|(last_row, last_at)| {
                last_row == row_index
                    && now.saturating_duration_since(last_at)
                        <= Duration::from_millis(DOUBLE_CLICK_MS)
            })
            .unwrap_or(false);

        if marker_hit || double_click {
            if let Some(pid) = self.selected_pid {
                self.toggle_expand_for_pid(pid);
            }
            self.last_click = None;
            return;
        }

        self.last_click = Some((row_index, now));
    }

    fn hit_test_row(&self, row: u16) -> Option<(usize, usize)> {
        let rect = self.table_body_rect?;
        if row < rect.y || row >= rect.y.saturating_add(rect.height) {
            return None;
        }

        let local = usize::from(row.saturating_sub(rect.y));
        let global = self.rendered_start.saturating_add(local);
        if global >= self.rows.len() {
            return None;
        }

        Some((global, local))
    }

    fn hit_toggle_zone(&self, local_row: usize, column: u16) -> bool {
        if column < self.comm_col_x {
            return false;
        }

        let Some(range) = self
            .rendered_toggle_zones
            .get(local_row)
            .and_then(|zone| zone.as_ref())
        else {
            return false;
        };

        range.contains(&column)
    }

    fn enter_filter_input(&mut self) {
        self.filter_input = self.filter_committed.clone();
        self.mode = AppMode::FilterInput;
    }

    fn commit_filter_input(&mut self) {
        self.filter_committed = self.filter_input.clone();
        self.rebuild_rows();
    }

    /// Pre-sets the committed process filter (CLI `-f/--filter`).
    pub fn set_filter(&mut self, text: &str) {
        self.filter_input = text.to_owned();
        self.filter_committed = text.to_owned();
        self.rebuild_rows();
    }

    fn clear_filter(&mut self) {
        self.filter_input.clear();
        self.filter_committed.clear();
        self.rebuild_rows();
    }

    fn move_help_scroll_by(&mut self, delta: isize) {
        let next = if delta >= 0 {
            self.help_scroll.saturating_add(delta as usize)
        } else {
            self.help_scroll.saturating_sub(delta.unsigned_abs())
        };
        self.help_scroll =
            help::clamp_scroll_offset(next, self.help_content_lines, self.help_viewport_lines);
    }
}

pub fn cycle_mem_kind_value(current: MemKind, delta: isize, available: &[MemKind]) -> MemKind {
    if available.is_empty() {
        return current;
    }
    let idx = available
        .iter()
        .position(|kind| *kind == current)
        .unwrap_or(0);
    available[cycle_index(idx, available.len(), delta)]
}

pub fn cycle_sort_key_value(current: SortBy, delta: isize) -> SortBy {
    let idx = SORT_ORDER
        .iter()
        .position(|sort_by| *sort_by == current)
        .unwrap_or(0);
    SORT_ORDER[cycle_index(idx, SORT_ORDER.len(), delta)]
}

pub fn parse_mem_kind_config(value: &str) -> MemKind {
    let parsed = match value.trim().to_ascii_lowercase().as_str() {
        "rss" => MemKind::Rss,
        "vsz" => MemKind::Vsz,
        "footprint" => MemKind::Footprint,
        "pss" => MemKind::Pss,
        "uss" => MemKind::Uss,
        "swap" => MemKind::Swap,
        _ => MemKind::Rss,
    };

    if MemKind::available().contains(&parsed) {
        parsed
    } else {
        MemKind::Rss
    }
}

pub fn parse_agg_mode_config(value: &str) -> AggMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "flat" => AggMode::Flat,
        _ => AggMode::Rollup,
    }
}

pub fn parse_sort_config(value: &str) -> SortBy {
    match value.trim().to_ascii_lowercase().as_str() {
        "own" => SortBy::Own,
        "pid" => SortBy::Pid,
        "comm" => SortBy::Comm,
        _ => SortBy::Aggregated,
    }
}

pub fn sort_to_config(sort_by: SortBy) -> &'static str {
    match sort_by {
        SortBy::Aggregated => "agg",
        SortBy::Own => "own",
        SortBy::Pid => "pid",
        SortBy::Comm => "comm",
    }
}

fn bool_label(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn point_in_rect(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

fn remap_expanded_set(
    previous_expanded: &HashSet<i32>,
    old_pids: &HashSet<i32>,
    new_pids: &HashSet<i32>,
) -> HashSet<i32> {
    let mut mapped = new_pids.clone();
    for pid in old_pids.intersection(new_pids) {
        if !previous_expanded.contains(pid) {
            mapped.remove(pid);
        }
    }
    mapped
}

fn build_display_rows(
    tree: &ProcTree,
    aggregates: &HashMap<i32, AggregateRow>,
    visible: &[VisibleRow],
    expanded: &HashSet<i32>,
) -> Vec<DisplayRow> {
    let mut rows = Vec::with_capacity(visible.len());
    let mut ancestor_last = Vec::new();

    for visible_row in visible {
        ancestor_last.truncate(visible_row.depth);

        let continuation_depth = visible_row.depth.saturating_sub(1);
        let (prefix, marker) = build_tree_prefix(
            visible_row.depth,
            visible_row.is_last_child,
            visible_row.has_children,
            expanded.contains(&visible_row.pid),
            &ancestor_last[..continuation_depth],
        );

        let aggregate = aggregates.get(&visible_row.pid);
        let proc_rec = tree.proc(visible_row.pid);
        rows.push(DisplayRow {
            pid: visible_row.pid,
            comm: proc_rec
                .map(|proc_rec| proc_rec.comm.clone())
                .unwrap_or_else(|| "?".to_owned()),
            own: aggregate.map(|row| row.own).unwrap_or_default(),
            own_approximate: aggregate.map(|row| row.own_approximate).unwrap_or(true),
            aggregated: aggregate.map(|row| row.aggregated).unwrap_or_default(),
            aggregated_approximate: aggregate
                .map(|row| row.aggregated_approximate)
                .unwrap_or(true),
            child_count: aggregate.map(|row| row.child_count).unwrap_or_default(),
            tree_prefix: prefix,
            toggle_marker: marker,
        });

        ancestor_last.push(visible_row.is_last_child);
    }

    rows
}

pub fn build_tree_prefix(
    depth: usize,
    is_last_child: bool,
    has_children: bool,
    expanded: bool,
    ancestor_is_last: &[bool],
) -> (String, Option<Range<usize>>) {
    let marker_text = if expanded { "[-]" } else { "[+]" };
    let mut prefix = String::new();

    if depth > 0 {
        for is_last in ancestor_is_last {
            prefix.push_str(if *is_last { "   " } else { "│  " });
        }
        prefix.push_str(if is_last_child { "└─ " } else { "├─ " });
    }

    let marker = if has_children {
        let start = prefix.chars().count();
        prefix.push_str(marker_text);
        let end = prefix.chars().count();
        prefix.push(' ');
        Some(start..end)
    } else {
        None
    };

    (prefix, marker)
}

pub fn clamp_cursor_scroll(
    cursor: usize,
    scroll: usize,
    row_count: usize,
    viewport_height: usize,
) -> (usize, usize) {
    if row_count == 0 {
        return (0, 0);
    }

    let height = viewport_height.max(1);
    let max_cursor = row_count.saturating_sub(1);
    let mut clamped_cursor = cursor.min(max_cursor);
    let max_scroll = row_count.saturating_sub(height);
    let mut clamped_scroll = scroll.min(max_scroll);

    if clamped_cursor < clamped_scroll {
        clamped_scroll = clamped_cursor;
    }

    let bottom = clamped_scroll.saturating_add(height);
    if clamped_cursor >= bottom {
        clamped_scroll = clamped_cursor.saturating_add(1).saturating_sub(height);
    }

    clamped_scroll = clamped_scroll.min(max_scroll);
    clamped_cursor = clamped_cursor.min(max_cursor);
    (clamped_cursor, clamped_scroll)
}

fn relocate_cursor_to_selected(
    selected_pid: Option<i32>,
    rows: &[DisplayRow],
    fallback_cursor: usize,
) -> usize {
    if rows.is_empty() {
        return 0;
    }

    if let Some(pid) = selected_pid {
        if let Some(index) = rows.iter().position(|row| row.pid == pid) {
            return index;
        }
    }

    fallback_cursor.min(rows.len().saturating_sub(1))
}

fn is_quit_key(ch: char, modifiers: KeyModifiers) -> bool {
    let lowered = ch.to_ascii_lowercase();
    if modifiers.contains(KeyModifiers::CONTROL) {
        lowered == 'c' || lowered == 'q'
    } else {
        lowered == 'q'
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_tree_prefix, clamp_cursor_scroll, clamp_update_ms, cycle_index, cycle_mem_kind_value,
        cycle_sort_key_value, next_tick_deadline, relocate_cursor_to_selected, App, AppMode,
        DisplayRow, GraphSymbolSetting, MAX_UPDATE_MS, MIN_UPDATE_MS,
    };
    use crate::model::{MemKind, SortBy};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::ops::Range;
    use std::time::{Duration, Instant};

    fn row(pid: i32) -> DisplayRow {
        DisplayRow {
            pid,
            comm: format!("p{pid}"),
            own: 0,
            own_approximate: false,
            aggregated: 0,
            aggregated_approximate: false,
            child_count: 0,
            tree_prefix: String::new(),
            toggle_marker: None,
        }
    }

    #[test]
    fn tree_prefix_for_roots_and_children_matches_expected_glyphs() {
        let (root_prefix, root_marker) = build_tree_prefix(0, true, true, true, &[]);
        assert_eq!(root_prefix, "[-] ");
        assert_eq!(root_marker, Some(0..3));

        let (middle_child, middle_marker) = build_tree_prefix(1, false, true, false, &[]);
        assert_eq!(middle_child, "├─ [+] ");
        assert_eq!(middle_marker, Some(3..6));

        let (deep_leaf, deep_marker) = build_tree_prefix(3, true, false, false, &[false, true]);
        assert_eq!(deep_leaf, "│     └─ ");
        assert_eq!(deep_marker, None::<Range<usize>>);
    }

    #[test]
    fn clamp_cursor_scroll_keeps_cursor_visible() {
        assert_eq!(clamp_cursor_scroll(0, 0, 0, 10), (0, 0));
        assert_eq!(clamp_cursor_scroll(10, 0, 5, 3), (4, 2));
        assert_eq!(clamp_cursor_scroll(1, 5, 20, 4), (1, 1));
        assert_eq!(clamp_cursor_scroll(15, 2, 20, 5), (15, 11));
    }

    #[test]
    fn selection_relocation_prefers_pid_then_fallback_cursor() {
        let rows = vec![row(10), row(20), row(30), row(40)];
        assert_eq!(relocate_cursor_to_selected(Some(30), &rows, 0), 2);
        assert_eq!(relocate_cursor_to_selected(Some(999), &rows, 3), 3);
        assert_eq!(relocate_cursor_to_selected(None, &rows, 99), 3);
        assert_eq!(relocate_cursor_to_selected(Some(30), &[], 4), 0);
    }

    #[test]
    fn cycle_index_wraps_in_both_directions() {
        assert_eq!(cycle_index(0, 4, -1), 3);
        assert_eq!(cycle_index(3, 4, 1), 0);
        assert_eq!(cycle_index(1, 4, 2), 3);
        assert_eq!(cycle_index(1, 4, -2), 3);
    }

    #[test]
    fn cycle_mem_kind_uses_available_list() {
        let kinds = [MemKind::Rss, MemKind::Vsz, MemKind::Swap];
        assert_eq!(cycle_mem_kind_value(MemKind::Rss, 1, &kinds), MemKind::Vsz);
        assert_eq!(
            cycle_mem_kind_value(MemKind::Rss, -1, &kinds),
            MemKind::Swap
        );
        assert_eq!(
            cycle_mem_kind_value(MemKind::Footprint, 1, &kinds),
            MemKind::Vsz
        );
    }

    #[test]
    fn cycle_sort_keys_wraps() {
        assert_eq!(cycle_sort_key_value(SortBy::Aggregated, 1), SortBy::Own);
        assert_eq!(cycle_sort_key_value(SortBy::Comm, 1), SortBy::Aggregated);
        assert_eq!(cycle_sort_key_value(SortBy::Aggregated, -1), SortBy::Comm);
    }

    #[test]
    fn graph_symbol_setting_cycle_wraps() {
        assert_eq!(
            GraphSymbolSetting::Auto.cycle(1),
            GraphSymbolSetting::Braille
        );
        assert_eq!(GraphSymbolSetting::Auto.cycle(-1), GraphSymbolSetting::Tty);
        assert_eq!(
            GraphSymbolSetting::from_config("block"),
            GraphSymbolSetting::Block
        );
        assert_eq!(
            GraphSymbolSetting::from_config("unknown"),
            GraphSymbolSetting::Auto
        );
    }

    #[test]
    fn update_ms_is_clamped_so_deadline_cannot_overflow() {
        assert_eq!(clamp_update_ms(0), MIN_UPDATE_MS);
        assert_eq!(clamp_update_ms(u64::MAX), MAX_UPDATE_MS);
        let now = Instant::now();
        assert_eq!(
            next_tick_deadline(now, u64::MAX),
            now + Duration::from_millis(MAX_UPDATE_MS)
        );
    }

    #[test]
    fn ctrl_c_quits_from_every_mode() {
        for mode in [
            AppMode::Normal,
            AppMode::FilterInput,
            AppMode::Menu,
            AppMode::Options,
            AppMode::Help,
        ] {
            let mut app = App::new(1_000);
            app.mode = mode;
            app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
            assert!(app.should_quit, "Ctrl-C ignored in {mode:?}");
        }
    }

    #[test]
    fn menu_closes_on_q_without_quitting() {
        let mut app = App::new(1_000);
        app.mode = AppMode::Menu;
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert_eq!(app.mode, AppMode::Normal);
        assert!(!app.should_quit);
    }
}
