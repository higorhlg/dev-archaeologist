//! Jira Cloud REST API v3 with an API token: the account check, a search for the user's own
//! tickets in a date range, and a search for tickets that link a GitHub pull request. Read only.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use chrono::DateTime;
use serde::Serialize;
use serde_json::{json, Value};

use super::{Candidate, DayRange, Epic};
use crate::http::{self, KeychainItem};

const TOKEN: KeychainItem = KeychainItem("jira-api-token");
/// Where Atlassian users create API tokens. "Sign in" opens this page in the browser.
pub const TOKEN_PAGE: &str = "https://id.atlassian.com/manage-profile/security/api-tokens";
const MAX_PAGES: usize = 3;
/// Issue fields every search asks for. `parent` carries the epic; `assignee` and `reporter` say
/// whether the ticket is the user's.
const FIELDS: [&str; 6] = ["summary", "created", "updated", "parent", "assignee", "reporter"];
const PAGE_SIZE: usize = 100;
/// GitHub tickets looked up per run; each one is a Jira search.
const MAX_LINK_LOOKUPS: usize = 25;

pub fn has_token() -> bool {
    TOKEN.get().is_some()
}

#[derive(Debug, Clone, Serialize)]
pub struct JiraAccount {
    pub display_name: String,
    pub email: String,
}

/// Accepts `acme`, `acme.atlassian.net` or a full URL and returns the bare host.
pub fn normalize_site(site: &str) -> String {
    let host = site.trim().trim_start_matches("https://").trim_start_matches("http://");
    let host = host.split('/').next().unwrap_or("").to_lowercase();
    if host.is_empty() || host.contains('.') { host } else { format!("{host}.atlassian.net") }
}

fn get(site: &str, email: &str, token: &str, path: &str) -> Result<Value> {
    let resp = http::agent(30)
        .get(format!("https://{site}{path}"))
        .header("Authorization", http::basic_auth(email, token))
        .header("Accept", "application/json")
        .call()?;
    http::read_json(resp, "Jira")
}

fn post(site: &str, email: &str, token: &str, path: &str, body: &Value) -> Result<Value> {
    let resp = http::agent(30)
        .post(format!("https://{site}{path}"))
        .header("Authorization", http::basic_auth(email, token))
        .header("Accept", "application/json")
        .send_json(body)?;
    http::read_json(resp, "Jira")
}

/// Checks the token against the site, then saves it to the Keychain.
pub fn connect(site: &str, email: &str, token: &str) -> Result<JiraAccount> {
    let me = get(site, email.trim(), token.trim(), "/rest/api/3/myself")?;
    TOKEN.set(token)?;
    Ok(JiraAccount {
        display_name: me["displayName"].as_str().unwrap_or_default().to_string(),
        email: me["emailAddress"].as_str().unwrap_or(email).to_string(),
    })
}

/// Tickets assigned to the user at any time in the range, and tickets the user reported
/// or logged work on that changed in the range.
fn jql(range: &DayRange) -> String {
    let (from, to) = (range.from.format("%Y-%m-%d"), range.to.succ_opt().unwrap_or(range.to).format("%Y-%m-%d"));
    format!(
        "assignee WAS currentUser() DURING (\"{from}\", \"{to}\") \
         OR ((reporter = currentUser() OR worklogAuthor = currentUser()) AND updated >= \"{from}\") \
         ORDER BY updated DESC"
    )
}

fn parse_time(v: &Value) -> Option<i64> {
    DateTime::parse_from_str(v.as_str()?, "%Y-%m-%dT%H:%M:%S%.f%z").ok().map(|t| t.timestamp_millis())
}

/// The Jira account id of the user the token belongs to.
fn my_account_id(site: &str, email: &str, token: &str) -> Result<String> {
    let me = get(site, email, token, "/rest/api/3/myself")?;
    me["accountId"].as_str().map(String::from).ok_or_else(|| anyhow!("Jira did not say who the token belongs to"))
}

