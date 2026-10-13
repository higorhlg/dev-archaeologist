//! Finds the Jira key or GitHub issue a session worked on, from local evidence only.

use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

use crate::event::{kind, WorkEvent};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Ticket {
    /// `SHOP-1261` or `insight-ui#128`.
    pub key: String,
    /// Where the strongest match came from: `branch`, `commit`, `link`, `prompt`, or `you`.
    pub source: String,
}

impl Ticket {
    /// True for a GitHub issue or pull request the app guessed from evidence. A Jira key that
    /// "Find tickets" finds for the same work replaces it, because time sheets file work under Jira.
    pub fn yields_to_jira(&self) -> bool {
        self.source != "you" && is_github_key(&self.key)
    }
}

/// `insight-ui#128` names a GitHub issue or pull request; `SHOP-1261` does not.
pub fn is_github_key(key: &str) -> bool {
    key.contains('#')
}

/// How much one sighting counts, by where it was seen.
const BRANCH: u32 = 5;
const COMMIT_SCOPE: u32 = 4;
const ISSUE_LINK: u32 = 3;
const PR_LINK: u32 = 2;
const COMMIT_REF: u32 = 3;
const PROMPT: u32 = 1;
const MEETING_TITLE: u32 = 2;

fn jira_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b([A-Z][A-Z0-9]{1,9})-(\d{1,6})\b").unwrap())
}

/// `fix(SHOP-1261): …`, `SHOP-1261: …`, `[SHOP-1261] …` — a key in the scope position of a commit.
fn scope_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(?:\w+\(([A-Z][A-Z0-9]{1,9}-\d+)\)|\[?([A-Z][A-Z0-9]{1,9}-\d+)\]?[:\s])").unwrap())
}

fn gh_link_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"github\.com/[\w.-]+/([\w.-]+)/(issues|pull)/(\d+)").unwrap())
}

/// `(#341)` at the end of a squash-merge subject, or `closes #12`.
fn gh_ref_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)(?:\(#(\d+)\)\s*$|\b(?:closes|fixes|resolves|refs?)\s+#(\d+))").unwrap())
}

/// `issue-128`, `issues/128`, `gh-128` in a branch name.
fn branch_issue_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\b(?:issue|issues|gh)[-/_](\d+)\b").unwrap())
}

/// Jira prefixes seen where only real tickets appear: branch names and commit scopes.
/// A key such as `SKU-20052` in free text is ignored unless `SKU` is in this set.
pub fn known_prefixes(events: &[WorkEvent]) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for e in events {
        if let Some(b) = &e.branch {
            set.extend(jira_re().captures_iter(b).map(|c| c[1].to_string()));
        }
        if e.kind == kind::COMMIT {
            if let Some(c) = e.detail.as_deref().and_then(|d| scope_re().captures(d)) {
                if let Some(m) = c.get(1).or_else(|| c.get(2)) {
                    set.extend(jira_re().captures_iter(m.as_str()).map(|c| c[1].to_string()));
                }
            }
        }
    }
    set
}

#[derive(Default)]
struct Tally {
    score: HashMap<String, u32>,
    best_source: HashMap<String, (u32, &'static str)>,
}

impl Tally {
    fn add(&mut self, key: String, weight: u32, source: &'static str) {
        *self.score.entry(key.clone()).or_default() += weight;
        let best = self.best_source.entry(key).or_insert((0, source));
        if weight > best.0 {
            *best = (weight, source);
        }
    }

    fn winner(self) -> Option<Ticket> {
        let (key, _) = self.score.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))?;
        Some(Ticket { source: self.best_source[key].1.to_string(), key: key.clone() })
    }
}

/// The first Jira key in `text` whose prefix is in `prefixes`.
pub fn known_jira_key(text: &str, prefixes: &BTreeSet<String>) -> Option<String> {
    jira_re().captures_iter(text).map(|c| c[0].to_string()).find(|k| k.split('-').next().is_some_and(|p| prefixes.contains(p)))
}

