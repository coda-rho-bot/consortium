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

    /// Short display topic (truncated)
    pub fn display_topic(&self) -> String {
        if self.topic.len() > 60 {
            format!("{}...", &self.topic[..57])
        } else {
            self.topic.clone()
        }
    }

    /// Display date
    pub fn display_date(&self) -> String {
        self.timestamp
            .map(|t| t.format("%b %d %H:%M").to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Display participants
    pub fn display_participants(&self) -> String {
        if self.participants.is_empty() {
            "?".to_string()
        } else {
            self.participants.join(", ")
        }
    }
}

/// Live consortium status (written by consortium.py during execution)
#[derive(Serialize, Deserialize, Clone)]
pub struct ConsortiumStatus {
    pub topic: String,
    pub started_at: String,
    pub participants: Vec<String>,
    pub max_messages: usize,
    pub status: String, // "running", "ended"
    pub messages: Vec<StatusMessage>,
    pub current_speakers: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct StatusMessage {
    pub sender: String,
    pub text: String,
    pub timestamp: String,
    pub msg_type: String, // "message", "pass", "system"
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
    // Format: YYYYMMDD-HHMMSS-slug.md
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
    // First line: "# Consortium: {topic}"
    for line in raw.lines() {
        if line.starts_with("# Consortium:") {
            return line["# Consortium:".len()..].trim().to_string();
        }
    }
    // Fallback: use filename slug
    "Untitled".to_string()
}

fn parse_participants(raw: &str) -> Vec<String> {
    // Line: "**Participants:** Alice, Bob, Charlie"
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

fn parse_messages(raw: &str) -> Vec<TranscriptMessage> {
    let mut messages = Vec::new();
    let mut in_messages = false;

    for line in raw.lines() {
        // Start of messages section
        if line.starts_with("---") && !in_messages {
            // Skip the first --- (header separator)
            in_messages = true;
            continue;
        }

        if !in_messages {
            continue;
        }

        // End of messages section
        if line.starts_with("---") && in_messages {
            break;
        }

        // Parse message line
        if line.starts_with("**[") {
            // Find the closing ]
            if let Some(end_bracket) = line.find("]**") {
                let sender = line[3..end_bracket].to_string();
                let rest = &line[end_bracket + 3..];

                let is_pass = rest.contains("PASS") || rest.contains("(explicitly passed)");
                let is_system = sender == "System";

                let text = if is_pass {
                    "PASS".to_string()
                } else {
                    rest.trim().to_string()
                };

                messages.push(TranscriptMessage {
                    sender,
                    text,
                    is_pass,
                    is_system,
                });
            }
        }
    }

    messages
}