/// @param me the user's Jira account id, from `my_account_id`.
fn to_candidate(site: &str, issue: &Value, range_end_ms: i64, me: &str) -> Option<Candidate> {
    let key = issue["key"].as_str()?.to_string();
    let f = &issue["fields"];
    let is_me = |person: &Value| person["accountId"].as_str() == Some(me);
    Some(Candidate {
        url: format!("https://{site}/browse/{key}"),
        title: f["summary"].as_str().unwrap_or_default().to_string(),
        tracker: "jira".into(),
        repo: None,
        created_ms: parse_time(&f["created"]).unwrap_or(0),
        last_ms: parse_time(&f["updated"]).unwrap_or(range_end_ms).max(range_end_ms),
        epic: epic_of(&f["parent"]),
        body_keys: None,
        mine: Some(is_me(&f["assignee"]) || is_me(&f["reporter"])),
        key,
    })
}

/// The parent of an issue when it is an epic (hierarchy level 1). A sub-task's parent is a
/// story, not an epic, so it gives none.
fn epic_of(parent: &Value) -> Option<Epic> {
    let kind = &parent["fields"]["issuetype"];
    let is_epic = kind["hierarchyLevel"].as_i64() == Some(1) || kind["name"].as_str() == Some("Epic");
    if !is_epic {
        return None;
    }
    Some(Epic { key: parent["key"].as_str()?.to_string(), title: parent["fields"]["summary"].as_str().unwrap_or_default().to_string() })
}

/// Title, link, epic and whether it is the user's, of each Jira key, in batches of `PAGE_SIZE`.
/// Keys Jira does not know are left out.
pub fn tickets_by_key(site: &str, email: &str, keys: &[String], range: &DayRange) -> Result<Vec<Candidate>> {
    let token = TOKEN.get().ok_or_else(|| anyhow!("Connect Jira in Sources first"))?;
    let me = my_account_id(site, email, &token)?;
    let mut out = Vec::new();
    for batch in keys.chunks(PAGE_SIZE) {
        let jql = format!("key in ({})", batch.join(", "));
        let body = json!({"jql": jql, "fields": FIELDS, "maxResults": PAGE_SIZE});
        let page = post(site, email, &token, "/rest/api/3/search/jql", &body)?;
        let issues = page["issues"].as_array().cloned().unwrap_or_default();
        out.extend(issues.iter().filter_map(|i| to_candidate(site, i, range.end_ms(), &me)));
    }
    Ok(out)
}

/// The user's tickets for the range, newest first, up to three pages.
pub fn search(site: &str, email: &str, range: &DayRange) -> Result<Vec<Candidate>> {
    let token = TOKEN.get().ok_or_else(|| anyhow!("Connect Jira in Sources first"))?;
    let me = my_account_id(site, email, &token)?;
    let mut out = Vec::new();
    let mut next: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut body = json!({"jql": jql(range), "fields": FIELDS, "maxResults": PAGE_SIZE});
        if let Some(t) = &next {
            body["nextPageToken"] = json!(t);
        }
        let page = post(site, email, &token, "/rest/api/3/search/jql", &body)?;
        let issues = page["issues"].as_array().cloned().unwrap_or_default();
        out.extend(issues.iter().filter_map(|i| to_candidate(site, i, range.end_ms(), &me)));
        next = page["nextPageToken"].as_str().map(String::from);
        if next.is_none() || page["isLast"].as_bool() == Some(true) {
            break;
        }
    }
    Ok(out)
}

/// JQL for tickets whose text (summary, description or comments) holds the URL of a GitHub
/// pull request or issue. `key` is `repo#N`.
fn link_jql(key: &str) -> Option<String> {
    let (repo, n) = key.split_once('#')?;
    if repo.is_empty() || n.is_empty() || !n.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("text ~ \"\\\"{repo}/pull/{n}\\\"\" OR text ~ \"\\\"{repo}/issues/{n}\\\"\" ORDER BY updated DESC"))
}

