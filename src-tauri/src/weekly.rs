//! The weekly report: one Markdown file per ISO week of work, written without a click after the
//! week ends, for reading or pasting into a status update.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate};

use crate::api::{self, DaySchedule};
use crate::pipeline::Pipeline;
use crate::reports::{self, ContextFilter, Grouping, Report, ReportEntry, ReportQuery, TicketMode};

/// The folder next to the database where weekly reports go.
pub fn reports_dir() -> PathBuf {
    let db = crate::store::default_db_path();
    db.parent().map(PathBuf::from).unwrap_or_default().join("reports")
}

/// Monday of the ISO week that holds `day`.
pub fn week_start(day: NaiveDate) -> NaiveDate {
    day - Duration::days(i64::from(day.weekday().num_days_from_monday()))
}

/// `2026-W43` for the ISO week starting on `monday`.
pub fn week_name(monday: NaiveDate) -> String {
    let w = monday.iso_week();
    format!("{}-W{:02}", w.year(), w.week())
}

/// Where the report for the week starting on `monday` is written.
pub fn report_path(monday: NaiveDate) -> PathBuf {
    reports_dir().join(format!("{}.md", week_name(monday)))
}

/// Writes the work report for the ISO week starting on `monday`, replacing an earlier one.
/// @returns the file path.
pub fn write_week(p: &Pipeline, monday: NaiveDate) -> Result<PathBuf> {
    let sunday = monday + Duration::days(6);
    let q = ReportQuery {
        from: monday.format("%Y-%m-%d").to_string(),
        to: sunday.format("%Y-%m-%d").to_string(),
        grouping: Grouping::TicketDay,
        tickets: TicketMode::Prefix,
        context: ContextFilter::Work,
        fit_day_min: None, fit_drift_min: None,
    };
    let report = reports::build(&p.store, &q)?;
    let days = api::report_days(p, &q)?;
    let mut journals = Vec::new();
    let mut day = monday;
    while day <= sunday {
        let key = day.format("%Y-%m-%d").to_string();
        if let Some(j) = p.store.journal(&key)?.filter(|j| !j.lines.is_empty()) {
            journals.push((key, j.lines.into_iter().map(|l| l.text).collect::<Vec<_>>()));
        }
        day = day.succ_opt().unwrap();
    }
    let path = report_path(monday);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, render(monday, &report, &days, &journals))?;
    Ok(path)
}

/// `35m` under an hour, `9h05` from an hour up.
fn hours(min: i64) -> String {
    if min < 60 {
        format!("{min}m")
    } else {
        format!("{}h{:02}", min / 60, min % 60)
    }
}

/// `Mon 12 Oct` for `2026-10-12`; the text itself when it is not a date.
fn day_name(day: &str) -> String {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").map(|d| d.format("%a %-d %b").to_string()).unwrap_or_else(|_| day.to_string())
}

/// Markdown table cells hold no pipes or new lines.
fn cell(s: &str) -> String {
    s.replace('|', "/").replace('\n', " ")
}

/// The ticket an entry is filed under, with its title and epic, or `None`.
fn ticket_of(e: &ReportEntry) -> Option<(String, String, String)> {
    let (t, info) = match (&e.jira, &e.github) {
        (Some(t), _) => (t, e.jira_info.as_ref()),
        (None, Some(t)) => (t, e.github_info.as_ref()),
        _ => return None,
    };
    let title = info.map(|i| i.title.clone()).unwrap_or_default();
    let epic = info.and_then(|i| i.epic.as_ref()).map(|ep| if ep.title.is_empty() { ep.key.clone() } else { ep.title.clone() }).unwrap_or_default();
    Some((t.key.clone(), title, epic))
}

