use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};

use crate::collect::{self, Snapshot};
use crate::model::{AggMode, MemKind, ProcTree};
use crate::ui::theme::Theme;

pub const DEFAULT_UPDATE_MS: u64 = 2_000;
pub const MIN_UPDATE_MS: u64 = 100;

pub fn clamp_update_ms(update_ms: u64) -> u64 {
    update_ms.max(MIN_UPDATE_MS)
}

pub fn next_tick_deadline(last_tick_at: Instant, update_ms: u64) -> Instant {
    last_tick_at + Duration::from_millis(clamp_update_ms(update_ms))
}

pub fn remaining_until_tick(now: Instant, next_tick_at: Instant) -> Duration {
    next_tick_at.saturating_duration_since(now)
}

pub struct App {
    pub snapshot: Snapshot,
    pub tree: ProcTree,
    pub agg_mode: AggMode,
    pub mem_kind: MemKind,
    pub update_ms: u64,
    pub theme: Theme,
    pub should_quit: bool,
    pub last_tick_at: Instant,
    pub next_tick_at: Instant,
    pub last_mouse_position: Option<(u16, u16)>,
}

impl App {
    pub fn new(update_ms: u64) -> Self {
        let update_ms = clamp_update_ms(update_ms);
        let snapshot = collect::take_snapshot();
        let tree = ProcTree::build(&snapshot);
        let now = Instant::now();

        Self {
            snapshot,
            tree,
            agg_mode: AggMode::Rollup,
            mem_kind: MemKind::Rss,
            update_ms,
            theme: Theme::detect(),
            should_quit: false,
            last_tick_at: now,
            next_tick_at: next_tick_deadline(now, update_ms),
            last_mouse_position: None,
        }
    }

    pub fn poll_timeout(&self, now: Instant) -> Duration {
        remaining_until_tick(now, self.next_tick_at)
    }

    pub fn should_tick(&self, now: Instant) -> bool {
        now >= self.next_tick_at
    }

    pub fn on_tick(&mut self) {
        self.snapshot = collect::take_snapshot();
        self.tree = ProcTree::build(&self.snapshot);
        self.last_tick_at = Instant::now();
        self.next_tick_at = next_tick_deadline(self.last_tick_at, self.update_ms);
    }

    pub fn on_key(&mut self, key_event: KeyEvent) {
        if key_event.kind != KeyEventKind::Press {
            return;
        }

        match key_event.code {
            KeyCode::Char(ch) if is_quit_key(ch, key_event.modifiers) => {
                self.should_quit = true;
            }
            KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => {}
            KeyCode::Enter | KeyCode::Esc => {}
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, mouse_event: MouseEvent) {
        self.last_mouse_position = Some((mouse_event.column, mouse_event.row));
    }
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
    use super::{clamp_update_ms, next_tick_deadline, remaining_until_tick, MIN_UPDATE_MS};
    use std::time::{Duration, Instant};

    #[test]
    fn clamp_update_ms_enforces_minimum() {
        assert_eq!(clamp_update_ms(0), MIN_UPDATE_MS);
        assert_eq!(clamp_update_ms(42), MIN_UPDATE_MS);
        assert_eq!(clamp_update_ms(MIN_UPDATE_MS), MIN_UPDATE_MS);
        assert_eq!(clamp_update_ms(250), 250);
    }

    #[test]
    fn next_tick_deadline_uses_clamped_interval() {
        let now = Instant::now();
        let deadline = next_tick_deadline(now, 1);
        assert_eq!(
            deadline.saturating_duration_since(now),
            Duration::from_millis(MIN_UPDATE_MS)
        );
    }

    #[test]
    fn remaining_until_tick_saturates_to_zero() {
        let now = Instant::now();
        let next = now + Duration::from_millis(250);
        assert_eq!(remaining_until_tick(now, next), Duration::from_millis(250));
        assert_eq!(
            remaining_until_tick(next + Duration::from_millis(1), next),
            Duration::ZERO
        );
    }
}
