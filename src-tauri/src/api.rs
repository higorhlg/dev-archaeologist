//! View models and actions shared by the Tauri commands and the CLI.

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use chrono::{Duration, Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::backfill::{self, BackfillReport, Discovery};
use crate::corrections::Rule;
use crate::event::WorkEvent;
use crate::infer::journal;
use crate::pipeline::Pipeline;
use crate::sessions::{attribute::attribute, cluster::evidence_counts, Session};
use crate::settings::Settings;
use crate::reports::{self, Report, ReportQuery};
use crate::store::{Achievement, Journal};
use crate::toggl;

#[derive(Debug, Serialize)]
pub struct Chip {
    pub label: String,
    pub n: usize,
}

#[derive(Debug, Serialize)]
pub struct SessionView {
    #[serde(flatten)]
    pub session: Session,
    pub chips: Vec<Chip>,
}

#[derive(Debug, Serialize)]
pub struct DayView {
    pub day: String,
    pub active_min: i64,
    pub sessions: Vec<SessionView>,
    pub achievements: Vec<Achievement>,
    pub journal: Option<Journal>,
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub event_counts: Vec<(String, i64)>,
    pub days: Vec<String>,
    pub has_api_key: bool,
    pub has_toggl_token: bool,
    pub settings: Settings,
}

#[derive(Debug, Serialize)]
pub struct ProjectRow {
    pub project: String,
    pub total_min: i64,
    /// Active minutes per day, keyed `YYYY-MM-DD`.
    pub days: BTreeMap<String, i64>,
}

/// Edit fields the user can change on a session. `apply_rule` turns the edit into a rule.
#[derive(Debug, Deserialize)]
pub struct SessionEdit {
    pub label: Option<String>,
    pub project: Option<String>,
    pub active_min: Option<i64>,
    #[serde(default)]
    pub apply_rule: bool,
}

pub fn status(p: &Pipeline) -> Result<Status> {
    let offset = Local::now().offset().local_minus_utc() / 60;
    Ok(Status {
        event_counts: p.store.source_counts()?,
        days: p.store.event_days(offset)?,
        has_api_key: journal::has_api_key(),
        has_toggl_token: has_toggl_token(),
        settings: Settings::load(&p.store)?,
    })
}

pub fn discover(p: &Pipeline, days: i64) -> Result<Discovery> {
    Ok(backfill::discover(&Settings::load(&p.store)?, Utc::now() - Duration::days(days)))
}

pub fn run_backfill(p: &Pipeline, days: i64) -> Result<BackfillReport> {
    backfill::backfill(p, Utc::now() - Duration::days(days))
}

/// Day total: every minute counts once across overlapping sessions. A session whose
/// minutes the user edited adds its edited value; excluded sessions add nothing.
fn day_active_min(sessions: &[(Session, Vec<WorkEvent>)]) -> i64 {
    let machine: Vec<&WorkEvent> = sessions
        .iter()
        .filter(|(s, _)| s.state != "excluded" && s.state != "edited")
        .flat_map(|(_, evs)| evs.iter())
        .collect();
    let edited: i64 = sessions.iter().filter(|(s, _)| s.state == "edited").map(|(s, _)| s.active_min).sum();
    attribute(&machine).0 + edited
}

pub fn day_view(p: &Pipeline, day: &str) -> Result<DayView> {
    let mut loaded = Vec::new();
    for s in p.store.sessions_for_day(day)? {
        let events = p.store.events_by_ids(&p.store.session_event_ids(&s.id)?)?;
        loaded.push((s, events));
    }
    let active = day_active_min(&loaded);
    let sessions = loaded
        .into_iter()
        .map(|(s, events)| {
            let refs: Vec<&WorkEvent> = events.iter().collect();
            let chips = evidence_counts(&refs).into_iter().map(|(label, n)| Chip { label, n }).collect();
            SessionView { session: s, chips }
        })
        .collect();
    Ok(DayView {
        day: day.to_string(),
        active_min: active,
        sessions,
        achievements: p.store.achievements(day)?,
        journal: p.store.journal(day)?,
    })
}

pub fn session_events(p: &Pipeline, session_id: &str) -> Result<Vec<WorkEvent>> {
    p.store.events_by_ids(&p.store.session_event_ids(session_id)?)
}

pub fn projects(p: &Pipeline, from: &str, to: &str) -> Result<Vec<ProjectRow>> {
    let mut rows: BTreeMap<String, ProjectRow> = BTreeMap::new();
    for s in p.store.sessions_between_days(from, to)? {
        let name = s.project.clone().unwrap_or_else(|| "Other".into());
        let row = rows.entry(name.clone()).or_insert(ProjectRow { project: name, total_min: 0, days: BTreeMap::new() });
        row.total_min += s.active_min;
        *row.days.entry(s.day.clone()).or_default() += s.active_min;
    }
    let mut out: Vec<ProjectRow> = rows.into_values().collect();
    out.sort_by_key(|r| -r.total_min);
    Ok(out)
}

fn load_session(p: &Pipeline, id: &str) -> Result<Session> {
    p.store.session(id)?.ok_or_else(|| anyhow!("no session {id}"))
}

pub fn confirm_session(p: &Pipeline, id: &str) -> Result<()> {
    let mut s = load_session(p, id)?;
    s.state = "confirmed".into();
    p.store.update_session(&s)
}

pub fn exclude_session(p: &Pipeline, id: &str) -> Result<()> {
    let mut s = load_session(p, id)?;
    s.state = "excluded".into();
    p.store.update_session(&s)
}

/// Applies a user edit. With `apply_rule`, a relabel becomes a branch rule and a project
/// change becomes a path rule, so later sessions get the same correction.
pub fn edit_session(p: &Pipeline, id: &str, edit: SessionEdit) -> Result<()> {
    let mut s = load_session(p, id)?;
    if edit.apply_rule {
        save_rules_for_edit(p, &s, &edit)?;
    }
    if let Some(l) = edit.label.filter(|l| !l.trim().is_empty()) {
        s.label = Some(l);
    }
    if let Some(proj) = edit.project.filter(|x| !x.trim().is_empty()) {
        s.project = Some(proj);
    }
    if let Some(m) = edit.active_min {
        s.active_min = m.max(0);
    }
    s.state = "edited".into();
    s.confidence = Some("high".into());
    p.store.update_session(&s)
}

fn save_rules_for_edit(p: &Pipeline, s: &Session, edit: &SessionEdit) -> Result<()> {
    if let (Some(label), Some(project), Some(branch)) = (&edit.label, &s.project, &s.branch) {
        p.store.add_rule(&Rule::LabelForBranch { project: project.clone(), branch: branch.clone(), label: label.clone() })?;
    }
    if let Some(new_project) = &edit.project {
        let events = session_events(p, &s.id)?;
        if let Some(prefix) = events.iter().find_map(|e| e.path.clone()).and_then(|path| {
            crate::project::find_repo_root(std::path::Path::new(&path)).map(|r| r.to_string_lossy().into_owned())
        }) {
            p.store.add_rule(&Rule::ProjectForPath { prefix, project: new_project.clone() })?;
        }
    }
    Ok(())
}

pub fn generate_journal(p: &Pipeline, day: &str) -> Result<Journal> {
    journal::generate(&p.store, day)
}

pub fn confirm_journal(p: &Pipeline, day: &str) -> Result<()> {
    let mut j = p.store.journal(day)?.ok_or_else(|| anyhow!("no journal for {day}"))?;
    j.state = "confirmed".into();
    p.store.save_journal(&j)
}

pub fn rebuild_day(p: &Pipeline, day: &str) -> Result<usize> {
    p.rebuild_day(NaiveDate::parse_from_str(day, "%Y-%m-%d")?)
}

pub fn save_settings(p: &Pipeline, s: &Settings) -> Result<()> {
    s.save(&p.store)
}

/// Writes the zsh hook to `~/.dev-arch/dev-arch.zsh`. The user adds the `source` line
/// to `~/.zshrc` themselves; the app never edits shell startup files.
/// @returns the line to add.
pub fn install_shell_hook() -> Result<String> {
    let dir = dirs::home_dir().ok_or_else(|| anyhow!("no home folder"))?.join(".dev-arch");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("dev-arch.zsh"), crate::collectors::shell::ZSH_HOOK)?;
    Ok("source ~/.dev-arch/dev-arch.zsh".into())
}

