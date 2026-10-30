//! GitHub issues and pull requests through the accounts the `gh` CLI is signed in with.
//! Tokens come from `gh auth token` for one call and are never stored. Read only.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};
use chrono::DateTime;
use serde_json::Value;

use super::{Candidate, DayRange};
use crate::http;

const API: &str = "https://api.github.com";
const MAX_PAGES: usize = 3;

/// Logins `gh` is signed in with on github.com. Empty when `gh` is missing or signed out.
pub fn accounts() -> Vec<String> {
    let out = Command::new("gh")
        .args(["auth", "status", "--hostname", "github.com", "--json", "hosts", "--jq", r#".hosts["github.com"][] | select(.state == "success") | .login"#])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect(),
        _ => vec![],
    }
}

fn token_for(login: &str) -> Result<String> {
    let out = Command::new("gh").args(["auth", "token", "--hostname", "github.com", "--user", login]).output()?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || token.is_empty() {
        bail!("gh has no token for {login}");
    }
    Ok(token)
}

/// `owner/name` of a repo's `origin` remote on github.com, from SSH or HTTPS URLs.
pub fn repo_slug(repo_root: &Path) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(repo_root).args(["remote", "get-url", "origin"]).output().ok()?;
    parse_remote(String::from_utf8_lossy(&out.stdout).trim())
}

fn parse_remote(url: &str) -> Option<String> {
    let rest = url.split_once("github.com").map(|(_, r)| r)?;
    let slug = rest.trim_start_matches([':', '/']).trim_end_matches('/').trim_end_matches(".git");
    let mut parts = slug.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !name.is_empty()).then(|| format!("{owner}/{name}").to_lowercase())
}

fn parse_time(v: &Value) -> Option<i64> {
    DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|t| t.timestamp_millis())
}

/// The key matches what the local ticket finder writes: `repo-name#123`.
fn to_candidate(item: &Value) -> Option<Candidate> {
    let slug = item["repository_url"].as_str()?.strip_prefix("https://api.github.com/repos/")?.to_lowercase();
    let name = slug.rsplit('/').next()?.to_string();
    let number = item["number"].as_i64()?;
    let created = parse_time(&item["created_at"]).unwrap_or(0);
    let last = [&item["updated_at"], &item["closed_at"]].iter().filter_map(|v| parse_time(v)).max().unwrap_or(created);
    Some(Candidate {
        key: format!("{name}#{number}"),
        title: item["title"].as_str().unwrap_or_default().to_string(),
        url: item["html_url"].as_str().unwrap_or_default().to_string(),
        tracker: "github".into(),
        repo: Some(slug),
        created_ms: created,
        last_ms: last,
        epic: None,
        body_keys: Some(crate::tickets::jira_keys_in(item["body"].as_str().unwrap_or_default())),
        mine: None,
    })
}

fn get_json(token: &str, url: &str) -> Result<Value> {
    let resp = http::agent(30)
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "dev-archaeologist")
        .call()?;
    http::read_json(resp, "GitHub")
}

/// One issue or pull request by `owner/name` and number, through the first `gh` account that can read it.
pub fn ticket(slug: &str, number: &str) -> Result<Candidate> {
    let url = format!("{API}/repos/{slug}/issues/{number}");
    let mut last_err = anyhow::anyhow!("gh is not signed in");
    for login in accounts() {
        match token_for(&login).and_then(|t| get_json(&t, &url)) {
            Ok(item) => return to_candidate(&item).ok_or_else(|| anyhow::anyhow!("GitHub returned no issue for {slug}#{number}")),
            Err(err) => last_err = err,
        }
    }
    Err(last_err)
}

fn search_page(token: &str, query: &str, page: usize) -> Result<Value> {
    let resp = http::agent(30)
        .get(format!("{API}/search/issues"))
        .query("q", query)
        .query("per_page", "100")
        .query("page", page.to_string())
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "dev-archaeologist")
        .call()?;
    http::read_json(resp, "GitHub")
}

/// Issues and pull requests that involve `login` and changed since the range started,
/// kept only when they live in one of `repos` (`owner/name`, lowercase).
pub fn search(login: &str, range: &DayRange, repos: &[String]) -> Result<Vec<Candidate>> {
    let token = token_for(login)?;
    let query = format!("involves:{login} updated:>={}", range.from.format("%Y-%m-%d"));
    let mut out = Vec::new();
    for page in 1..=MAX_PAGES {
        let v = search_page(&token, &query, page)?;
        let items = v["items"].as_array().cloned().unwrap_or_default();
        out.extend(items.iter().filter_map(to_candidate).filter(|c| c.repo.as_ref().is_some_and(|r| repos.contains(r))));
        if items.len() < 100 {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn remotes_become_slugs() {
        assert_eq!(parse_remote("git@github.com:Acme/Insight-UI.git").as_deref(), Some("acme/insight-ui"));
        assert_eq!(parse_remote("https://github.com/acme/api").as_deref(), Some("acme/api"));
        assert_eq!(parse_remote("ssh://git@github.com/acme/api.git").as_deref(), Some("acme/api"));
        assert_eq!(parse_remote("git@gitlab.com:acme/api.git"), None);
    }

    #[test]
    fn search_item_becomes_a_candidate() {
        let item = json!({
            "number": 128, "title": "Basket total off by one", "html_url": "https://github.com/acme/insight-ui/issues/128",
            "repository_url": "https://api.github.com/repos/acme/insight-ui",
            "created_at": "2026-10-01T09:00:00Z", "updated_at": "2026-10-03T09:00:00Z", "closed_at": "2026-10-06T09:00:00Z"
        });
        let c = to_candidate(&item).unwrap();
        assert_eq!(c.key, "insight-ui#128");
        assert_eq!(c.repo.as_deref(), Some("acme/insight-ui"));
        assert!(c.last_ms > c.created_ms);
    }
}
