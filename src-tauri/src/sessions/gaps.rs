//! Quiet time inside the user's work hours, sorted into three kinds by evidence:
//! at the computer (keyboard or mouse input), bridged (a short idle gap between work),
//! or unaccounted (the user decides). Time outside work hours is never filled.

use chrono::{Datelike, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use super::attribute::{idle_intervals, minus, total_ms, union, Interval};
use super::{stable_hash, Session};
use crate::event::{kind, WorkEvent};
use crate::settings::WorkSchedule;

/// Pauses shorter than this already sit inside sessions.
pub const MIN_GAP_MS: i64 = 15 * 60 * 1000;
/// One foreground sample shows about this much presence; the collector samples once a minute.
const FOREGROUND_SAMPLE_MS: i64 = 65 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    AtComputer,
    Bridged,
    Unaccounted,
}

impl GapKind {
    pub fn as_str(self) -> &'static str {
        match self {
            GapKind::AtComputer => "at_computer",
            GapKind::Bridged => "bridged",
            GapKind::Unaccounted => "unaccounted",
        }
    }
}

/// One quiet stretch inside work hours. `targets` are the sessions that get its minutes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gap {
    pub id: String,
    pub day: String,
    pub start_ts: i64,
    pub end_ts: i64,
    pub kind: String,
    /// `open` | `approved` | `assigned` | `not_work`
    pub state: String,
    pub targets: Vec<(String, i64)>,
    pub project: Option<String>,
    pub label: Option<String>,
}

impl Gap {
    pub fn minutes(&self) -> i64 {
        (self.end_ts - self.start_ts) / 60_000
    }

    /// Minutes that count without a further decision: input at the computer, or an
    /// approved bridge. Assigned blocks count through their own session instead.
    pub fn counts(&self) -> bool {
        self.kind == "at_computer" || (self.kind == "bridged" && self.state == "approved")
    }

    pub fn is_open_unaccounted(&self) -> bool {
        self.kind == "unaccounted" && self.state == "open"
    }
}

fn parse_time(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M").ok()
}

fn local_ms(day: NaiveDate, t: NaiveTime) -> Option<i64> {
    Local.from_local_datetime(&day.and_time(t)).earliest().map(|d| d.with_timezone(&Utc).timestamp_millis())
}

/// The work hours of one day as UTC intervals, with breaks removed. Empty on days off.
pub fn work_windows(day: NaiveDate, s: &WorkSchedule) -> Vec<Interval> {
    if !s.days.contains(&day.weekday().num_days_from_monday()) {
        return vec![];
    }
    let (Some(start), Some(end)) = (parse_time(&s.start), parse_time(&s.end)) else { return vec![] };
    let (Some(a), Some(b)) = (local_ms(day, start), local_ms(day, end)) else { return vec![] };
    let breaks: Vec<Interval> = s
        .breaks
        .iter()
        .filter_map(|[bs, be]| Some((local_ms(day, parse_time(bs)?)?, local_ms(day, parse_time(be)?)?)))
        .collect();
    minus(&[(a, b)], &union(breaks))
}

/// Time the user sat at the computer: once-a-minute foreground samples, minus idle stretches.
fn input_intervals(events: &[&WorkEvent]) -> Vec<Interval> {
    let samples = events
        .iter()
        .filter(|e| e.kind == kind::APP_ACTIVE)
        .map(|e| {
            let t = e.ts.timestamp_millis();
            (t, t + FOREGROUND_SAMPLE_MS)
        })
        .collect();
    minus(&union(samples), &idle_intervals(events))
}

fn session_spans(sessions: &[Session]) -> Vec<Interval> {
    union(sessions.iter().filter(|s| s.state != "excluded").map(|s| (s.start_ts, s.end_ts)).collect())
}

/// The session that ends exactly at the gap start and the one that starts exactly at its
/// end. A gap edge on a break or the end of work hours has no neighbour on that side.
fn neighbours<'a>(gap: Interval, sessions: &'a [Session]) -> (Option<&'a Session>, Option<&'a Session>) {
    let live = || sessions.iter().filter(|s| s.state != "excluded");
    let before = live().filter(|s| s.end_ts == gap.0).max_by_key(|s| s.end_ts - s.start_ts);
    let after = live().filter(|s| s.start_ts == gap.1).max_by_key(|s| s.end_ts - s.start_ts);
    (before, after)
}