pub fn report(p: &Pipeline, q: &ReportQuery) -> Result<Report> {
    reports::build(&p.store, q)
}

/// Approving an entry confirms every session in it.
pub fn approve_sessions(p: &Pipeline, ids: &[String]) -> Result<()> {
    ids.iter().try_for_each(|id| confirm_session(p, id))
}

/// Confirms the high-confidence sessions in the range that have a project.
/// @returns how many sessions it confirmed.
pub fn approve_confident(p: &Pipeline, q: &ReportQuery) -> Result<usize> {
    let ids = reports::confident_session_ids(&p.store, q)?;
    approve_sessions(p, &ids)?;
    Ok(ids.len())
}

/// Sets the description of a one-session entry. Edits the session label.
pub fn set_entry_label(p: &Pipeline, session_id: &str, label: &str) -> Result<()> {
    edit_session(p, session_id, SessionEdit { label: Some(label.into()), project: None, active_min: None, apply_rule: false })
}

/// Pins a ticket on sessions. `Some("")` means "no ticket"; `None` goes back to detection.
pub fn set_ticket(p: &Pipeline, session_ids: &[String], ticket: Option<String>) -> Result<()> {
    session_ids.iter().try_for_each(|id| p.store.set_ticket_override(id, ticket.as_deref()))
}

