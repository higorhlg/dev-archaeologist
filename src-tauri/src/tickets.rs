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
    /// Where the strongest match came from: `branch`, `commit`, `agent`, `link`, `prompt`,
    /// `meeting`, `you`, `tracker` (a "Find tickets" suggestion applied without a click), or
    /// `pr` (the Jira ticket the session's pull request names).
    pub source: String,
}

/// `insight-ui#128` names a GitHub issue or pull request; `SHOP-1261` does not.
/// "Find tickets" offers a Jira key next to a GitHub ticket, because time sheets file work under Jira.
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
/// An agent changed the Jira ticket: moved it, edited it, commented or logged work on it.
const AGENT_JIRA_WRITE: u32 = 4;
/// An agent opened the Jira ticket to read it.
const AGENT_JIRA_READ: u32 = 2;
/// The agent session opened or pushed to this pull request.
const AGENT_PR_OPEN: u32 = 4;
/// The agent session edited, commented on, reviewed, merged or checked out this pull request.
const AGENT_PR_WRITE: u32 = 3;
/// The agent session only looked at this pull request (`gh pr view`, `diff`, `checks`).
const AGENT_PR_READ: u32 = 2;
/// The agent session only looked at a pull request in a repo other than the session's project.
const AGENT_PR_READ_ELSEWHERE: u32 = 1;

fn jira_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b([A-Z][A-Z0-9]{1,9})-(\d{1,6})\b").unwrap())
}

/// A Jira key in any case, such as `shop-1068` in `feat/shop-1068-pricing`.
fn any_case_key_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\b([a-z][a-z0-9]{1,9})-(\d{1,6})\b").unwrap())
}

/// The Jira keys in a branch name, uppercased. An uppercase key always counts; a lowercase one
/// such as `shop-1068` counts when its prefix is in `prefixes`, so `fix-2` names no ticket.
fn branch_keys(branch: &str, prefixes: &BTreeSet<String>) -> Vec<String> {
    any_case_key_re()
        .captures_iter(branch)
        .filter(|c| jira_re().is_match(&c[0]) || prefixes.contains(&c[1].to_uppercase()))
        .map(|c| c[0].to_uppercase())
        .collect()
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

/// True when `key` is shaped like a Jira key, such as `SHOP-1261`.
pub fn is_jira_key(key: &str) -> bool {
    jira_re().find(key).is_some_and(|m| m.start() == 0 && m.end() == key.len())
}

/// The Jira key an agent tool call acted on, with `write` or `read`.
fn agent_jira_target(e: &WorkEvent) -> Option<(&str, &str)> {
    if e.kind != kind::TOOL_USE {
        return None;
    }
    let key = e.meta.get("ticket")?.as_str()?;
    Some((key, e.meta.get("ticket_action").and_then(|a| a.as_str()).unwrap_or("read")))
}

/// Jira prefixes seen where only real tickets appear: branch names, commit scopes, and tickets
/// an agent opened in Jira. A key such as `SKU-20052` in free text is ignored unless `SKU` is in
/// this set.
pub fn known_prefixes(events: &[WorkEvent]) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for e in events {
        if let Some((key, _)) = agent_jira_target(e) {
            set.extend(key.split('-').next().map(String::from));
        }
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

    /// The key with the most weight among those `keep` accepts; ties go to the smaller key.
    fn winner(&self, keep: impl Fn(&str) -> bool) -> Option<Ticket> {
        let (key, _) =
            self.score.iter().filter(|(k, _)| keep(k)).max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))?;
        Some(Ticket { source: self.best_source[key].1.to_string(), key: key.clone() })
    }
}

/// The Jira ticket and the GitHub issue or pull request a session's evidence points to.
#[derive(Debug, Default)]
pub struct Detected {
    pub jira: Option<Ticket>,
    pub github: Option<Ticket>,
}

/// Every Jira-shaped key in `text`, once each, in order.
pub fn jira_keys_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in jira_re().captures_iter(text) {
        if !out.contains(&c[0].to_string()) {
            out.push(c[0].to_string());
        }
    }
    out
}

/// The Jira ticket a pull request is for: the one key its title names, else the one key its
/// body names. Keys outside `prefixes` do not count; a title or body naming several keys, as a
/// release PR does, names none.
pub fn pr_jira_key(title: &str, body_keys: &[String], prefixes: &BTreeSet<String>) -> Option<String> {
    let known = |keys: Vec<String>| keys.into_iter().filter(|k| k.split('-').next().is_some_and(|p| prefixes.contains(p))).collect::<Vec<_>>();
    let only = |keys: Vec<String>| (keys.len() == 1).then(|| keys[0].clone());
    let in_title = known(jira_keys_in(title));
    if !in_title.is_empty() {
        return only(in_title);
    }
    let mut in_body = known(body_keys.to_vec());
    in_body.dedup();
    only(in_body)
}