/// Bridged and at-computer minutes go to the next session when both sides are one project;
/// otherwise they split at the middle. A gap at the edge of the day goes to its one neighbour.
pub fn targets(gap: Interval, before: Option<&Session>, after: Option<&Session>) -> Vec<(String, i64)> {
    let min = (gap.1 - gap.0) / 60_000;
    match (before, after) {
        (Some(b), Some(a)) if b.project == a.project => vec![(a.id.clone(), min)],
        (Some(b), Some(a)) => vec![(b.id.clone(), min / 2), (a.id.clone(), min - min / 2)],
        (Some(only), None) | (None, Some(only)) => vec![(only.id.clone(), min)],
        (None, None) => vec![],
    }
}

/// Picks the kind of one gap from the input during it and its length.
pub fn classify(gap: Interval, input: &[Interval], has_both_sides: bool, bridge_max_min: i64) -> GapKind {
    let present = total_ms(&minus(&[gap], &minus(&[gap], input)));
    let len = gap.1 - gap.0;
    if present * 2 >= len {
        GapKind::AtComputer
    } else if has_both_sides && len <= bridge_max_min * 60_000 {
        GapKind::Bridged
    } else {
        GapKind::Unaccounted
    }
}

/// All gaps of one day inside work hours, at least 15 minutes long. Work hours that have
/// not happened yet are not gaps.
pub fn find_gaps(day: NaiveDate, sessions: &[Session], events: &[WorkEvent], s: &WorkSchedule) -> Vec<Gap> {
    let key = day.format("%Y-%m-%d").to_string();
    let refs: Vec<&WorkEvent> = events.iter().collect();
    let input = input_intervals(&refs);
    let past = minus(&work_windows(day, s), &[(Utc::now().timestamp_millis(), i64::MAX)]);
    let quiet = minus(&past, &session_spans(sessions));
    quiet
        .into_iter()
        .filter(|(a, b)| b - a >= MIN_GAP_MS)
        .map(|gap| {
            let (before, after) = neighbours(gap, sessions);
            let kind = classify(gap, &input, before.is_some() && after.is_some(), s.bridge_max_min);
            Gap {
                id: stable_hash(&["gap", &key, &gap.0.to_string()]),
                day: key.clone(),
                start_ts: gap.0,
                end_ts: gap.1,
                kind: kind.as_str().into(),
                state: "open".into(),
                targets: if kind == GapKind::Unaccounted { vec![] } else { targets(gap, before, after) },
                project: None,
                label: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: i64 = 60_000;

    fn sess(id: &str, project: &str, start: i64, end: i64) -> Session {
        Session { id: id.into(), project: Some(project.into()), start_ts: start, end_ts: end, state: "observed".into(), ..Default::default() }
    }

    #[test]
    fn short_idle_gap_between_work_is_bridged() {
        assert_eq!(classify((0, 40 * M), &[], true, 60), GapKind::Bridged);
        assert_eq!(classify((0, 90 * M), &[], true, 60), GapKind::Unaccounted);
        assert_eq!(classify((0, 40 * M), &[], false, 60), GapKind::Unaccounted);
    }

    #[test]
    fn input_for_half_the_gap_means_at_the_computer() {
        assert_eq!(classify((0, 90 * M), &[(0, 50 * M)], true, 60), GapKind::AtComputer);
    }

    #[test]
    fn same_project_goes_to_the_next_session_else_split() {
        let a = sess("a", "web", 0, 10 * M);
        let b = sess("b", "web", 40 * M, 60 * M);
        let c = sess("c", "api", 40 * M, 60 * M);
        assert_eq!(targets((10 * M, 40 * M), Some(&a), Some(&b)), vec![("b".to_string(), 30)]);
        assert_eq!(targets((10 * M, 40 * M), Some(&a), Some(&c)), vec![("a".to_string(), 15), ("c".to_string(), 15)]);
    }

    #[test]
    fn breaks_and_days_off_are_never_work() {
        let s = WorkSchedule::default();
        let monday = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        let sunday = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
        let windows = work_windows(monday, &s);
        assert_eq!(windows.len(), 2, "lunch splits the day");
        assert_eq!(total_ms(&windows), 8 * 60 * M);
        assert!(work_windows(sunday, &s).is_empty());
    }
}
