mod actions;
mod app;
mod bucket;
mod config;
mod input;
mod search;
mod ui;

use anyhow::Result;
use app::{App, AsyncEvent};
use config::LoadedConfig;
use crossterm::event::{Event, EventStream, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::panic;
use tokio::sync::mpsc;

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}

fn install_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        restore_terminal();
        original_hook(panic_info);
    }));
}

#[tokio::main]
async fn main() -> Result<()> {
    install_panic_hook();

    let loaded_config = LoadedConfig::load_or_init()?;
    let mut terminal = setup_terminal()?;

    let (timeout_tx, mut timeout_rx) = mpsc::unbounded_channel::<u64>();
    let (async_tx, mut async_rx) = mpsc::unbounded_channel::<AsyncEvent>();

    let mut app = App::new(loaded_config, timeout_tx, async_tx);
    let mut events = EventStream::new();

    let run_result: Result<()> = async {
        loop {
            terminal.draw(|frame| ui::draw(frame, &app))?;

            if app.should_quit {
                break;
            }

            tokio::select! {
                maybe_event = events.next() => {
                    match maybe_event {
                        Some(Ok(Event::Key(key))) => {
                            if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat {
                                app.handle_key(key);
                            }
                        }
                        Some(Ok(Event::Resize(_, _))) => {
                            // Redraw on next loop iteration
                        }
                        Some(Err(_)) | None => break,
                        _ => {}
                    }
                }
                Some(gen) = timeout_rx.recv() => {
                    app.handle_chord_timeout(gen);
                }
                Some(async_ev) = async_rx.recv() => {
                    app.handle_async_event(async_ev);
                }
            }
        }
        Ok(())
    }
    .await;

    restore_terminal();
    run_result
}
