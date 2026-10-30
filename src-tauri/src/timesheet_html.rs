//! A report as one self-contained HTML page to read before a time sheet goes out: a timeline
//! and a table per day, with meetings, away time and the measured time beside the reported time.

use std::collections::BTreeMap;
use std::fmt::Write;

use chrono::{DateTime, Local, NaiveDate, TimeZone};

use crate::reports::{Report, ReportEntry};

const PALETTE: [&str; 10] = ["#0b83d9", "#9e5bd9", "#d94182", "#e36a00", "#2da608", "#06a893", "#c9806b", "#465bb3", "#bf7000", "#990099"];

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn hm(min: i64) -> String {
    format!("{}h{:02}", min / 60, min % 60)
}

/// `H:MM:SS`, as a time tracker shows durations.
fn hms(min: i64) -> String {
    format!("{}:{:02}:00", min / 60, min % 60)
}

fn local(ms: i64) -> DateTime<Local> {
    Local.timestamp_millis_opt(ms).single().unwrap_or_default()
}

fn clock(ms: i64) -> String {
    local(ms).format("%H:%M").to_string()
}

/// Minutes since local midnight of `day`.
fn minute_of_day(day: NaiveDate, ms: i64) -> i64 {
    let midnight = Local.from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap()).earliest().unwrap().timestamp_millis();
    (ms - midnight) / 60_000
}

/// The timesheet page for `report`, with the away time stored for its days and a week of five
/// fitted days (8 h each when the report is not fitted).
/// @param include_unapproved also lists entries still waiting for approval.
pub fn page(store: &crate::store::Store, report: &Report, include_unapproved: bool) -> anyhow::Result<String> {
    let entries: Vec<&ReportEntry> = if include_unapproved {
        crate::reports::exportable_unapproved(report).collect()
    } else {
        crate::reports::exportable(report).collect()
    };
    let away = crate::away::spans_ms(store, &report.query.from, &report.query.to)?;
    let week_min = report.query.fit_day_min.unwrap_or(8 * 60) * 5;
    Ok(render(report, &entries, &away, week_min))
}