/// The report as Markdown: totals, projects, tickets, days (against the schedule when one is
/// set), journal lines, the entries without a ticket, and how much is still unapproved.
pub fn render(monday: NaiveDate, report: &Report, days: &[DaySchedule], journals: &[(String, Vec<String>)]) -> String {
    let sunday = monday + Duration::days(6);
    let counted: Vec<&ReportEntry> = report.entries.iter().filter(|e| !e.skip).collect();
    let mut out = String::new();
    let _ = writeln!(out, "# {} · {} to {}\n", week_name(monday), monday.format("%a %-d %b"), sunday.format("%a %-d %b %Y"));
    write_totals(&mut out, report, counted.len());
    write_projects(&mut out, report);
    write_tickets(&mut out, &counted);
    write_days(&mut out, monday, &counted, days);
    write_journal(&mut out, journals);
    write_open(&mut out, &counted);
    out
}

fn write_totals(out: &mut String, report: &Report, entries: usize) {
    let mut parts = vec![format!("**{}** of work in {entries} entries", hours(report.total_min))];
    if report.billable_min > 0 {
        parts.push(format!("{} billable", hours(report.billable_min)));
    }
    parts.push(if report.approved_min > 0 { format!("{} approved", hours(report.approved_min)) } else { "none approved yet".into() });
    let _ = writeln!(out, "{}\n", parts.join(" · "));
}

fn write_projects(out: &mut String, report: &Report) {
    if report.by_project.is_empty() {
        return;
    }
    let total = report.by_project.iter().map(|p| p.min).sum::<i64>().max(1);
    let _ = writeln!(out, "## Projects\n\n| Project | Time | Share |\n|---|---:|---:|");
    for p in &report.by_project {
        let pct = (p.min * 100 + total / 2) / total;
        let share = if pct == 0 { "<1%".to_string() } else { format!("{pct}%") };
        let _ = writeln!(out, "| {} | {} | {share} |", cell(&p.project), hours(p.min));
    }
    out.push('\n');
}

/// Time per ticket, longest first, with the time on no ticket last. The epic column appears
/// only when some ticket has an epic.
fn write_tickets(out: &mut String, counted: &[&ReportEntry]) {
    let mut by_ticket: BTreeMap<String, (String, String, i64)> = BTreeMap::new();
    let mut untracked = 0;
    for e in counted {
        match ticket_of(e) {
            Some((key, title, epic)) => by_ticket.entry(key).or_insert((title, epic, 0)).2 += e.duration_min,
            None => untracked += e.duration_min,
        }
    }
    if by_ticket.is_empty() && untracked == 0 {
        return;
    }
    let mut rows: Vec<_> = by_ticket.into_iter().collect();
    rows.sort_by_key(|(_, (_, _, min))| -min);
    let epics = rows.iter().any(|(_, (_, epic, _))| !epic.is_empty());
    let _ = if epics {
        writeln!(out, "## Tickets\n\n| Ticket | Title | Epic | Time |\n|---|---|---|---:|")
    } else {
        writeln!(out, "## Tickets\n\n| Ticket | Title | Time |\n|---|---|---:|")
    };
    for (key, (title, epic, min)) in rows {
        let epic = if epics { format!(" {} |", cell(&epic)) } else { String::new() };
        let _ = writeln!(out, "| {} | {} |{epic} {} |", cell(&key), cell(&title), hours(min));
    }
    if untracked > 0 {
        let _ = writeln!(out, "| *No ticket* | |{} {} |", if epics { " |" } else { "" }, hours(untracked));
    }
    out.push('\n');
}