/// Writes approved entries as a Toggl CSV in ~/Downloads and shows the file in Finder.
/// @returns the file path.
pub fn export_toggl_csv(p: &Pipeline, q: &ReportQuery) -> Result<String> {
    let report = reports::build(&p.store, q)?;
    let settings = Settings::load(&p.store)?;
    let dir = dirs::download_dir().or_else(dirs::home_dir).ok_or_else(|| anyhow!("no Downloads folder"))?;
    let path = dir.join(format!("toggl-{}-to-{}.csv", q.from, q.to));
    std::fs::write(&path, reports::to_csv(&report, &settings.toggl.email))?;
    let _ = std::process::Command::new("open").arg("-R").arg(&path).status();
    Ok(path.to_string_lossy().into_owned())
}

/// Saves the token, looks up the account, and stores its email and workspace.
pub fn connect_toggl(p: &Pipeline, token: &str) -> Result<toggl::TogglAccount> {
    let account = toggl::account(token.trim())?;
    toggl::save_token(token)?;
    let mut s = Settings::load(&p.store)?;
    s.toggl.workspace_id = Some(account.workspace_id);
    if s.toggl.email.is_empty() {
        s.toggl.email = account.email.clone();
    }
    s.save(&p.store)?;
    Ok(account)
}

pub fn toggl_projects(p: &Pipeline) -> Result<Vec<toggl::TogglProjectInfo>> {
    let token = toggl::token().ok_or_else(|| anyhow!("no Toggl token"))?;
    let ws = Settings::load(&p.store)?.toggl.workspace_id.ok_or_else(|| anyhow!("no Toggl workspace"))?;
    toggl::projects(&token, ws)
}

pub fn send_to_toggl(p: &Pipeline, q: &ReportQuery) -> Result<toggl::SendResult> {
    let ws = Settings::load(&p.store)?.toggl.workspace_id.ok_or_else(|| anyhow!("Connect Toggl in Sources first"))?;
    let report = reports::build(&p.store, q)?;
    toggl::send(&p.store, &report, ws)
}

pub fn has_toggl_token() -> bool {
    toggl::token().is_some()
}