/// For each GitHub ticket (`repo#N`), the most recently changed Jira ticket that links it, by
/// anyone. Teams paste pull request links into Jira comments, so this finds the ticket even when
/// someone else owns it. Looks up at most `MAX_LINK_LOOKUPS` keys.
pub fn tickets_linking(site: &str, email: &str, github_keys: &[String], range: &DayRange) -> Result<HashMap<String, Candidate>> {
    let token = TOKEN.get().ok_or_else(|| anyhow!("Connect Jira in Sources first"))?;
    let me = my_account_id(site, email, &token)?;
    let mut out = HashMap::new();
    for key in github_keys.iter().take(MAX_LINK_LOOKUPS) {
        let Some(jql) = link_jql(key) else { continue };
        let body = json!({"jql": jql, "fields": FIELDS, "maxResults": 1});
        let page = post(site, email, &token, "/rest/api/3/search/jql", &body)?;
        let first = page["issues"].as_array().and_then(|a| a.first()).and_then(|i| to_candidate(site, i, range.end_ms(), &me));
        if let Some(c) = first {
            out.insert(key.clone(), c);
        }
    }
    Ok(out)
}

/// A likely Jira site for a work email: `me@acme.com` → `acme.atlassian.net`.
pub fn guess_site(work_email: &str) -> Option<String> {
    let domain = work_email.rsplit_once('@')?.1;
    let stem = domain.split('.').next().filter(|s| !s.is_empty())?;
    Some(format!("{stem}.atlassian.net"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn day(d: &str) -> NaiveDate {
        NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn only_an_epic_parent_is_an_epic() {
        let epic = json!({"key": "SHOP-640", "fields": {"summary": "Warehouse EU Setup", "issuetype": {"name": "Epic", "hierarchyLevel": 1}}});
        assert_eq!(epic_of(&epic), Some(Epic { key: "SHOP-640".into(), title: "Warehouse EU Setup".into() }));
        let story = json!({"key": "SHOP-1", "fields": {"summary": "s", "issuetype": {"name": "Story", "hierarchyLevel": 0}}});
        assert_eq!(epic_of(&story), None);
        assert_eq!(epic_of(&Value::Null), None);
    }

    #[test]
    fn link_jql_quotes_the_pull_and_issue_urls() {
        assert_eq!(
            link_jql("web-frontend#3117").unwrap(),
            r#"text ~ "\"web-frontend/pull/3117\"" OR text ~ "\"web-frontend/issues/3117\"" ORDER BY updated DESC"#
        );
        assert!(link_jql("SHOP-1277").is_none());
    }

    #[test]
    fn site_forms_become_a_host() {
        assert_eq!(normalize_site("acme"), "acme.atlassian.net");
        assert_eq!(normalize_site("https://Acme.atlassian.net/jira/your-work"), "acme.atlassian.net");
        assert_eq!(normalize_site("jira.acme.com"), "jira.acme.com");
        assert_eq!(guess_site("me@acme.com").as_deref(), Some("acme.atlassian.net"));
    }

    #[test]
    fn jql_covers_the_whole_last_day() {
        let q = jql(&DayRange { from: day("2026-10-05"), to: day("2026-10-09") });
        assert!(q.contains("DURING (\"2026-10-05\", \"2026-10-10\")"), "{q}");
    }

    #[test]
    fn issue_json_becomes_a_candidate() {
        let issue = json!({"key": "SHOP-12", "fields": {"summary": "Fix VAT", "created": "2026-10-01T09:00:00.000+0000", "updated": "2026-10-06T10:00:00.000+0100", "assignee": {"accountId": "other"}, "reporter": {"accountId": "me"}}});
        let c = to_candidate("acme.atlassian.net", &issue, 0, "me").unwrap();
        assert_eq!(c.url, "https://acme.atlassian.net/browse/SHOP-12");
        assert_eq!(c.title, "Fix VAT");
        assert!(c.created_ms > 0 && c.last_ms > c.created_ms);
        assert_eq!(c.mine, Some(true), "the reporter is the user");
        assert_eq!(to_candidate("acme.atlassian.net", &issue, 0, "someone").unwrap().mine, Some(false));
    }
}
