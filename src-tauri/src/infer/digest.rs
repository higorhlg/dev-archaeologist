use std::collections::HashSet;

use chrono::{Local, TimeZone, Utc};
use serde::Serialize;

use crate::event::{kind, WorkEvent};
use crate::sessions::Session;
use crate::store::{Achievement, Store};

const MAX_PROMPTS: usize = 8;
const MAX_COMMITS: usize = 12;

/// The only data that leaves the machine: a short, redacted outline of one day.
/// Raw logs, file contents, tool output and paths never go into it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDigest {
    pub date: String,
    pub sessions: Vec<DigestSession>,
    pub achievements: Vec<DigestAchievement>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestSession {
    pub id: String,
    pub span: [String; 2],
    pub active_min: i64,
    pub agent_only_min: i64,
    pub project: Option<String>,
    pub branch: Option<String>,
    pub agent_titles: Vec<String>,
    pub agent_prompts: Vec<DigestItem>,
    pub commits: Vec<DigestCommit>,
    pub tests: DigestTests,
    pub apps: Vec<String>,
    /// The ticket the user set or accepted for this session. Absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket: Option<DigestTicket>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DigestTicket {
    pub key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DigestItem {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DigestCommit {
    pub id: String,
    pub msg: String,
    pub files: i64,
    pub plus: i64,
    pub minus: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestTests {
    pub passed_runs: usize,
    pub failed_runs: usize,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DigestAchievement {
    pub id: String,
    pub rule: String,
    pub detail: String,
}

fn hhmm(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms).single().map(|t| t.with_timezone(&Local).format("%H:%M").to_string()).unwrap_or_default()
}

/// Builds the digest for a day. Excluded sessions and opted-out projects are left out.
/// A session carries a ticket only when the user set or accepted one.
pub fn build(store: &Store, day: &str, opt_out: &[String]) -> anyhow::Result<SessionDigest> {
    let confirmed = store.ticket_overrides()?;
    let info = store.ticket_info()?;
    let mut sessions = Vec::new();
    for s in store.sessions_for_day(day)? {
        if s.state == "excluded" || s.project.as_ref().is_some_and(|p| opt_out.contains(p)) {
            continue;
        }
        let events = store.events_by_ids(&store.session_event_ids(&s.id)?)?;
        let mut d = digest_session(&s, &events);
        d.ticket = confirmed.get(&s.id).filter(|k| !k.is_empty()).map(|k| DigestTicket {
            key: k.clone(),
            title: info.get(k).map(|i| i.title.clone()).unwrap_or_default(),
        });
        sessions.push(d);
    }
    let achievements = store
        .achievements(day)?
        .into_iter()
        .map(|a: Achievement| DigestAchievement { id: a.id, rule: a.rule, detail: a.detail })
        .collect();
    Ok(SessionDigest { date: day.to_string(), sessions, achievements })
}

fn of_kind<'a>(events: &'a [WorkEvent], k: &'a str) -> impl Iterator<Item = &'a WorkEvent> + 'a {
    events.iter().filter(move |e| e.kind == k)
}

fn digest_session(s: &Session, events: &[WorkEvent]) -> DigestSession {
    let of = |k: &'static str| of_kind(events, k);
    let mut titles: Vec<String> = of(kind::AI_TITLE).filter_map(|e| e.detail.clone()).collect();
    titles.dedup();
    let mut apps: Vec<String> = of(kind::APP_ACTIVE).filter_map(|e| e.detail.clone()).collect::<HashSet<_>>().into_iter().collect();
    apps.sort();
    let tests_ok: Vec<&WorkEvent> = of(kind::TEST_PASSED).collect();
    let tests_bad: Vec<&WorkEvent> = of(kind::TEST_FAILED).collect();
    DigestSession {
        id: s.id.clone(),
        span: [hhmm(s.start_ts), hhmm(s.end_ts)],
        active_min: s.active_min,
        agent_only_min: s.agent_only_min,
        project: s.project.clone(),
        branch: s.branch.clone(),
        agent_titles: titles,
        agent_prompts: of(kind::PROMPT)
            .take(MAX_PROMPTS)
            .map(|e| DigestItem { id: e.id.clone(), text: e.detail.clone().unwrap_or_default() })
            .collect(),
        commits: of(kind::COMMIT)
            .take(MAX_COMMITS)
            .map(|e| DigestCommit {
                id: e.id.clone(),
                msg: e.detail.clone().unwrap_or_default(),
                files: e.meta_i64("files").unwrap_or(0),
                plus: e.meta_i64("plus").unwrap_or(0),
                minus: e.meta_i64("minus").unwrap_or(0),
            })
            .collect(),
        tests: DigestTests {
            passed_runs: tests_ok.len(),
            failed_runs: tests_bad.len(),
            evidence_ids: tests_bad.iter().chain(tests_ok.iter()).take(6).map(|e| e.id.clone()).collect(),
        },
        apps,
        ticket: None,
    }
}

impl SessionDigest {
    /// Every id the model may cite: sessions, prompts, commits, test runs, achievements.
    pub fn evidence_ids(&self) -> HashSet<String> {
        let mut ids = HashSet::new();
        for s in &self.sessions {
            ids.insert(s.id.clone());
            ids.extend(s.agent_prompts.iter().map(|p| p.id.clone()));
            ids.extend(s.commits.iter().map(|c| c.id.clone()));
            ids.extend(s.tests.evidence_ids.iter().cloned());
        }
        ids.extend(self.achievements.iter().map(|a| a.id.clone()));
        ids
    }
}
