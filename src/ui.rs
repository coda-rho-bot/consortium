use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::models::{ConsortiumStatus, Transcript};

/// App state
pub struct App {
    pub transcripts: Vec<Transcript>,
    pub list_state: ListState,
    pub selected: Option<usize>,
    pub view_mode: ViewMode,
    pub scroll: usize,
    pub live_status: Option<ConsortiumStatus>,
    pub live_last_check: std::time::Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    History,
    Transcript,
    Live,
}

impl App {
    pub fn new() -> Self {
        let transcripts = Transcript::load_all();
        let mut list_state = ListState::default();
        if !transcripts.is_empty() {
            list_state.select(Some(0));
        }

        Self {
            transcripts,
            list_state,
            selected: None,
            view_mode: ViewMode::History,
            scroll: 0,
            live_status: None,
            live_last_check: std::time::Instant::now(),
        }
    }

    pub fn current_transcript(&self) -> Option<&Transcript> {
        self.list_state
            .selected()
            .and_then(|i| self.transcripts.get(i))
    }

    pub fn refresh_live(&mut self) {
        if self.live_last_check.elapsed() < std::time::Duration::from_secs(1) {
            return;
        }
        self.live_last_check = std::time::Instant::now();
        self.live_status = ConsortiumStatus::load_live();
    }

    pub fn next(&mut self) {
        if self.transcripts.is_empty() {
            return;
        }
        let i = self.list_state.selected().map_or(0, |i| {
            (i + 1).min(self.transcripts.len() - 1)
        });
        self.list_state.select(Some(i));
        self.scroll = 0;
    }

    pub fn previous(&mut self) {
        if self.transcripts.is_empty() {
            return;
        }
        let i = self.list_state
            .selected()
            .map_or(0, |i| if i == 0 { 0 } else { i - 1 });
        self.list_state.select(Some(i));
        self.scroll = 0;
    }

    pub fn open_transcript(&mut self) {
        if self.current_transcript().is_some() {
            self.view_mode = ViewMode::Transcript;
            self.scroll = 0;
        }
    }

    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn back(&mut self) {
        self.view_mode = ViewMode::History;
    }

    pub fn toggle_live(&mut self) {
        match self.view_mode {
            ViewMode::Live => self.view_mode = ViewMode::History,
            _ => {
                self.view_mode = ViewMode::Live;
                self.refresh_live();
            }
        }
    }
}

pub fn render(app: &mut App, frame: &mut Frame) {
    let area = frame.area();

    // Main layout: content + status bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(area);

    match app.view_mode {
        ViewMode::History => render_history(app, frame, chunks[0]),
        ViewMode::Transcript => render_transcript_viewer(app, frame, chunks[0]),
        ViewMode::Live => render_live(app, frame, chunks[0]),
    }

    render_status_bar(app, frame, chunks[1]);
}

