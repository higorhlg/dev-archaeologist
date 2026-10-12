pub mod attribute;
pub mod cluster;

use serde::{Deserialize, Serialize};

pub use cluster::cluster_sessions;

/// A block of work on one project. `state` follows observed → inferred → edited → confirmed | excluded.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub day: String,
    pub start_ts: i64,
    pub end_ts: i64,
    pub project: Option<String>,
    pub branch: Option<String>,
    pub label: Option<String>,
    pub summary: Option<String>,
    pub active_min: i64,
    pub agent_only_min: i64,
    pub state: String,
    pub confidence: Option<String>,
    #[serde(default)]
    pub event_ids: Vec<String>,
}

impl Session {
    pub fn is_user_touched(&self) -> bool {
        matches!(self.state.as_str(), "edited" | "confirmed" | "excluded")
    }
}

/// FNV-1a: a hash that stays the same across builds, for session ids.
pub fn stable_hash(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        for b in p.bytes().chain(std::iter::once(0u8)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("s{h:016x}")
}
