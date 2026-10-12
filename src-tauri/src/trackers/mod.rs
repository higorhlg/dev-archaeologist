//! Issue trackers the user signs in to, so report entries without a ticket can get one.
//! Trackers are only asked when the user presses "Find tickets"; nothing is written to them.

pub mod find;
pub mod github;
pub mod jira;
pub mod matcher;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::pipeline::day_bounds;
use crate::settings::Settings;

/// One of the user's tickets, as a tracker returned it.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    /// `SHOP-1261` or `insight-ui#128`.
    pub key: String,
    pub title: String,
    pub url: String,
    /// `jira` or `github`.
    pub tracker: String,
    /// `owner/name` for GitHub tickets, lowercase.
    pub repo: Option<String>,
    pub created_ms: i64,
    /// The last time the ticket changed, or the range end when the tracker says it was the
    /// user's during the range.
    pub last_ms: i64,
}

/// Title and link of a ticket, kept so the app can show and open it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketInfo {
    pub key: String,
    pub title: String,
    pub url: String,
    pub tracker: String,
}

impl From<&Candidate> for TicketInfo {
    fn from(c: &Candidate) -> Self {
        Self { key: c.key.clone(), title: c.title.clone(), url: c.url.clone(), tracker: c.tracker.clone() }
    }
}

/// Local calendar days, both ends included.
#[derive(Debug, Clone, Copy)]
pub struct DayRange {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

impl DayRange {
    pub fn end_ms(&self) -> i64 {
        day_bounds(self.to).1.timestamp_millis()
    }
}

/// What Sources shows about each tracker.
#[derive(Debug, Serialize)]
pub struct TrackerStatus {
    pub jira_site: String,
    pub jira_email: String,
    pub jira_connected: bool,
    /// A site and email to offer when Jira is not set up, from the work emails.
    pub jira_site_guess: Option<String>,
    pub jira_email_guess: Option<String>,
    pub github_enabled: bool,
    /// Logins the `gh` CLI is signed in with. Empty when `gh` is missing.
    pub github_accounts: Vec<String>,
}

pub fn status(settings: &Settings) -> TrackerStatus {
    let t = &settings.trackers;
    let work_email = settings.contexts.work_emails.first().cloned();
    TrackerStatus {
        jira_site: t.jira_site.clone(),
        jira_email: t.jira_email.clone(),
        jira_connected: jira::has_token() && !t.jira_site.is_empty() && !t.jira_email.is_empty(),
        jira_site_guess: work_email.as_deref().and_then(jira::guess_site),
        jira_email_guess: work_email,
        github_enabled: t.github,
        github_accounts: github::accounts(),
    }
}