fn render_history(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Left: transcript list
    let items: Vec<ListItem> = app
        .transcripts
        .iter()
        .map(|t| {
            let date = t.display_date();
            let topic = t.display_topic();
            let msg_count = t.messages.len();

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<12} ", date),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(topic),
                Span::styled(
                    format!(" ({} msgs)", msg_count),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Consortium History ")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .scroll_padding(3);

    frame.render_stateful_widget(list, chunks[0], &mut app.list_state);

    // Right: preview of selected transcript
    if let Some(t) = app.current_transcript() {
        let preview = build_preview(t);
        let para = Paragraph::new(preview)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", t.display_topic()))
                    .border_style(Style::default().fg(Color::Gray)),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(para, chunks[1]);
    } else {
        let para = Paragraph::new("No consortiums found.\n\nStart one with:\npython3 ~/dev/infra/consortium/consortium.py --topic \"...\" --config agents.yaml")
            .block(Block::default().borders(Borders::ALL).title(" Preview "));
        frame.render_widget(para, chunks[1]);
    }
}

fn build_preview(t: &Transcript) -> Text {
    let mut lines = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("Date: ", Style::default().fg(Color::Yellow)),
        Span::raw(t.display_date()),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Participants: ", Style::default().fg(Color::Yellow)),
        Span::raw(t.display_participants()),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Max msgs: ", Style::default().fg(Color::Yellow)),
        Span::raw(t.max_messages.to_string()),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Messages: ", Style::default().fg(Color::Yellow)),
        Span::raw(t.messages.len().to_string()),
    ]));

    lines.push(Line::from(""));

    // Show first 15 messages
    for msg in t.messages.iter().take(15) {
        if msg.is_system {
            lines.push(Line::from(Span::styled(
                format!("  {}", msg.text),
                Style::default().fg(Color::DarkGray),
            )));
        } else if msg.is_pass {
            lines.push(Line::from(Span::styled(
                format!("  [{}] PASS", msg.sender),
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            let sender_color = sender_color(&msg.sender);
            lines.push(Line::from(vec![
                Span::styled(
                    format!("[{}] ", msg.sender),
                    Style::default().fg(sender_color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(truncate(&msg.text, 80)),
            ]));
        }
    }

    if t.messages.len() > 15 {
        lines.push(Line::from(Span::styled(
            format!("\n  ... and {} more", t.messages.len() - 15),
            Style::default().fg(Color::DarkGray),
        )));
    }

    Text::from(lines)
}

fn render_transcript_viewer(app: &mut App, frame: &mut Frame, area: Rect) {
    let transcript = match app.current_transcript() {
        Some(t) => t.clone(),
        None => {
            let para = Paragraph::new("No transcript selected");
            frame.render_widget(para, area);
            return;
        }
    };

    let mut lines: Vec<Line> = Vec::new();

    // ── Full details header ──
    lines.push(Line::from(Span::styled(
        &transcript.topic,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )));
    lines.push(Line::from(""));

    lines.push(Line::from(vec![
        Span::styled("Date:           ", Style::default().fg(Color::Yellow)),
        Span::raw(transcript.display_date()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Participants:   ", Style::default().fg(Color::Yellow)),
        Span::raw(transcript.display_participants()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Max messages:   ", Style::default().fg(Color::Yellow)),
        Span::raw(transcript.max_messages.to_string()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Total messages: ", Style::default().fg(Color::Yellow)),
        Span::raw(transcript.messages.len().to_string()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("File:           ", Style::default().fg(Color::Yellow)),
        Span::raw(transcript.filename.clone()),
    ]));
    lines.push(Line::from(Span::styled(
        "─────────────────────────────────────────────────────",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    // ── Messages (let ratatui handle all wrapping) ──
    for msg in &transcript.messages {
        if msg.is_system {
            lines.push(Line::from(Span::styled(
                format!("── {} ──", msg.text),
                Style::default().fg(Color::DarkGray),
            )));
        } else if msg.is_pass {
            lines.push(Line::from(Span::styled(
                format!("  [{}] PASS", msg.sender),
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            )));
        } else {
            let color = sender_color(&msg.sender);
            // Split multi-line messages into separate Line objects
            for (i, line_text) in msg.text.lines().enumerate() {
                if i == 0 {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("[{}]: ", msg.sender),
                            Style::default().fg(color).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(line_text.to_string()),
                    ]));
                } else {
                    lines.push(Line::from(Span::raw(line_text.to_string())));
                }
            }
            // If message was empty after stripping, still show the sender
            if msg.text.lines().count() == 0 {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("[{}]: ", msg.sender),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
        }
        lines.push(Line::from("")); // spacing between messages
    }

    // ratatui Paragraph handles scrolling internally — don't clamp,
    // just pass the scroll offset. Line wrapping is also handled by ratatui.
    let para = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", transcript.display_topic()))
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.scroll as u16, 0));

    frame.render_widget(para, area);
}

fn render_live(app: &mut App, frame: &mut Frame, area: Rect) {
    app.refresh_live();

    let status = match &app.live_status {
        Some(s) => s,
        None => {
            let para = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "No consortium in progress.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from("Start one with:"),
                Line::from(Span::styled(
                    "  python3 ~/dev/infra/consortium/consortium.py --topic \"...\" --config agents.yaml",
                    Style::default().fg(Color::Green),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Press L to check again, H for history",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Live Consortium ")
                    .border_style(Style::default().fg(Color::Yellow)),
            );
            frame.render_widget(para, area);
            return;
        }
    };

    let mut lines: Vec<Line> = Vec::new();

    // Header
    lines.push(Line::from(Span::styled(
        &status.topic,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )));

    lines.push(Line::from(Span::styled(
        format!(
            "Started: {} | Participants: {} | Status: {}",
            status.started_at,
            status.participants.join(", "),
            status.status
        ),
        Style::default().fg(Color::DarkGray),
    )));

    if !status.current_speakers.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Thinking: ", Style::default().fg(Color::Yellow)),
            Span::styled(
                status.current_speakers.join(", "),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    lines.push(Line::from(""));

    // Messages
    for msg in &status.messages {
        let color = sender_color(&msg.sender);

        if msg.msg_type == "pass" {
            lines.push(Line::from(Span::styled(
                format!("  [{}] PASS", msg.sender),
                Style::default().fg(Color::DarkGray),
            )));
        } else if msg.msg_type == "system" {
            lines.push(Line::from(Span::styled(
                format!("── {} ──", msg.text),
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("[{}]: ", msg.sender),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(&msg.text),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Auto-scroll to bottom for live view
    let para = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Live Consortium ({} msgs) ", status.messages.len()))
                .border_style(Style::default().fg(Color::Green)),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.scroll as u16, 0));

    frame.render_widget(para, area);
}

fn render_status_bar(app: &App, frame: &mut Frame, area: Rect) {
    let mode_text = match app.view_mode {
        ViewMode::History => "HISTORY",
        ViewMode::Transcript => "TRANSCRIPT",
        ViewMode::Live => "LIVE",
    };

    let live_indicator = if app.live_status.is_some() {
        " ● LIVE"
    } else {
        ""
    };

    let line = Line::from(vec![
        Span::styled(
            format!(" {} ", mode_text),
            Style::default().bg(Color::Cyan).fg(Color::Black),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{} consortiums", app.transcripts.len()),
            Style::default().fg(Color::Gray),
        ),
        Span::raw(" │ "),
        Span::styled("↑↓ navigate", Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled("Enter view", Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled("L live", Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled("H history", Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled("q quit", Style::default().fg(Color::DarkGray)),
        Span::styled(live_indicator, Style::default().fg(Color::Green)),
    ]);

    let para = Paragraph::new(line).style(Style::default().bg(Color::Black));
    frame.render_widget(para, area);
}

fn sender_color(name: &str) -> Color {
    match name.to_lowercase().as_str() {
        "coda" => Color::Cyan,
        "angus" => Color::Red,
        "beacon" => Color::Blue,
        "forge" => Color::Green,
        "sinter" => Color::Magenta,
        "human" => Color::Yellow,
        _ => Color::White,
    }
}

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > max {
        let truncated: String = chars[..max.saturating_sub(3)].iter().collect();
        format!("{}...", truncated)
    } else {
        s.to_string()
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }

    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        let mut current_line = String::new();
        for word in paragraph.split_whitespace() {
            // Use char count, not byte length
            let current_chars = current_line.chars().count();
            let word_chars = word.chars().count();

            if current_line.is_empty() {
                current_line = word.to_string();
            } else if current_chars + 1 + word_chars <= width {
                current_line.push(' ');
                current_line.push_str(word);
            } else {
                if !current_line.is_empty() {
                    result.push(current_line.clone());
                }
                current_line = word.to_string();
            }
        }
        result.push(current_line);
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}
