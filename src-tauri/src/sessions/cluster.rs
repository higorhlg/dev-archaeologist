use std::collections::HashMap;

use super::attribute::{attribute, PRESENCE_MS};
use super::{stable_hash, Session};
use crate::event::{kind, WorkEvent};

/// A pause longer than this ends a session.
pub const IDLE_GAP_MS: i64 = 15 * 60 * 1000;
/// A visit to another project shorter than this, with no prompt or commit, joins a nearby session.
pub const SWITCH_MIN_MS: i64 = 5 * 60 * 1000;

/// Groups one day of events into sessions with fixed rules. No AI call happens here.
/// Each project is grouped on its own, so sessions on two projects can overlap when
/// agents work in parallel. Events with no project join the session they fall inside.
/// @param events one local day of events, any order.
pub fn cluster_sessions(day: &str, events: &[WorkEvent]) -> Vec<Session> {
    let mut sorted: Vec<&WorkEvent> = events.iter().collect();
    sorted.sort_by_key(|e| e.ts);
    let mut by_project: HashMap<&str, Vec<&WorkEvent>> = HashMap::new();
    let mut neutral = Vec::new();
    for e in sorted {
        match e.project.as_deref() {
            Some(p) => by_project.entry(p).or_default().push(e),
            None => neutral.push(e),
        }
    }
    let mut runs: Vec<Vec<&WorkEvent>> = by_project.into_values().flat_map(|evs| split_by_gap(&evs)).collect();
    runs = absorb_short_visits(runs);
    let orphans = attach_neutral(&mut runs, neutral);
    runs.extend(split_by_gap(&orphans));
    let mut sessions: Vec<Session> = runs
        .into_iter()
        .filter(|run| run.iter().any(|e| e.kind != kind::IDLE_START && e.kind != kind::IDLE_END && e.kind != kind::APP_ACTIVE))
        .map(|mut run| {
            run.sort_by_key(|e| e.ts);
            build_session(day, &run)
        })
        .collect();
    sessions.sort_by_key(|s| s.start_ts);
    sessions
}

fn event_end(e: &WorkEvent) -> i64 {
    let s = e.ts.timestamp_millis();
    if e.kind == kind::AGENT_TURN {
        s + e.meta_i64("dur_ms").unwrap_or(0)
    } else {
        s
    }
}

fn split_by_gap<'a>(events: &[&'a WorkEvent]) -> Vec<Vec<&'a WorkEvent>> {
    let mut blocks: Vec<Vec<&WorkEvent>> = Vec::new();
    let mut end = i64::MIN;
    for &e in events {
        let t = e.ts.timestamp_millis();
        if blocks.is_empty() || t - end > IDLE_GAP_MS {
            blocks.push(Vec::new());
            end = t;
        }
        blocks.last_mut().unwrap().push(e);
        end = end.max(event_end(e));
    }
    blocks
}

fn bounds(run: &[&WorkEvent]) -> (i64, i64) {
    let s = run.iter().map(|e| e.ts.timestamp_millis()).min().unwrap_or(0);
    (s, run.iter().map(|e| event_end(e)).max().unwrap_or(s))
}

fn has_strong_evidence(run: &[&WorkEvent]) -> bool {
    run.iter().any(|e| e.kind == kind::PROMPT || e.kind == kind::COMMIT)
}

/// The run whose time span, widened by `slack`, holds `t`, picking the closest start.
fn covering_run(runs: &[Vec<&WorkEvent>], t: i64, slack: i64, skip: Option<usize>) -> Option<usize> {
    runs.iter()
        .enumerate()
        .filter(|(i, r)| Some(*i) != skip && !r.is_empty())
        .filter(|(_, r)| {
            let (s, e) = bounds(r);
            t >= s - slack && t <= e + slack
        })
        .min_by_key(|(_, r)| (t - bounds(r).0).abs())
        .map(|(i, _)| i)
}

/// A short visit to another project, with no prompt or commit, is not its own session.
fn absorb_short_visits<'a>(mut runs: Vec<Vec<&'a WorkEvent>>) -> Vec<Vec<&'a WorkEvent>> {
    let mut i = 0;
    while i < runs.len() {
        let (s, e) = bounds(&runs[i]);
        if e - s < SWITCH_MIN_MS && !has_strong_evidence(&runs[i]) {
            if let Some(host) = covering_run(&runs, s, PRESENCE_MS, Some(i)) {
                let moved = std::mem::take(&mut runs[i]);
                runs[host].extend(moved);
                runs.remove(i);
                continue;
            }
        }
        i += 1;
    }
    runs
}

/// Puts each no-project event (app focus, idle) into the session it falls inside.
/// @returns the events no session covers.
fn attach_neutral<'a>(runs: &mut [Vec<&'a WorkEvent>], neutral: Vec<&'a WorkEvent>) -> Vec<&'a WorkEvent> {
    let mut orphans = Vec::new();
    for e in neutral {
        match covering_run(runs, e.ts.timestamp_millis(), PRESENCE_MS, None) {
            Some(i) => runs[i].push(e),
            None => orphans.push(e),
        }
    }
    orphans
}

