//! Interactive ratatui terminal interface, event loop, theme, graphs, and overlays.

pub mod app;
pub mod clock;
pub mod draw;
pub mod graph;
pub mod help;
pub mod theme;

use std::error::Error;
use std::io;
use std::panic::{self, PanicHookInfo};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

pub use app::{GraphSymbolSetting, UiSettings};

const MAX_EVENT_BATCH: usize = 64;

type PanicHook = dyn for<'a> Fn(&PanicHookInfo<'a>) + Send + Sync + 'static;

struct TerminalGuard {
    previous_hook: Arc<PanicHook>,
    restored: bool,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        setup_terminal_state()?;
        let previous_hook = install_panic_hook();
        Ok(Self {
            previous_hook,
            restored: false,
        })
    }

    fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;

        if !thread::panicking() {
            let previous_hook = Arc::clone(&self.previous_hook);
            panic::set_hook(Box::new(move |panic_info| previous_hook(panic_info)));
        }

        restore_terminal_state()
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

pub fn run_tui(
    settings: UiSettings,
    initial_filter: Option<String>,
) -> Result<UiSettings, Box<dyn Error>> {
    let mut app = app::App::from_settings(settings);
    if let Some(filter) = initial_filter {
        app.set_filter(&filter);
    }
    let mut terminal_guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    // `setup_terminal_state` enabled mouse capture; `disable_mouse` is applied (and re-applied
    // when toggled in Options) on every iteration.
    let mut mouse_captured = true;
    let mut first_frame = true;
    let mut last_clock: Option<String> = None;

    while !app.should_quit {
        sync_mouse_capture(&mut mouse_captured, app.mouse_enabled())?;
        let dirty = process_iteration(
            &mut app,
            &mut first_frame,
            &mut last_clock,
            Instant::now(),
            poll_and_read_event,
        )?;
        if app.should_quit {
            break;
        }
        sync_mouse_capture(&mut mouse_captured, app.mouse_enabled())?;
        if dirty {
            terminal.draw(|frame| draw::draw(frame, &mut app))?;
        }

        let timeout = app.poll_timeout(Instant::now());
        let _ = event::poll(timeout)?;
    }

    drop(terminal);
    terminal_guard.restore()?;
    Ok(app.ui_settings())
}

fn poll_and_read_event(timeout: Duration) -> io::Result<Option<Event>> {
    if event::poll(timeout)? {
        event::read().map(Some)
    } else {
        Ok(None)
    }
}

fn handle_terminal_event(app: &mut app::App, event: Event) -> bool {
    match event {
        Event::Key(key_event) => {
            app.on_key(key_event);
            true
        }
        Event::Mouse(mouse_event) => app.on_mouse(mouse_event),
        Event::Resize(_, _) => true,
        _ => false,
    }
}

/// Drains up to `MAX_EVENT_BATCH` ready (`Duration::ZERO`) events, stopping immediately if
/// `app.should_quit` becomes `true`. Returns `true` when any processed event dirtied the UI.
fn drain_ready_events<F>(app: &mut app::App, mut next_event: F) -> io::Result<bool>
where
    F: FnMut(Duration) -> io::Result<Option<Event>>,
{
    let mut dirty = false;
    for _ in 0..MAX_EVENT_BATCH {
        if app.should_quit {
            break;
        }
        let Some(event) = next_event(Duration::ZERO)? else {
            break;
        };
        if handle_terminal_event(app, event) {
            dirty = true;
        }
        if app.should_quit {
            break;
        }
    }
    Ok(dirty)
}

fn process_iteration<F>(
    app: &mut app::App,
    first_frame: &mut bool,
    last_clock: &mut Option<String>,
    now: Instant,
    next_event: F,
) -> io::Result<bool>
where
    F: FnMut(Duration) -> io::Result<Option<Event>>,
{
    let mut dirty = false;
    if *first_frame {
        *first_frame = false;
        dirty = true;
    }
    if drain_ready_events(app, next_event)? {
        dirty = true;
    }

    if app.should_quit {
        return Ok(false);
    }

    if app.poll_snapshot() {
        dirty = true;
    }
    if app.should_tick(now) {
        app.on_tick();
        dirty = true;
    }
    let clock_now = app.clock_text();
    if clock_now != *last_clock {
        *last_clock = clock_now;
        dirty = true;
    }

    Ok(dirty)
}