/// The first Jira key in `text` whose prefix is in `prefixes`.
pub fn known_jira_key(text: &str, prefixes: &BTreeSet<String>) -> Option<String> {
    jira_re().captures_iter(text).map(|c| c[0].to_string()).find(|k| k.split('-').next().is_some_and(|p| prefixes.contains(p)))
}

/// Picks the Jira ticket and the GitHub ref with the most weighted evidence in a session.
/// @param project the session's project, used to name GitHub issues (`project#128`).
/// @param prefixes from `known_prefixes`, over a wide window of history.
pub fn detect(events: &[WorkEvent], branch: Option<&str>, project: Option<&str>, prefixes: &BTreeSet<String>) -> Detected {
    let mut t = Tally::default();
    let known = |key: &str| key.split('-').next().is_some_and(|p| prefixes.contains(p));
    let gh = |n: &str| format!("{}#{n}", project.unwrap_or("gh"));

    let branches: BTreeSet<&str> = events.iter().filter_map(|e| e.branch.as_deref()).chain(branch).collect();
    for b in branches {
        for key in branch_keys(b, prefixes) {
            t.add(key, BRANCH, "branch");
        }
        for c in branch_issue_re().captures_iter(b) {
            t.add(gh(&c[1]), BRANCH, "branch");
        }
    }
    let mut agent_prs: HashMap<&str, u32> = HashMap::new();
    for e in events {
        if let Some((key, action)) = agent_jira_target(e) {
            t.add(key.to_string(), if action == "write" { AGENT_JIRA_WRITE } else { AGENT_JIRA_READ }, "agent");
            continue;
        }
        let Some(text) = e.detail.as_deref() else { continue };
        match e.kind.as_str() {
            kind::PR_LINK => {
                let weight = agent_pr_weight(e, text, project);
                let best = agent_prs.entry(text).or_default();
                *best = (*best).max(weight);
            }
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
    for (key, weight) in agent_prs {
        t.add(key.to_string(), weight, "agent");
    }
    Detected { jira: t.winner(|k| !is_github_key(k)), github: t.winner(is_github_key) }
}

/// How much one agent sighting of the pull request `key` (`repo#N`) counts, by the event's
/// `action`: `open` (or none, as in Claude Code's `pr-link`), `write` or `read`. Each pull request
/// counts once per session at its strongest sighting, so checking a PR's status again and again
/// does not outweigh the PR the session opened.
fn agent_pr_weight(e: &WorkEvent, key: &str, project: Option<&str>) -> u32 {
    match e.meta.get("action").and_then(|a| a.as_str()) {
        Some("write") => AGENT_PR_WRITE,
        Some("read") => {
            let repo = key.split('#').next().unwrap_or_default();
            if project.is_some_and(|p| p.eq_ignore_ascii_case(repo)) {
                AGENT_PR_READ
            } else {
                AGENT_PR_READ_ELSEWHERE
            }
        }
        _ => AGENT_PR_OPEN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pull_request_names_one_jira_ticket_in_its_title_else_its_body() {
        let prefixes: BTreeSet<String> = ["SHOP".to_string(), "PLAT".to_string()].into();
        let body = |k: &[&str]| k.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pr_jira_key("fix(SHOP-1331): re-size savings", &body(&["SHOP-1262"]), &prefixes).as_deref(), Some("SHOP-1331"));
        assert_eq!(pr_jira_key("fix(portal): price lines", &body(&["SHOP-1293"]), &prefixes).as_deref(), Some("SHOP-1293"));
        assert_eq!(pr_jira_key("1.10.1", &body(&["PLAT-4603", "SHOP-1329"]), &prefixes), None, "a release PR names several");
        assert_eq!(pr_jira_key("UTF-8 names", &[], &prefixes), None, "unknown prefixes do not count");
    }
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
        let t = detect(&events, None, Some("orders-api"), &prefixes).jira.unwrap();
        assert_eq!(t.key, "SHOP-1261");
        assert_eq!(t.source, "commit");
    }

    #[test]
    fn github_issue_from_branch_and_squash_ref() {
        let events = vec![ev(kind::COMMIT, "feat: cache price history in Redis (#341)", Some("issue-128"))];
        let found = detect(&events, Some("issue-128"), Some("insight-ui"), &BTreeSet::new());
        let t = found.github.unwrap();
        assert_eq!(t.key, "insight-ui#128");
        assert_eq!(t.source, "branch");
        assert!(found.jira.is_none());
    }

    #[test]
    fn jira_and_github_are_found_side_by_side() {
        let events = vec![ev(kind::COMMIT, "fix(SHOP-1277): enable EU markets (#3117)", Some("shop-1277-eu"))];
        let prefixes = known_prefixes(&events);
        let found = detect(&events, None, Some("web-frontend"), &prefixes);
        assert_eq!(found.jira.unwrap().key, "SHOP-1277");
        assert_eq!(found.github.unwrap().key, "web-frontend#3117");
    }

    #[test]
    fn agent_jira_actions_and_pr_links_count() {
        let mut moved = ev(kind::TOOL_USE, "mcp__claude_ai_Atlassian__transitionJiraIssue", None);
        moved.meta = serde_json::json!({ "ticket": "SHOP-1300", "ticket_action": "write" });
        let mut read = ev(kind::TOOL_USE, "mcp__claude_ai_Atlassian__getJiraIssue", None);
        read.meta = serde_json::json!({ "ticket": "SHOP-1299", "ticket_action": "read" });
        let pr = ev(kind::PR_LINK, "subscription-api#330", None);
        let events = vec![read, moved, pr, ev(kind::PROMPT, "look at SHOP-1299 first", None)];
        let prefixes = known_prefixes(&events);
        assert!(prefixes.contains("SHOP"));
        let found = detect(&events, Some("main"), Some("subscription-api"), &prefixes);
        let jira = found.jira.unwrap();
        assert_eq!((jira.key.as_str(), jira.source.as_str()), ("SHOP-1300", "agent"));
        assert_eq!(found.github.unwrap().key, "subscription-api#330");
    }

    #[test]
    fn opened_pull_request_beats_ones_only_viewed() {
        let viewed = |key: &str| {
            let mut e = ev(kind::PR_LINK, key, None);
            e.meta = serde_json::json!({ "action": "read" });
            e
        };
        let opened = ev(kind::PR_LINK, "web-frontend#2400", None);
        let mut reviewed = ev(kind::PR_LINK, "web-frontend#2100", None);
        reviewed.meta = serde_json::json!({ "action": "write" });
        let events = [viewed("web-frontend#2322"), viewed("web-frontend#2322"), viewed("web-frontend#2322"), reviewed, opened];
        let found = detect(&events, None, Some("web-frontend"), &BTreeSet::new());
        assert_eq!(found.github.unwrap().key, "web-frontend#2400");

        let events = [viewed("admin-app#191"), viewed("web-frontend#2322")];
        let found = detect(&events, None, Some("web-frontend"), &BTreeSet::new());
        assert_eq!(found.github.unwrap().key, "web-frontend#2322", "a PR in the session's own repo wins");
    }

    #[test]
    fn reads_lowercase_branch_keys_with_known_prefixes() {
        let prefixes: BTreeSet<String> = ["SHOP".to_string()].into();
        assert_eq!(branch_keys("feat/shop-1068-exact-pricing", &prefixes), vec!["SHOP-1068"]);
        assert_eq!(branch_keys("shop-910/warehouse", &prefixes), vec!["SHOP-910"]);
        assert_eq!(branch_keys("SKU-12-x", &prefixes), vec!["SKU-12"]);
        assert!(branch_keys("fix-2-typo", &prefixes).is_empty());
        let found = detect(&[], Some("feat/shop-1068-exact-pricing"), None, &prefixes);
        assert_eq!(found.jira.unwrap().key, "SHOP-1068");
    }

    #[test]
    fn jira_key_shape() {
        assert!(is_jira_key("SHOP-1261"));
        assert!(!is_jira_key("10042"));
        assert!(!is_jira_key("SHOP-1261 extra"));
    }

    #[test]
    fn github_keys_have_a_hash() {
        assert!(is_github_key("orders-api#310"));
        assert!(!is_github_key("SHOP-1261"));
    }

    #[test]
    fn no_evidence_no_ticket() {
        let events = vec![ev(kind::PROMPT, "make the tests pass", None)];
        let found = detect(&events, Some("main"), Some("x"), &BTreeSet::new());
        assert!(found.jira.is_none() && found.github.is_none());
    }
}
