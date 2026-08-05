use chrono::{DateTime, Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// A parsed consortium transcript
#[derive(Clone)]
pub struct Transcript {
    pub path: PathBuf,
    pub filename: String,
    pub timestamp: Option<DateTime<Local>>,
    pub topic: String,
    pub participants: Vec<String>,
    pub max_messages: usize,
    pub messages: Vec<TranscriptMessage>,
    pub raw_markdown: String,
}

#[derive(Clone)]
pub struct TranscriptMessage {
    pub sender: String,
    pub text: String,
    pub is_pass: bool,
    pub is_system: bool,
}

impl Transcript {
    /// Load all transcripts from ~/consortium-transcripts/
    pub fn load_all() -> Vec<Transcript> {
        let dir = dirs();
        let mut transcripts: Vec<Transcript> = Vec::new();

        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "md") {
                    if let Ok(t) = Self::load_file(&path) {
                        transcripts.push(t);
                    }
                }
            }
        }

        // Sort by timestamp descending (newest first)
        transcripts.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        transcripts
    }

    /// Load a single transcript file
    pub fn load_file(path: &Path) -> Result<Transcript, Box<dyn std::error::Error>> {
        let raw = fs::read_to_string(path)?;
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let timestamp = parse_timestamp_from_filename(&filename);
        let topic = parse_topic(&raw);
        let participants = parse_participants(&raw);
        let max_messages = parse_max_messages(&raw);
        let messages = parse_messages(&raw);

        Ok(Transcript {
            path: path.to_path_buf(),
            filename,
            timestamp,
            topic,
            participants,
            max_messages,
            messages,
            raw_markdown: raw,
        })
    }

    pub fn display_topic(&self) -> String {
        let t = self.topic.replace('\n', " ");
        let chars: Vec<char> = t.chars().collect();
        if chars.len() > 60 {
            let truncated: String = chars[..57].iter().collect();
            format!("{}...", truncated)
        } else {
            t
        }
    }

    pub fn display_date(&self) -> String {
        self.timestamp
            .map(|t| t.format("%b %d %H:%M").to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    pub fn display_participants(&self) -> String {
        if self.participants.is_empty() {
            "?".to_string()
        } else {
            self.participants.join(", ")
        }
    }
}

/// Live consortium status
#[derive(Serialize, Deserialize, Clone)]
pub struct ConsortiumStatus {
    pub topic: String,
    pub started_at: String,
    pub participants: Vec<String>,
    pub max_messages: usize,
    pub status: String,
    pub messages: Vec<StatusMessage>,
    pub current_speakers: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct StatusMessage {
    pub sender: String,
    pub text: String,
    pub timestamp: String,
    pub msg_type: String,
}

impl ConsortiumStatus {
    pub fn load_live() -> Option<ConsortiumStatus> {
        let path = status_file_path();
        if !path.exists() {
            return None;
        }
        match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).ok(),
            Err(_) => None,
        }
    }
}

fn dirs() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/home/rhomancer".to_string()))
        .join("consortium-transcripts")
}

pub fn status_file_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/home/rhomancer".to_string()))
        .join(".consortium-status.json")
}

fn parse_timestamp_from_filename(filename: &str) -> Option<DateTime<Local>> {
    let parts: Vec<&str> = filename.splitn(3, '-').collect();
    if parts.len() >= 2 {
        let datetime_str = format!("{}-{}", parts[0], parts[1]);
        if let Ok(dt) = NaiveDateTime::parse_from_str(&datetime_str, "%Y%m%d-%H%M%S") {
            return Some(dt.and_utc().with_timezone(&Local));
        }
    }
    None
}

fn parse_topic(raw: &str) -> String {
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# Consortium:") {
            return trimmed["# Consortium:".len()..].trim().to_string();
        }
    }
    "Untitled".to_string()
}

fn parse_participants(raw: &str) -> Vec<String> {
    for line in raw.lines() {
        if line.starts_with("**Participants:**") {
            let rest = &line["**Participants:**".len()..];
            return rest
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    Vec::new()
}

fn parse_max_messages(raw: &str) -> usize {
    for line in raw.lines() {
        if line.starts_with("**Max messages per agent:**") {
            let rest = &line["**Max messages per agent:**".len()..];
            return rest.trim().parse().unwrap_or(0);
        }
    }
    0
}

/// Parse messages from transcript markdown.
/// Messages start with **[Sender]** and can span multiple lines
/// until the next **[ or --- or end of file.
fn parse_messages(raw: &str) -> Vec<TranscriptMessage> {
    let mut messages = Vec::new();
    let mut in_messages = false;
    let mut current_sender: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();

    // Agent name mapping for short IDs
    let name_map = [
        ("agent-b499137a", "Coda"),
        ("agent-c51de213", "Angus"),
        ("agent-e6f1a549", "Beacon"),
        ("agent-2ee946fb", "FORGE"),
        ("agent-5b2254e8", "Sinter"),
        ("agent-8c1f9353", "Linus"),
    ];

    let resolve_name = |raw: &str| -> String {
        for (id, name) in &name_map {
            if raw.starts_with(id) || raw.contains(id) {
                return name.to_string();
            }
        }
        raw.to_string()
    };

    let flush = |messages: &mut Vec<TranscriptMessage>,
                 sender: &mut Option<String>,
                 lines: &mut Vec<String>| {
        if let Some(s) = sender.take() {
            let text = lines.join("\n");
            lines.clear();

            let is_pass = text.trim() == "PASS"
                || text.contains("PASS")
                || text.contains("(explicitly passed)")
                || text.contains("(PASS");
            let is_system = s == "System";

            messages.push(TranscriptMessage {
                sender: s,
                text: text.trim().to_string(),
                is_pass,
                is_system,
            });
        }
    };

    for line in raw.lines() {
        let trimmed = line.trim();

        // Detect first --- separator (start of messages section)
        if trimmed.starts_with("---") && !in_messages {
            // Flush any pre-message content
            flush(&mut messages, &mut current_sender, &mut current_lines);
            in_messages = true;
            continue;
        }

        if !in_messages {
            continue;
        }

        // End of messages section (second ---)
        if trimmed.starts_with("---") && in_messages {
            flush(&mut messages, &mut current_sender, &mut current_lines);
            break;
        }

        // New message starts with **[Sender]**
        if trimmed.starts_with("**[") {
            // Flush previous message
            flush(&mut messages, &mut current_sender, &mut current_lines);

            // Parse sender from **[Sender]**
            if let Some(end_bracket) = trimmed.find("]**") {
                let raw_sender = &trimmed[3..end_bracket];
                let sender = resolve_name(raw_sender);
                let rest = trimmed[end_bracket + 3..].trim().to_string();

                current_sender = Some(sender);
                if !rest.is_empty() {
                    current_lines.push(rest);
                }
            }
            continue;
        }

        // Continuation line for current message
        if current_sender.is_some() {
            current_lines.push(line.to_string());
        }
    }

    // Flush last message
    flush(&mut messages, &mut current_sender, &mut current_lines);

    messages
}