fn most_common<'a>(values: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for v in values {
        *counts.entry(v).or_default() += 1;
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(v, _)| v.to_string())
}

fn build_session(day: &str, run: &[&WorkEvent]) -> Session {
    let project = most_common(run.iter().filter_map(|e| e.project.as_deref()));
    let branch = most_common(
        run.iter().filter(|e| e.project == project).filter_map(|e| e.branch.as_deref()).filter(|b| *b != "HEAD"),
    );
    let (active_min, agent_only_min, end) = attribute(run);
    let start_ts = run[0].ts.timestamp_millis();
    let last_user = run.iter().filter(|e| e.is_user_activity()).map(|e| e.ts.timestamp_millis() + PRESENCE_MS).max();
    Session {
        id: stable_hash(&[day, project.as_deref().unwrap_or("-"), &run[0].id]),
        day: day.to_string(),
        start_ts,
        end_ts: end.max(last_user.unwrap_or(start_ts)).max(run.last().unwrap().ts.timestamp_millis()),
        label: observed_label(run),
        summary: Some(evidence_summary(run)),
        confidence: Some(confidence(run, project.is_some()).into()),
        project,
        branch,
        active_min,
        agent_only_min,
        state: "observed".into(),
        event_ids: run.iter().map(|e| e.id.clone()).collect(),
    }
}

/// A label taken straight from the evidence: an agent session title, else the last commit
/// subject, else the first prompt.
fn observed_label(run: &[&WorkEvent]) -> Option<String> {
    run.iter()
        .rev()
        .find(|e| e.kind == kind::AI_TITLE)
        .or_else(|| run.iter().rev().find(|e| e.kind == kind::COMMIT))
        .map(|e| e.detail.clone())
        .or_else(|| run.iter().find(|e| e.kind == kind::PROMPT).map(|e| e.detail.as_deref().map(first_line)))
        .flatten()
        .filter(|l| !l.is_empty())
}

/// The first line of a prompt, cut before any pasted block, at most 80 chars.
fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("");
    crate::event::clip(line.split('<').next().unwrap_or(""), 80)
}

pub fn evidence_counts(run: &[&WorkEvent]) -> Vec<(String, usize)> {
    let (mut commits, mut tests_ok, mut tests_bad, mut files, mut shell) = (0, 0, 0, 0, 0);
    for e in run {
        match e.kind.as_str() {
            kind::COMMIT => commits += 1,
            kind::TEST_PASSED => tests_ok += 1,
            kind::TEST_FAILED => tests_bad += 1,
            kind::FILE_CHANGED => files += 1,
            kind::SHELL_COMMAND => shell += 1,
            _ => {}
        }
    }
    let mut out = Vec::new();
    for src in ["claude_code", "codex"] {
        let n = run.iter().filter(|e| e.source == src && e.kind == kind::PROMPT).count();
        if n > 0 {
            out.push((format!("{} prompts", if src == "codex" { "Codex" } else { "Claude Code" }), n));
        }
    }
    for (label, n) in [
        ("test runs passed", tests_ok),
        ("test runs failed", tests_bad),
        ("commits", commits),
        ("file changes", files),
        ("commands", shell),
    ] {
        if n > 0 {
            out.push((label.to_string(), n));
        }
    }
    out
}

fn evidence_summary(run: &[&WorkEvent]) -> String {
    evidence_counts(run).iter().map(|(l, n)| format!("{n} {l}")).collect::<Vec<_>>().join(" · ")
}

fn confidence(run: &[&WorkEvent], has_project: bool) -> &'static str {
    let strong = run.iter().any(|e| e.kind == kind::COMMIT || e.kind == kind::PROMPT);
    match (has_project, strong) {
        (true, true) => "high",
        (true, false) => "medium",
        _ => "low",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    fn ev(min: i64, project: Option<&str>, k: &str) -> WorkEvent {
        let mut e = WorkEvent::new("test", format!("{min}{k}"), Utc.with_ymd_and_hms(2026, 10, 8, 9, 0, 0).unwrap() + Duration::minutes(min), k);
        e.project = project.map(String::from);
        e
    }

    #[test]
    fn gap_splits_and_short_switch_stays() {
        let events = vec![
            ev(0, Some("a"), kind::PROMPT),
            ev(10, Some("a"), kind::FILE_CHANGED),
            ev(12, Some("b"), kind::FILE_CHANGED), // short visit to b with no prompt: joins a
            ev(14, Some("a"), kind::COMMIT),
            ev(60, Some("b"), kind::PROMPT), // after a 46-minute gap: new session
            ev(70, Some("b"), kind::COMMIT),
        ];
        let s = cluster_sessions("2026-10-08", &events);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].project.as_deref(), Some("a"));
        assert_eq!(s[1].project.as_deref(), Some("b"));
        assert_eq!(s[0].confidence.as_deref(), Some("high"));
    }
}
