//! Time reports built from sessions: review, approve, then export to Toggl.
//! Only sessions the user confirmed count as approved; nothing uncertain is exported.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::Result;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Utc};
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
    /// Fits each work day's entries to this many minutes, for time sheets that expect full days.
    /// `None` reports the measured time.
    #[serde(default)]
    pub fit_day_min: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportEntry {
    pub key: String,
    pub day: String,
    pub start_ts: i64,
    pub duration_min: i64,
    /// The measured minutes, before `fit_day_min` scaled the entry. Equal to `duration_min`
    /// when the day was not fitted.
    pub measured_min: i64,
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
    /// The Jira ticket. The entry is filed under it in Toggl.
    pub jira: Option<Ticket>,
    /// Title, link and epic of the Jira ticket, when Jira returned them.
    pub jira_info: Option<TicketInfo>,
    /// The GitHub issue or pull request. The entry is filed under it only when there is no Jira ticket.
    pub github: Option<Ticket>,
    pub github_info: Option<TicketInfo>,
    /// A ticket "Find tickets" proposed. On entries without a Jira ticket, and on entries whose
    /// Jira ticket is that suggestion applied without a click (source `tracker`).
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
    /// Applied without a click; the entry's Jira ticket is this one.
    pub auto: bool,
}

/// Ticket titles and open suggestions, read once per report.
struct TicketLookups {
    info: std::collections::HashMap<String, TicketInfo>,
    suggestions: std::collections::HashMap<String, StoredSuggestion>,
}

impl TicketLookups {
    fn info_for(&self, key: &str) -> TicketInfo {
        self.info.get(key).cloned().unwrap_or_else(|| TicketInfo { key: key.into(), title: String::new(), url: String::new(), tracker: String::new(), epic: None })
    }

    /// The open suggestion for `key` in any of the sessions.
    fn suggestion_of(&self, session_ids: &[String], key: &str) -> Option<Suggestion> {
        let s = session_ids.iter().filter_map(|id| self.suggestions.get(id)).find(|s| s.ticket == key)?;
        Some(Suggestion { info: self.info_for(&s.ticket), reason: s.reason.clone(), by_ai: s.by_ai, auto: s.auto })
    }

