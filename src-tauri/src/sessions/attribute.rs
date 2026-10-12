use crate::event::{kind, WorkEvent};

/// One user action keeps the user "present" for this long, unless an idle event cuts it.
pub const PRESENCE_MS: i64 = 5 * 60 * 1000;
const ROUND_MIN: i64 = 5;

pub type Interval = (i64, i64);

/// Sorts and joins overlapping intervals.
pub fn union(mut v: Vec<Interval>) -> Vec<Interval> {
    v.sort();
    let mut out: Vec<Interval> = Vec::new();
    for (s, e) in v.into_iter().filter(|(s, e)| e > s) {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// Removes every `cut` interval from `base`. Both inputs must be unions.
pub fn minus(base: &[Interval], cut: &[Interval]) -> Vec<Interval> {
    let mut out = Vec::new();
    for &(mut s, e) in base {
        for &(cs, ce) in cut {
            if ce <= s || cs >= e {
                continue;
            }
            if cs > s {
                out.push((s, cs));
            }
            s = s.max(ce);
            if s >= e {
                break;
            }
        }
        if s < e {
            out.push((s, e));
        }
    }
    out
}

pub fn total_ms(v: &[Interval]) -> i64 {
    v.iter().map(|(s, e)| e - s).sum()
}

/// Rounds to 5 minutes, with at least 5 for any real activity. No false precision.
pub fn round_min(ms: i64) -> i64 {
    if ms <= 0 {
        return 0;
    }
    (((ms as f64 / 60_000.0) / ROUND_MIN as f64).round() as i64).max(1) * ROUND_MIN
}

/// An agent tool call or test run shows about one minute of agent work, which matters when
/// the agent edits a project other than the folder its turn started in.
const AGENT_ACTION_MS: i64 = 60 * 1000;

fn is_agent_action(e: &WorkEvent) -> bool {
    (e.source == "claude_code" || e.source == "codex")
        && matches!(e.kind.as_str(), kind::TOOL_USE | kind::TEST_PASSED | kind::TEST_FAILED)
}

pub fn agent_intervals(events: &[&WorkEvent]) -> Vec<Interval> {
    union(
        events
            .iter()
            .filter_map(|e| {
                let s = e.ts.timestamp_millis();
                match e.kind.as_str() {
                    kind::AGENT_TURN => Some((s, s + e.meta_i64("dur_ms").unwrap_or(0))),
                    _ if is_agent_action(e) => Some((s, s + AGENT_ACTION_MS)),
                    _ => None,
                }
            })
            .collect(),
    )
}

pub fn idle_intervals(events: &[&WorkEvent]) -> Vec<Interval> {
    let mut out = Vec::new();
    let mut open: Option<i64> = None;
    for e in events {
        match e.kind.as_str() {
            kind::IDLE_START => open = Some(e.ts.timestamp_millis()),
            kind::IDLE_END => {
                if let Some(s) = open.take() {
                    out.push((s, e.ts.timestamp_millis()));
                }
            }
            _ => {}
        }
    }
    union(out)
}

/// A meeting keeps the user present for its whole length, even with no keyboard input.
fn meeting_intervals(events: &[&WorkEvent]) -> Vec<Interval> {
    events
        .iter()
        .filter(|e| e.kind == kind::MEETING)
        .map(|e| {
            let s = e.ts.timestamp_millis();
            (s, s + e.meta_i64("dur_ms").unwrap_or(0))
        })
        .collect()
}

pub fn user_intervals(events: &[&WorkEvent]) -> Vec<Interval> {
    let raw = events
        .iter()
        .filter(|e| e.is_user_activity())
        .map(|e| {
            let s = e.ts.timestamp_millis();
            (s, s + PRESENCE_MS)
        })
        .collect();
    let typed = minus(&union(raw), &idle_intervals(events));
    union(typed.into_iter().chain(meeting_intervals(events)).collect())
}

/// Active minutes count each minute once, whether the user, an agent, or both worked in it.
/// `agent_only` is agent time while the user showed no activity.
/// @returns `(active_min, agent_only_min, span_end_ms)`
pub fn attribute(events: &[&WorkEvent]) -> (i64, i64, i64) {
    let user = user_intervals(events);
    let agent = agent_intervals(events);
    let all = union(user.iter().chain(agent.iter()).copied().collect());
    let agent_only = minus(&agent, &user);
    let end = all.last().map(|i| i.1).unwrap_or(0);
    (round_min(total_ms(&all)), round_min(total_ms(&agent_only)), end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_counts_once() {
        let u = union(vec![(0, 10), (5, 20), (30, 40)]);
        assert_eq!(u, vec![(0, 20), (30, 40)]);
        assert_eq!(minus(&u, &[(8, 12), (35, 50)]), vec![(0, 8), (12, 20), (30, 35)]);
    }

    #[test]
    fn rounds_to_five() {
        assert_eq!(round_min(60_000), 5);
        assert_eq!(round_min(13 * 60_000), 15);
        assert_eq!(round_min(0), 0);
    }
}
