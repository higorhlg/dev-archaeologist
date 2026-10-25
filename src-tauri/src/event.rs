use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One observed fact from a local source. Events are evidence: they are never inferred.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkEvent {
    pub id: String,
    pub source: String,
    pub source_id: String,
    pub ts: DateTime<Utc>,
    pub kind: String,
    pub project: Option<String>,
    pub path: Option<String>,
    pub branch: Option<String>,
    pub detail: Option<String>,
    pub meta: Value,
}

impl WorkEvent {
    /// Builds an event with a fresh ULID. `source_id` must be stable across re-scans so
    /// that the store's `(source, source_id)` key removes duplicates.
    pub fn new(source: &str, source_id: impl Into<String>, ts: DateTime<Utc>, kind: &str) -> Self {
        Self {
            id: ulid::Ulid::new().to_string(),
            source: source.into(),
            source_id: source_id.into(),
            ts,
            kind: kind.into(),
            meta: Value::Null,
            ..Default::default()
        }
    }

    pub fn meta_i64(&self, key: &str) -> Option<i64> {
        self.meta.get(key).and_then(Value::as_i64)
    }

    /// Events the user caused directly; agent events alone do not prove the user was present.
    pub fn is_user_activity(&self) -> bool {
        matches!(
            self.kind.as_str(),
            kind::PROMPT | kind::SHELL_COMMAND | kind::FILE_CHANGED | kind::COMMIT | kind::APP_ACTIVE
        ) || (self.kind.starts_with("test_") && self.source == "shell")
    }
}

pub mod kind {
    pub const COMMIT: &str = "commit_created";
    pub const PROMPT: &str = "prompt";
    pub const TOOL_USE: &str = "tool_use";
    pub const AI_TITLE: &str = "ai_title";
    pub const AGENT_TURN: &str = "agent_turn";
    pub const TEST_PASSED: &str = "test_passed";
    pub const TEST_FAILED: &str = "test_failed";
    pub const SHELL_COMMAND: &str = "shell_command";
    pub const FILE_CHANGED: &str = "file_changed";
    pub const APP_ACTIVE: &str = "app_active";
    pub const IDLE_START: &str = "idle_start";
    pub const IDLE_END: &str = "idle_end";
    pub const MEETING: &str = "meeting";
    /// An agent session opened or pushed a pull request. `detail` is `repo#N`.
    pub const PR_LINK: &str = "pr_link";
}

/// Cuts a string to at most `max` chars on a char boundary.
pub fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}
