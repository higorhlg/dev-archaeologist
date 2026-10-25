//! Ticket evidence read from the shell commands an agent ran: the pull requests it opened,
//! changed or read with `gh pr`, and the branch it created with git. Only the repo, number
//! and branch name leave these functions; the command text is never stored.

use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::json;

use crate::event::{kind, WorkEvent};

/// A pull request a `gh pr` command acted on.
#[derive(Debug, PartialEq)]
pub struct PrRef {
    /// `owner/repo`, lowercase.
    pub repo: String,
    pub number: u64,
    /// `open` when the agent opened it, `write` when a command changed or checked it out,
    /// `read` when a command only looked.
    pub action: &'static str,
}

const WRITES: &[&str] = &["edit", "merge", "comment", "review", "ready", "close", "reopen", "checkout"];
const READS: &[&str] = &["view", "diff", "checks"];

fn gh_pr_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\bgh\s+pr\s+([a-z]+)\b").unwrap())
}

/// Where one command in a chained or quoted command line ends.
fn command_end_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"&&|\|\||[;|\n]|\\n|\bgh\s+pr\s").unwrap())
}

fn pr_url_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"github\.com/([\w.-]+/[\w.-]+)/pull/(\d+)").unwrap())
}

fn repo_flag_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?:--repo|-R)[=\s]+\\?["']?([\w.-]+/[\w.-]+)"#).unwrap())
}

/// The pull request number as the first argument, such as `gh pr view 2322`.
fn leading_number_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"^\s+\\?["']?#?(\d+)\b"#).unwrap())
}

/// `git switch -c NAME`, `git checkout -b NAME`, `git worktree add -b NAME`, `gh pr create --head NAME`.
fn new_branch_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?:\bgit\s+(?:switch\s+-[cC]|checkout\s+-[bB]|worktree\s+add\s+(?:\S+\s+)*?-[bB])|--head[=\s])\s*\\?["']?([\w][\w./-]*)"#)
            .unwrap()
    })
}

/// The pull requests named by the `gh pr` commands in `command`. A command counts only when it
/// names both the repo (`--repo`, `-R` or a pull request URL) and the number. `gh pr create`
/// names no number; its output does, so callers read that with `pr_urls`.
pub fn pr_commands(command: &str) -> Vec<PrRef> {
    gh_pr_re()
        .captures_iter(command)
        .filter_map(|c| {
            let sub = c.get(1)?.as_str();
            let action = if WRITES.contains(&sub) {
                "write"
            } else if READS.contains(&sub) {
                "read"
            } else {
                return None;
            };
            let rest = &command[c.get(0)?.end()..];
            let rest = &rest[..command_end_re().find(rest).map_or(rest.len(), |m| m.start())];
            if let Some(u) = pr_url_re().captures(rest) {
                return Some(PrRef { repo: u[1].to_lowercase(), number: u[2].parse().ok()?, action });
            }
            let repo = repo_flag_re().captures(rest)?[1].to_lowercase();
            let number = leading_number_re().captures(rest)?[1].parse().ok()?;
            Some(PrRef { repo, number, action })
        })
        .collect()
}

/// True when `command` opens a pull request, whose URL then appears in its output.
pub fn creates_pr(command: &str) -> bool {
    gh_pr_re().captures_iter(command).any(|c| &c[1] == "create")
}

/// The pull requests whose URLs appear in `output`, as written by `gh pr create`.
pub fn pr_urls(output: &str) -> Vec<PrRef> {
    let mut refs: Vec<PrRef> = pr_url_re()
        .captures_iter(output)
        .filter_map(|c| Some(PrRef { repo: c[1].to_lowercase(), number: c[2].parse().ok()?, action: "open" }))
        .collect();
    refs.dedup();
    refs
}

/// The branch `command` creates and switches to, if any.
pub fn created_branch(command: &str) -> Option<String> {
    new_branch_re().captures_iter(command).last().map(|c| c[1].trim_end_matches(['.', '/']).to_string())
}

/// A `PR_LINK` event for `pr`, keyed by the tool call that named it. `detail` is `repo#N`
/// with the repo's name only, the form tracker keys take.
pub fn pr_event(params: PrEvent) -> WorkEvent {
    let PrEvent { source, call_id, pr, ts, path } = params;
    let name = pr.repo.rsplit('/').next().unwrap_or(&pr.repo);
    let mut e = WorkEvent::new(source, format!("{call_id}:pr:{}#{}", pr.repo, pr.number), ts, kind::PR_LINK);
    e.detail = Some(format!("{name}#{}", pr.number));
    e.path = path.clone();
    e.meta = json!({ "repo": pr.repo, "number": pr.number, "action": pr.action });
    e
}

pub struct PrEvent<'a> {
    pub source: &'a str,
    pub call_id: &'a str,
    pub pr: &'a PrRef,
    pub ts: DateTime<Utc>,
    pub path: &'a Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(repo: &str, number: u64, action: &'static str) -> PrRef {
        PrRef { repo: repo.into(), number, action }
    }

    #[test]
    fn reads_pr_commands() {
        assert_eq!(
            pr_commands("gh pr view 2322 --repo acme/web-frontend --json headRefName"),
            vec![pr("acme/web-frontend", 2322, "read")]
        );
        assert_eq!(
            pr_commands(r#"GH_TOKEN=$(gh auth token) gh pr edit 243 --repo acme/Insight-UI --title \"x\""#),
            vec![pr("acme/insight-ui", 243, "write")]
        );
        assert_eq!(
            pr_commands("gh pr review https://github.com/a/b/pull/7 --approve && gh pr view 8 -R a/c"),
            vec![pr("a/b", 7, "write"), pr("a/c", 8, "read")]
        );
    }

    #[test]
    fn skips_pr_commands_without_repo_or_number() {
        assert!(pr_commands("gh pr view 12 --json url").is_empty());
        assert!(pr_commands("gh pr view --repo a/b").is_empty());
        assert!(pr_commands("gh pr list --repo a/b --limit 20").is_empty());
        assert!(pr_commands("gh pr view 12; git push --repo a/b").is_empty());
    }

    #[test]
    fn reads_created_pr_from_output() {
        assert!(creates_pr(r#"{"cmd":"gh pr create --draft --repo a/b --title \"t\""}"#));
        assert!(!creates_pr("gh pr view 1 --repo a/b"));
        assert_eq!(pr_urls("https://github.com/A/B/pull/42\n"), vec![pr("a/b", 42, "open")]);
    }

    #[test]
    fn reads_created_branch() {
        assert_eq!(created_branch("git switch -c feat/SHOP-1029-scroll"), Some("feat/SHOP-1029-scroll".into()));
        assert_eq!(created_branch("cd x && git checkout -b SHOP-1000-landing-hero"), Some("SHOP-1000-landing-hero".into()));
        assert_eq!(created_branch("git worktree add ../wt -b codex/issue-166-titles main"), Some("codex/issue-166-titles".into()));
        assert_eq!(created_branch(r#"gh pr create --base main --head \"insight-bot/issue-229\""#), Some("insight-bot/issue-229".into()));
        assert_eq!(created_branch("git switch main"), None);
    }
}
