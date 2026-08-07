mod models;
mod ui;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle},
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
    // Set terminal window title
    execute!(stdout, SetTitle("Consortium ACP"))?;
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
    let mut last_transcript_refresh = std::time::Instant::now();

    loop {
        // Auto-refresh live statuses every iteration (throttled to 1/sec internally)
        app.refresh_live();

        // Auto-refresh transcript list every 5 seconds (picks up new consortiums)
        if last_transcript_refresh.elapsed() >= Duration::from_secs(5) {
            app.transcripts = models::Transcript::load_all();
            last_transcript_refresh = std::time::Instant::now();
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
                        match app.view_mode {
                            ViewMode::History => break,
                            ViewMode::Live => app.view_mode = ViewMode::LiveList,
                            _ => app.back(),
                        }
                    }
                    KeyCode::Char('h') => {
                        app.view_mode = ViewMode::History;
                    }
                    KeyCode::Char('l') => {
                        app.toggle_live();
                    }
                    KeyCode::Char('r') => {
                        app.transcripts = models::Transcript::load_all();
                    }
                    KeyCode::Down | KeyCode::Char('j') => match app.view_mode {
                        ViewMode::History => app.next(),
                        ViewMode::LiveList => app.live_next(),
                        ViewMode::Transcript => app.scroll_down(),
                        ViewMode::Live => app.scroll_down(),
                    },
                    KeyCode::Up | KeyCode::Char('k') => match app.view_mode {
                        ViewMode::History => app.previous(),
                        ViewMode::LiveList => app.live_previous(),
                        ViewMode::Transcript => app.scroll_up(),
                        ViewMode::Live => app.scroll_up(),
                    },
                    KeyCode::PageDown => {
                        match app.view_mode {
                            ViewMode::History => {
                                for _ in 0..5 { app.next(); }
                            }
                            ViewMode::LiveList => {
                                for _ in 0..5 { app.live_next(); }
                            }
                            ViewMode::Transcript => app.scroll_page_down(20),
                            ViewMode::Live => app.scroll_page_down(20),
                        }
                    }
                    KeyCode::PageUp => {
                        match app.view_mode {
                            ViewMode::History => {
                                for _ in 0..5 { app.previous(); }
                            }
                            ViewMode::LiveList => {
                                for _ in 0..5 { app.live_previous(); }
                            }
                            ViewMode::Transcript => app.scroll_page_up(20),
                            ViewMode::Live => app.scroll_page_up(20),
                        }
                    }
                    KeyCode::Home => match app.view_mode {
                        ViewMode::Transcript | ViewMode::Live => app.scroll_to_top(),
                        ViewMode::History => {
                            app.list_state.select(Some(0));
                            app.scroll = 0;
                        }
                        ViewMode::LiveList => {
                            if !app.live_statuses.is_empty() {
                                app.live_list_state.select(Some(0));
                            }
                        }
                    },
                    KeyCode::End => match app.view_mode {
                        ViewMode::Transcript | ViewMode::Live => app.scroll_to_bottom(),
                        ViewMode::History => {
                            if !app.transcripts.is_empty() {
                                app.list_state.select(Some(app.transcripts.len() - 1));
                                app.scroll = 0;
                            }
                        }
                        ViewMode::LiveList => {
                            if !app.live_statuses.is_empty() {
                                app.live_list_state.select(Some(app.live_statuses.len() - 1));
                            }
                        }
                    },
                    KeyCode::Enter => match app.view_mode {
                        ViewMode::History => app.open_transcript(),
                        ViewMode::LiveList => app.open_live(),
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
