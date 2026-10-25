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
    let db = std::env::var("DEV_ARCH_DB").map(PathBuf::from).unwrap_or_else(|_| crate::store::default_db_path());
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

fn hours(min: i64) -> String {
    format!("{}h{:02}", min / 60, min % 60)
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

/// The report as Markdown: totals, projects, tickets, days against the schedule, journal lines,
/// and what still needs attention.
pub fn render(monday: NaiveDate, report: &Report, days: &[DaySchedule], journals: &[(String, Vec<String>)]) -> String {
    let sunday = monday + Duration::days(6);
    let counted: Vec<&ReportEntry> = report.entries.iter().filter(|e| !e.skip).collect();
    let mut out = String::new();
    let _ = writeln!(out, "# {} · {} to {}\n", week_name(monday), monday.format("%a %-d %b"), sunday.format("%a %-d %b %Y"));
    let _ = writeln!(
        out,
        "**{}** of work · {} approved · {} billable · {} entries to review\n",
        hours(report.total_min),
        hours(report.approved_min),
        hours(report.billable_min),
        report.review_count
    );

    if !report.by_project.is_empty() {
        let _ = writeln!(out, "## Projects\n\n| Project | Time |\n|---|---:|");
        for p in &report.by_project {
            let _ = writeln!(out, "| {} | {} |", cell(&p.project), hours(p.min));
        }
        out.push('\n');
    }

    let mut by_ticket: BTreeMap<String, (String, String, i64)> = BTreeMap::new();
    let mut untracked = 0;
    for e in &counted {
        match ticket_of(e) {
            Some((key, title, epic)) => by_ticket.entry(key).or_insert((title, epic, 0)).2 += e.duration_min,
            None => untracked += e.duration_min,
        }
    }
    if !by_ticket.is_empty() || untracked > 0 {
        let mut rows: Vec<_> = by_ticket.into_iter().collect();
        rows.sort_by_key(|(_, (_, _, min))| -min);
        let _ = writeln!(out, "## Tickets\n\n| Ticket | Title | Epic | Time |\n|---|---|---|---:|");
        for (key, (title, epic, min)) in rows {
            let _ = writeln!(out, "| {} | {} | {} | {} |", cell(&key), cell(&title), cell(&epic), hours(min));
        }
        if untracked > 0 {
            let _ = writeln!(out, "| (no ticket) | | | {} |", hours(untracked));
        }
        out.push('\n');
    }

    if !days.is_empty() {
        let _ = writeln!(out, "## Days\n\n| Day | Worked | Schedule | Unaccounted |\n|---|---:|---:|---:|");
        for d in days {
            let worked: i64 = counted.iter().filter(|e| e.day == d.day).map(|e| e.duration_min).sum();
            let _ = writeln!(out, "| {} | {} | {} | {} |", d.day, hours(worked), hours(d.schedule_min), hours(d.unaccounted_min));
        }
        out.push('\n');
    }

    if !journals.is_empty() {
        let _ = writeln!(out, "## Journal\n");
        for (day, lines) in journals {
            let _ = writeln!(out, "### {day}\n");
            for l in lines {
                let _ = writeln!(out, "- {}", l.replace('\n', " "));
            }
            out.push('\n');
        }
    }

    let open: Vec<&&ReportEntry> = counted.iter().filter(|e| !e.approved || ticket_of(e).is_none()).collect();
    if !open.is_empty() {
        let _ = writeln!(out, "## Needs attention\n");
        for e in open {
            let mut why = e.reasons.clone();
            if ticket_of(e).is_none() {
                why.push("No ticket".into());
            }
            let _ = writeln!(out, "- {} · {} · {} · {} — {}", e.day, hours(e.duration_min), e.project.as_deref().unwrap_or("No project"), e.label.replace('\n', " "), why.join(", "));
        }
    }
    out
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
        let q = ReportQuery { from: "2026-10-19".into(), to: "2026-10-25".into(), grouping: Grouping::TicketDay, tickets: TicketMode::Prefix, context: ContextFilter::Work };
        let store = crate::store::Store::open_in_memory().unwrap();
        let report = reports::build(&store, &q).unwrap();
        let md = render(monday, &report, &[], &[]);
        assert!(md.starts_with("# 2026-W43 · Mon 19 Oct to Sun 25 Oct 2026"));
        assert!(md.contains("**0h00** of work"));
        assert!(!md.contains("## Tickets"));
    }
}