/// Time worked each day that has any, with the schedule and unaccounted time when a schedule is set.
fn write_days(out: &mut String, monday: NaiveDate, counted: &[&ReportEntry], days: &[DaySchedule]) {
    let rows: Vec<(String, i64, Option<&DaySchedule>)> = (0..7)
        .map(|i| (monday + Duration::days(i)).format("%Y-%m-%d").to_string())
        .map(|day| {
            let worked = counted.iter().filter(|e| e.day == day).map(|e| e.duration_min).sum();
            let schedule = days.iter().find(|d| d.day == day);
            (day, worked, schedule)
        })
        .filter(|(_, worked, schedule)| *worked > 0 || schedule.is_some())
        .collect();
    if rows.is_empty() {
        return;
    }
    if days.is_empty() {
        let _ = writeln!(out, "## Days\n\n| Day | Worked |\n|---|---:|");
    } else {
        let _ = writeln!(out, "## Days\n\n| Day | Worked | Schedule | Unaccounted |\n|---|---:|---:|---:|");
    }
    for (day, worked, schedule) in rows {
        let extra = match (days.is_empty(), schedule) {
            (true, _) => String::new(),
            (false, Some(d)) => format!(" {} | {} |", hours(d.schedule_min), hours(d.unaccounted_min)),
            (false, None) => " – | – |".into(),
        };
        let _ = writeln!(out, "| {} | {} |{extra}", day_name(&day), hours(worked));
    }
    out.push('\n');
}

fn write_journal(out: &mut String, journals: &[(String, Vec<String>)]) {
    if journals.is_empty() {
        return;
    }
    let _ = writeln!(out, "## Journal\n");
    for (day, lines) in journals {
        let _ = writeln!(out, "### {}\n", day_name(day));
        for l in lines {
            let _ = writeln!(out, "- {}", l.replace('\n', " "));
        }
        out.push('\n');
    }
}

/// The entries without a ticket, longest first, and one line for what is still unapproved.
fn write_open(out: &mut String, counted: &[&ReportEntry]) {
    let mut untracked: Vec<&&ReportEntry> = counted.iter().filter(|e| ticket_of(e).is_none()).collect();
    let unapproved: Vec<&&ReportEntry> = counted.iter().filter(|e| !e.approved).collect();
    if untracked.is_empty() && unapproved.is_empty() {
        return;
    }
    let _ = writeln!(out, "## Needs attention\n");
    if !unapproved.is_empty() {
        let min: i64 = unapproved.iter().map(|e| e.duration_min).sum();
        let which = if unapproved.len() == counted.len() {
            format!("None of the {} entries ({}) is approved yet", counted.len(), hours(min))
        } else {
            format!("{} of {} entries ({}) are not approved yet", unapproved.len(), counted.len(), hours(min))
        };
        let _ = writeln!(out, "{which}; approve them in Reports.\n");
    }
    if !untracked.is_empty() {
        untracked.sort_by_key(|e| (-e.duration_min, e.day.clone()));
        let _ = writeln!(out, "**Without a ticket**\n");
        for e in untracked {
            let project = e.project.as_deref().unwrap_or("No project");
            let label = crate::event::clip(&e.label.replace('\n', " "), 90);
            let _ = writeln!(out, "- {} · {} · {project} · {label}", day_name(&e.day), hours(e.duration_min));
        }
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_iso_weeks_from_their_monday() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
        assert_eq!(week_start(d), NaiveDate::from_ymd_opt(2026, 10, 19).unwrap());
        assert_eq!(week_name(week_start(d)), "2026-W43");
        assert_eq!(week_name(NaiveDate::from_ymd_opt(2026, 12, 28).unwrap()), "2026-W53");
    }

    #[test]
    fn renders_an_empty_week() {
        let monday = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
        let q = ReportQuery { from: "2026-10-19".into(), to: "2026-10-25".into(), grouping: Grouping::TicketDay, tickets: TicketMode::Prefix, context: ContextFilter::Work, fit_day_min: None, fit_drift_min: None };
        let store = crate::store::Store::open_in_memory().unwrap();
        let report = reports::build(&store, &q).unwrap();
        let md = render(monday, &report, &[], &[]);
        assert!(md.starts_with("# 2026-W43 · Mon 19 Oct to Sun 25 Oct 2026"));
        assert!(md.contains("**0m** of work in 0 entries"));
        assert!(!md.contains("## Tickets"));
    }
}
