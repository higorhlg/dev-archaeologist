use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Result};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::json;

use crate::event::{clip, kind, WorkEvent};

pub const SOURCE: &str = "git";

/// Finds Git repositories (folders or worktrees) under each root, at most `depth` levels down.
/// Skips dependency folders so a scan of a large tree stays fast.
pub fn discover_repos(roots: &[PathBuf], depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in roots {
        let walker = walkdir::WalkDir::new(root).max_depth(depth).into_iter().filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            e.file_type().is_dir() && !matches!(n.as_ref(), "node_modules" | "target" | "dist" | ".venv" | "vendor")
        });
        for entry in walker.flatten() {
            if entry.path().join(".git").exists() {
                found.push(entry.into_path());
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").arg("-C").arg(repo).args(args).output()?;
    if !out.status.success() {
        bail!("git {:?} failed in {}", args.first(), repo.display());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The emails that count as "the user" in this repo: the repo identity and the global one.
fn user_emails(repo: &Path) -> HashSet<String> {
    let mut set = HashSet::new();
    for args in [&["config", "user.email"][..], &["config", "--global", "user.email"][..]] {
        if let Ok(e) = git(repo, args) {
            if !e.trim().is_empty() {
                set.insert(e.trim().to_lowercase());
            }
        }
    }
    set
}

/// Commits a scan read, split into the user's own work and commits it must not count.
pub struct Scan {
    pub events: Vec<WorkEvent>,
    /// Shas of the user's commits that are not local work: stash entries, and commits GitHub
    /// made when it merged a pull request. Events stored for them earlier must be removed.
    pub not_work: Vec<String>,
}

/// GitHub is the committer of squash and rebase merges done on the website.
const GITHUB_COMMITTER: &str = "noreply@github.com";

/// Subjects git gives the commits of a stash entry.
pub const STASH_SUBJECT_PREFIXES: [&str; 3] = ["WIP on ", "index on ", "untracked files on "];

/// True for the subject of a commit `git stash` made. Dropped stashes leave the ref, so the
/// subject is the one sign left.
fn is_stash_subject(subject: &str) -> bool {
    STASH_SUBJECT_PREFIXES.iter().any(|p| subject.starts_with(p))
}

struct Commit {
    sha: String,
    ts: DateTime<Utc>,
    branch: Option<String>,
    subject: String,
    files: i64,
    plus: i64,
    minus: i64,
}

/// Reads the user's commits since `since` on every branch, with line counts.
/// Commits by other authors are left out. Stash entries and commits GitHub made on a merge
/// go to `not_work`, because the work in them happened earlier or is not a commit.
/// The commit subject is kept; diffs are read only to count changed characters on one-line commits.
pub fn scan(repo: &Path, since: DateTime<Utc>) -> Result<Scan> {
    let emails = user_emails(repo);
    let log = git(
        repo,
        &[
            "log", "--all", "--source", "--no-merges", &format!("--since=@{}", since.timestamp()),
            "--format=%x1e%H%x1f%at%x1f%ae%x1f%S%x1f%ce%x1f%s", "--numstat",
        ],
    )?;
    let mut scan = Scan { events: Vec::new(), not_work: Vec::new() };
    for chunk in log.split('\x1e').filter(|c| !c.trim().is_empty()) {
        let Some(parsed) = parse_commit(chunk) else { continue };
        if !emails.is_empty() && !emails.contains(&parsed.author) {
            continue;
        }
        if parsed.is_work() {
            scan.events.push(commit_event(repo, parsed.commit));
        } else {
            scan.not_work.push(parsed.commit.sha);
        }
    }
    Ok(scan)
}

struct ParsedCommit {
    commit: Commit,
    author: String,
    committer: String,
    /// The ref `--source` reached the commit from, such as `refs/heads/main` or `refs/stash`.
    source_ref: String,
}

impl ParsedCommit {
    /// False for stash entries and for commits GitHub made when it merged a pull request.
    fn is_work(&self) -> bool {
        self.source_ref != "refs/stash" && !is_stash_subject(&self.commit.subject) && self.committer != GITHUB_COMMITTER
    }
}

fn parse_commit(chunk: &str) -> Option<ParsedCommit> {
    let mut lines = chunk.lines();
    let head: Vec<&str> = lines.next()?.splitn(6, '\x1f').collect();
    if head.len() < 6 {
        return None;
    }
    let (mut files, mut plus, mut minus) = (0, 0, 0);
    for l in lines.filter(|l| !l.trim().is_empty()) {
        let mut parts = l.split('\t');
        let a = parts.next().and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
        let d = parts.next().and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
        files += 1;
        plus += a;
        minus += d;
    }
    let commit = Commit {
        sha: head[0].to_string(),
        ts: Utc.timestamp_opt(head[1].parse().ok()?, 0).single()?,
        branch: Some(head[3].trim_start_matches("refs/heads/").trim_start_matches("refs/remotes/").to_string())
            .filter(|b| !b.is_empty() && !b.starts_with("refs/")),
        subject: head[5].to_string(),
        files,
        plus,
        minus,
    };
    Some(ParsedCommit {
        commit,
        author: head[2].to_lowercase(),
        committer: head[4].to_lowercase(),
        source_ref: head[3].to_string(),
    })
}

fn commit_event(repo: &Path, c: Commit) -> WorkEvent {
    let changed_chars = if c.files == 1 && c.plus == 1 && c.minus == 1 { one_line_changed_chars(repo, &c.sha) } else { None };
    let mut e = WorkEvent::new(SOURCE, c.sha.clone(), c.ts, kind::COMMIT);
    e.path = Some(repo.to_string_lossy().into_owned());
    e.branch = c.branch;
    e.detail = Some(clip(&c.subject, 200));
    e.meta = json!({
        "sha": &c.sha[..c.sha.len().min(10)], "files": c.files, "plus": c.plus, "minus": c.minus,
        "changed_chars": changed_chars,
    });
    e
}

/// For a commit that swaps one line for another, counts the characters that differ
/// after the common prefix and suffix are removed.
fn one_line_changed_chars(repo: &Path, sha: &str) -> Option<i64> {
    let diff = git(repo, &["show", "-U0", "--format=", sha]).ok()?;
    let removed = diff.lines().find(|l| l.starts_with('-') && !l.starts_with("---"))?[1..].to_string();
    let added = diff.lines().find(|l| l.starts_with('+') && !l.starts_with("+++"))?[1..].to_string();
    Some(char_distance(&removed, &added))
}

pub fn char_distance(a: &str, b: &str) -> i64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(x, y)| x == y).count();
    (a.len() - prefix - suffix).max(b.len() - prefix - suffix) as i64
}

#[cfg(test)]
mod tests {
    fn chunk(source_ref: &str, committer: &str, subject: &str) -> String {
        format!("abc123\x1f1760000000\x1fme@x.com\x1f{source_ref}\x1f{committer}\x1f{subject}\n1\t1\tsrc/a.ts\n")
    }

    #[test]
    fn github_merges_and_stashes_are_not_work() {
        let merged = super::parse_commit(&chunk("refs/remotes/origin/HEAD", "noreply@github.com", "fix: a (#310)")).unwrap();
        assert!(!merged.is_work());
        let stash = super::parse_commit(&chunk("refs/stash", "me@x.com", "index on main: abc fix")).unwrap();
        assert!(!stash.is_work());
        let dropped_stash = super::parse_commit(&chunk("refs/heads/main", "me@x.com", "WIP on main: abc fix")).unwrap();
        assert!(!dropped_stash.is_work());
        let local = super::parse_commit(&chunk("refs/heads/feat/x", "me@x.com", "feat: b")).unwrap();
        assert!(local.is_work());
        assert_eq!(local.commit.branch.as_deref(), Some("feat/x"));
        assert_eq!(local.commit.subject, "feat: b");
    }

    #[test]
    fn char_distance_counts_a_typo() {
        assert_eq!(super::char_distance("const ttl = 6O;", "const ttl = 60;"), 1);
        assert_eq!(super::char_distance("abc", "abc"), 0);
    }
}