/// Picks the ticket with the most weighted evidence in a session.
/// @param project the session's project, used to name GitHub issues (`project#128`).
/// @param prefixes from `known_prefixes`, over a wide window of history.
pub fn detect(events: &[WorkEvent], branch: Option<&str>, project: Option<&str>, prefixes: &BTreeSet<String>) -> Option<Ticket> {
    let mut t = Tally::default();
    let known = |key: &str| key.split('-').next().is_some_and(|p| prefixes.contains(p));
    let gh = |n: &str| format!("{}#{n}", project.unwrap_or("gh"));

    let branches: BTreeSet<&str> = events.iter().filter_map(|e| e.branch.as_deref()).chain(branch).collect();
    for b in branches {
        for c in jira_re().captures_iter(b) {
            t.add(c[0].to_string(), BRANCH, "branch");
        }
        for c in branch_issue_re().captures_iter(b) {
            t.add(gh(&c[1]), BRANCH, "branch");
        }
    }
    for e in events {
        let Some(text) = e.detail.as_deref() else { continue };
        match e.kind.as_str() {
            kind::COMMIT => {
                if let Some(c) = scope_re().captures(text) {
                    if let Some(m) = c.get(1).or_else(|| c.get(2)) {
                        t.add(m.as_str().to_string(), COMMIT_SCOPE, "commit");
                    }
                }
                for c in jira_re().captures_iter(text).filter(|c| known(&c[0])) {
                    t.add(c[0].to_string(), PROMPT, "commit");
                }
                for c in gh_ref_re().captures_iter(text) {
                    if let Some(n) = c.get(1).or_else(|| c.get(2)) {
                        t.add(gh(n.as_str()), COMMIT_REF, "commit");
                    }
                }
            }
            kind::MEETING => {
                for c in jira_re().captures_iter(text).filter(|c| known(&c[0])) {
                    t.add(c[0].to_string(), MEETING_TITLE, "meeting");
                }
            }
            kind::PROMPT => {
                for c in gh_link_re().captures_iter(text) {
                    let weight = if &c[2] == "issues" { ISSUE_LINK } else { PR_LINK };
                    t.add(format!("{}#{}", &c[1], &c[3]), weight, "link");
                }
                for c in jira_re().captures_iter(text).filter(|c| known(&c[0])) {
                    t.add(c[0].to_string(), PROMPT, "prompt");
                }
            }
            _ => {}
        }
    }
    t.winner()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn ev(k: &str, detail: &str, branch: Option<&str>) -> WorkEvent {
        let mut e = WorkEvent::new("t", detail, Utc::now(), k);
        e.detail = Some(detail.into());
        e.branch = branch.map(String::from);
        e
    }

    #[test]
    fn branch_and_scope_beat_skus() {
        let events = vec![
            ev(kind::COMMIT, "fix(SHOP-1261): leave the Invoice header off EU/UK orders", None),
            ev(kind::COMMIT, "fix: expand kit KO-1AAA-2BBB-XYZ to SKU-20052 + BOX-20030", None),
        ];
        let prefixes = known_prefixes(&events);
        assert_eq!(prefixes.iter().collect::<Vec<_>>(), vec!["SHOP"]);
        let t = detect(&events, None, Some("orders-api"), &prefixes).unwrap();
        assert_eq!(t.key, "SHOP-1261");
        assert_eq!(t.source, "commit");
    }

    #[test]
    fn github_issue_from_branch_and_squash_ref() {
        let events = vec![ev(kind::COMMIT, "feat: cache price history in Redis (#341)", Some("issue-128"))];
        let t = detect(&events, Some("issue-128"), Some("insight-ui"), &BTreeSet::new()).unwrap();
        assert_eq!(t.key, "insight-ui#128");
        assert_eq!(t.source, "branch");
    }

    #[test]
    fn only_guessed_github_tickets_yield_to_jira() {
        let t = |key: &str, source: &str| Ticket { key: key.into(), source: source.into() };
        assert!(t("orders-api#310", "commit").yields_to_jira());
        assert!(!t("orders-api#310", "you").yields_to_jira());
        assert!(!t("SHOP-1261", "branch").yields_to_jira());
    }

    #[test]
    fn no_evidence_no_ticket() {
        let events = vec![ev(kind::PROMPT, "make the tests pass", None)];
        assert!(detect(&events, Some("main"), Some("x"), &BTreeSet::new()).is_none());
    }
}
