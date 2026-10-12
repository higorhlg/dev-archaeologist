use std::collections::HashSet;

use chrono::{Local, Timelike};

use crate::event::{kind, WorkEvent};
use crate::store::Achievement;

/// Runs every rule over one day. A badge exists only when a rule matches real events;
/// the AI may rename a badge later but never creates one.
/// @param prior the 30 days before `day`, for rules that need history.
pub fn detect(day: &str, events: &[WorkEvent], prior: &[WorkEvent]) -> Vec<Achievement> {
    let mut sorted: Vec<&WorkEvent> = events.iter().collect();
    sorted.sort_by_key(|e| e.ts);
    let mut out = Vec::new();
    out.extend(one_typo_many_bugs(day, &sorted));
    out.extend(night_owl(day, &sorted));
    out.extend(comeback(day, &sorted, prior));
    out.extend(agent_whisperer(day, &sorted));
    out.extend(yak_shave(day, &sorted));
    out
}

fn award(day: &str, rule: &str, title: &str, detail: String, evidence: &WorkEvent) -> Achievement {
    Achievement {
        id: crate::sessions::stable_hash(&[day, rule, &evidence.id]),
        day: day.into(),
        rule: rule.into(),
        title: title.into(),
        detail,
        evidence_id: evidence.id.clone(),
    }
}

fn same_project(a: &WorkEvent, b: &WorkEvent) -> bool {
    a.project.is_some() && a.project == b.project
}

/// A commit that changes at most 3 characters, after a test run with 3 or more failures
/// and before a clean run, in the same project.
fn one_typo_many_bugs(day: &str, ev: &[&WorkEvent]) -> Option<Achievement> {
    ev.iter().filter(|c| c.kind == kind::COMMIT).find_map(|c| {
        let chars = c.meta_i64("changed_chars")?;
        if chars > 3 {
            return None;
        }
        let before = ev.iter().rev().filter(|e| e.ts <= c.ts && same_project(e, c)).find(|e| e.kind.starts_with("test_"))?;
        let after = ev.iter().filter(|e| e.ts > c.ts && same_project(e, c)).find(|e| e.kind.starts_with("test_"))?;
        let fixed = before.meta_i64("fail").unwrap_or(0);
        (before.kind == kind::TEST_FAILED && after.kind == kind::TEST_PASSED && fixed >= 3).then(|| {
            award(
                day,
                "one_typo_many_bugs",
                &format!("Fixed {} bugs caused by one typo", number_word(fixed)),
                format!("{chars} changed character(s) · {fixed} failing tests now pass"),
                c,
            )
        })
    })
}

fn number_word(n: i64) -> String {
    ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"]
        .get(n as usize)
        .map(|s| s.to_string())
        .unwrap_or_else(|| n.to_string())
}

/// A commit between 23:00 and 04:00 local time.
fn night_owl(day: &str, ev: &[&WorkEvent]) -> Option<Achievement> {
    let c = ev.iter().rev().find(|e| {
        let h = e.ts.with_timezone(&Local).hour();
        e.kind == kind::COMMIT && (h >= 23 || h < 4)
    })?;
    let at = c.ts.with_timezone(&Local).format("%H:%M");
    Some(award(day, "night_owl", "Night owl", format!("Committed at {at}"), c))
}

/// The first commit in a project that had no events in the previous 30 days.
fn comeback(day: &str, ev: &[&WorkEvent], prior: &[WorkEvent]) -> Option<Achievement> {
    let active_before: HashSet<&str> = prior.iter().filter_map(|e| e.project.as_deref()).collect();
    if active_before.is_empty() {
        return None;
    }
    let c = ev.iter().find(|e| e.kind == kind::COMMIT && e.project.as_deref().is_some_and(|p| !active_before.contains(p)))?;
    Some(award(
        day,
        "comeback",
        "The comeback",
        format!("Back in {} after 30+ quiet days", c.project.as_deref().unwrap_or("?")),
        c,
    ))
}

/// Tests failed in the 30 minutes before a prompt; after that one prompt they pass,
/// with no failure between.
fn agent_whisperer(day: &str, ev: &[&WorkEvent]) -> Option<Achievement> {
    ev.iter().enumerate().filter(|(_, e)| e.kind == kind::PROMPT).find_map(|(i, p)| {
        let rest = &ev[i + 1..];
        let next_prompt = rest.iter().position(|e| e.kind == kind::PROMPT && e.source == p.source).unwrap_or(rest.len());
        let window = &rest[..next_prompt];
        let pass = window.iter().find(|e| e.kind == kind::TEST_PASSED && same_project(e, p))?;
        let failed = window.iter().any(|e| e.kind == kind::TEST_FAILED && e.ts < pass.ts);
        let earlier_fail = ev[..i]
            .iter()
            .rev()
            .take_while(|e| (p.ts - e.ts).num_minutes() <= 30)
            .any(|e| e.kind == kind::TEST_FAILED && same_project(e, p));
        (!failed && earlier_fail).then(|| {
            award(day, "agent_whisperer", "Agent whisperer", "One prompt turned red tests green".into(), pass)
        })
    })
}

/// Five or more different projects within one hour.
fn yak_shave(day: &str, ev: &[&WorkEvent]) -> Option<Achievement> {
    for (i, e) in ev.iter().enumerate() {
        let projects: HashSet<&str> = ev[i..]
            .iter()
            .take_while(|x| (x.ts - e.ts).num_minutes() <= 60)
            .filter(|x| x.is_user_activity())
            .filter_map(|x| x.project.as_deref())
            .collect();
        if projects.len() >= 5 {
            return Some(award(day, "yak_shave", "Yak shaver", format!("{} projects in one hour", projects.len()), e));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::json;

    fn ev(min: i64, k: &str, meta: serde_json::Value) -> WorkEvent {
        let mut e = WorkEvent::new("t", format!("{min}"), Utc.with_ymd_and_hms(2026, 10, 8, 12, 0, 0).unwrap() + Duration::minutes(min), k);
        e.project = Some("api".into());
        e.meta = meta;
        e
    }

    #[test]
    fn typo_rule_needs_all_three_facts() {
        let events = vec![
            ev(0, kind::TEST_FAILED, json!({"fail": 3, "pass": 10})),
            ev(5, kind::COMMIT, json!({"changed_chars": 1})),
            ev(6, kind::TEST_PASSED, json!({"fail": 0, "pass": 13})),
        ];
        let got = detect("2026-10-08", &events, &[]);
        assert!(got.iter().any(|a| a.title == "Fixed three bugs caused by one typo"));
        let no_fix = vec![events[0].clone(), events[1].clone()];
        assert!(detect("2026-10-08", &no_fix, &[]).iter().all(|a| a.rule != "one_typo_many_bugs"));
    }
}
