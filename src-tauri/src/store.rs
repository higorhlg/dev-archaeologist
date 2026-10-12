use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::corrections::Rule;
use crate::event::WorkEvent;
use crate::sessions::Session;

const SCHEMA: &str = include_str!("../migrations/0001_init.sql");

/// The one local database. Every collector writes through `insert_events`; the UI reads here.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Achievement {
    pub id: String,
    pub day: String,
    pub rule: String,
    pub title: String,
    pub detail: String,
    pub evidence_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalLine {
    pub text: String,
    pub cites: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Journal {
    pub day: String,
    pub lines: Vec<JournalLine>,
    pub state: String,
    pub model: Option<String>,
}

pub fn default_db_path() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("dev-archaeologist").join("devarch.db")
}

fn event_from_row(r: &rusqlite::Row) -> rusqlite::Result<WorkEvent> {
    let ts: i64 = r.get(3)?;
    let meta: Option<String> = r.get(9)?;
    Ok(WorkEvent {
        id: r.get(0)?,
        source: r.get(1)?,
        source_id: r.get(2)?,
        ts: Utc.timestamp_millis_opt(ts).single().unwrap_or_default(),
        kind: r.get(4)?,
        project: r.get(5)?,
        path: r.get(6)?,
        branch: r.get(7)?,
        detail: r.get(8)?,
        meta: meta.and_then(|m| serde_json::from_str(&m).ok()).unwrap_or_default(),
    })
}

const EVENT_COLS: &str = "id, source, source_id, ts, kind, project, path, branch, detail, meta";

fn session_from_row(r: &rusqlite::Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        day: r.get(1)?,
        start_ts: r.get(2)?,
        end_ts: r.get(3)?,
        project: r.get(4)?,
        branch: r.get(5)?,
        label: r.get(6)?,
        summary: r.get(7)?,
        active_min: r.get(8)?,
        agent_only_min: r.get(9)?,
        state: r.get(10)?,
        confidence: r.get(11)?,
        event_ids: Vec::new(),
    })
}