    fn suggestion_for(&self, session_ids: &[String]) -> Option<Suggestion> {
        let s = session_ids.iter().find_map(|id| self.suggestions.get(id))?;
        Some(Suggestion { info: self.info_for(&s.ticket), reason: s.reason.clone(), by_ai: s.by_ai, auto: s.auto })
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
    jira: Option<Ticket>,
    github: Option<Ticket>,
    /// The session's minutes after overlapping sessions on the same day share their minutes.
    report_min: i64,
    /// Gap minutes that count: at-computer time and approved bridges aimed at this session.
    gap_min: i64,
    /// Bridged minutes aimed at this session that still wait for approval.
    pending_min: i64,
    context: Context,
}

impl Loaded {
    /// The key Toggl files the session under: the Jira ticket, else the GitHub ref.
    fn filed_under(&self) -> Option<&Ticket> {
        self.jira.as_ref().or(self.github.as_ref())
    }
}

/// What can decide a session's tickets besides its own evidence.
struct TicketChoices {
    /// Tickets the user set. A GitHub key here counts as the GitHub ref.
    overrides: HashMap<String, String>,
    github_overrides: HashMap<String, String>,
    /// "Find tickets" suggestions; only the ones marked `auto` apply here.
    suggestions: HashMap<String, StoredSuggestion>,
}

/// The session's Jira ticket and GitHub ref. Each comes from the user's choice when there is one,
/// else from the session's own evidence, else from an `auto` suggestion (source `tracker`), whose
/// `via` also fills a missing GitHub ref.
fn resolve_tickets(session_id: &str, detected: tickets::Detected, choices: &TicketChoices) -> (Option<Ticket>, Option<Ticket>) {
    let chosen = |key: &String| (!key.is_empty()).then(|| Ticket { key: key.clone(), source: "you".into() });
    let found = |key: &String| Ticket { key: key.clone(), source: "tracker".into() };
    let auto = choices.suggestions.get(session_id).filter(|s| s.auto);
    let ticket_override = choices.overrides.get(session_id);
    let jira = match ticket_override {
        Some(key) if !tickets::is_github_key(key) => chosen(key),
        Some(_) => detected.jira,
        None => detected.jira.or_else(|| auto.filter(|s| !tickets::is_github_key(&s.ticket)).map(|s| found(&s.ticket))),
    };
    let github = match (choices.github_overrides.get(session_id), ticket_override) {
        (Some(key), _) => chosen(key),
        (None, Some(key)) if tickets::is_github_key(key) => chosen(key),
        _ => detected.github.or_else(|| auto.and_then(|s| s.via.as_ref()).map(found)),
    };
    (jira, github)
}

fn parse_day(d: &str) -> Result<NaiveDate> {
    Ok(NaiveDate::parse_from_str(d, "%Y-%m-%d")?)
}

fn load(store: &Store, q: &ReportQuery) -> Result<(Vec<Loaded>, BTreeSet<String>)> {
    let (from, _) = day_bounds(parse_day(&q.from)?);
    let (_, to) = day_bounds(parse_day(&q.to)?);
    let history = store.events_between((from - Duration::days(PREFIX_HISTORY_DAYS)).timestamp_millis(), to.timestamp_millis())?;
    let prefixes = tickets::known_prefixes(&history);
    let choices = TicketChoices {
        overrides: store.ticket_overrides()?,
        github_overrides: store.github_ref_overrides()?,
        suggestions: store.ticket_suggestions()?,
    };
    let rules = Settings::load(store)?.contexts;
    let samples = store.project_sample_paths()?;
    let mut out: Vec<(Loaded, Vec<WorkEvent>)> = Vec::new();
    for session in store.sessions_between_days(&q.from, &q.to)? {
        let events = store.events_by_ids(&store.session_event_ids(&session.id)?)?;
        let detected = tickets::detect(&events, session.branch.as_deref(), session.project.as_deref(), &prefixes);
        let (jira, github) = resolve_tickets(&session.id, detected, &choices);
        let context = rules.resolve(session.project.as_deref().unwrap_or(""), samples.get(session.project.as_deref().unwrap_or("")).map(String::as_str));
        out.push((Loaded { session, jira, github, report_min: 0, gap_min: 0, pending_min: 0, context }, events));
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
    match (g, l.filed_under()) {
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
    let jira = group.iter().find_map(|l| l.jira.clone());
    let github = group.iter().find_map(|l| l.github.clone());
    let label = joined_labels(&group);
    let (description, tags) = match (jira.as_ref().or(github.as_ref()), q.tickets) {
        (Some(t), TicketMode::Prefix) => (format!("{} · {label}", t.key), vec![]),
        (Some(t), TicketMode::Tag) => (label.clone(), vec![t.key.clone()]),
        _ => (label.clone(), vec![]),
    };
    let reasons = review_reasons(&group);
    let session_ids: Vec<String> = group.iter().map(|l| l.session.id.clone()).collect();
    ReportEntry {
        jira_info: jira.as_ref().and_then(|t| lookups.info.get(&t.key).cloned()),
        github_info: github.as_ref().and_then(|t| lookups.info.get(&t.key).cloned()),
        suggestion: match (&jira, &github) {
            (Some(t), _) if t.source == "tracker" => lookups.suggestion_of(&session_ids, &t.key),
            (Some(_), _) => None,
            (None, None) => lookups.suggestion_for(&session_ids),
            (None, Some(_)) => lookups.suggestion_for(&session_ids).filter(|s| !tickets::is_github_key(&s.info.key)),
        },
        day: first.session.day.clone(),
        start_ts: first.session.start_ts,
        duration_min: group.iter().map(|l| l.report_min + l.gap_min).sum(),
        measured_min: group.iter().map(|l| l.report_min + l.gap_min).sum(),
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
        jira,
        github,
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
    if let Some(target) = q.fit_day_min.filter(|m| *m > 0) {
        let work_days = settings.schedule.as_ref().map_or_else(|| vec![0, 1, 2, 3, 4], |s| s.days.clone());
        fit_days(&mut entries, target, &work_days, &presence_windows(&loaded));
    }

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

/// Breaks between sessions up to this long count as presence; longer ones are time away.
const PRESENCE_BRIDGE_MS: i64 = 30 * 60_000;

/// Each day's spans of presence: the union of its session spans, joining breaks of at most
/// `PRESENCE_BRIDGE_MS`. Excluded sessions do not count.
/// @returns day → sorted, non-overlapping `(start_ms, end_ms)` windows.
fn presence_windows(loaded: &[Loaded]) -> BTreeMap<String, Vec<(i64, i64)>> {
    let mut spans: BTreeMap<String, Vec<(i64, i64)>> = BTreeMap::new();
    for l in loaded.iter().filter(|l| l.session.state != "excluded") {
        spans.entry(l.session.day.clone()).or_default().push((l.session.start_ts, l.session.end_ts));
    }
    for day_spans in spans.values_mut() {
        day_spans.sort();
        let mut joined: Vec<(i64, i64)> = Vec::new();
        for &(start, end) in day_spans.iter() {
            match joined.last_mut() {
                Some(last) if start - last.1 <= PRESENCE_BRIDGE_MS => last.1 = last.1.max(end),
                _ => joined.push((start, end)),
            }
        }
        *day_spans = joined;
    }
    spans
}

/// Scales each work day's entries in proportion so the day adds up to `target` minutes, in
/// 5-minute steps when `target` allows, then lays them out without overlap inside the day's
/// presence windows (see `lay_in_windows`). Entries mapped to "never send" keep their time and
/// do not count. Entries already sent to Toggl keep their time and count toward the day. Days
/// off the schedule's work days (Monday to Friday without a schedule) and days already at or
/// past `target` once sent time is taken out keep their measured time.
/// @param work_days 0 = Monday … 6 = Sunday.
/// @param windows day → presence windows from `presence_windows`.
fn fit_days(entries: &mut [ReportEntry], target: i64, work_days: &[u32], windows: &BTreeMap<String, Vec<(i64, i64)>>) {
    let mut days: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, e) in entries.iter().enumerate().filter(|(_, e)| !e.skip) {
        days.entry(e.day.clone()).or_default().push(i);
    }
    for (day, idx) in days {
        let weekday = NaiveDate::parse_from_str(&day, "%Y-%m-%d").map(|d| d.weekday().num_days_from_monday());
        if !weekday.is_ok_and(|w| work_days.contains(&w)) {
            continue;
        }
        let sent: i64 = idx.iter().filter(|&&i| entries[i].sent).map(|&i| entries[i].duration_min).sum();
        let open: Vec<usize> = idx.iter().copied().filter(|&i| !entries[i].sent).collect();
        let measured: Vec<i64> = open.iter().map(|&i| entries[i].duration_min).collect();
        let Some(fitted) = scale_to(&measured, target - sent) else { continue };
        for (&i, min) in open.iter().zip(fitted) {
            entries[i].duration_min = min;
        }
        lay_in_windows(entries, &open, windows.get(&day).map_or(&[][..], Vec::as_slice));
    }
}

/// `parts` scaled to sum to `total`, in 5-minute steps when `total` is a multiple of 5 (else
/// 1-minute steps), by largest remainder, with every non-zero part kept above zero when there
/// are enough steps. `None` when there is nothing to scale or `total` is not positive.
fn scale_to(parts: &[i64], total: i64) -> Option<Vec<i64>> {
    let sum: i64 = parts.iter().sum();
    if sum <= 0 || total <= 0 {
        return None;
    }
    let unit = if total % 5 == 0 { 5 } else { 1 };
    let steps = total / unit;
    let exact: Vec<f64> = parts.iter().map(|&p| p as f64 * steps as f64 / sum as f64).collect();
    let mut out: Vec<i64> = exact.iter().map(|x| x.floor() as i64).collect();
    let mut order: Vec<usize> = (0..parts.len()).collect();
    order.sort_by(|&a, &b| (exact[b] - exact[b].floor()).total_cmp(&(exact[a] - exact[a].floor())));
    let left = steps - out.iter().sum::<i64>();
    for &i in order.iter().cycle().take(left.max(0) as usize) {
        out[i] += 1;
    }
    for i in 0..out.len() {
        if out[i] == 0 && parts[i] > 0 {
            let biggest = (0..out.len()).max_by_key(|&j| out[j]).unwrap();
            if out[biggest] > 1 {
                out[biggest] -= 1;
                out[i] = 1;
            }
        }
    }
    Some(out.into_iter().map(|s| s * unit).collect())
}

/// Moves each of `idx` (in start order) to start no earlier than the previous one ends. An entry
/// that would start in a break between `windows` waits for the next window while the windows
/// left still hold every remaining minute, so breaks stay empty unless the day needs them.
/// Without windows, entries run back to back.
fn lay_in_windows(entries: &mut [ReportEntry], idx: &[usize], windows: &[(i64, i64)]) {
    let mut order = idx.to_vec();
    order.sort_by_key(|&i| entries[i].start_ts);
    let mut remaining_ms: i64 = order.iter().map(|&i| entries[i].duration_min * 60_000).sum();
    let mut cursor = i64::MIN;
    for i in order {
        let mut start = entries[i].start_ts.max(cursor);
        let in_window = windows.iter().any(|&(s, e)| s <= start && start < e);
        if let Some(&(next, _)) = windows.iter().find(|&&(s, _)| s > start).filter(|_| !in_window) {
            if window_ms_from(windows, next) >= remaining_ms {
                start = next;
            }
        }
        entries[i].start_ts = start;
        cursor = start + entries[i].duration_min * 60_000;
        remaining_ms -= entries[i].duration_min * 60_000;
    }
}

/// Window time at or after `at`.
fn window_ms_from(windows: &[(i64, i64)], at: i64) -> i64 {
    windows.iter().map(|&(s, e)| (e - s.max(at)).max(0)).sum()
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

/// Work entries that would leave the app once approved: like `exportable`, without the approval.
pub fn exportable_unapproved(report: &Report) -> impl Iterator<Item = &ReportEntry> {
    report.entries.iter().filter(|e| !e.skip && e.context == Context::Work)
}

/// `entries` in Toggl's Detailed report columns, which its CSV importer reads back.
pub fn to_csv<'a>(entries: impl Iterator<Item = &'a ReportEntry>, email: &str) -> String {
    let mut out = String::from("Email,Client,Project,Task,Description,Billable,Start date,Start time,Duration,Tags\n");
    for e in entries {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(day: &str, start: &str, min: i64) -> ReportEntry {
        let start_ts = Local.from_local_datetime(&chrono::NaiveDateTime::parse_from_str(&format!("{day} {start}"), "%Y-%m-%d %H:%M").unwrap()).unwrap().timestamp_millis();
        ReportEntry {
            key: format!("{day}{start}"), day: day.into(), start_ts, duration_min: min, measured_min: min, project: None,
            toggl_project: String::new(), client: String::new(), billable: false, skip: false, context: Context::Work,
            label: String::new(), description: String::new(), tags: vec![], jira: None, jira_info: None, github: None,
            github_info: None, suggestion: None, session_ids: vec![], approved: false, reasons: vec![], sent: false,
            parallel_min: 0, gap_min: 0, pending_bridge_min: 0,
        }
    }

    #[test]
    fn scales_parts_to_an_exact_total_in_five_minute_steps() {
        assert_eq!(scale_to(&[60, 30, 10], 480), Some(vec![290, 145, 45]));
        assert_eq!(scale_to(&[200, 1], 480).unwrap().iter().sum::<i64>(), 480);
        assert_eq!(scale_to(&[200, 1], 480).unwrap()[1], 5, "a short entry keeps a step");
        assert_eq!(scale_to(&[50, 50], 457), Some(vec![229, 228]));
        assert_eq!(scale_to(&[0, 0], 480), None);
    }

    #[test]
    fn fits_work_days_and_leaves_weekends_and_sent_time() {
        let mut entries = vec![
            entry("2026-10-19", "09:00", 120),
            entry("2026-10-19", "09:30", 60),
            entry("2026-10-24", "10:00", 25),
            entry("2026-10-20", "09:00", 60),
            entry("2026-10-20", "11:00", 60),
        ];
        entries[3].sent = true;
        fit_days(&mut entries, 480, &[0, 1, 2, 3, 4], &BTreeMap::new());
        assert_eq!((entries[0].duration_min, entries[1].duration_min), (320, 160));
        assert_eq!(entries[1].start_ts, entries[0].start_ts + 320 * 60_000, "laid back to back");
        assert_eq!(entries[2].duration_min, 25, "Saturday keeps its measured time");
        assert_eq!((entries[3].duration_min, entries[4].duration_min), (60, 420), "sent time counts and stays");
        assert_eq!(entries[0].measured_min, 120);
    }

    #[test]
    fn fitted_entries_skip_breaks_the_windows_can_absorb_and_fill_them_otherwise() {
        let at = |t: &str| entry("2026-10-27", t, 0).start_ts;
        let windows = BTreeMap::from([(
            "2026-10-27".to_string(),
            vec![(at("09:00"), at("12:00")), (at("13:00"), at("18:00"))],
        )]);
        let day = || vec![entry("2026-10-27", "09:00", 90), entry("2026-10-27", "10:30", 30), entry("2026-10-27", "13:30", 120)];
        let mut roomy = day();
        fit_days(&mut roomy, 480, &[0, 1, 2, 3, 4], &windows);
        assert_eq!(roomy.iter().map(|e| e.duration_min).collect::<Vec<_>>(), vec![180, 60, 240]);
        assert_eq!(roomy[1].start_ts, at("13:00"), "waits out lunch while the afternoon holds the rest");
        assert_eq!(roomy[2].start_ts, at("14:00"));

        let mut short = day();
        fit_days(&mut short, 540, &[0, 1, 2, 3, 4], &windows);
        assert_eq!(short[1].start_ts, short[0].start_ts + short[0].duration_min * 60_000, "a day longer than its windows fills the break");
    }

    fn choices(auto: bool) -> TicketChoices {
        let s = StoredSuggestion { ticket: "SHOP-7".into(), reason: "links api#3 in Jira".into(), by_ai: false, via: Some("api#3".into()), auto };
        TicketChoices { overrides: HashMap::new(), github_overrides: HashMap::new(), suggestions: HashMap::from([("s1".to_string(), s)]) }
    }

    #[test]
    fn auto_suggestions_fill_missing_tickets() {
        let (jira, github) = resolve_tickets("s1", tickets::Detected::default(), &choices(true));
        assert_eq!(jira.map(|t| (t.key, t.source)), Some(("SHOP-7".to_string(), "tracker".to_string())));
        assert_eq!(github.unwrap().key, "api#3");
    }

    #[test]
    fn plain_suggestions_and_choices_win_over_auto() {
        assert_eq!(resolve_tickets("s1", tickets::Detected::default(), &choices(false)), (None, None));
        let mut c = choices(true);
        c.overrides.insert("s1".into(), String::new());
        assert_eq!(resolve_tickets("s1", tickets::Detected::default(), &c).0, None, "\"no ticket\" stays");
        let detected = tickets::Detected { jira: Some(Ticket { key: "SHOP-9".into(), source: "branch".into() }), github: None };
        assert_eq!(resolve_tickets("s1", detected, &choices(true)).0.unwrap().key, "SHOP-9");
    }
}
