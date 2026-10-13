//! Time reports built from sessions: review, approve, then export to Toggl.
//! Only sessions the user confirmed count as approved; nothing uncertain is exported.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::contexts::Context;
use crate::event::{clip, WorkEvent};
use crate::pipeline::day_bounds;
use crate::sessions::{attribute::attribute, gaps::Gap, Session};
use crate::settings::{Settings, TogglProject};
use crate::store::{Store, StoredSuggestion};
use crate::tickets::{self, Ticket};
use crate::trackers::TicketInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    /// One entry per session.
    Session,
    /// One entry per ticket per day; sessions with no ticket group by project.
    TicketDay,
    /// One entry per project per day.
    ProjectDay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketMode {
    /// `SHOP-1261 · Invoice header for EU orders`
    Prefix,
    /// The ticket goes into the Toggl tags.
    Tag,
    Off,
}

/// Which side of the user's work a report covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContextFilter {
    #[default]
    Work,
    Personal,
    All,
}

impl ContextFilter {
    pub fn admits(self, c: Context) -> bool {
        matches!((self, c), (ContextFilter::All, _) | (ContextFilter::Work, Context::Work) | (ContextFilter::Personal, Context::Personal))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportQuery {
    pub from: String,
    pub to: String,
    pub grouping: Grouping,
    pub tickets: TicketMode,
    #[serde(default)]
    pub context: ContextFilter,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportEntry {
    pub key: String,
    pub day: String,
    pub start_ts: i64,
    pub duration_min: i64,
    pub project: Option<String>,
    pub toggl_project: String,
    pub client: String,
    pub billable: bool,
    /// Mapped to "never send to Toggl".
    pub skip: bool,
    pub context: Context,
    pub label: String,
    pub description: String,
    pub tags: Vec<String>,
    pub ticket: Option<Ticket>,
    /// Title and link of the ticket, when a tracker returned it.
    pub ticket_info: Option<TicketInfo>,
    /// A ticket "Find tickets" proposed. Only on entries without a ticket.
    pub suggestion: Option<Suggestion>,
    pub session_ids: Vec<String>,
    /// Every session in the entry is confirmed.
    pub approved: bool,
    /// Why the entry needs review. Empty when it is safe to approve.
    pub reasons: Vec<String>,
    pub sent: bool,
    /// Minutes this entry shared with parallel sessions, removed from `duration_min`.
    pub parallel_min: i64,
    /// Inferred minutes inside `duration_min`: at-computer time and approved bridges.
    pub gap_min: i64,
    /// Bridged minutes not in `duration_min` until the entry is approved.
    pub pending_bridge_min: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub info: TicketInfo,
    /// Why the ticket fits, such as "same repo, shares invoice, header".
    pub reason: String,
    pub by_ai: bool,
}

/// Ticket titles and open suggestions, read once per report.
struct TicketLookups {
    info: std::collections::HashMap<String, TicketInfo>,
    suggestions: std::collections::HashMap<String, StoredSuggestion>,
}

impl TicketLookups {
    fn info_for(&self, key: &str) -> TicketInfo {
        self.info.get(key).cloned().unwrap_or_else(|| TicketInfo { key: key.into(), title: String::new(), url: String::new(), tracker: String::new() })
    }

    fn suggestion_for(&self, session_ids: &[String]) -> Option<Suggestion> {
        let s = session_ids.iter().find_map(|id| self.suggestions.get(id))?;
        Some(Suggestion { info: self.info_for(&s.ticket), reason: s.reason.clone(), by_ai: s.by_ai })
    }
}

#[derive(Debug, Serialize)]
pub struct ProjectTotal {
    pub project: String,
    pub min: i64,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub query: ReportQuery,
    pub entries: Vec<ReportEntry>,
    pub total_min: i64,
    pub approved_min: i64,
    pub billable_min: i64,
    pub review_count: usize,
    pub by_project: Vec<ProjectTotal>,
    /// Jira prefixes the ticket finder trusts, for display.
    pub known_prefixes: Vec<String>,
    pub parallel_min: i64,
}

/// History window that teaches the ticket finder which Jira prefixes are real.
const PREFIX_HISTORY_DAYS: i64 = 90;

struct Loaded {
    session: Session,
    ticket: Option<Ticket>,
    /// The session's minutes after overlapping sessions on the same day share their minutes.
    report_min: i64,
    /// Gap minutes that count: at-computer time and approved bridges aimed at this session.
    gap_min: i64,
    /// Bridged minutes aimed at this session that still wait for approval.
    pending_min: i64,
    context: Context,
}

fn parse_day(d: &str) -> Result<NaiveDate> {
    Ok(NaiveDate::parse_from_str(d, "%Y-%m-%d")?)
}

fn load(store: &Store, q: &ReportQuery) -> Result<(Vec<Loaded>, BTreeSet<String>)> {
    let (from, _) = day_bounds(parse_day(&q.from)?);
    let (_, to) = day_bounds(parse_day(&q.to)?);
    let history = store.events_between((from - Duration::days(PREFIX_HISTORY_DAYS)).timestamp_millis(), to.timestamp_millis())?;
    let prefixes = tickets::known_prefixes(&history);
    let overrides = store.ticket_overrides()?;
    let rules = Settings::load(store)?.contexts;
    let samples = store.project_sample_paths()?;
    let mut out: Vec<(Loaded, Vec<WorkEvent>)> = Vec::new();
    for session in store.sessions_between_days(&q.from, &q.to)? {
        let events = store.events_by_ids(&store.session_event_ids(&session.id)?)?;
        let ticket = match overrides.get(&session.id) {
            Some(key) if key.is_empty() => None,
            Some(key) => Some(Ticket { key: key.clone(), source: "you".into() }),
            None => tickets::detect(&events, session.branch.as_deref(), session.project.as_deref(), &prefixes),
        };
        let context = rules.resolve(session.project.as_deref().unwrap_or(""), samples.get(session.project.as_deref().unwrap_or("")).map(String::as_str));
        out.push((Loaded { session, ticket, report_min: 0, gap_min: 0, pending_min: 0, context }, events));
    }
    share_parallel_minutes(&mut out);
    add_gap_minutes(&mut out, &store.gaps_between_days(&q.from, &q.to)?);
    out.retain(|(l, _)| q.context.admits(l.context));
    Ok((out.into_iter().map(|(l, _)| l).collect(), prefixes))
}

/// Runs over work and personal sessions together, before the report filters by context,
/// so the two sides never claim the same minute.
/// Parallel agent sessions overlap, but a time sheet holds wall-clock time. On each day,
/// machine-made session minutes scale down so they add up to the day's real active
/// minutes. A session whose minutes the user set keeps them.
fn share_parallel_minutes(loaded: &mut [(Loaded, Vec<WorkEvent>)]) {
    let days: BTreeSet<String> = loaded.iter().map(|(l, _)| l.session.day.clone()).collect();
    for day in days {
        let machine = |l: &Loaded| l.session.day == day && !l.session.has_user_minutes();
        let events: Vec<&WorkEvent> = loaded.iter().filter(|(l, _)| machine(l)).flat_map(|(_, e)| e.iter()).collect();
        let wall = attribute(&events).0;
        let summed: i64 = loaded.iter().filter(|(l, _)| machine(l)).map(|(l, _)| l.session.active_min).sum();
        let factor = if summed > wall && summed > 0 { wall as f64 / summed as f64 } else { 1.0 };
        for (l, _) in loaded.iter_mut().filter(|(l, _)| l.session.day == day) {
            l.report_min = if l.session.has_user_minutes() {
                l.session.active_min
            } else {
                round_to_five(l.session.active_min as f64 * factor, l.session.active_min)
            };
        }
    }
}

fn round_to_five(min: f64, original: i64) -> i64 {
    let rounded = ((min / 5.0).round() as i64) * 5;
    if original > 0 { rounded.max(5) } else { 0 }
}

/// Gives each session the gap minutes aimed at it.
fn add_gap_minutes(loaded: &mut [(Loaded, Vec<WorkEvent>)], gaps: &[Gap]) {
    for g in gaps.iter().filter(|g| g.kind != "unaccounted") {
        for (id, min) in &g.targets {
            if let Some((l, _)) = loaded.iter_mut().find(|(l, _)| &l.session.id == id) {
                if g.counts() {
                    l.gap_min += min;
                } else if g.state == "open" {
                    l.pending_min += min;
                }
            }
        }
    }
}

fn group_key(l: &Loaded, g: Grouping) -> String {
    let project = l.session.project.as_deref().unwrap_or("-");
    match (g, &l.ticket) {
        (Grouping::Session, _) => l.session.id.clone(),
        (Grouping::TicketDay, Some(t)) => format!("{}|{}", l.session.day, t.key),
        (Grouping::TicketDay, None) | (Grouping::ProjectDay, _) => format!("{}|{project}", l.session.day),
    }
}

fn mapping(settings: &Settings, project: Option<&str>) -> TogglProject {
    let name = project.unwrap_or("").to_string();
    settings.toggl.project_map.get(&name).cloned().map(|mut m| {
        if m.name.trim().is_empty() {
            m.name = name.clone();
        }
        m
    }).unwrap_or(TogglProject { name, billable: settings.toggl.default_billable, ..Default::default() })
}

/// Labels of the grouped sessions, without repeats, in time order.
fn joined_labels(group: &[&Loaded]) -> String {
    let mut seen = BTreeSet::new();
    let labels: Vec<&str> = group
        .iter()
        .filter_map(|l| l.session.label.as_deref().or(l.session.project.as_deref()))
        .filter(|l| seen.insert(*l))
        .collect();
    clip(&labels.join("; "), 180)
}

fn review_reasons(group: &[&Loaded]) -> Vec<String> {
    let mut r = Vec::new();
    let pending: i64 = group.iter().map(|l| l.pending_min).sum();
    if pending > 0 {
        r.push(format!("{pending} min bridged, not approved"));
    }
    if group.iter().any(|l| l.session.state != "confirmed") {
        r.push("Not confirmed".into());
    }
    if group.iter().any(|l| l.session.confidence.as_deref() == Some("low")) {
        r.push("Low confidence".into());
    }
    if group.iter().any(|l| l.session.project.is_none()) {
        r.push("No project".into());
    }
    r
}

struct EntryContext<'a> {
    q: &'a ReportQuery,
    settings: &'a Settings,
    sent: &'a BTreeSet<String>,
    lookups: &'a TicketLookups,
}

fn build_entry(key: String, group: Vec<&Loaded>, cx: &EntryContext) -> ReportEntry {
    let EntryContext { q, settings, sent, lookups } = *cx;
    let first = group.iter().min_by_key(|l| l.session.start_ts).unwrap();
    let project = first.session.project.clone();
    let map = mapping(settings, project.as_deref());
    let ticket = group.iter().find_map(|l| l.ticket.clone());
    let label = joined_labels(&group);
    let (description, tags) = match (&ticket, q.tickets) {
        (Some(t), TicketMode::Prefix) => (format!("{} · {label}", t.key), vec![]),
        (Some(t), TicketMode::Tag) => (label.clone(), vec![t.key.clone()]),
        _ => (label.clone(), vec![]),
    };
    let reasons = review_reasons(&group);
    let session_ids: Vec<String> = group.iter().map(|l| l.session.id.clone()).collect();
    ReportEntry {
        ticket_info: ticket.as_ref().and_then(|t| lookups.info.get(&t.key).cloned()),
        suggestion: match &ticket {
            None => lookups.suggestion_for(&session_ids),
            Some(t) if t.yields_to_jira() => lookups.suggestion_for(&session_ids).filter(|s| !tickets::is_github_key(&s.info.key)),
            Some(_) => None,
        },
        day: first.session.day.clone(),
        start_ts: first.session.start_ts,
        duration_min: group.iter().map(|l| l.report_min + l.gap_min).sum(),
        gap_min: group.iter().map(|l| l.gap_min).sum(),
        pending_bridge_min: group.iter().map(|l| l.pending_min).sum(),
        parallel_min: group.iter().map(|l| l.session.active_min - l.report_min).sum::<i64>().max(0),
        toggl_project: map.name,
        client: map.client,
        billable: map.billable,
        skip: map.skip,
        approved: group.iter().all(|l| l.session.state == "confirmed"),
        session_ids,
        sent: sent.contains(&key),
        key,
        context: first.context,
        project,
        label,
        description,
        tags,
        ticket,
        reasons,
    }
}

pub fn build(store: &Store, q: &ReportQuery) -> Result<Report> {
    let settings = Settings::load(store)?;
    let (loaded, prefixes) = load(store, q)?;
    let sent = store.toggl_sent_keys()?;
    let lookups = TicketLookups { info: store.ticket_info()?, suggestions: store.ticket_suggestions()? };
    let cx = EntryContext { q, settings: &settings, sent: &sent, lookups: &lookups };
    let mut groups: BTreeMap<String, Vec<&Loaded>> = BTreeMap::new();
    for l in &loaded {
        groups.entry(group_key(l, q.grouping)).or_default().push(l);
    }
    let mut entries: Vec<ReportEntry> =
        groups.into_iter().map(|(k, g)| build_entry(k, g, &cx)).filter(|e| e.duration_min > 0).collect();
    entries.sort_by_key(|e| e.start_ts);

    let mut by_project: BTreeMap<String, i64> = BTreeMap::new();
    for e in entries.iter().filter(|e| !e.skip) {
        *by_project.entry(e.project.clone().unwrap_or_else(|| "Other".into())).or_default() += e.duration_min;
    }
    let mut by_project: Vec<ProjectTotal> = by_project.into_iter().map(|(project, min)| ProjectTotal { project, min }).collect();
    by_project.sort_by_key(|p| -p.min);
    let counted = || entries.iter().filter(|e| !e.skip);
    Ok(Report {
        total_min: counted().map(|e| e.duration_min).sum(),
        approved_min: counted().filter(|e| e.approved).map(|e| e.duration_min).sum(),
        billable_min: counted().filter(|e| e.billable).map(|e| e.duration_min).sum(),
        review_count: counted().filter(|e| !e.approved).count(),
        by_project,
        known_prefixes: prefixes.into_iter().collect(),
        parallel_min: counted().map(|e| e.parallel_min).sum(),
        query: q.clone(),
        entries,
    })
}

/// Sessions safe to approve in one step: high confidence, with a project, not hidden.
pub fn confident_session_ids(store: &Store, q: &ReportQuery) -> Result<Vec<String>> {
    Ok(store
        .sessions_between_days(&q.from, &q.to)?
        .into_iter()
        .filter(|s| s.state != "confirmed" && s.confidence.as_deref() == Some("high") && s.project.is_some())
        .map(|s| s.id)
        .collect())
}

/// Entries that may leave the app: approved work, not mapped to "never send".
/// Personal entries never go to Toggl, whatever the report shows.
pub fn exportable(report: &Report) -> impl Iterator<Item = &ReportEntry> {
    report.entries.iter().filter(|e| e.approved && !e.skip && e.context == Context::Work)
}

fn local(ms: i64) -> DateTime<Local> {
    Utc.timestamp_millis_opt(ms).single().unwrap_or_default().with_timezone(&Local)
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Toggl's Detailed report columns, which its CSV importer reads back.
pub fn to_csv(report: &Report, email: &str) -> String {
    let mut out = String::from("Email,Client,Project,Task,Description,Billable,Start date,Start time,Duration,Tags\n");
    for e in exportable(report) {
        let start = local(e.start_ts);
        let row = [
            email.to_string(),
            e.client.clone(),
            e.toggl_project.clone(),
            String::new(),
            e.description.clone(),
            if e.billable { "Yes".into() } else { "No".into() },
            start.format("%Y-%m-%d").to_string(),
            start.format("%H:%M:%S").to_string(),
            format!("{:02}:{:02}:00", e.duration_min / 60, e.duration_min % 60),
            e.tags.join(", "),
        ];
        out.push_str(&row.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}
