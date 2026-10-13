//! View models and actions shared by the Tauri commands and the CLI.

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};
use chrono::{Duration, Local, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::backfill::{self, BackfillReport, Discovery};
use crate::corrections::Rule;
use crate::event::WorkEvent;
use crate::infer::journal;
use crate::pipeline::Pipeline;
use crate::sessions::{attribute::attribute, cluster::evidence_counts, gaps::{work_windows, Gap}, Session};
use crate::settings::WorkSchedule;
use crate::contexts::{Context, ContextRules};
use crate::reports::ContextFilter;
use std::collections::HashMap;
use crate::settings::Settings;
use crate::tickets;
use crate::reports::{self, Report, ReportQuery};
use crate::store::{Achievement, Journal};
use crate::toggl;
use crate::trackers;

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
    pub context: Context,
}

/// One day measured against the work schedule.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DaySchedule {
    pub day: String,
    pub schedule_min: i64,
    pub observed_min: i64,
    /// At-computer time and approved bridges.
    pub filled_min: i64,
    pub pending_bridge_min: i64,
    pub unaccounted_min: i64,
}

#[derive(Debug, Serialize)]
pub struct DayView {
    pub day: String,
    pub active_min: i64,
    pub gaps: Vec<Gap>,
    pub schedule: Option<DaySchedule>,
    pub work_windows: Vec<(i64, i64)>,
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
/// minutes the user set (an edit or an assigned block) adds them as given; excluded
/// sessions add nothing.
fn day_active_min(sessions: &[(Session, Vec<WorkEvent>)]) -> i64 {
    let machine: Vec<&WorkEvent> = sessions
        .iter()
        .filter(|(s, _)| s.state != "excluded" && !s.has_user_minutes())
        .flat_map(|(_, evs)| evs.iter())
        .collect();
    let edited: i64 =
        sessions.iter().filter(|(s, _)| s.state != "excluded" && s.has_user_minutes()).map(|(s, _)| s.active_min).sum();
    attribute(&machine).0 + edited
}

/// Sums one day's minutes by kind. Assigned blocks are sessions, so they sit in `observed`.
pub fn measure_day(day: &str, windows: &[(i64, i64)], observed_min: i64, gaps: &[Gap]) -> DaySchedule {
    let sum = |f: &dyn Fn(&&Gap) -> bool| gaps.iter().filter(f).map(|g| g.minutes()).sum::<i64>();
    DaySchedule {
        day: day.to_string(),
        schedule_min: windows.iter().map(|(a, b)| (b - a) / 60_000).sum(),
        observed_min,
        filled_min: sum(&|g| g.counts()),
        pending_bridge_min: sum(&|g| g.kind == "bridged" && g.state == "open"),
        unaccounted_min: sum(&|g| g.is_open_unaccounted()),
    }
}

/// Work or personal, for each session's project.
fn session_context(rules: &ContextRules, samples: &HashMap<String, String>, s: &Session) -> Context {
    let project = s.project.as_deref().unwrap_or_default();
    rules.resolve(project, samples.get(project).map(String::as_str))
}

/// One day for one side. Work hours, gaps and the schedule belong to work, so the
/// personal side shows none of them.
pub fn day_view(p: &Pipeline, day: &str, side: ContextFilter) -> Result<DayView> {
    let settings = Settings::load(&p.store)?;
    let samples = p.store.project_sample_paths()?;
    let mut loaded = Vec::new();
    for s in p.store.sessions_for_day(day)? {
        let context = session_context(&settings.contexts, &samples, &s);
        if side.admits(context) {
            let events = p.store.events_by_ids(&p.store.session_event_ids(&s.id)?)?;
            loaded.push((s, events, context));
        }
    }
    let pairs: Vec<(Session, Vec<WorkEvent>)> = loaded.iter().map(|(s, e, _)| (s.clone(), e.clone())).collect();
    let active = day_active_min(&pairs);
    let sessions = loaded
        .into_iter()
        .map(|(s, events, context)| {
            let refs: Vec<&WorkEvent> = events.iter().collect();
            let chips = evidence_counts(&refs).into_iter().map(|(label, n)| Chip { label, n }).collect();
            SessionView { session: s, chips, context }
        })
        .collect();
    let shows_work_hours = side != ContextFilter::Personal;
    let gaps = if shows_work_hours { p.store.gaps_between_days(day, day)? } else { vec![] };
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d")?;
    let windows = match (&settings.schedule, shows_work_hours) {
        (Some(s), true) => work_windows(date, s),
        _ => vec![],
    };
    let schedule = settings.schedule.as_ref().filter(|_| shows_work_hours).map(|_| measure_day(day, &windows, active, &gaps));
    Ok(DayView {
        day: day.to_string(),
        active_min: active,
        gaps,
        schedule,
        work_windows: windows,
        sessions,
        achievements: p.store.achievements(day)?,
        journal: p.store.journal(day)?,
    })
}

pub fn session_events(p: &Pipeline, session_id: &str) -> Result<Vec<WorkEvent>> {
    p.store.events_by_ids(&p.store.session_event_ids(session_id)?)
}

pub fn projects(p: &Pipeline, from: &str, to: &str, side: ContextFilter) -> Result<Vec<ProjectRow>> {
    let rules = Settings::load(&p.store)?.contexts;
    let samples = p.store.project_sample_paths()?;
    let mut rows: BTreeMap<String, ProjectRow> = BTreeMap::new();
    for s in p.store.sessions_between_days(from, to)?.into_iter().filter(|s| side.admits(session_context(&rules, &samples, s))) {
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

/// Merges the given top-level settings sections into the stored settings. Each screen
/// sends only what it changed, so two screens never overwrite each other.
pub fn patch_settings(p: &Pipeline, patch: serde_json::Value) -> Result<Settings> {
    let mut current = serde_json::to_value(Settings::load(&p.store)?)?;
    if let (Some(cur), Some(changes)) = (current.as_object_mut(), patch.as_object()) {
        for (k, v) in changes {
            cur.insert(k.clone(), v.clone());
        }
    }
    let merged: Settings = serde_json::from_value(current)?;
    merged.save(&p.store)?;
    Ok(merged)
}

/// Reads the work folders and emails from the global git config again.
pub fn detect_contexts() -> ContextRules {
    let (work_paths, work_emails) = crate::contexts::detect_work_identities();
    ContextRules { work_paths, work_emails, ..ContextRules::default() }
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

/// The schedule picture for each work day in the range. Empty without a schedule.
pub fn report_days(p: &Pipeline, q: &ReportQuery) -> Result<Vec<DaySchedule>> {
    let Some(schedule) = Settings::load(&p.store)?.schedule else { return Ok(vec![]) };
    let mut day = NaiveDate::parse_from_str(&q.from, "%Y-%m-%d")?;
    let last = NaiveDate::parse_from_str(&q.to, "%Y-%m-%d")?;
    let mut out = Vec::new();
    while day <= last {
        let key = day.format("%Y-%m-%d").to_string();
        let windows = work_windows(day, &schedule);
        if !windows.is_empty() {
            let observed = day_view(p, &key, ContextFilter::Work)?.active_min;
            out.push(measure_day(&key, &windows, observed, &p.store.gaps_between_days(&key, &key)?));
        }
        day = day.succ_opt().unwrap();
    }
    Ok(out)
}

/// Approving an entry confirms every session in it and approves the bridges aimed at them.
pub fn approve_sessions(p: &Pipeline, ids: &[String]) -> Result<()> {
    ids.iter().try_for_each(|id| confirm_session(p, id))?;
    let days: Vec<String> = ids.iter().filter_map(|id| p.store.session(id).ok().flatten()).map(|s| s.day).collect();
    if let (Some(from), Some(to)) = (days.iter().min(), days.iter().max()) {
        approve_bridges_for(p, ids, from, to)?;
    }
    Ok(())
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

/// Pins a ticket on sessions. A GitHub key (`repo#N`) goes to the GitHub ref; anything else is
/// the Jira ticket. `Some("")` means "no Jira ticket"; `None` goes back to detection.
/// Pinning a Jira ticket keeps the GitHub ref: one stored as the ticket moves to the GitHub ref,
/// and a suggestion found through a GitHub ticket pins that ticket as the GitHub ref.
/// It also removes the sessions' open suggestions.
pub fn set_ticket(p: &Pipeline, session_ids: &[String], ticket: Option<String>) -> Result<()> {
    let ticket = ticket.map(|t| t.trim().to_string());
    if let Some(key) = ticket.as_deref().filter(|k| tickets::is_github_key(k)) {
        return set_github_ref(p, session_ids, Some(key.to_string()));
    }
    let overrides = p.store.ticket_overrides()?;
    let suggestions = p.store.ticket_suggestions()?;
    for id in session_ids {
        let moved = overrides.get(id).filter(|k| tickets::is_github_key(k));
        let via = suggestions.get(id).filter(|s| Some(&s.ticket) == ticket.as_ref()).and_then(|s| s.via.as_ref());
        if let Some(github) = via.or(moved) {
            p.store.set_github_ref_override(id, Some(github))?;
        }
        p.store.set_ticket_override(id, ticket.as_deref())?;
    }
    if ticket.as_deref().is_some_and(|t| !t.is_empty()) {
        p.store.clear_suggestions(session_ids)?;
    }
    Ok(())
}

/// Pins the GitHub ref of sessions. `Some("")` means "no GitHub ref"; `None` goes back to detection.
pub fn set_github_ref(p: &Pipeline, session_ids: &[String], github_ref: Option<String>) -> Result<()> {
    session_ids.iter().try_for_each(|id| p.store.set_github_ref_override(id, github_ref.as_deref()))
}

/// Hides the sessions' suggestion. The same ticket is never suggested for them again.
pub fn dismiss_suggestion(p: &Pipeline, session_ids: &[String]) -> Result<()> {
    p.store.dismiss_suggestions(session_ids)
}

pub fn tracker_status(p: &Pipeline) -> Result<trackers::TrackerStatus> {
    Ok(trackers::status(&Settings::load(&p.store)?))
}

/// Checks the Jira token, saves it to the Keychain, and stores the site and email.
pub fn connect_jira(p: &Pipeline, site: &str, email: &str, token: &str) -> Result<trackers::jira::JiraAccount> {
    let site = trackers::jira::normalize_site(site);
    let account = trackers::jira::connect(&site, email, token)?;
    let mut s = Settings::load(&p.store)?;
    s.trackers.jira_site = site;
    s.trackers.jira_email = email.trim().to_string();
    s.save(&p.store)?;
    Ok(account)
}

/// Opens Atlassian's API token page in the default browser.
pub fn open_jira_token_page() -> Result<()> {
    open_in_browser(trackers::jira::TOKEN_PAGE)
}

fn open_in_browser(url: &str) -> Result<()> {
    if !url.starts_with("https://") {
        bail!("only https links open");
    }
    std::process::Command::new("open").arg(url).status()?;
    Ok(())
}

/// The ticket's page: the link a tracker returned, else the Jira page for a Jira-shaped key.
fn ticket_url(p: &Pipeline, key: &str) -> Result<Option<String>> {
    if let Some(info) = p.store.ticket_info()?.remove(key).filter(|i| !i.url.is_empty()) {
        return Ok(Some(info.url));
    }
    let site = Settings::load(&p.store)?.trackers.jira_site;
    let jira_shaped = key.split_once('-').is_some_and(|(prefix, n)| !prefix.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    Ok((jira_shaped && !site.is_empty()).then(|| format!("https://{site}/browse/{key}")))
}

/// Opens the ticket's page in the default browser.
pub fn open_ticket(p: &Pipeline, key: &str) -> Result<()> {
    let url = ticket_url(p, key)?.ok_or_else(|| anyhow!("no link known for {key}; run Find tickets or connect Jira"))?;
    open_in_browser(&url)
}

pub fn find_tickets(p: &Pipeline, q: &ReportQuery) -> Result<trackers::find::FindResult> {
    trackers::find::find_tickets(&p.store, q)
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

/// Days in the range with sessions that still carry only evidence labels (no AI label yet).
pub fn days_needing_labels(p: &Pipeline, q: &ReportQuery) -> Result<Vec<String>> {
    let mut days: Vec<String> = p
        .store
        .sessions_between_days(&q.from, &q.to)?
        .into_iter()
        .filter(|s| s.state == "observed")
        .map(|s| s.day)
        .collect();
    days.dedup();
    Ok(days)
}

/// Models the saved key can use; the known list when there is no key or no network.
pub fn ai_models() -> Vec<crate::infer::models::ModelInfo> {
    match journal::api_key() {
        Some(key) => crate::infer::models::available_models(&key),
        None => crate::infer::models::known_models(),
    }
}

/// What the user decided about an unaccounted block.
#[derive(Debug, Deserialize)]
pub struct GapResolution {
    pub project: Option<String>,
    pub label: Option<String>,
    #[serde(default)]
    pub not_work: bool,
}

fn find_gap(p: &Pipeline, id: &str) -> Result<Gap> {
    let all = p.store.gaps_between_days("0000-00-00", "9999-99-99")?;
    all.into_iter().find(|g| g.id == id).ok_or_else(|| anyhow!("no gap {id}"))
}

/// Assigning a block makes a confirmed session from it; "not work" only marks it.
pub fn resolve_gap(p: &Pipeline, id: &str, r: GapResolution) -> Result<()> {
    let g = find_gap(p, id)?;
    if r.not_work {
        return p.store.set_gap_state(id, "not_work", None, None);
    }
    let session = Session {
        id: format!("m{}", g.id),
        day: g.day.clone(),
        start_ts: g.start_ts,
        end_ts: g.end_ts,
        project: r.project.clone(),
        label: r.label.clone().or_else(|| r.project.clone()),
        summary: Some("Added from an unaccounted block".into()),
        active_min: g.minutes(),
        state: "confirmed".into(),
        confidence: Some("high".into()),
        ..Default::default()
    };
    p.store.insert_manual_session(&session)?;
    p.store.set_gap_state(id, "assigned", r.project.as_deref(), r.label.as_deref())
}

/// Approves the open bridges whose minutes go to any of these sessions.
pub fn approve_bridges_for(p: &Pipeline, session_ids: &[String], from: &str, to: &str) -> Result<usize> {
    let mut n = 0;
    for g in p.store.gaps_between_days(from, to)? {
        if g.kind == "bridged" && g.state == "open" && g.targets.iter().any(|(id, _)| session_ids.contains(id)) {
            p.store.set_gap_state(&g.id, "approved", None, None)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Approves every open bridge in the range.
pub fn approve_bridges(p: &Pipeline, from: &str, to: &str) -> Result<usize> {
    let open: Vec<Gap> = p.store.gaps_between_days(from, to)?.into_iter().filter(|g| g.kind == "bridged" && g.state == "open").collect();
    for g in &open {
        p.store.set_gap_state(&g.id, "approved", None, None)?;
    }
    Ok(open.len())
}

/// Assigns every open unaccounted block of a day to one project the user picked.
/// @returns the minutes it filled.
pub fn fill_to_schedule(p: &Pipeline, day: &str, project: &str) -> Result<i64> {
    let mut filled = 0;
    for g in p.store.gaps_between_days(day, day)?.into_iter().filter(Gap::is_open_unaccounted) {
        filled += g.minutes();
        resolve_gap(p, &g.id, GapResolution { project: Some(project.into()), label: None, not_work: false })?;
    }
    Ok(filled)
}

/// Saves the schedule and recomputes gaps for the last 90 days.
pub fn save_schedule(p: &Pipeline, schedule: Option<WorkSchedule>) -> Result<()> {
    let mut s = Settings::load(&p.store)?;
    s.schedule = schedule;
    s.schedule_asked = true;
    s.save(&p.store)?;
    p.rebuild_range(Utc::now() - Duration::days(90), Utc::now())?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ProjectContext {
    pub project: String,
    pub context: Context,
    /// The user chose this side; it is not from a rule.
    pub chosen: bool,
}

/// Every project with a session in the last 90 days, with its side and where that came from.
pub fn project_contexts(p: &Pipeline) -> Result<Vec<ProjectContext>> {
    let rules = Settings::load(&p.store)?.contexts;
    let samples = p.store.project_sample_paths()?;
    let today = Local::now().date_naive();
    let from = (today - Duration::days(90)).format("%Y-%m-%d").to_string();
    let mut names: Vec<String> = p
        .store
        .sessions_between_days(&from, &today.format("%Y-%m-%d").to_string())?
        .into_iter()
        .filter_map(|s| s.project)
        .collect();
    names.sort();
    names.dedup();
    Ok(names
        .into_iter()
        .map(|project| ProjectContext {
            context: rules.resolve(&project, samples.get(&project).map(String::as_str)),
            chosen: rules.overrides.contains_key(&project),
            project,
        })
        .collect())
}

/// Pins a project to work or personal. `None` goes back to the rules.
pub fn set_project_context(p: &Pipeline, project: &str, context: Option<Context>) -> Result<()> {
    let mut s = Settings::load(&p.store)?;
    match context {
        Some(c) => s.contexts.overrides.insert(project.to_string(), c),
        None => s.contexts.overrides.remove(project),
    };
    s.save(&p.store)
}

#[derive(Debug, Serialize)]
pub struct CalendarStatus {
    pub access: crate::adapters::calendar::Access,
    pub calendars: Vec<crate::adapters::calendar::CalendarInfo>,
    pub excluded: Vec<String>,
}

pub fn calendar_status(p: &Pipeline) -> Result<CalendarStatus> {
    Ok(CalendarStatus {
        access: crate::adapters::calendar::access(),
        calendars: crate::adapters::calendar::calendars(),
        excluded: Settings::load(&p.store)?.calendar_excluded,
    })
}

/// Asks macOS for Calendar access; on a yes, reads 90 days of meetings and rebuilds.
pub fn connect_calendar(p: &Pipeline) -> Result<CalendarStatus> {
    let access = crate::adapters::calendar::request_access();
    if access == crate::adapters::calendar::Access::Granted {
        reimport_meetings(p)?;
    }
    calendar_status(p)
}

/// Reads 90 days of meetings again, for example after the excluded calendars change.
/// Meetings from a newly excluded calendar stay until the user hides them.
pub fn reimport_meetings(p: &Pipeline) -> Result<usize> {
    let since = Utc::now() - Duration::days(90);
    let n = crate::backfill::import_meetings(p, since)?;
    p.rebuild_range(since, Utc::now())?;
    Ok(n)
}