const SESSION_COLS: &str =
    "id, day, start_ts, end_ts, project, branch, label, summary, active_min, agent_only_min, state, confidence";

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    /// Inserts events and skips any `(source, source_id)` already stored.
    /// @returns the number of new rows.
    pub fn insert_events(&self, events: &[WorkEvent]) -> Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut n = 0;
        {
            let mut stmt = tx.prepare_cached(&format!(
                "INSERT OR IGNORE INTO work_events ({EVENT_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"
            ))?;
            for e in events {
                let meta = if e.meta.is_null() { None } else { Some(e.meta.to_string()) };
                n += stmt.execute(params![
                    e.id, e.source, e.source_id, e.ts.timestamp_millis(), e.kind, e.project, e.path, e.branch,
                    e.detail, meta
                ])?;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    pub fn events_between(&self, start_ms: i64, end_ms: i64) -> Result<Vec<WorkEvent>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {EVENT_COLS} FROM work_events WHERE ts >= ?1 AND ts < ?2 ORDER BY ts"
        ))?;
        let rows = stmt.query_map(params![start_ms, end_ms], event_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn events_by_ids(&self, ids: &[String]) -> Result<Vec<WorkEvent>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!("SELECT {EVENT_COLS} FROM work_events WHERE id = ?1"))?;
        let mut out = Vec::new();
        for id in ids {
            if let Some(e) = stmt.query_row([id], event_from_row).optional()? {
                out.push(e);
            }
        }
        out.sort_by_key(|e| e.ts);
        Ok(out)
    }

    pub fn session_event_ids(&self, session_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT event_id FROM session_events WHERE session_id = ?1")?;
        let rows = stmt.query_map([session_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Latest event time for one source, so a re-scan can start where the last one stopped.
    pub fn last_ts(&self, source: &str) -> Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("SELECT MAX(ts) FROM work_events WHERE source = ?1", [source], |r| r.get(0))?)
    }

    pub fn source_counts(&self) -> Result<Vec<(String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT source, COUNT(*) FROM work_events GROUP BY source ORDER BY 2 DESC")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn sessions_for_day(&self, day: &str) -> Result<Vec<Session>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare(&format!("SELECT {SESSION_COLS} FROM sessions WHERE day = ?1 ORDER BY start_ts"))?;
        let rows = stmt.query_map([day], session_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn sessions_between_days(&self, from: &str, to: &str) -> Result<Vec<Session>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SESSION_COLS} FROM sessions WHERE day >= ?1 AND day <= ?2 AND state != 'excluded' ORDER BY start_ts"
        ))?;
        let rows = stmt.query_map([from, to], session_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn session(&self, id: &str) -> Result<Option<Session>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(&format!("SELECT {SESSION_COLS} FROM sessions WHERE id = ?1"), [id], session_from_row)
            .optional()?)
    }

    /// Replaces the machine-made sessions of a day. Sessions the user touched
    /// (`edited`, `confirmed`, `excluded`) keep their own fields. A fresh session that
    /// matches an AI-labelled one (same id, or same project with an overlapping span)
    /// keeps that label and summary, so a rebuild never discards an inference.
    pub fn replace_day_sessions(&self, day: &str, fresh: &[Session]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let inferred: Vec<Session> = {
            let mut stmt =
                tx.prepare(&format!("SELECT {SESSION_COLS} FROM sessions WHERE day = ?1 AND state = 'inferred'"))?;
            let rows = stmt.query_map([day], session_from_row)?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let kept: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT id FROM sessions WHERE day = ?1 AND state IN ('edited','confirmed','excluded')",
            )?;
            let rows = stmt.query_map([day], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        tx.execute(
            "DELETE FROM session_events WHERE session_id IN (SELECT id FROM sessions WHERE day = ?1 AND state IN ('observed','inferred'))",
            [day],
        )?;
        tx.execute("DELETE FROM sessions WHERE day = ?1 AND state IN ('observed','inferred')", [day])?;
        for s in fresh.iter().filter(|s| !kept.contains(&s.id)) {
            let s = &carry_inference(s, &inferred);
            tx.execute(
                &format!("INSERT INTO sessions ({SESSION_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"),
                params![
                    s.id, s.day, s.start_ts, s.end_ts, s.project, s.branch, s.label, s.summary, s.active_min,
                    s.agent_only_min, s.state, s.confidence
                ],
            )?;
            for e in &s.event_ids {
                tx.execute("INSERT OR IGNORE INTO session_events VALUES (?1, ?2)", params![s.id, e])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn update_session(&self, s: &Session) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET start_ts=?2, end_ts=?3, project=?4, label=?5, summary=?6, active_min=?7, state=?8, confidence=?9 WHERE id=?1",
            params![s.id, s.start_ts, s.end_ts, s.project, s.label, s.summary, s.active_min, s.state, s.confidence],
        )?;
        Ok(())
    }

    pub fn rules(&self) -> Result<Vec<Rule>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT body FROM rules ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|b| b.ok().and_then(|b| serde_json::from_str(&b).ok())).collect())
    }

    pub fn add_rule(&self, rule: &Rule) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO rules (id, kind, body) VALUES (?1, ?2, ?3)",
            params![ulid::Ulid::new().to_string(), rule.kind(), serde_json::to_string(rule)?],
        )?;
        Ok(())
    }

    pub fn replace_achievements(&self, day: &str, list: &[Achievement]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM achievements WHERE day = ?1", [day])?;
        for a in list {
            tx.execute(
                "INSERT INTO achievements VALUES (?1,?2,?3,?4,?5,?6)",
                params![a.id, a.day, a.rule, a.title, a.detail, a.evidence_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn achievements(&self, day: &str) -> Result<Vec<Achievement>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, day, rule, title, detail, evidence_id FROM achievements WHERE day = ?1")?;
        let rows = stmt.query_map([day], |r| {
            Ok(Achievement {
                id: r.get(0)?,
                day: r.get(1)?,
                rule: r.get(2)?,
                title: r.get(3)?,
                detail: r.get(4)?,
                evidence_id: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn save_journal(&self, j: &Journal) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO journals VALUES (?1, ?2, ?3, ?4, ?5)",
            params![j.day, serde_json::to_string(&j.lines)?, j.state, j.model, Utc::now().timestamp_millis()],
        )?;
        Ok(())
    }

    pub fn journal(&self, day: &str) -> Result<Option<Journal>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row("SELECT day, body, state, model FROM journals WHERE day = ?1", [day], |r| {
                let body: String = r.get(1)?;
                Ok(Journal {
                    day: r.get(0)?,
                    lines: serde_json::from_str(&body).unwrap_or_default(),
                    state: r.get(2)?,
                    model: r.get(3)?,
                })
            })
            .optional()?)
    }

    /// Tickets the user set by hand, keyed by session id. An empty ticket means "none".
    pub fn ticket_overrides(&self) -> Result<std::collections::HashMap<String, String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT session_id, ticket FROM ticket_overrides")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// @param ticket `Some("")` pins "no ticket"; `None` removes the override so detection runs again.
    pub fn set_ticket_override(&self, session_id: &str, ticket: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        match ticket {
            Some(t) => conn.execute("INSERT OR REPLACE INTO ticket_overrides VALUES (?1, ?2)", params![session_id, t.trim()])?,
            None => conn.execute("DELETE FROM ticket_overrides WHERE session_id = ?1", [session_id])?,
        };
        Ok(())
    }

    pub fn toggl_sent_keys(&self) -> Result<std::collections::BTreeSet<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT entry_key FROM toggl_sync")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn record_toggl_sent(&self, key: &str, toggl_id: i64, duration_min: i64, description: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO toggl_sync VALUES (?1, ?2, ?3, ?4, ?5)",
            params![key, toggl_id, duration_min, description, Utc::now().timestamp_millis()],
        )?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT OR REPLACE INTO settings VALUES (?1, ?2)", params![key, value])?;
        Ok(())
    }

    /// Distinct days that hold events, newest first.
    pub fn event_days(&self, tz_offset_min: i32) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT date((ts / 1000) + ?1 * 60, 'unixepoch') AS d FROM work_events ORDER BY d DESC",
        )?;
        let rows = stmt.query_map([tz_offset_min], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn overlaps(a: &Session, b: &Session) -> bool {
    a.start_ts < b.end_ts && b.start_ts < a.end_ts
}

/// Copies the AI label and summary from the matching inferred session, if one exists.
fn carry_inference(fresh: &Session, inferred: &[Session]) -> Session {
    let hit = inferred
        .iter()
        .find(|old| old.id == fresh.id)
        .or_else(|| inferred.iter().find(|old| old.project.is_some() && old.project == fresh.project && overlaps(old, fresh)));
    match hit {
        Some(old) => Session {
            label: old.label.clone(),
            summary: old.summary.clone(),
            state: "inferred".into(),
            ..fresh.clone()
        },
        None => fresh.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sess(id: &str, project: &str, start: i64, end: i64, state: &str, label: &str) -> Session {
        Session {
            id: id.into(),
            day: "2026-10-12".into(),
            start_ts: start,
            end_ts: end,
            project: Some(project.into()),
            label: Some(label.into()),
            state: state.into(),
            ..Default::default()
        }
    }

    #[test]
    fn rebuild_keeps_ai_labels() {
        let store = Store::open_in_memory().unwrap();
        store.replace_day_sessions("2026-10-12", &[sess("a", "api", 0, 100, "inferred", "AI label")]).unwrap();
        let fresh = [sess("b", "api", 50, 200, "observed", "commit subject")];
        store.replace_day_sessions("2026-10-12", &fresh).unwrap();
        let got = store.sessions_for_day("2026-10-12").unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].label.as_deref(), Some("AI label"));
        assert_eq!(got[0].state, "inferred");
        assert_eq!(got[0].end_ts, 200);
    }
}