/// The page for `entries` of `report`, laid out like a time tracker's list view.
/// @param entries the entries to show, in start order: usually the work entries that would leave the app.
/// @param away day → spans the user was away, from `away::spans_ms`.
/// @param week_min the minutes a full week should add up to, for the summary line.
pub fn render(report: &Report, entries: &[&ReportEntry], away: &BTreeMap<String, Vec<(i64, i64)>>, week_min: i64) -> String {
    let mut by_day: BTreeMap<&str, Vec<&ReportEntry>> = BTreeMap::new();
    for e in entries {
        by_day.entry(e.day.as_str()).or_default().push(e);
    }
    let colors = project_colors(entries);
    let hours = hour_range(entries, away);
    let total: i64 = entries.iter().map(|e| e.duration_min).sum();
    let measured: i64 = entries.iter().map(|e| e.measured_min).sum();
    let days_html: String = by_day
        .iter()
        .rev()
        .map(|(day, day_entries)| day_section(day, day_entries, away.get(*day).map_or(&[][..], Vec::as_slice), &colors, hours))
        .collect();
    let week_bars: String = by_day
        .iter()
        .map(|(day, es)| {
            let min: i64 = es.iter().map(|e| e.duration_min).sum();
            let label = NaiveDate::parse_from_str(day, "%Y-%m-%d").map_or_else(|_| esc(day), |d| d.format("%a").to_string());
            format!(r#"<div class="wbar"><div class="wfill" style="height:{:.1}%"></div><span>{label}</span><b>{}</b></div>"#, (min as f64 / 600.0 * 100.0).min(100.0), hms(min))
        })
        .collect();
    let legend: String = colors
        .iter()
        .map(|(p, c)| {
            let min: i64 = entries.iter().filter(|e| &e.toggl_project == p).map(|e| e.duration_min).sum();
            format!(r#"<div class="lrow"><span class="pname" style="color:{c}"><i style="background:{c}"></i>{}</span><b>{}</b></div>"#, esc(p), hms(min))
        })
        .collect();
    let left = week_min - total;
    let week_line = if left > 0 { format!("{} to go", hms(left)) } else { "done".to_string() };

    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Timesheet {from} to {to}</title>
<link rel="preconnect" href="https://fonts.googleapis.com"><link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet">
<style>{CSS}</style></head>
<body>
<div class="topbar"><div class="inner"><span class="brand">Timesheet</span><span class="range">{from_h} – {to_h}</span></div></div>
<main>
<section class="summary">
  <div class="totals">
    <div class="big"><span>Week total</span><b>{total}</b><small>measured {measured} · {week_line} of {week}</small></div>
    <div class="week">{week_bars}</div>
  </div>
  <div class="legend">{legend}</div>
</section>
{days_html}
<footer>Reported time is fitted from measured time. Hatched spans on a day's timeline are time away.</footer>
</main></body></html>"#,
        from = esc(&report.query.from),
        to = esc(&report.query.to),
        from_h = pretty_day(&report.query.from),
        to_h = pretty_day(&report.query.to),
        total = hms(total),
        measured = hms(measured),
        week = hms(week_min),
    )
}

fn pretty_day(day: &str) -> String {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").map_or_else(|_| esc(day), |d| d.format("%a, %-d %b").to_string())
}

/// A color per Toggl project, the busiest project first.
fn project_colors(entries: &[&ReportEntry]) -> Vec<(String, &'static str)> {
    let mut totals: BTreeMap<&str, i64> = BTreeMap::new();
    for e in entries {
        *totals.entry(&e.toggl_project).or_default() += e.duration_min;
    }
    let mut projects: Vec<(&str, i64)> = totals.into_iter().collect();
    projects.sort_by_key(|(_, m)| -m);
    projects.into_iter().enumerate().map(|(i, (p, _))| (p.to_string(), PALETTE[i % PALETTE.len()])).collect()
}

/// The whole hours every day's timeline spans, so the days line up.
fn hour_range(entries: &[&ReportEntry], away: &BTreeMap<String, Vec<(i64, i64)>>) -> (i64, i64) {
    let mut lo = i64::MAX;
    let mut hi = i64::MIN;
    for e in entries {
        let Ok(day) = NaiveDate::parse_from_str(&e.day, "%Y-%m-%d") else { continue };
        for b in e.blocks() {
            lo = lo.min(minute_of_day(day, b.start_ts));
            hi = hi.max(minute_of_day(day, b.start_ts) + b.duration_min);
        }
        for &(s, end) in away.get(&e.day).into_iter().flatten() {
            lo = lo.min(minute_of_day(day, s));
            hi = hi.max(minute_of_day(day, end));
        }
    }
    if lo > hi {
        return (8, 18);
    }
    (lo / 60, ((hi + 59) / 60).max(lo / 60 + 1))
}

/// One row per piece, newest first within the day, as a time tracker lists them.
fn day_section(day: &str, entries: &[&ReportEntry], away: &[(i64, i64)], colors: &[(String, &str)], hours: (i64, i64)) -> String {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap_or_default();
    let span = ((hours.1 - hours.0) * 60) as f64;
    let pos = |ms: i64| (minute_of_day(date, ms) - hours.0 * 60) as f64 / span * 100.0;
    let color = |p: &str| colors.iter().find(|(name, _)| name == p).map_or("#888", |(_, c)| c);
    let total: i64 = entries.iter().map(|e| e.duration_min).sum();
    let measured: i64 = entries.iter().map(|e| e.measured_min).sum();

    let mut bar = String::new();
    for &(s, e) in away {
        let _ = write!(bar, r#"<div class="away" style="left:{:.3}%;width:{:.3}%" title="Away {}–{}"></div>"#, pos(s), pos(e) - pos(s), clock(s), clock(e));
    }
    let mut pieces: Vec<(&ReportEntry, crate::reports::Piece)> = entries.iter().flat_map(|e| e.blocks().into_iter().map(move |b| (*e, b))).collect();
    pieces.sort_by_key(|(_, b)| -b.start_ts);
    for (e, b) in &pieces {
        let end = b.start_ts + b.duration_min * 60_000;
        let _ = write!(
            bar,
            r#"<div class="blk" style="left:{:.3}%;width:{:.3}%;background:{}" title="{}–{} · {} · {}"></div>"#,
            pos(b.start_ts),
            pos(end) - pos(b.start_ts),
            color(&e.toggl_project),
            clock(b.start_ts),
            clock(end),
            esc(&e.toggl_project),
            esc(&e.label)
        );
    }
    let ticks: String = (hours.0..=hours.1)
        .step_by(2)
        .map(|h| format!(r#"<span style="left:{:.3}%">{h:02}:00</span>"#, ((h - hours.0) * 60) as f64 / span * 100.0))
        .collect();
    let rows: String = pieces.iter().map(|(e, b)| entry_row(e, *b, color(&e.toggl_project))).collect();
    let away_note = if away.is_empty() {
        String::new()
    } else {
        format!(r#"<span class="awaynote">Away {}</span>"#, away.iter().map(|&(s, e)| format!("{}–{}", clock(s), clock(e))).collect::<Vec<_>>().join(", "))
    };
    format!(
        r#"<section class="day"><div class="dayhead"><h2>{title}</h2>{away_note}<span class="dtotal"><small>measured {measured}</small>{total}</span></div>
<div class="timeline"><div class="track">{bar}</div><div class="ticks">{ticks}</div></div>
<ul class="entries">{rows}</ul></section>
"#,
        title = date.format("%a, %-d %b"),
        total = hms(total),
        measured = hm(measured),
    )
}

fn entry_row(e: &ReportEntry, b: crate::reports::Piece, color: &str) -> String {
    let end = b.start_ts + b.duration_min * 60_000;
    let ticket = |t: &Option<crate::tickets::Ticket>, info: &Option<crate::trackers::TicketInfo>| match (t, info) {
        (Some(t), Some(i)) if !i.url.is_empty() => format!(r#"<a class="tag" href="{}" title="{}">{}</a>"#, esc(&i.url), esc(&i.title), esc(&t.key)),
        (Some(t), _) => format!(r#"<span class="tag">{}</span>"#, esc(&t.key)),
        _ => String::new(),
    };
    let tags = ticket(&e.jira, &e.jira_info) + &ticket(&e.github, &e.github_info);
    let tags = if tags.is_empty() { r#"<span class="tag none">no ticket</span>"#.to_string() } else { tags };
    let suggested = e.suggestion.as_ref().is_some_and(|s| e.description.split(' ').any(|w| w == s.info.key) && e.jira.as_ref().is_none_or(|j| j.key != s.info.key));
    let part = format!(
        "{}{}",
        if suggested { r#"<span class="tag suggested" title="Ticket from Find tickets, not yet confirmed">suggested</span>"# } else { "" },
        if e.pieces.len() > 1 { r#"<span class="tag split">split</span>"# } else { "" }
    );
    format!(
        r#"<li><div class="what"><div class="desc">{}</div><div class="label">{}</div><div class="meta"><span class="pname" style="color:{color}"><i style="background:{color}"></i>{}</span>{tags}{part}</div></div><div class="when"><span class="range">{} – {}</span><span class="dur">{}</span></div></li>"#,
        esc(&e.description),
        if e.description == e.label { String::new() } else { esc(&e.label) },
        esc(&e.toggl_project),
        clock(b.start_ts),
        clock(end),
        hms(b.duration_min)
    )
}

const CSS: &str = r#"
:root{--bg:#fbf9fb;--card:#fff;--ink:#271a2c;--muted:#7a6e7e;--line:#ece6ee;--track:#f3eef4;--top:#2c1338;--accent:#e57cd8;--tag:#f4eff5}
@media (prefers-color-scheme:dark){:root{--bg:#1a1020;--card:#241829;--ink:#f2ecf4;--muted:#a597aa;--line:#36283c;--track:#2f2235;--top:#120818;--tag:#33263a}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);font:14px/1.45 Inter,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}
.topbar{background:var(--top);color:#fff}
.topbar .inner{max-width:1040px;margin:0 auto;padding:14px 16px;display:flex;align-items:baseline;gap:14px}
.brand{font-weight:700;font-size:17px;letter-spacing:-.01em}
.brand::before{content:"";display:inline-block;width:10px;height:10px;border-radius:50%;background:var(--accent);margin-right:9px;vertical-align:1px}
.topbar .range{color:#cdbfd3;font-size:13px}
main{max-width:1040px;margin:0 auto;padding:24px 16px 48px}
.summary{display:grid;grid-template-columns:1.6fr 1fr;gap:16px;margin-bottom:24px}
@media (max-width:760px){.summary{grid-template-columns:1fr}}
.totals,.legend{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:18px}
.totals{display:flex;gap:24px;align-items:flex-end;flex-wrap:wrap}
.big span{display:block;color:var(--muted);font-size:12px;font-weight:600;text-transform:uppercase;letter-spacing:.05em}
.big b{display:block;font-size:34px;font-weight:700;letter-spacing:-.02em;font-variant-numeric:tabular-nums}
.big small{color:var(--muted)}
.week{display:flex;gap:10px;align-items:flex-end;flex:1;min-width:220px;height:110px}
.wbar{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:flex-end;height:100%;position:relative;font-size:11px;color:var(--muted)}
.wfill{width:100%;max-width:38px;background:var(--accent);border-radius:4px 4px 0 0;opacity:.85}
.wbar b{font-weight:500;color:var(--ink);font-variant-numeric:tabular-nums;font-size:10px}
.legend{display:flex;flex-direction:column;gap:6px}
.lrow{display:flex;justify-content:space-between;gap:8px;font-size:13px}
.lrow b{font-weight:500;font-variant-numeric:tabular-nums}
.pname{display:inline-flex;align-items:center;gap:6px;font-weight:500;white-space:nowrap}
.pname i{width:8px;height:8px;border-radius:50%;display:inline-block}
.day{background:var(--card);border:1px solid var(--line);border-radius:10px;margin-bottom:18px;overflow:hidden}
.dayhead{display:flex;align-items:center;gap:12px;padding:12px 18px;border-bottom:1px solid var(--line);background:var(--bg)}
.dayhead h2{font-size:14px;font-weight:600;margin:0}
.awaynote{font-size:12px;color:var(--muted);background:var(--tag);padding:2px 8px;border-radius:4px}
.dtotal{margin-left:auto;font-weight:600;font-variant-numeric:tabular-nums}
.dtotal small{font-weight:400;color:var(--muted);margin-right:10px}
.timeline{padding:12px 18px 2px}
.track{position:relative;height:22px;background:var(--track);border-radius:4px;overflow:hidden}
.blk{position:absolute;top:3px;bottom:3px;border-radius:3px;border-right:1px solid var(--card)}
.away{position:absolute;top:0;bottom:0;background:repeating-linear-gradient(135deg,transparent 0 5px,rgba(122,110,126,.3) 5px 8px)}
.ticks{position:relative;height:18px;font-size:10px;color:var(--muted);font-variant-numeric:tabular-nums}
.ticks span{position:absolute;transform:translateX(-50%)}
.entries{list-style:none;margin:0;padding:0}
.entries li{display:flex;align-items:center;gap:16px;padding:10px 18px;border-top:1px solid var(--line)}
.entries li:hover{background:var(--bg)}
.what{flex:1;min-width:0}
.desc{font-weight:500;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.label{color:var(--muted);font-size:12px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.label:empty{display:none}
.meta{display:flex;flex-wrap:wrap;align-items:center;gap:8px;margin-top:3px;font-size:12px}
.tag{background:var(--tag);color:var(--muted);border-radius:4px;padding:1px 7px;font-size:11px;font-weight:500;text-decoration:none;white-space:nowrap}
a.tag:hover{color:var(--ink)}
.tag.none{font-style:italic;font-weight:400}
.tag.suggested{background:#fff1d6;color:#8a5a00}
@media (prefers-color-scheme:dark){.tag.suggested{background:#3d2e10;color:#f0c46a}}
.tag.split{background:transparent;border:1px dashed var(--line)}
.when{display:flex;align-items:center;gap:18px;white-space:nowrap;font-variant-numeric:tabular-nums}
.when .range{color:var(--muted);font-size:13px}
.when .dur{font-weight:600;min-width:64px;text-align:right}
@media (max-width:600px){.entries li{flex-direction:column;align-items:flex-start;gap:4px}.when{gap:10px}}
footer{color:var(--muted);font-size:12px;margin-top:20px}
"#;
