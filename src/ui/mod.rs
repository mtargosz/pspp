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
use std::time::Instant;

use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

pub use app::{
    parse_agg_mode_config, parse_mem_kind_config, parse_sort_config, sort_to_config,
    GraphSymbolSetting, UiSettings,
};

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
    // when toggled in Options) at the top of every iteration.
    let mut mouse_captured = true;

    while !app.should_quit {
        sync_mouse_capture(&mut mouse_captured, app.mouse_enabled())?;
        app.poll_snapshot();
        terminal.draw(|frame| draw::draw(frame, &mut app))?;

        let timeout = app.poll_timeout(Instant::now());
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key_event) => app.on_key(key_event),
                Event::Mouse(mouse_event) => app.on_mouse(mouse_event),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if app.should_tick(Instant::now()) {
            app.on_tick();
        }
    }

    drop(terminal);
    terminal_guard.restore()?;
    Ok(app.ui_settings())
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