fn sync_mouse_capture(captured: &mut bool, wanted: bool) -> io::Result<()> {
    if *captured == wanted {
        return Ok(());
    }
    let mut stdout = io::stdout();
    if wanted {
        execute!(stdout, EnableMouseCapture)?;
    } else {
        execute!(stdout, DisableMouseCapture)?;
    }
    *captured = wanted;
    Ok(())
}

fn setup_terminal_state() -> io::Result<()> {
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    if let Err(err) = enable_raw_mode() {
        let _ = execute!(stdout, LeaveAlternateScreen);
        return Err(err);
    }

    if let Err(err) = execute!(stdout, Hide, EnableMouseCapture) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout, LeaveAlternateScreen);
        return Err(err);
    }

    Ok(())
}

fn restore_terminal_state() -> io::Result<()> {
    let mut first_err: Option<io::Error> = None;
    let mut stdout = io::stdout();

    if let Err(err) = execute!(stdout, DisableMouseCapture, Show) {
        first_err = Some(err);
    }

    if let Err(err) = disable_raw_mode() {
        if first_err.is_none() {
            first_err = Some(err);
        }
    }

    if let Err(err) = execute!(stdout, LeaveAlternateScreen) {
        if first_err.is_none() {
            first_err = Some(err);
        }
    }

    if let Some(err) = first_err {
        Err(err)
    } else {
        Ok(())
    }
}

