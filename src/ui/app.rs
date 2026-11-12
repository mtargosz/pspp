use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::time::{Duration, Instant};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use crate::collect::{self, Collector, Snapshot};
use crate::config;
use crate::model::{AggMode, AggregateRow, MemKind, ProcTree, SortBy, VisibleRow};
use crate::ui::clock;
use crate::ui::graph::{GraphSymbols, History};
use crate::ui::help;
use crate::ui::theme::{self, PaletteMode, Theme, TruecolorMode};

pub const DEFAULT_UPDATE_MS: u64 = 2_000;
pub const MIN_UPDATE_MS: u64 = 100;
/// btop's upper bound (24h); also keeps `Instant + Duration` from overflowing.
pub const MAX_UPDATE_MS: u64 = 86_400_000;

const MOUSE_SCROLL_STEP: isize = 3;
const DOUBLE_CLICK_MS: u64 = 350;
const MENU_BUTTONS: [MenuButton; 3] = [MenuButton::Options, MenuButton::Help, MenuButton::Quit];
const OPTION_KEYS: [OptionKey; 17] = [
    OptionKey::UpdateMs,
    OptionKey::MemKind,
    OptionKey::AggMode,
    OptionKey::GraphSymbol,
    OptionKey::Base10,
    OptionKey::SaveOnExit,
    OptionKey::SortKey,
    OptionKey::SortReverse,
    OptionKey::ColorTheme,
    OptionKey::ThemeBackground,
    OptionKey::Truecolor,
    OptionKey::RoundedCorners,
    OptionKey::VimKeys,
    OptionKey::DisableMouse,
    OptionKey::ClockFormat,
    OptionKey::BackgroundUpdate,
    OptionKey::ForceTty,
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

    pub fn resolve(self, force_tty: bool) -> GraphSymbols {
        if force_tty {
            return GraphSymbols::Tty;
        }
        match self {
            Self::Auto => GraphSymbols::detect(false),
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
    ColorTheme,
    ThemeBackground,
    Truecolor,
    RoundedCorners,
    VimKeys,
    DisableMouse,
    ClockFormat,
    BackgroundUpdate,
    ForceTty,
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
            Self::ColorTheme => "color_theme",
            Self::ThemeBackground => "theme_background",
            Self::Truecolor => "truecolor",
            Self::RoundedCorners => "rounded_corners",
            Self::VimKeys => "vim_keys",
            Self::DisableMouse => "disable_mouse",
            Self::ClockFormat => "clock_format",
            Self::BackgroundUpdate => "background_update",
            Self::ForceTty => "force_tty",
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
            Self::ColorTheme => {
                "Theme: Default or a btop .theme file from <config dir>/themes (←/→ cycles)."
            }
            Self::ThemeBackground => {
                "Paint the theme's background colour (off keeps the terminal's)."
            }
            Self::Truecolor => {
                "24-bit colour: auto follows COLORTERM; off uses the 256-colour palette."
            }
            Self::RoundedCorners => "Rounded (╭╮╰╯) or square box corners.",
            Self::VimKeys => {
                "j/k/l/g/G navigate like vim. Off makes them inert (h always opens Help)."
            }
            Self::DisableMouse => "Stop capturing the mouse so the terminal's own selection works.",
            Self::ClockFormat => {
                "strftime text shown in the top border, e.g. %H:%M:%S. Empty hides the clock."
            }
            Self::BackgroundUpdate => {
                "Keep refreshing while Menu/Options/Help is open (off pauses)."
            }
            Self::ForceTty => "Force the tty graph glyphs even when the terminal supports braille.",
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
    pub color_theme: String,
    pub theme_background: bool,
    pub truecolor: TruecolorMode,
    pub rounded_corners: bool,
    pub vim_keys: bool,
    pub disable_mouse: bool,
    pub clock_format: String,
    pub background_update: bool,
    pub force_tty: bool,
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
            color_theme: config::DEFAULT_THEME_NAME.to_owned(),
            theme_background: true,
            truecolor: TruecolorMode::Auto,
            rounded_corners: true,
            vim_keys: true,
            disable_mouse: false,
            clock_format: String::new(),
            background_update: true,
            force_tty: false,
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

#[derive(Clone, Debug)]
struct FilterSnapshot {
    filter_committed: String,
    expanded: HashSet<i32>,
    selected_pid: Option<i32>,
}

pub struct App {
    /// The process forest; it owns the latest snapshot's records and system memory figures.
    pub tree: ProcTree,
    /// Background snapshot worker; `None` for apps built around injected snapshots (tests).
    collector: Option<Collector>,
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
    pub color_theme: String,
    pub theme_background: bool,
    pub truecolor: TruecolorMode,
    pub rounded_corners: bool,
    pub vim_keys: bool,
    pub disable_mouse: bool,
    pub clock_format: String,
    pub background_update: bool,
    pub force_tty: bool,
    pub(crate) no_color: bool,
    pub mode: AppMode,
    pub should_quit: bool,
    pub last_tick_at: Instant,
    pub next_tick_at: Instant,

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
    last_click: Option<(i32, Instant)>,
    header_hits: Vec<(Rect, SortBy)>,

    menu_selected: MenuButton,
    menu_overlay_rect: Option<Rect>,
    menu_button_rects: [Option<Rect>; 3],

    option_selected: usize,
    option_scroll: usize,
    update_ms_editor: Option<String>,
    clock_editor: Option<String>,
    overlay_parent: AppMode,
    option_overlay_rect: Option<Rect>,
    option_row_rects: Vec<(Rect, usize)>,
    help_overlay_rect: Option<Rect>,

    help_scroll: usize,
    help_content_lines: usize,
    help_viewport_lines: usize,

    filter_input: String,
    filter_committed: String,
    filter_snapshot: Option<FilterSnapshot>,
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

    /// Takes the first snapshot synchronously (so the first frame is populated), then hands
    /// all further collection to a worker thread.
    pub fn from_settings(settings: UiSettings) -> Self {
        let first = collect::take_snapshot(settings.mem_kind);
        let mut app = Self::with_snapshot(first, settings);
        if theme::no_color_from_env(std::env::var("NO_COLOR").ok().as_deref()) {
            app.set_no_color(true);
        }
        app.collector = Some(Collector::spawn());
        app
    }

    /// Builds the app around an existing snapshot (deterministic tests, injected data).
    pub(crate) fn with_snapshot(snapshot: Snapshot, settings: UiSettings) -> Self {
        let update_ms = clamp_update_ms(settings.update_ms);
        let tree = ProcTree::build(snapshot);
        let now = Instant::now();
        let expanded: HashSet<i32> = tree.procs().map(|proc_rec| proc_rec.pid).collect();

        let mut app = Self {
            tree,
            collector: None,
            agg_mode: settings.agg_mode,
            mem_kind: settings.mem_kind,
            sort_by: settings.sort_by,
            sort_desc: settings.sort_reverse,
            update_ms,
            base10: settings.base10,
            save_on_exit: settings.save_on_exit,
            theme: Theme::new(false),
            color_theme: settings.color_theme.clone(),
            theme_background: settings.theme_background,
            truecolor: settings.truecolor,
            rounded_corners: settings.rounded_corners,
            vim_keys: settings.vim_keys,
            disable_mouse: settings.disable_mouse,
            clock_format: settings.clock_format.clone(),
            background_update: settings.background_update,
            force_tty: settings.force_tty,
            no_color: false,
            mem_history: History::default(),
            graph_symbol: settings.graph_symbol,
            mode: AppMode::Normal,
            should_quit: false,
            last_tick_at: now,
            next_tick_at: next_tick_deadline(now, update_ms),
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
            header_hits: Vec::new(),
            menu_selected: MenuButton::Options,
            menu_overlay_rect: None,
            menu_button_rects: [None, None, None],
            option_selected: 0,
            option_scroll: 0,
            update_ms_editor: None,
            clock_editor: None,
            overlay_parent: AppMode::Menu,
            option_overlay_rect: None,
            option_row_rects: Vec::new(),
            help_overlay_rect: None,
            help_scroll: 0,
            help_content_lines: 0,
            help_viewport_lines: 1,
            filter_input: String::new(),
            filter_committed: String::new(),
            filter_snapshot: None,
            filter_visible: HashSet::new(),
            filter_direct: HashSet::new(),
        };
        app.rebuild_theme();
        app.record_mem_sample();
        app.rebuild_rows();
        app
    }

    pub(crate) fn set_no_color(&mut self, no_color: bool) {
        self.no_color = no_color;
        self.rebuild_theme();
    }

    #[cfg(test)]
    pub(crate) fn set_collector(&mut self, collector: Collector) {
        self.collector = Some(collector);
    }

    /// Rebuilds the colour theme from `no_color`, `force_tty`, `color_theme`, `truecolor` and
    /// `theme_background`.
    fn rebuild_theme(&mut self) {
        self.rebuild_theme_in(config::themes_dir().as_deref());
    }

    fn rebuild_theme_in(&mut self, dir: Option<&std::path::Path>) {
        let mode = if self.no_color {
            PaletteMode::NoColor
        } else if self.force_tty {
            PaletteMode::Ansi16
        } else if self
            .truecolor
            .resolve(std::env::var("COLORTERM").ok().as_deref())
        {
            PaletteMode::Truecolor
        } else {
            PaletteMode::Indexed256
        };
        let spec = match mode {
            PaletteMode::Truecolor | PaletteMode::Indexed256 => {
                theme::load_theme_spec(dir, &self.color_theme)
            }
            PaletteMode::Ansi16 | PaletteMode::NoColor => None,
        };
        self.theme = Theme::build_with_mode(mode, self.theme_background, spec.as_ref());
    }

    pub fn border_type(&self) -> ratatui::widgets::BorderType {
        if self.force_tty || self.no_color || !self.rounded_corners {
            ratatui::widgets::BorderType::Plain
        } else {
            ratatui::widgets::BorderType::Rounded
        }
    }

    /// Clock text for the top border, or `None` when disabled/invalid.
    pub fn clock_text(&self) -> Option<String> {
        clock::format_clock_now(&self.clock_format)
    }

    /// Whether the terminal should capture mouse events.
    pub fn mouse_enabled(&self) -> bool {
        !self.disable_mouse
    }

    /// Ticks pause while an overlay is open when `background_update` is off.
    fn ticks_paused(&self) -> bool {
        !self.background_update
            && matches!(self.mode, AppMode::Menu | AppMode::Options | AppMode::Help)
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
            color_theme: self.color_theme.clone(),
            theme_background: self.theme_background,
            truecolor: self.truecolor,
            rounded_corners: self.rounded_corners,
            vim_keys: self.vim_keys,
            disable_mouse: self.disable_mouse,
            clock_format: self.clock_format.clone(),
            background_update: self.background_update,
            force_tty: self.force_tty,
        }
    }

    pub fn resolved_graph_symbols(&self) -> GraphSymbols {
        if self.no_color {
            GraphSymbols::Tty
        } else {
            self.graph_symbol.resolve(self.force_tty)
        }
    }

    /// Whether meters should use ASCII `'#'`/`'-'` glyphs instead of `'■'`.
    pub fn ascii_meters(&self) -> bool {
        self.resolved_graph_symbols() == GraphSymbols::Tty || self.no_color
    }

    pub fn sort_by(&self) -> SortBy {
        self.sort_by
    }

    pub fn sort_desc(&self) -> bool {
        self.sort_desc
    }

    /// Σ of the selected memory kind over all processes. This can exceed physical RAM (shared
    /// pages are counted once per process; footprint includes compressed memory), so it is shown
    /// as a plain figure and never used as a graph or percentage scale.
    pub fn mem_kind_sum(&self) -> u64 {
        self.tree
            .procs()
            .filter_map(|proc_rec| self.mem_kind.get(&proc_rec.mem))
            .fold(0_u64, u64::saturating_add)
    }

    /// System-wide memory in use (what the history graph plots).
    pub fn mem_used(&self) -> u64 {
        self.tree.used_mem()
    }

    /// Graph scale: total physical memory.
    pub fn mem_graph_max(&self) -> u64 {
        self.tree.total_mem()
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

    /// First option shown in the (scrolling) options list, adjusted so the selection is visible.
    pub fn option_scroll_for(&mut self, visible_rows: usize) -> usize {
        let visible = visible_rows.max(1);
        if self.option_selected < self.option_scroll {
            self.option_scroll = self.option_selected;
        } else if self.option_selected >= self.option_scroll + visible {
            self.option_scroll = self.option_selected + 1 - visible;
        }
        self.option_scroll = self
            .option_scroll
            .min(OPTION_KEYS.len().saturating_sub(visible));
        self.option_scroll
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

    pub fn active_filter(&self) -> &str {
        if self.mode == AppMode::FilterInput {
            &self.filter_input
        } else {
            &self.filter_committed
        }
    }

    pub fn is_filter_active(&self) -> bool {
        !self.active_filter().is_empty()
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
            OptionKey::MemKind => self.mem_kind.to_string(),
            OptionKey::AggMode => self.agg_mode.to_string(),
            OptionKey::GraphSymbol => self.graph_symbol.label().to_owned(),
            OptionKey::Base10 => bool_label(self.base10).to_owned(),
            OptionKey::SaveOnExit => bool_label(self.save_on_exit).to_owned(),
            OptionKey::SortKey => self.sort_by.to_string(),
            OptionKey::SortReverse => bool_label(self.sort_desc).to_owned(),
            OptionKey::ColorTheme => self.color_theme.clone(),
            OptionKey::ThemeBackground => bool_label(self.theme_background).to_owned(),
            OptionKey::Truecolor => self.truecolor.as_config().to_owned(),
            OptionKey::RoundedCorners => bool_label(self.rounded_corners).to_owned(),
            OptionKey::VimKeys => bool_label(self.vim_keys).to_owned(),
            OptionKey::DisableMouse => bool_label(self.disable_mouse).to_owned(),
            OptionKey::ClockFormat => match &self.clock_editor {
                Some(buffer) if self.selected_option_key() == OptionKey::ClockFormat => {
                    format!("{buffer}_")
                }
                _ if self.clock_format.is_empty() => "(off)".to_owned(),
                _ => self.clock_format.clone(),
            },
            OptionKey::BackgroundUpdate => bool_label(self.background_update).to_owned(),
            OptionKey::ForceTty => bool_label(self.force_tty).to_owned(),
        }
    }

    fn record_mem_sample(&mut self) {
        self.mem_history.push(self.mem_used());
    }

    pub fn poll_timeout(&self, now: Instant) -> Duration {
        let tick_wait = if self.ticks_paused() {
            // Paused: still wake periodically so the clock and redraws stay live.
            Duration::from_millis(250)
        } else {
            remaining_until_tick(now, self.next_tick_at)
        };
        let mut wait = if clock::needs_second_resolution(&self.clock_format) {
            tick_wait.min(Duration::from_secs(1))
        } else {
            tick_wait
        };
        // While the worker is collecting, wake often enough to pick the result up promptly.
        if self.collector.as_ref().is_some_and(Collector::is_busy) {
            wait = wait.min(Duration::from_millis(50));
        }
        wait
    }

    pub fn should_tick(&self, now: Instant) -> bool {
        !self.ticks_paused() && now >= self.next_tick_at
    }

    /// Schedules the next tick and asks the worker for a fresh snapshot; never blocks on I/O.
    pub fn on_tick(&mut self) {
        self.last_tick_at = Instant::now();
        self.next_tick_at = next_tick_deadline(self.last_tick_at, self.update_ms);
        self.request_refresh();
    }

    fn request_refresh(&mut self) {
        let kind = self.mem_kind;
        if let Some(collector) = self.collector.as_mut() {
            collector.request(kind);
        }
    }

    /// Applies a finished background snapshot, if any. Called every loop iteration.
    /// Returns `true` when a snapshot was applied.
    pub fn poll_snapshot(&mut self) -> bool {
        let Some((kind, snapshot)) = self.collector.as_mut().and_then(Collector::try_take) else {
            return false;
        };
        self.apply_snapshot(snapshot);
        // The metric changed while this snapshot was being collected: fetch the right one.
        if kind != self.mem_kind {
            self.request_refresh();
        }
        true
    }

    pub(crate) fn apply_snapshot(&mut self, snapshot: Snapshot) {
        let old_pids: HashSet<i32> = self.tree.procs().map(|proc_rec| proc_rec.pid).collect();
        self.tree = ProcTree::build(snapshot);
        let new_pids: HashSet<i32> = self.tree.procs().map(|proc_rec| proc_rec.pid).collect();
        self.expanded = remap_expanded_set(&self.expanded, &old_pids, &new_pids);
        if let Some(filter_snap) = self.filter_snapshot.as_mut() {
            filter_snap.expanded = remap_expanded_set(&filter_snap.expanded, &old_pids, &new_pids);
        }
        if self.mode == AppMode::FilterInput && !self.filter_input.is_empty() {
            let filter = self.filter_input.clone();
            self.expand_ancestors_for_filter(&filter);
        }

        self.record_mem_sample();
        self.rebuild_rows();
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

        // F1/F2 open Help/Options from anywhere except while typing text.
        if !self.is_text_input_active() {
            match key_event.code {
                KeyCode::F(1) => return self.open_overlay(AppMode::Help),
                KeyCode::F(2) => return self.open_overlay(AppMode::Options),
                _ => {}
            }
        }

        if !self.vim_keys
            && !self.is_text_input_active()
            && !key_event
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && matches!(key_event.code, KeyCode::Char('j' | 'k' | 'l' | 'g' | 'G'))
        {
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

    pub fn on_mouse(&mut self, mouse_event: MouseEvent) -> bool {
        if self.disable_mouse
            || matches!(
                mouse_event.kind,
                MouseEventKind::Moved
                    | MouseEventKind::Drag(_)
                    | MouseEventKind::Up(_)
                    | MouseEventKind::ScrollLeft
                    | MouseEventKind::ScrollRight
            )
        {
            return false;
        }

        match self.mode {
            AppMode::Normal => match mouse_event.kind {
                MouseEventKind::ScrollUp => {
                    self.move_cursor_by(-MOUSE_SCROLL_STEP);
                    true
                }
                MouseEventKind::ScrollDown => {
                    self.move_cursor_by(MOUSE_SCROLL_STEP);
                    true
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if !self.handle_header_click(mouse_event.column, mouse_event.row) {
                        self.handle_left_click(mouse_event.column, mouse_event.row);
                    }
                    true
                }
                _ => false,
            },
            AppMode::FilterInput => false,
            AppMode::Menu => match mouse_event.kind {
                MouseEventKind::ScrollUp => {
                    self.move_menu_selection(-1);
                    true
                }
                MouseEventKind::ScrollDown => {
                    self.move_menu_selection(1);
                    true
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    self.handle_menu_click(mouse_event.column, mouse_event.row);
                    true
                }
                _ => false,
            },
            AppMode::Options => self.on_mouse_options(mouse_event),
            AppMode::Help => self.on_mouse_help(mouse_event),
        }
    }

    fn on_mouse_options(&mut self, mouse_event: MouseEvent) -> bool {
        match mouse_event.kind {
            MouseEventKind::ScrollUp => {
                self.step_option_selection_clamped(-1);
                true
            }
            MouseEventKind::ScrollDown => {
                self.step_option_selection_clamped(1);
                true
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // Any click abandons an in-progress update_ms edit.
                self.update_ms_editor = None;
                self.clock_editor = None;
                let (column, row) = (mouse_event.column, mouse_event.row);
                let Some(overlay) = self.option_overlay_rect else {
                    return true;
                };
                if !point_in_rect(overlay, column, row) {
                    self.mode = AppMode::Normal;
                    return true;
                }
                let hit = self
                    .option_row_rects
                    .iter()
                    .find(|(rect, _)| point_in_rect(*rect, column, row))
                    .map(|(_, idx)| *idx);
                if let Some(idx) = hit {
                    if idx == self.option_selected {
                        self.activate_selected_option();
                    } else {
                        self.option_selected = idx;
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn on_mouse_help(&mut self, mouse_event: MouseEvent) -> bool {
        match mouse_event.kind {
            MouseEventKind::ScrollUp => {
                self.move_help_scroll_by(-MOUSE_SCROLL_STEP);
                true
            }
            MouseEventKind::ScrollDown => {
                self.move_help_scroll_by(MOUSE_SCROLL_STEP);
                true
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(overlay) = self.help_overlay_rect {
                    if !point_in_rect(overlay, mouse_event.column, mouse_event.row) {
                        self.mode = AppMode::Normal;
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn step_option_selection_clamped(&mut self, delta: isize) {
        let max = OPTION_KEYS.len().saturating_sub(1) as isize;
        self.option_selected = (self.option_selected as isize + delta).clamp(0, max) as usize;
    }

    /// Enter / click-on-selected: edit `update_ms`, otherwise advance the value.
    fn activate_selected_option(&mut self) {
        match self.selected_option_key() {
            OptionKey::UpdateMs => self.update_ms_editor = Some(self.update_ms.to_string()),
            OptionKey::ClockFormat => self.clock_editor = Some(self.clock_format.clone()),
            _ => self.adjust_selected_option(1),
        }
    }

    pub fn set_options_hit_test(&mut self, overlay: Rect, rows: Vec<(Rect, usize)>) {
        self.option_overlay_rect = Some(overlay);
        self.option_row_rects = rows;
    }

    pub fn set_help_hit_test(&mut self, overlay: Rect) {
        self.help_overlay_rect = Some(overlay);
    }

    /// Forgets every overlay's hit-test rects (called when no overlay is drawn).
    pub fn clear_overlay_hit_tests(&mut self) {
        self.clear_menu_hit_test();
        self.option_overlay_rect = None;
        self.option_row_rects.clear();
        self.help_overlay_rect = None;
    }

    /// Number of rows scrolled out of view in the help overlay (for the scrollbar).
    pub fn help_overflow(&self) -> usize {
        self.help_content_lines
            .saturating_sub(self.help_viewport_lines)
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
        self.header_hits.clear();
    }

    /// Column-header click targets recorded by `draw` (sortable columns only).
    pub fn set_header_hit_test(&mut self, hits: Vec<(Rect, SortBy)>) {
        self.header_hits = hits;
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

    /// True while a text field (filter or an options editor) is consuming characters.
    fn is_text_input_active(&self) -> bool {
        self.mode == AppMode::FilterInput
            || self.update_ms_editor.is_some()
            || self.clock_editor.is_some()
    }

    /// Opens Help/Options directly (global keys, menu buttons). Esc returns to where it came from.
    fn open_overlay(&mut self, target: AppMode) {
        self.update_ms_editor = None;
        self.clock_editor = None;
        self.overlay_parent = if self.mode == AppMode::Menu {
            AppMode::Menu
        } else {
            AppMode::Normal
        };
        if target == AppMode::Help {
            self.help_scroll = 0;
        }
        self.mode = target;
    }

    fn close_overlay(&mut self) {
        self.mode = self.overlay_parent;
        self.overlay_parent = AppMode::Menu;
    }

    fn on_key_normal(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Esc => self.open_menu(),
            KeyCode::Char('h' | 'H' | '?') => self.open_overlay(AppMode::Help),
            KeyCode::Char('o' | 'O') => self.open_overlay(AppMode::Options),
            KeyCode::Char('+' | '=') => self.set_update_ms(self.update_ms.saturating_add(100)),
            KeyCode::Char('-' | '_') => self.set_update_ms(self.update_ms.saturating_sub(100)),
            KeyCode::Char('l') => self.expand_selected(),
            KeyCode::Char('z' | '[')
                if !key_event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.collapse_all();
            }
            KeyCode::Char('Z' | ']')
                if !key_event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.expand_all();
            }
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
                // btop-style: abandon the edit and restore the pre-filter snapshot.
                if let Some(snapshot) = self.filter_snapshot.take() {
                    self.filter_committed = snapshot.filter_committed;
                    self.expanded = snapshot.expanded;
                    self.selected_pid = snapshot.selected_pid;
                }
                self.filter_input = self.filter_committed.clone();
                self.mode = AppMode::Normal;
                self.rebuild_rows();
            }
            KeyCode::Enter => {
                self.filter_committed = self.filter_input.clone();
                self.filter_snapshot = None;
                self.mode = AppMode::Normal;
                let filter = self.filter_committed.clone();
                self.expand_ancestors_for_filter(&filter);
                self.rebuild_rows();
            }
            KeyCode::Backspace => {
                self.filter_input.pop();
                self.apply_live_filter_edit();
            }
            KeyCode::Delete => {
                self.filter_input.clear();
                self.apply_live_filter_edit();
            }
            KeyCode::Char('u' | 'U') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter_input.clear();
                self.apply_live_filter_edit();
            }
            KeyCode::Char('w' | 'W') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                delete_trailing_word(&mut self.filter_input);
                self.apply_live_filter_edit();
            }
            KeyCode::Char(ch)
                if !key_event.modifiers.contains(KeyModifiers::CONTROL)
                    && !key_event.modifiers.contains(KeyModifiers::ALT)
                    && !ch.is_control() =>
            {
                self.filter_input.push(ch);
                self.apply_live_filter_edit();
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
        if self.clock_editor.is_some() {
            self.on_key_options_editing_clock(key_event);
            return;
        }

        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => self.close_overlay(),
            KeyCode::Up | KeyCode::Char('k') => self.move_option_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_option_selection(1),
            KeyCode::Left => self.adjust_selected_option(-1),
            KeyCode::Right => self.adjust_selected_option(1),
            KeyCode::Char('h')
                if self.vim_keys
                    && !self.is_text_input_active()
                    && !key_event
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.adjust_selected_option(-1);
            }
            KeyCode::Char('l')
                if self.vim_keys
                    && !self.is_text_input_active()
                    && !key_event
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.adjust_selected_option(1);
            }
            KeyCode::Enter => self.activate_selected_option(),
            _ => {}
        }
    }

    fn on_key_options_editing_clock(&mut self, key_event: KeyEvent) {
        let Some(editor) = self.clock_editor.as_mut() else {
            return;
        };

        match key_event.code {
            KeyCode::Esc => self.clock_editor = None,
            KeyCode::Enter => {
                if let Some(text) = self.clock_editor.take() {
                    self.clock_format = text;
                }
            }
            KeyCode::Backspace => {
                editor.pop();
            }
            KeyCode::Char(ch)
                if !key_event.modifiers.contains(KeyModifiers::CONTROL)
                    && !ch.is_control()
                    && editor.chars().count() < 40 =>
            {
                editor.push(ch);
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
                self.clock_editor = None;
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
            KeyCode::Home => self.help_scroll = 0,
            KeyCode::End => {
                self.help_scroll = if self.help_content_lines == 0 {
                    usize::MAX
                } else {
                    self.help_overflow()
                };
            }
            KeyCode::Char('g') if self.vim_keys => self.help_scroll = 0,
            KeyCode::Char('G') if self.vim_keys => {
                self.help_scroll = if self.help_content_lines == 0 {
                    usize::MAX
                } else {
                    self.help_overflow()
                };
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => self.close_overlay(),
            _ => {}
        }
    }

    fn open_menu(&mut self) {
        self.mode = AppMode::Menu;
        self.update_ms_editor = None;
        self.clock_editor = None;
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
            MenuButton::Options => self.open_overlay(AppMode::Options),
            MenuButton::Help => self.open_overlay(AppMode::Help),
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
            OptionKey::SortKey => {
                self.sort_by = cycle_sort_key_value(self.sort_by, delta);
                self.rebuild_rows();
            }
            OptionKey::SortReverse => self.reverse_sort(),
            OptionKey::ColorTheme => self.cycle_color_theme(delta),
            OptionKey::ThemeBackground => {
                self.theme_background = !self.theme_background;
                self.rebuild_theme();
            }
            OptionKey::Truecolor => {
                self.truecolor = self.truecolor.cycle(delta);
                self.rebuild_theme();
            }
            OptionKey::RoundedCorners => self.rounded_corners = !self.rounded_corners,
            OptionKey::VimKeys => self.vim_keys = !self.vim_keys,
            OptionKey::DisableMouse => self.disable_mouse = !self.disable_mouse,
            // Free text: only editable via Enter/click, never cycled.
            OptionKey::ClockFormat => {}
            OptionKey::BackgroundUpdate => self.background_update = !self.background_update,
            OptionKey::ForceTty => {
                self.force_tty = !self.force_tty;
                self.rebuild_theme();
            }
        }
    }

    fn cycle_color_theme(&mut self, delta: isize) {
        self.cycle_color_theme_in(config::themes_dir().as_deref(), delta);
    }

    fn cycle_color_theme_in(&mut self, dir: Option<&std::path::Path>, delta: isize) {
        let mut names = theme::discover_themes(dir);
        // Keep a configured-but-missing theme selectable so cycling never loses the position.
        if !names.contains(&self.color_theme) {
            names.push(self.color_theme.clone());
        }
        let current = names
            .iter()
            .position(|name| *name == self.color_theme)
            .unwrap_or(0);
        self.color_theme = names[cycle_index(current, names.len(), delta)].clone();
        self.rebuild_theme_in(dir);
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
        let filter = self.active_filter();
        if filter.is_empty() {
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
            let match_set = self.tree.filter_match_set(filter);
            self.filter_visible = match_set.visible;
            self.filter_direct = match_set.direct;
            // The real `expanded` set applies while filtering; matches' ancestors were expanded
            // when the filter was edited/committed, so matches start visible but can still be collapsed.
            let visible_all = self.tree.flatten_visible_sorted(
                &self.expanded,
                &aggregates,
                self.sort_by,
                self.sort_desc,
            );
            let mut visible: Vec<VisibleRow> = visible_all
                .into_iter()
                .filter(|row| self.filter_visible.contains(&row.pid))
                .collect();
            // Hidden siblings must not leave dangling `├─` glyphs behind.
            relabel_last_children(&mut visible);
            self.rows = build_display_rows(&self.tree, &aggregates, &visible, &self.expanded);
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

    fn collapse_all(&mut self) {
        let roots_with_children: HashSet<i32> = self
            .tree
            .root_children()
            .iter()
            .copied()
            .filter(|&pid| !self.tree.child_pids(pid).is_empty())
            .collect();

        let has_expanded_visible_non_root = self.rows.iter().any(|row| {
            self.tree.parent_pid(row.pid).is_some()
                && !self.tree.child_pids(row.pid).is_empty()
                && self.expanded.contains(&row.pid)
        });
        if has_expanded_visible_non_root {
            self.expanded
                .retain(|pid| roots_with_children.contains(pid));
        } else {
            self.expanded.clear();
        }

        if let Some(pid) = self.selected_pid {
            self.selected_pid = Some(self.nearest_visible_ancestor(pid));
        }
        self.rebuild_rows();
    }

    fn expand_all(&mut self) {
        self.expanded
            .extend(self.tree.procs().map(|proc_rec| proc_rec.pid));
        self.rebuild_rows();
    }

    fn nearest_visible_ancestor(&self, pid: i32) -> i32 {
        let mut chain = vec![pid];
        let hop_limit = self.tree.proc_count();
        let mut current = self.tree.parent_pid(pid);
        while let Some(parent) = current {
            if chain.len() > hop_limit {
                break;
            }
            chain.push(parent);
            current = self.tree.parent_pid(parent);
        }

        let mut deepest_visible = *chain.last().unwrap_or(&pid);
        for window in chain.windows(2).rev() {
            let child = window[0];
            let parent = window[1];
            if !self.expanded.contains(&parent) {
                break;
            }
            if self.is_filter_active() && !self.filter_visible.contains(&child) {
                break;
            }
            deepest_visible = child;
        }
        deepest_visible
    }

    fn toggle_agg_mode(&mut self) {
        self.agg_mode = self.agg_mode.toggled();
        self.rebuild_rows();
    }

    fn cycle_mem_kind(&mut self, delta: isize) {
        self.mem_kind = cycle_mem_kind_value(self.mem_kind, delta, MemKind::available());
        // Platforms that read metric-specific data lazily need a fresh snapshot for the new kind.
        self.request_refresh();
        self.rebuild_rows();
    }

    fn cycle_sort_key(&mut self, delta: isize) {
        self.sort_by = cycle_sort_key_value(self.sort_by, delta);
        self.sort_desc = self.sort_by.natural_desc();
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

    /// Clicking a sortable column header sorts by it; clicking the active column reverses.
    /// Returns whether the click landed on a header.
    fn handle_header_click(&mut self, column: u16, row: u16) -> bool {
        let Some(key) = self
            .header_hits
            .iter()
            .find(|(rect, _)| point_in_rect(*rect, column, row))
            .map(|(_, key)| *key)
        else {
            return false;
        };

        self.last_click = None;
        if key == self.sort_by {
            self.reverse_sort();
        } else {
            self.sort_by = key;
            // Text and PID columns read naturally ascending; memory columns largest-first.
            self.sort_desc = key.natural_desc();
            self.rebuild_rows();
        }
        true
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
        let clicked_pid = self.selected_pid;
        let double_click = self
            .last_click
            .map(|(last_pid, last_at)| {
                Some(last_pid) == clicked_pid
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

        self.last_click = clicked_pid.map(|pid| (pid, now));
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
        self.filter_snapshot = Some(FilterSnapshot {
            filter_committed: self.filter_committed.clone(),
            expanded: self.expanded.clone(),
            selected_pid: self.selected_pid,
        });
        self.filter_input = self.filter_committed.clone();
        self.mode = AppMode::FilterInput;
    }

    fn apply_live_filter_edit(&mut self) {
        if let Some(snapshot) = &self.filter_snapshot {
            self.expanded = snapshot.expanded.clone();
            self.selected_pid = snapshot.selected_pid;
        }
        if !self.filter_input.is_empty() {
            let filter = self.filter_input.clone();
            self.expand_ancestors_for_filter(&filter);
        }
        self.rebuild_rows();
    }

    /// Expands every ancestor of a direct match for `filter` so matches are visible.
    fn expand_ancestors_for_filter(&mut self, filter: &str) {
        if filter.is_empty() {
            return;
        }
        let direct = self.tree.filter_match_set(filter).direct;
        let hop_limit = self.tree.proc_count();
        for pid in direct {
            let mut current = self.tree.parent_pid(pid);
            let mut hops = 0;
            while let Some(parent) = current {
                self.expanded.insert(parent);
                hops += 1;
                if hops > hop_limit {
                    break;
                }
                current = self.tree.parent_pid(parent);
            }
        }
    }

    /// Pre-sets the committed process filter (CLI `-f/--filter`).
    pub fn set_filter(&mut self, text: &str) {
        self.filter_input = text.to_owned();
        self.filter_committed = text.to_owned();
        self.filter_snapshot = None;
        let filter = self.filter_committed.clone();
        self.expand_ancestors_for_filter(&filter);
        self.rebuild_rows();
    }

    fn clear_filter(&mut self) {
        self.filter_input.clear();
        self.filter_committed.clear();
        self.filter_snapshot = None;
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
    let idx = SortBy::ALL
        .iter()
        .position(|sort_by| *sort_by == current)
        .unwrap_or(0);
    SortBy::ALL[cycle_index(idx, SortBy::ALL.len(), delta)]
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

/// Recomputes `is_last_child` among the siblings that are actually present in `rows`
/// (depth-first order), e.g. after a filter removed later siblings.
fn relabel_last_children(rows: &mut [VisibleRow]) {
    let mut seen_at_depth: Vec<bool> = Vec::new();
    for row in rows.iter_mut().rev() {
        if seen_at_depth.len() <= row.depth {
            seen_at_depth.resize(row.depth + 1, false);
        }
        // Rows deeper than this one (earlier in order) belong to a different parent's subtree.
        seen_at_depth.truncate(row.depth + 1);
        row.is_last_child = !seen_at_depth[row.depth];
        seen_at_depth[row.depth] = true;
    }
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

        // Depth-0 roots have no `├─`/`└─` connector column, so continuation bars only inspect
        // ancestors at depths >= 1.
        let ancestor_slice = if visible_row.depth > 1 && ancestor_last.len() >= visible_row.depth {
            ancestor_last.get(1..visible_row.depth).unwrap_or(&[])
        } else {
            &[]
        };
        let (prefix, marker) = build_tree_prefix(
            visible_row.depth,
            visible_row.is_last_child,
            visible_row.has_children,
            expanded.contains(&visible_row.pid),
            ancestor_slice,
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
        let start = UnicodeWidthStr::width(prefix.as_str());
        prefix.push_str(marker_text);
        let end = UnicodeWidthStr::width(prefix.as_str());
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

fn delete_trailing_word(text: &mut String) {
    while text.chars().next_back().is_some_and(char::is_whitespace) {
        text.pop();
    }
    while text
        .chars()
        .next_back()
        .is_some_and(|ch| !ch.is_whitespace())
    {
        text.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_tree_prefix, clamp_cursor_scroll, clamp_update_ms, cycle_index, cycle_mem_kind_value,
        cycle_sort_key_value, next_tick_deadline, relocate_cursor_to_selected, App, AppMode,
        DisplayRow, GraphSymbolSetting, OptionKey, MAX_UPDATE_MS, MIN_UPDATE_MS, OPTION_KEYS,
    };
    use crate::collect::{MemSet, ProcRec, Snapshot};
    use crate::model::{MemKind, SortBy};
    use crate::ui::app::UiSettings;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::layout::Rect;
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

    fn synthetic_app(procs: &[(i32, i32, &str, u64)]) -> App {
        let procs = procs
            .iter()
            .map(|&(pid, ppid, comm, rss)| ProcRec {
                pid,
                ppid,
                comm: comm.to_owned(),
                mem: MemSet {
                    rss: Some(rss),
                    vsz: Some(rss),
                    ..MemSet::default()
                },
            })
            .collect();
        let snapshot = Snapshot {
            procs,
            total_mem: 1_000_000,
            used_mem: 500_000,
        };
        App::with_snapshot(snapshot, UiSettings::default())
    }

    fn pids(app: &App) -> Vec<i32> {
        app.rows().iter().map(|row| row.pid).collect()
    }

    fn press(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn type_text(app: &mut App, text: &str) {
        for ch in text.chars() {
            press(app, KeyCode::Char(ch));
        }
    }

    // 1 launchd -> 2 shell -> 3 target ; 1 -> 4 other ; 5 stray root
    fn filter_app() -> App {
        synthetic_app(&[
            (1, 0, "launchd", 10),
            (2, 1, "shell", 20),
            (3, 2, "target", 30),
            (4, 1, "other", 40),
            (5, 0, "stray", 50),
        ])
    }

    fn click(app: &mut App, column: u16, row: u16) {
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
    }

    #[test]
    fn committing_a_filter_expands_ancestors_of_matches() {
        let mut app = filter_app();
        app.expanded.clear(); // everything collapsed
        press(&mut app, KeyCode::Char('f'));
        type_text(&mut app, "TARGET");
        press(&mut app, KeyCode::Enter);

        assert_eq!(pids(&app), vec![1, 2, 3]);
        assert!(app.is_direct_filter_match(3));
        assert!(app.is_context_filter_match(2));
    }

    #[test]
    fn filtered_view_honours_collapse_and_toggle_keys() {
        let mut app = filter_app();
        app.set_filter("target");
        assert_eq!(pids(&app), vec![1, 2, 3]);

        // Select `shell` (row 1) and collapse it with ←: the match underneath disappears.
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected_pid, Some(2));
        press(&mut app, KeyCode::Left);
        assert_eq!(pids(&app), vec![1, 2]);

        // Space/Enter and → re-expand while the filter is still active.
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(pids(&app), vec![1, 2, 3]);
        press(&mut app, KeyCode::Enter);
        assert_eq!(pids(&app), vec![1, 2]);
        press(&mut app, KeyCode::Right);
        assert_eq!(pids(&app), vec![1, 2, 3]);
        assert!(app.is_filter_active());
    }

    #[test]
    fn esc_in_filter_input_restores_the_committed_filter() {
        let mut app = filter_app();
        app.set_filter("other");
        press(&mut app, KeyCode::Char('f'));
        assert_eq!(app.mode, AppMode::FilterInput);
        type_text(&mut app, "xyz");
        press(&mut app, KeyCode::Esc);

        assert_eq!(app.mode, AppMode::Normal);
        assert_eq!(app.filter_indicator().as_deref(), Some("filter: other"));
        assert_eq!(pids(&app), vec![1, 4]);

        // `c` clears the filter from Normal mode.
        press(&mut app, KeyCode::Char('c'));
        assert!(!app.is_filter_active());
        assert_eq!(pids(&app), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn esc_in_filter_input_without_a_committed_filter_stays_unfiltered() {
        let mut app = filter_app();
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "zzz");
        press(&mut app, KeyCode::Esc);
        assert!(!app.is_filter_active());
        assert_eq!(pids(&app).len(), 5);
    }

    fn arm_table_hit_test(app: &mut App) {
        app.set_table_hit_test(Rect::new(0, 1, 80, 10), 0, 0, vec![None; 10]);
    }

    #[test]
    fn double_click_is_keyed_by_pid_not_row_index() {
        let mut app = filter_app();
        arm_table_hit_test(&mut app);
        let before = pids(&app);
        assert_eq!(before[0], 1);

        // A click on pid 1 was recorded; a different row now sits at its old index.
        app.last_click = Some((999, Instant::now()));
        click(&mut app, 40, 1); // row 0 = launchd
        assert!(app.expanded.contains(&1), "stale pid must not double-click");
        assert_eq!(app.last_click.map(|(pid, _)| pid), Some(1));

        // Second click on the same pid within the window toggles it.
        click(&mut app, 40, 1);
        assert!(!app.expanded.contains(&1));
    }

    #[test]
    fn header_click_sorts_and_clicking_active_column_reverses() {
        let mut app = filter_app();
        app.set_header_hit_test(vec![
            (Rect::new(0, 0, 20, 1), SortBy::Comm),
            (Rect::new(21, 0, 6, 1), SortBy::Pid),
            (Rect::new(28, 0, 10, 1), SortBy::Own),
            (Rect::new(39, 0, 10, 1), SortBy::Aggregated),
        ]);

        click(&mut app, 22, 0);
        assert_eq!((app.sort_by, app.sort_desc), (SortBy::Pid, false));
        click(&mut app, 22, 0);
        assert_eq!((app.sort_by, app.sort_desc), (SortBy::Pid, true));
        click(&mut app, 30, 0);
        assert_eq!((app.sort_by, app.sort_desc), (SortBy::Own, true));
        click(&mut app, 5, 0);
        assert_eq!((app.sort_by, app.sort_desc), (SortBy::Comm, false));
        click(&mut app, 5, 0);
        assert!(app.sort_desc);
        // Roots now ordered by comm descending: stray, launchd.
        assert_eq!(pids(&app)[0], 5);

        // Clicks outside every header rect fall through to the table.
        click(&mut app, 70, 0);
        assert_eq!(app.sort_by, SortBy::Comm);
    }

    #[test]
    fn pressing_s_cycles_sort_keys_with_natural_default_direction() {
        let mut app = filter_app();
        assert_eq!((app.sort_by, app.sort_desc), (SortBy::Aggregated, true));
        assert_eq!(pids(&app)[0], 1); // launchd agg=100 > stray agg=50

        press(&mut app, KeyCode::Char('s'));
        assert_eq!((app.sort_by, app.sort_desc), (SortBy::Own, true));
        assert_eq!(pids(&app)[0], 5); // stray own=50 > launchd own=10

        press(&mut app, KeyCode::Char('s'));
        assert_eq!((app.sort_by, app.sort_desc), (SortBy::Pid, false));
        assert_eq!(pids(&app), vec![1, 2, 3, 4, 5]);

        press(&mut app, KeyCode::Char('s'));
        assert_eq!((app.sort_by, app.sort_desc), (SortBy::Comm, false));
        // launchd < stray; under launchd, other (4) < shell (2).
        assert_eq!(pids(&app), vec![1, 4, 2, 3, 5]);

        press(&mut app, KeyCode::Char('s'));
        assert_eq!((app.sort_by, app.sort_desc), (SortBy::Aggregated, true));

        // In Options, SortKey stays independent of SortReverse.
        app.mode = AppMode::Options;
        app.sort_desc = true;
        app.option_selected = OPTION_KEYS
            .iter()
            .position(|key| *key == OptionKey::SortKey)
            .expect("sort_key option");
        press(&mut app, KeyCode::Right); // Own
        press(&mut app, KeyCode::Right); // Pid
        assert_eq!((app.sort_by, app.sort_desc), (SortBy::Pid, true));
    }

    fn wheel(app: &mut App, kind: MouseEventKind) {
        app.on_mouse(MouseEvent {
            kind,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
    }

    fn options_app() -> App {
        let mut app = filter_app();
        app.mode = AppMode::Options;
        let rows = (0..8)
            .map(|i| (Rect::new(10, 4 + i, 40, 1), usize::from(i)))
            .collect();
        app.set_options_hit_test(Rect::new(8, 2, 44, 14), rows);
        app
    }

    #[test]
    fn options_click_selects_then_activates_and_wheel_moves() {
        let mut app = options_app();
        assert_eq!(app.option_selected, 0);

        // Click a different row: selects it without changing the value.
        let base10_row = 4;
        click(&mut app, 12, 4 + base10_row);
        assert_eq!(app.option_selected, 4);
        assert!(!app.base10);

        // Click the selected row again: advances the value (Enter semantics).
        click(&mut app, 12, 4 + base10_row);
        assert!(app.base10);

        // Clicking the update_ms row twice opens its editor; any later click cancels it.
        click(&mut app, 12, 4);
        click(&mut app, 12, 4);
        assert!(app.update_ms_editor.is_some());
        click(&mut app, 12, 5);
        assert!(app.update_ms_editor.is_none());

        // Wheel moves the selection and clamps at both ends.
        wheel(&mut app, MouseEventKind::ScrollDown);
        assert_eq!(app.option_selected, 2);
        for _ in 0..30 {
            wheel(&mut app, MouseEventKind::ScrollDown);
        }
        assert_eq!(app.option_selected, 16);
        for _ in 0..30 {
            wheel(&mut app, MouseEventKind::ScrollUp);
        }
        assert_eq!(app.option_selected, 0);
    }

    #[test]
    fn options_click_outside_closes_overlay() {
        let mut app = options_app();
        click(&mut app, 0, 0);
        assert_eq!(app.mode, AppMode::Normal);
    }

    #[test]
    fn help_wheel_scrolls_and_click_outside_closes() {
        let mut app = filter_app();
        app.mode = AppMode::Help;
        app.set_help_hit_test(Rect::new(4, 2, 60, 20));
        app.set_help_scroll_metrics(100, 10);
        assert_eq!(app.help_overflow(), 90);

        wheel(&mut app, MouseEventKind::ScrollDown);
        assert_eq!(app.help_scroll(), 3);
        wheel(&mut app, MouseEventKind::ScrollUp);
        wheel(&mut app, MouseEventKind::ScrollUp);
        assert_eq!(app.help_scroll(), 0);

        click(&mut app, 10, 10); // inside: stays open
        assert_eq!(app.mode, AppMode::Help);
        click(&mut app, 0, 0); // outside: closes
        assert_eq!(app.mode, AppMode::Normal);
    }

    fn plain_press(app: &mut App, ch: char) {
        press(app, KeyCode::Char(ch));
    }

    #[test]
    fn new_settings_round_trip_through_the_app() {
        let settings = UiSettings {
            color_theme: "Nord".into(),
            theme_background: false,
            truecolor: crate::ui::theme::TruecolorMode::Off,
            rounded_corners: false,
            vim_keys: false,
            disable_mouse: true,
            clock_format: "%H:%M".into(),
            background_update: false,
            force_tty: true,
            ..UiSettings::default()
        };
        let app = App::with_snapshot(
            Snapshot {
                procs: Vec::new(),
                total_mem: 1,
                used_mem: 0,
            },
            settings.clone(),
        );
        assert_eq!(app.ui_settings(), settings);
        assert!(!app.mouse_enabled());
        assert_eq!(app.border_type(), ratatui::widgets::BorderType::Plain);
    }

    #[test]
    fn global_keys_open_overlays_and_adjust_update_interval() {
        let mut app = filter_app();
        let base = app.update_ms;
        plain_press(&mut app, '+');
        assert_eq!(app.update_ms, base + 100);
        plain_press(&mut app, '-');
        plain_press(&mut app, '-');
        assert_eq!(app.update_ms, base - 100);
        for _ in 0..100 {
            plain_press(&mut app, '-');
        }
        assert_eq!(app.update_ms, MIN_UPDATE_MS);

        for key in [KeyCode::F(1), KeyCode::Char('?'), KeyCode::Char('h')] {
            press(&mut app, key);
            assert_eq!(app.mode, AppMode::Help, "{key:?}");
            press(&mut app, KeyCode::Esc);
            assert_eq!(
                app.mode,
                AppMode::Normal,
                "Esc returns where Help was opened from"
            );
        }
        for key in [KeyCode::F(2), KeyCode::Char('o')] {
            press(&mut app, key);
            assert_eq!(app.mode, AppMode::Options, "{key:?}");
            press(&mut app, KeyCode::Esc);
            assert_eq!(app.mode, AppMode::Normal);
        }

        // Opened through the menu, Esc still steps back to the menu.
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, AppMode::Menu);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.mode, AppMode::Options);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, AppMode::Menu);
    }

    #[test]
    fn global_letter_keys_are_text_while_filtering() {
        let mut app = filter_app();
        press(&mut app, KeyCode::Char('f'));
        type_text(&mut app, "oh+-?");
        assert_eq!(app.mode, AppMode::FilterInput);
        assert_eq!(app.filter_input, "oh+-?");
        // F1 is not text, but still must not hijack typing mode.
        press(&mut app, KeyCode::F(1));
        assert_eq!(app.mode, AppMode::FilterInput);
    }

    #[test]
    fn vim_keys_off_makes_jklgg_inert_but_arrows_and_help_work() {
        let mut app = filter_app();
        let before = app.cursor();
        plain_press(&mut app, 'j');
        assert_eq!(app.cursor(), before + 1, "vim_keys on by default");
        plain_press(&mut app, 'g');
        assert_eq!(app.cursor(), 0);

        app.vim_keys = false;
        for ch in ['j', 'k', 'l', 'g', 'G'] {
            plain_press(&mut app, ch);
            assert_eq!(app.cursor(), 0, "{ch} must be inert");
            assert_eq!(app.mode, AppMode::Normal);
        }
        press(&mut app, KeyCode::Down);
        assert_eq!(app.cursor(), 1);
        press(&mut app, KeyCode::End);
        assert_eq!(app.cursor(), app.rows().len() - 1);
        plain_press(&mut app, 'h');
        assert_eq!(app.mode, AppMode::Help, "h always opens help");
    }

    #[test]
    fn vim_l_expands_the_selected_node() {
        let mut app = filter_app();
        app.expanded.clear();
        app.rebuild_rows();
        assert_eq!(pids(&app), vec![1, 5]);
        plain_press(&mut app, 'l');
        assert_eq!(pids(&app), vec![1, 2, 4, 5]);
    }

    #[test]
    fn background_update_off_pauses_ticks_while_an_overlay_is_open() {
        let mut app = filter_app();
        app.background_update = false;
        app.next_tick_at = Instant::now() - Duration::from_secs(1);
        assert!(app.should_tick(Instant::now()));
        for mode in [AppMode::Menu, AppMode::Options, AppMode::Help] {
            app.mode = mode;
            assert!(!app.should_tick(Instant::now()), "{mode:?}");
            assert!(app.poll_timeout(Instant::now()) <= Duration::from_millis(250));
        }
        app.background_update = true;
        app.mode = AppMode::Options;
        assert!(app.should_tick(Instant::now()));
    }

    #[test]
    fn disable_mouse_ignores_mouse_events() {
        let mut app = filter_app();
        app.mode = AppMode::Help;
        app.set_help_hit_test(Rect::new(4, 2, 60, 20));
        app.disable_mouse = true;
        click(&mut app, 0, 0);
        assert_eq!(app.mode, AppMode::Help);
        assert!(!app.mouse_enabled());
    }

    #[test]
    fn clock_format_option_is_edited_in_a_text_field() {
        let mut app = filter_app();
        press(&mut app, KeyCode::Char('o'));
        app.option_selected = OPTION_KEYS
            .iter()
            .position(|key| *key == OptionKey::ClockFormat)
            .expect("clock option");
        assert_eq!(app.option_value(OptionKey::ClockFormat), "(off)");

        press(&mut app, KeyCode::Enter);
        assert!(app.clock_editor.is_some());
        // j/k/h/o are text here, even though they are navigation keys elsewhere.
        type_text(&mut app, "%H:%M jkho");
        assert_eq!(app.mode, AppMode::Options);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.clock_format, "%H:%M jkho");

        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.clock_format, "%H:%M jkho", "Esc cancels the edit");
        assert!(app.clock_editor.is_none());
        assert_eq!(app.mode, AppMode::Options, "Esc only closes the editor");
    }

    #[test]
    fn toggle_options_apply_live() {
        let mut app = filter_app();
        app.mode = AppMode::Options;
        for key in [
            OptionKey::ThemeBackground,
            OptionKey::RoundedCorners,
            OptionKey::VimKeys,
            OptionKey::DisableMouse,
            OptionKey::BackgroundUpdate,
            OptionKey::ForceTty,
        ] {
            app.option_selected = OPTION_KEYS.iter().position(|k| *k == key).expect("option");
            let before = app.option_value(key);
            press(&mut app, KeyCode::Right);
            assert_ne!(app.option_value(key), before, "{key:?}");
            press(&mut app, KeyCode::Left);
            assert_eq!(app.option_value(key), before, "{key:?}");
        }
        app.option_selected = OPTION_KEYS
            .iter()
            .position(|k| *k == OptionKey::Truecolor)
            .expect("option");
        press(&mut app, KeyCode::Right);
        assert_eq!(app.truecolor, crate::ui::theme::TruecolorMode::On);
    }

    #[test]
    fn color_theme_cycles_discovered_files_and_applies_them() {
        let dir = std::env::temp_dir().join(format!("pspp-app-themes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("Mono.theme"), "theme[main_fg]=\"#ffffff\"\n").expect("write");

        let mut app = filter_app();
        app.truecolor = crate::ui::theme::TruecolorMode::On;
        app.cycle_color_theme_in(Some(&dir), 1);
        assert_eq!(app.color_theme, "Mono");
        assert_eq!(app.theme.main_fg, crate::ui::theme::Rgb(255, 255, 255));
        app.cycle_color_theme_in(Some(&dir), 1);
        assert_eq!(app.color_theme, "Default");
        assert_ne!(app.theme.main_fg, crate::ui::theme::Rgb(255, 255, 255));

        // A configured theme whose file vanished stays selectable and falls back to Default colours.
        app.color_theme = "Gone".into();
        app.cycle_color_theme_in(Some(&dir), 0);
        assert_eq!(app.color_theme, "Gone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn options_list_scrolls_to_keep_the_selection_visible() {
        let mut app = filter_app();
        assert_eq!(app.option_scroll_for(5), 0);
        app.option_selected = 9;
        assert_eq!(app.option_scroll_for(5), 5);
        app.option_selected = 2;
        assert_eq!(app.option_scroll_for(5), 2);
        app.option_selected = 16;
        assert_eq!(app.option_scroll_for(5), 12);
        assert_eq!(app.option_scroll_for(100), 0, "everything fits");
    }

    fn vrow(pid: i32, depth: usize) -> crate::model::VisibleRow {
        crate::model::VisibleRow {
            pid,
            depth,
            has_children: false,
            is_last_child: false,
        }
    }

    #[test]
    fn relabel_last_children_follows_the_rows_that_remain() {
        // 1 ─ 2 ─ 3
        //   └ 4        (4 is the last child of 1; 3 the only child of 2)
        // 5
        let mut rows = vec![vrow(1, 0), vrow(2, 1), vrow(3, 2), vrow(4, 1), vrow(5, 0)];
        super::relabel_last_children(&mut rows);
        let flags: Vec<(i32, bool)> = rows.iter().map(|r| (r.pid, r.is_last_child)).collect();
        assert_eq!(
            flags,
            vec![(1, false), (2, false), (3, true), (4, true), (5, true)]
        );

        // A lone chain: everything is the last child of its parent.
        let mut chain = vec![vrow(1, 0), vrow(2, 1), vrow(3, 2)];
        super::relabel_last_children(&mut chain);
        assert!(chain.iter().all(|r| r.is_last_child));
        super::relabel_last_children(&mut []);
    }

    #[test]
    fn filtering_draws_last_child_glyphs_for_the_visible_siblings() {
        let mut app = filter_app();
        app.set_filter("target");
        let prefixes: Vec<&str> = app.rows().iter().map(|r| r.tree_prefix.as_str()).collect();
        // launchd -> shell -> target; `other` (a later sibling of shell) is filtered out.
        assert_eq!(prefixes, vec!["[-] ", "└─ [-] ", "   └─ "]);
    }

    #[test]
    fn single_root_four_level_tree_draws_continuation_bars_under_non_last_children() {
        // Sole root launchd (is_last = true at depth 0) -> edge + code -> helper + gpu -> leaf.
        let app = synthetic_app(&[
            (1, 0, "launchd", 10),
            (2, 1, "edge", 400),
            (3, 2, "helper", 300),
            (4, 3, "leaf", 200),
            (5, 2, "gpu", 100),
            (6, 1, "code", 50),
        ]);
        let rendered: Vec<String> = app
            .rows()
            .iter()
            .map(|r| format!("{}{}", r.tree_prefix, r.comm))
            .collect();
        assert_eq!(
            rendered,
            vec![
                "[-] launchd",
                "├─ [-] edge",
                "│  ├─ [-] helper",
                "│  │  └─ leaf",
                "│  └─ gpu",
                "└─ code",
            ]
        );
    }

    #[test]
    fn multi_root_non_last_root_last_child_uses_blank_continuation_for_grandchildren() {
        // Two roots: root_a (is_last = false, rollup 300) and root_b (rollup 100).
        // Under root_a: first_child (is_last = false, rollup 200) -> first_gc,
        // and child (is_last = true, rollup 100) -> grandchild.
        let app = synthetic_app(&[
            (1, 0, "root_a", 0),
            (2, 1, "first_child", 100),
            (3, 2, "first_gc", 100),
            (4, 1, "child", 50),
            (5, 4, "grandchild", 50),
            (6, 0, "root_b", 100),
        ]);
        let rendered: Vec<String> = app
            .rows()
            .iter()
            .map(|r| format!("{}{}", r.tree_prefix, r.comm))
            .collect();
        assert_eq!(
            rendered,
            vec![
                "[-] root_a",
                "├─ [-] first_child",
                "│  └─ first_gc",
                "└─ [-] child",
                "   └─ grandchild",
                "root_b",
            ]
        );
    }

    fn snapshot_of(pids: &[i32]) -> Snapshot {
        Snapshot {
            procs: pids
                .iter()
                .map(|&pid| ProcRec {
                    pid,
                    ppid: 0,
                    comm: format!("p{pid}"),
                    mem: MemSet {
                        rss: Some(0),
                        vsz: Some(0),
                        ..MemSet::default()
                    },
                })
                .collect(),
            total_mem: 100,
            used_mem: 10,
        }
    }

    fn wait_for_snapshot(app: &mut App) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut applied = false;
        while app.collector.as_ref().is_some_and(|c| c.is_busy()) {
            applied |= app.poll_snapshot();
            assert!(Instant::now() < deadline, "worker never answered");
            std::thread::sleep(Duration::from_millis(2));
        }
        applied | app.poll_snapshot()
    }

    #[test]
    fn tick_requests_in_the_background_and_poll_applies_the_result() {
        let mut app = filter_app();
        assert!(!app.poll_snapshot());
        assert_eq!(app.tree.proc_count(), 5);
        app.collector = Some(crate::collect::Collector::spawn_with(|_| {
            std::thread::sleep(Duration::from_millis(60));
            snapshot_of(&[1, 2])
        }));

        let started = Instant::now();
        app.on_tick();
        assert!(
            started.elapsed() < Duration::from_millis(40),
            "tick must not block on I/O"
        );
        assert_eq!(
            app.tree.proc_count(),
            5,
            "old data stays until the worker answers"
        );
        assert!(app.poll_timeout(Instant::now()) <= Duration::from_millis(50));

        assert!(wait_for_snapshot(&mut app));
        assert!(
            !app.poll_snapshot(),
            "second poll with nothing ready is false"
        );
        assert_eq!(app.tree.proc_count(), 2);
        let mut shown = pids(&app);
        shown.sort_unstable();
        assert_eq!(shown, vec![1, 2]);
        assert_eq!(app.mem_used(), 10);
    }

    #[test]
    fn changing_the_memory_kind_requests_a_snapshot_for_the_new_kind() {
        use std::sync::{Arc, Mutex};
        let seen: Arc<Mutex<Vec<MemKind>>> = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let mut app = filter_app();
        app.mem_kind = MemKind::Rss;
        app.collector = Some(crate::collect::Collector::spawn_with(move |kind| {
            std::thread::sleep(Duration::from_millis(30));
            if let Ok(mut log) = log.lock() {
                log.push(kind);
            }
            snapshot_of(&[1])
        }));

        app.on_tick(); // request in flight for Rss
        press(&mut app, KeyCode::Char('m')); // kind changes mid-flight
        let new_kind = app.mem_kind;
        assert_ne!(new_kind, MemKind::Rss);
        wait_for_snapshot(&mut app); // stale Rss result applied, follow-up requested
        wait_for_snapshot(&mut app);

        let seen = seen.lock().map(|log| log.clone()).unwrap_or_default();
        assert_eq!(seen, vec![MemKind::Rss, new_kind]);
    }

    #[test]
    fn apply_snapshot_keeps_collapsed_nodes_and_adds_new_ones_expanded() {
        let mut app = filter_app();
        app.expanded.remove(&1);
        app.rebuild_rows();
        let mut next = Snapshot {
            procs: filter_app().tree.procs().cloned().collect(),
            total_mem: 100,
            used_mem: 50,
        };
        next.procs.push(ProcRec {
            pid: 6,
            ppid: 5,
            comm: "newbie".into(),
            mem: MemSet {
                rss: Some(0),
                vsz: Some(0),
                ..MemSet::default()
            },
        });
        app.apply_snapshot(next);

        assert!(
            !app.expanded.contains(&1),
            "user's collapse survives a refresh"
        );
        assert!(app.expanded.contains(&6), "new pids start expanded");
        assert_eq!(app.mem_used(), 50);
        assert_eq!(app.mem_history.pushed_count(), 2);
    }

    #[test]
    fn live_filter_updates_rows_per_keystroke_and_esc_restores_snapshot() {
        let mut app = filter_app();
        // Collapse `shell` (pid 2) so `target` (pid 3) starts hidden; select `other` (pid 4).
        app.expanded.remove(&2);
        app.rebuild_rows();
        assert_eq!(pids(&app), vec![1, 2, 4, 5]);
        app.select_pid(4);
        assert_eq!(app.selected_pid, Some(4));

        press(&mut app, KeyCode::Char('f'));
        assert_eq!(app.mode, AppMode::FilterInput);
        assert_eq!(app.active_filter(), "");
        assert!(!app.is_filter_active());

        // Live filtering expands ancestors of direct matches before Enter is pressed.
        type_text(&mut app, "tar");
        assert_eq!(app.active_filter(), "tar");
        assert!(app.is_filter_active());
        assert_eq!(pids(&app), vec![1, 2, 3]);
        assert!(app.is_direct_filter_match(3));
        assert!(app.is_context_filter_match(2));

        // Backspacing back to empty resets `expanded` to the pre-filter snapshot (`2` folded).
        for _ in 0..3 {
            press(&mut app, KeyCode::Backspace);
        }
        assert_eq!(app.active_filter(), "");
        assert!(!app.is_filter_active());
        assert_eq!(pids(&app), vec![1, 2, 4, 5]);

        // Ctrl-W deletes trailing whitespace + trailing word one word at a time.
        type_text(&mut app, "alpha beta  ");
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(app.filter_input, "alpha ");
        app.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(app.filter_input, "");

        // Ctrl-U clears the filter input and refreshes rows live.
        type_text(&mut app, "stray");
        assert_eq!(pids(&app), vec![5]);
        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(app.filter_input, "");
        assert_eq!(pids(&app), vec![1, 2, 4, 5]);

        // Delete also clears the filter input.
        type_text(&mut app, "target");
        assert_eq!(pids(&app), vec![1, 2, 3]);
        press(&mut app, KeyCode::Delete);
        assert_eq!(app.filter_input, "");
        assert_eq!(pids(&app), vec![1, 2, 4, 5]);

        // Recovering from a temporary zero-match typo via Backspace or Ctrl-U re-selects pid 4.
        type_text(&mut app, "othex");
        assert!(pids(&app).is_empty());
        assert_eq!(app.selected_pid, None);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.filter_input, "othe");
        assert_eq!(app.selected_pid, Some(4));
        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(app.selected_pid, Some(4));

        type_text(&mut app, "no-such-proc");
        assert!(pids(&app).is_empty());
        assert_eq!(app.selected_pid, None);
        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(pids(&app), vec![1, 2, 4, 5]);
        assert_eq!(app.selected_pid, Some(4));

        // Esc after a zero-match edit restores committed filter, expanded set, and selected_pid.
        type_text(&mut app, "no-such-proc");
        assert!(pids(&app).is_empty());
        assert_eq!(app.selected_pid, None);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, AppMode::Normal);
        assert!(!app.is_filter_active());
        assert_eq!(pids(&app), vec![1, 2, 4, 5]);
        assert_eq!(app.selected_pid, Some(4));
    }

    #[test]
    fn apply_snapshot_while_filtering_remaps_filter_snapshot_expanded() {
        let mut app = filter_app();
        app.expanded.remove(&2);
        app.rebuild_rows();

        press(&mut app, KeyCode::Char('f'));
        type_text(&mut app, "target");
        assert!(app.expanded.contains(&2), "live filter expanded ancestor 2");

        let mut next = Snapshot {
            procs: filter_app().tree.procs().cloned().collect(),
            total_mem: 100,
            used_mem: 50,
        };
        next.procs.push(ProcRec {
            pid: 6,
            ppid: 5,
            comm: "child_of_stray".into(),
            mem: MemSet {
                rss: Some(10),
                vsz: Some(10),
                ..MemSet::default()
            },
        });
        app.apply_snapshot(next);
        assert_eq!(pids(&app), vec![1, 2, 3]);

        // Cancelling FilterInput keeps pid 2 collapsed while keeping new pid 6 expanded.
        press(&mut app, KeyCode::Esc);
        assert!(!app.expanded.contains(&2));
        assert!(app.expanded.contains(&6));
    }

    #[test]
    fn collapse_all_and_expand_all_fold_non_roots_then_roots_and_preserve_parent_selection() {
        // Multi-root tree where root 0 is a leaf root with the largest memory (row 0),
        // root 1 has two 2-level subtrees (10 -> 11 -> 12, 20 -> 21), and root 30 has child 31.
        let mut app = synthetic_app(&[
            (0, 0, "kernel_task", 5_000),
            (1, 0, "launchd", 10),
            (10, 1, "edge", 400),
            (11, 10, "helper", 300),
            (12, 11, "worker", 200),
            (20, 1, "code", 150),
            (21, 20, "tsserver", 100),
            (30, 0, "daemon", 50),
            (31, 30, "daemon_worker", 40),
        ]);
        assert_eq!(pids(&app)[0], 0);
        app.select_pid(21);
        assert_eq!(app.selected_pid, Some(21));

        // First `z`: folds non-root subtrees (`expanded == {1, 30}`) and moves selection from
        // `tsserver` (21) onto its folded parent `code` (20) rather than row 0 (`kernel_task`).
        plain_press(&mut app, 'z');
        assert_eq!(app.expanded, [1, 30].into_iter().collect());
        assert_eq!(pids(&app), vec![0, 1, 10, 20, 30, 31]);
        assert_eq!(app.selected_pid, Some(20));

        // Second `z`: folds roots too (`expanded` cleared) and moves selection from `code` (20)
        // onto its folded root `launchd` (1), still not jumping to row 0 (`kernel_task`).
        plain_press(&mut app, 'z');
        assert!(app.expanded.is_empty());
        assert_eq!(pids(&app), vec![0, 1, 30]);
        assert_eq!(app.selected_pid, Some(1));

        // Third `z`: monotonic — never re-expands collapsed roots.
        plain_press(&mut app, 'z');
        assert!(app.expanded.is_empty());
        assert_eq!(pids(&app), vec![0, 1, 30]);

        // `Z` expands every node again.
        plain_press(&mut app, 'Z');
        assert_eq!(app.rows().len(), 9);

        // `[` and `]` aliases behave identically.
        app.select_pid(12);
        plain_press(&mut app, '[');
        assert_eq!(app.selected_pid, Some(10));
        plain_press(&mut app, '[');
        assert_eq!(app.selected_pid, Some(1));
        plain_press(&mut app, ']');
        assert_eq!(app.rows().len(), 9);

        // Background `apply_snapshot` adding a new leaf process after first `z` must not block
        // the second `z` from folding roots, nor cause a third `z` to re-expand roots.
        plain_press(&mut app, 'z');
        assert_eq!(app.expanded, [1, 30].into_iter().collect());
        let mut procs: Vec<ProcRec> = app.tree.procs().cloned().collect();
        procs.push(ProcRec {
            pid: 32,
            ppid: 30,
            comm: "daemon_leaf".into(),
            mem: MemSet {
                rss: Some(25),
                vsz: Some(25),
                ..MemSet::default()
            },
        });
        app.apply_snapshot(Snapshot {
            procs,
            total_mem: 10_000,
            used_mem: 6_000,
        });
        assert!(app.expanded.contains(&32));
        plain_press(&mut app, 'z');
        assert!(app.expanded.is_empty());
        assert_eq!(pids(&app), vec![0, 1, 30]);
        plain_press(&mut app, 'z');
        assert!(app.expanded.is_empty());

        // On a 2-level tree, the first `z` immediately folds roots instead of being a no-op.
        let mut two_level = synthetic_app(&[(1, 0, "launchd", 100), (2, 1, "leaf", 50)]);
        plain_press(&mut two_level, 'z');
        assert!(two_level.expanded.is_empty());
        assert_eq!(pids(&two_level), vec![1]);
    }

    #[test]
    fn force_tty_and_no_color_override_symbols_borders_and_palette_mode() {
        use crate::ui::graph::GraphSymbols;
        use crate::ui::theme::{PaletteMode, TruecolorMode};
        use ratatui::widgets::BorderType;

        for setting in [
            GraphSymbolSetting::Auto,
            GraphSymbolSetting::Braille,
            GraphSymbolSetting::Block,
            GraphSymbolSetting::Tty,
        ] {
            assert_eq!(setting.resolve(true), GraphSymbols::Tty, "{setting:?}");
        }

        let mut app = App::with_snapshot(
            Snapshot {
                procs: Vec::new(),
                total_mem: 100,
                used_mem: 50,
            },
            UiSettings {
                graph_symbol: GraphSymbolSetting::Braille,
                rounded_corners: true,
                truecolor: TruecolorMode::On,
                force_tty: false,
                ..UiSettings::default()
            },
        );
        assert_eq!(app.resolved_graph_symbols(), GraphSymbols::Braille);
        assert_eq!(app.border_type(), BorderType::Rounded);
        assert_eq!(app.theme.mode(), PaletteMode::Truecolor);

        // Toggling `ForceTty` in Options switches theme to Ansi16, borders to Plain, and symbols
        // to Tty without mutating `rounded_corners` or `graph_symbol`.
        app.mode = AppMode::Options;
        app.option_selected = OPTION_KEYS
            .iter()
            .position(|k| *k == OptionKey::ForceTty)
            .expect("force_tty");
        press(&mut app, KeyCode::Right);
        assert!(app.force_tty);
        assert!(app.rounded_corners);
        assert_eq!(app.graph_symbol, GraphSymbolSetting::Braille);
        assert_eq!(app.resolved_graph_symbols(), GraphSymbols::Tty);
        assert!(app.ascii_meters());
        assert_eq!(app.border_type(), BorderType::Plain);
        assert_eq!(app.theme.mode(), PaletteMode::Ansi16);

        // `set_no_color(true)` takes highest precedence (`NoColor` palette, `Tty` symbols, `Plain`).
        app.set_no_color(true);
        assert_eq!(app.theme.mode(), PaletteMode::NoColor);
        assert_eq!(app.resolved_graph_symbols(), GraphSymbols::Tty);
        assert_eq!(app.border_type(), BorderType::Plain);

        press(&mut app, KeyCode::Left); // force_tty = false while no_color remains true
        assert!(!app.force_tty);
        assert_eq!(app.theme.mode(), PaletteMode::NoColor);
        assert_eq!(app.resolved_graph_symbols(), GraphSymbols::Tty);
        assert_eq!(app.border_type(), BorderType::Plain);
        assert!(app.rounded_corners);

        app.set_no_color(false);
        assert_eq!(app.theme.mode(), PaletteMode::Truecolor);
        assert_eq!(app.resolved_graph_symbols(), GraphSymbols::Braille);
        assert_eq!(app.border_type(), BorderType::Rounded);
    }

    #[test]
    fn overlay_navigation_supports_options_hl_help_home_end_gg_and_menu_wheel() {
        use super::MenuButton;

        let mut app = filter_app();
        app.mode = AppMode::Options;
        app.option_selected = OPTION_KEYS
            .iter()
            .position(|k| *k == OptionKey::UpdateMs)
            .expect("update_ms");
        let base = app.update_ms;
        plain_press(&mut app, 'l');
        assert_eq!(app.update_ms, base + 100);
        plain_press(&mut app, 'h');
        assert_eq!(app.update_ms, base);

        // Ctrl-H/Ctrl-L and Alt-H/Alt-L must not step options.
        for mods in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
            app.on_key(KeyEvent::new(KeyCode::Char('l'), mods));
            app.on_key(KeyEvent::new(KeyCode::Char('h'), mods));
            assert_eq!(app.update_ms, base);
        }

        // Coalesced `h` + `G` before the first Help frame (`help_content_lines == 0`) latches to
        // `usize::MAX` and clamps to `help_overflow()` once `set_help_scroll_metrics` runs.
        app.mode = AppMode::Normal;
        plain_press(&mut app, 'h');
        assert_eq!(app.mode, AppMode::Help);
        assert_eq!(app.help_content_lines, 0);
        app.on_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(app.help_scroll(), usize::MAX);
        app.set_help_scroll_metrics(100, 15);
        assert_eq!(app.help_scroll(), 85);
        press(&mut app, KeyCode::Home);
        assert_eq!(app.help_scroll(), 0);

        // With `vim_keys = false`, `h`/`l` are inert in Options, while `h` in Normal still opens Help.
        app.mode = AppMode::Options;
        app.vim_keys = false;
        plain_press(&mut app, 'l');
        plain_press(&mut app, 'h');
        assert_eq!(app.update_ms, base);
        app.mode = AppMode::Normal;
        plain_press(&mut app, 'h');
        assert_eq!(app.mode, AppMode::Help);

        // Help supports Home/End regardless of vim_keys, and g/G when vim_keys is true.
        app.set_help_scroll_metrics(100, 15);
        assert_eq!(app.help_overflow(), 85);
        app.on_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(app.help_scroll(), 0, "G ignored when vim_keys is false");
        press(&mut app, KeyCode::End);
        assert_eq!(app.help_scroll(), 85);
        press(&mut app, KeyCode::Home);
        assert_eq!(app.help_scroll(), 0);

        app.vim_keys = true;
        app.on_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(app.help_scroll(), 85);
        plain_press(&mut app, 'g');
        assert_eq!(app.help_scroll(), 0);

        // Menu handles mouse wheel scrolling and `on_mouse` reports false for motion/drag/up/hscroll.
        app.mode = AppMode::Menu;
        assert_eq!(app.menu_selected(), MenuButton::Options);
        assert!(app.on_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(app.menu_selected(), MenuButton::Help);
        wheel(&mut app, MouseEventKind::ScrollDown);
        assert_eq!(app.menu_selected(), MenuButton::Quit);
        wheel(&mut app, MouseEventKind::ScrollUp);
        assert_eq!(app.menu_selected(), MenuButton::Help);

        for noop_kind in [
            MouseEventKind::Moved,
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::ScrollLeft,
            MouseEventKind::ScrollRight,
        ] {
            assert!(
                !app.on_mouse(MouseEvent {
                    kind: noop_kind,
                    column: 5,
                    row: 5,
                    modifiers: KeyModifiers::NONE,
                }),
                "{noop_kind:?} must not mark frame dirty"
            );
        }
        app.disable_mouse = true;
        assert!(!app.on_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }));
    }
}
