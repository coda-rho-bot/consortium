mod models;
mod ui;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

use ui::{App, ViewMode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run app
    let result = run_app(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();

    loop {
        // Check for live consortium status every render in live mode
        if app.view_mode == ViewMode::Live {
            app.refresh_live();
        }

        terminal.draw(|frame| ui::render(&mut app, frame))?;

        // Poll for events with short timeout (for live view auto-refresh)
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        if app.view_mode == ViewMode::History {
                            break;
                        } else {
                            app.back();
                        }
                    }
                    KeyCode::Char('h') => {
                        app.view_mode = ViewMode::History;
                    }
                    KeyCode::Char('l') => {
                        app.toggle_live();
                    }
                    KeyCode::Char('r') => {
                        // Refresh transcript list
                        app.transcripts = models::Transcript::load_all();
                    }
                    KeyCode::Down | KeyCode::Char('j') => match app.view_mode {
                        ViewMode::History => app.next(),
                        ViewMode::Transcript => app.scroll_down(),
                        ViewMode::Live => {}
                    },
                    KeyCode::Up | KeyCode::Char('k') => match app.view_mode {
                        ViewMode::History => app.previous(),
                        ViewMode::Transcript => app.scroll_up(),
                        ViewMode::Live => {}
                    },
                    KeyCode::PageDown => {
                        for _ in 0..5 {
                            match app.view_mode {
                                ViewMode::History => app.next(),
                                ViewMode::Transcript => app.scroll_down(),
                                _ => {}
                            }
                        }
                    }
                    KeyCode::PageUp => {
                        for _ in 0..5 {
                            match app.view_mode {
                                ViewMode::History => app.previous(),
                                ViewMode::Transcript => app.scroll_up(),
                                _ => {}
                            }
                        }
                    }
                    KeyCode::Enter => match app.view_mode {
                        ViewMode::History => app.open_transcript(),
                        _ => {}
                    },
                    KeyCode::Backspace => {
                        app.back();
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