fn install_panic_hook() -> Arc<PanicHook> {
    let previous_hook: Arc<PanicHook> = Arc::from(panic::take_hook());
    let previous_hook_for_panic = Arc::clone(&previous_hook);

    panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal_state();
        previous_hook_for_panic(panic_info);
    }));

    previous_hook
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    use super::*;
    use crate::collect::{MemSet, ProcRec, Snapshot};

    fn test_app(row_count: i32) -> app::App {
        let procs = (1..=row_count)
            .map(|pid| ProcRec {
                pid,
                ppid: 0,
                comm: format!("proc-{pid}"),
                mem: MemSet {
                    rss: Some(u64::try_from(row_count - pid + 1).unwrap_or(0) * 1024),
                    vsz: Some(1024),
                    ..MemSet::default()
                },
            })
            .collect();
        app::App::with_snapshot(
            Snapshot {
                procs,
                total_mem: 1_000_000,
                used_mem: 500_000,
            },
            UiSettings::default(),
        )
    }

    fn drain_queue(app: &mut app::App, queue: &mut VecDeque<Event>) -> io::Result<bool> {
        drain_ready_events(app, |timeout| {
            assert_eq!(timeout, Duration::ZERO);
            Ok(queue.pop_front())
        })
    }

    fn run_iter(
        app: &mut app::App,
        first_frame: &mut bool,
        last_clock: &mut Option<String>,
        now: Instant,
        queue: &mut VecDeque<Event>,
    ) -> io::Result<bool> {
        process_iteration(app, first_frame, last_clock, now, |timeout| {
            assert_eq!(timeout, Duration::ZERO);
            Ok(queue.pop_front())
        })
    }

    #[test]
    fn drain_ready_events_caps_at_64_per_batch_and_stops_immediately_on_quit() {
        let mut app = test_app(120);
        let mut queue: VecDeque<Event> = (0..100)
            .map(|_| Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))
            .collect();

        assert!(drain_queue(&mut app, &mut queue).expect("batch 1"));
        assert_eq!(queue.len(), 36, "must drain at most 64 events per batch");
        assert_eq!(app.cursor(), 64);

        assert!(drain_queue(&mut app, &mut queue).expect("batch 2"));
        assert!(queue.is_empty());
        assert_eq!(app.cursor(), 100);

        // Once `should_quit` flips true, remaining queued events stay untouched.
        let mut quit_queue = VecDeque::from([
            Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
        ]);
        let _ = drain_queue(&mut app, &mut quit_queue).expect("quit batch");
        assert!(app.should_quit);
        assert_eq!(quit_queue.len(), 2, "must stop immediately after quit");
        assert_eq!(app.cursor(), 99);
    }

    #[test]
    fn process_iteration_tracks_dirty_triggers_and_ignores_mouse_motion() {
        let mut app = test_app(10);
        let mut first_frame = true;
        let mut last_clock = None;
        let mut queue = VecDeque::new();
        let start = app.last_tick_at;

        // 1. First frame is always dirty.
        assert!(run_iter(
            &mut app,
            &mut first_frame,
            &mut last_clock,
            start,
            &mut queue
        )
        .expect("first frame"));
        assert!(!first_frame);

        // 2. Idle wake with no events, no snapshot, no tick, and unchanged clock is clean.
        assert!(!run_iter(
            &mut app,
            &mut first_frame,
            &mut last_clock,
            start,
            &mut queue
        )
        .expect("idle"));

        // 3. Mouse motion / drag / button-up / horizontal scroll drain without marking dirty.
        for kind in [
            MouseEventKind::Moved,
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::ScrollLeft,
            MouseEventKind::ScrollRight,
        ] {
            queue.push_back(Event::Mouse(MouseEvent {
                kind,
                column: 10,
                row: 4,
                modifiers: KeyModifiers::NONE,
            }));
        }
        assert!(!run_iter(
            &mut app,
            &mut first_frame,
            &mut last_clock,
            start,
            &mut queue
        )
        .expect("motion batch"));
        assert!(queue.is_empty());

        // 4. Meaningful mouse event (`ScrollDown`) marks dirty.
        queue.push_back(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 4,
            modifiers: KeyModifiers::NONE,
        }));
        assert!(run_iter(
            &mut app,
            &mut first_frame,
            &mut last_clock,
            start,
            &mut queue
        )
        .expect("mouse scroll"));

        // 5. Resize marks dirty.
        queue.push_back(Event::Resize(100, 30));
        assert!(run_iter(
            &mut app,
            &mut first_frame,
            &mut last_clock,
            start,
            &mut queue
        )
        .expect("resize"));

        // 6. Key event marks dirty.
        queue.push_back(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
        assert!(run_iter(
            &mut app,
            &mut first_frame,
            &mut last_clock,
            start,
            &mut queue
        )
        .expect("key"));

        // 7. Formatted top-border clock string change marks dirty once.
        app.clock_format = "FIXED_CLOCK".to_owned();
        assert!(run_iter(
            &mut app,
            &mut first_frame,
            &mut last_clock,
            start,
            &mut queue
        )
        .expect("clock changed"));
        assert_eq!(last_clock.as_deref(), Some("FIXED_CLOCK"));
        assert!(!run_iter(
            &mut app,
            &mut first_frame,
            &mut last_clock,
            start,
            &mut queue
        )
        .expect("clock unchanged"));

        // 8. Reaching `next_tick_at` triggers `on_tick` and marks dirty.
        let due = app.next_tick_at;
        assert!(
            run_iter(&mut app, &mut first_frame, &mut last_clock, due, &mut queue).expect("tick")
        );

        // 9. Ready background snapshot from `poll_snapshot()` marks dirty.
        app.set_collector(crate::collect::Collector::spawn_with(|_| Snapshot {
            procs: Vec::new(),
            total_mem: 1_000_000,
            used_mem: 250_000,
        }));
        app.on_tick();
        let after_tick = app.last_tick_at;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut saw_snapshot_dirty = false;
        while Instant::now() < deadline {
            if run_iter(
                &mut app,
                &mut first_frame,
                &mut last_clock,
                after_tick,
                &mut queue,
            )
            .expect("snapshot poll")
            {
                saw_snapshot_dirty = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(saw_snapshot_dirty);
        assert_eq!(app.mem_used(), 250_000);
    }
}
