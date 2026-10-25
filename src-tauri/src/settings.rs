use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::contexts::ContextRules;
use crate::reports::{ContextFilter, Grouping, TicketMode};
use crate::store::Store;

/// The model every AI feature uses unless the user picks another in Sources.
pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";

/// What the user allows the app to collect and where it may send the daily digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub sources: SourceToggles,
    pub repo_roots: Vec<String>,
    pub excluded_paths: Vec<String>,
    /// Projects whose sessions never go into a cloud digest.
    pub cloud_opt_out: Vec<String>,
    pub model: String,
    pub backfill_done: bool,
    pub toggl: TogglSettings,
    /// The report options the user applied last, restored the next time Reports opens.
    pub report_prefs: ReportPrefs,
    /// `None` when the user skipped the step; then no gap is ever filled.
    pub schedule: Option<WorkSchedule>,
    /// Onboarding asked about work hours, whether or not the user gave them.
    pub schedule_asked: bool,
    /// Which projects are work and which are personal.
    pub contexts: ContextRules,
    /// Calendar names whose events never become meetings, such as "Birthdays".
    pub calendar_excluded: Vec<String>,
    /// Where "Find tickets" looks for the user's tickets.
    pub trackers: TrackerSettings,
    /// Reports the app writes on its own.
    pub automation: Automation,
}

/// Work the app does once a day without a click.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Automation {
    /// Write the AI journal for finished days of the past week that have none. Needs an API key;
    /// projects that never leave the Mac stay out of the digest as always.
    pub journals: bool,
    /// Write last week's work report as Markdown once the week is over.
    pub weekly_report: bool,
}

impl Default for Automation {
    fn default() -> Self {
        Self { journals: true, weekly_report: true }
    }
}

/// Issue trackers that "Find tickets" asks. Secrets stay in the Keychain, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackerSettings {
    /// Jira Cloud site host, such as `acme.atlassian.net`. Empty when Jira is not set up.
    pub jira_site: String,
    /// The Atlassian account email that owns the API token.
    pub jira_email: String,
    /// Ask GitHub through the accounts the `gh` CLI is signed in with.
    pub github: bool,
    /// Minutes between automatic ticket syncs while the app runs, and at launch. 0 turns them off.
    pub auto_sync_min: u32,
}

impl Default for TrackerSettings {
    fn default() -> Self {
        Self { jira_site: String::new(), jira_email: String::new(), github: true, auto_sync_min: 30 }
    }
}

/// The user's regular hours. One range for every work day, with breaks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkSchedule {
    /// 0 = Monday … 6 = Sunday.
    pub days: Vec<u32>,
    /// Local time, `HH:MM`.
    pub start: String,
    pub end: String,
    pub breaks: Vec<[String; 2]>,
    /// The longest idle gap between two sessions that the app bridges.
    pub bridge_max_min: i64,
    /// The project "Fill to schedule" offers first. It never applies on its own.
    pub fill_project: Option<String>,
}

impl Default for WorkSchedule {
    fn default() -> Self {
        Self {
            days: vec![0, 1, 2, 3, 4],
            start: "08:00".into(),
            end: "17:00".into(),
            breaks: vec![["12:00".into(), "13:00".into()]],
            bridge_max_min: 60,
            fill_project: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReportPrefs {
    /// `this_week` | `last_week` | `this_month` | `last_month`
    pub range: String,
    pub grouping: Grouping,
    pub tickets: TicketMode,
    pub context: ContextFilter,
}

impl Default for ReportPrefs {
    fn default() -> Self {
        Self { range: "this_week".into(), grouping: Grouping::TicketDay, tickets: TicketMode::Prefix, context: ContextFilter::Work }
    }
}

/// How local projects become Toggl time entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TogglSettings {
    /// The email Toggl's CSV importer matches to a workspace member.
    pub email: String,
    pub workspace_id: Option<i64>,
    pub default_billable: bool,
    /// Local project name → Toggl project. An unmapped project keeps its local name.
    pub project_map: BTreeMap<String, TogglProject>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TogglProject {
    pub name: String,
    pub id: Option<i64>,
    pub client: String,
    pub billable: bool,
    /// Personal work that never goes to Toggl.
    pub skip: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceToggles {
    pub app: bool,
    pub window_titles: bool,
    pub fs: bool,
    pub git: bool,
    pub agents: bool,
    pub shell: bool,
    /// Meetings from the macOS Calendar app. Reads nothing until macOS grants access.
    pub calendar: bool,
}

impl Default for SourceToggles {
    fn default() -> Self {
        Self { app: true, window_titles: false, fs: true, git: true, agents: true, shell: true, calendar: true }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sources: SourceToggles::default(),
            repo_roots: default_repo_roots(),
            excluded_paths: Vec::new(),
            cloud_opt_out: Vec::new(),
            model: DEFAULT_MODEL.into(),
            backfill_done: false,
            toggl: TogglSettings::default(),
            report_prefs: ReportPrefs::default(),
            schedule: None,
            schedule_asked: false,
            contexts: ContextRules::default(),
            calendar_excluded: Vec::new(),
            trackers: TrackerSettings::default(),
            automation: Automation::default(),
        }
    }
}

/// Common code folders in the home folder that exist on this machine.
fn default_repo_roots() -> Vec<String> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    ["code", "projects", "dev", "src", "repos", "work", "Developer"]
        .iter()
        .map(|d| home.join(d))
        .filter(|p| p.is_dir())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

impl Settings {
    pub fn load(store: &Store) -> Result<Self> {
        Ok(store.setting("settings")?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default())
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.set_setting("settings", &serde_json::to_string(self)?)
    }

    pub fn source_enabled(&self, source: &str) -> bool {
        match source {
            "app" => self.sources.app,
            "fs" => self.sources.fs,
            "git" => self.sources.git,
            "claude_code" | "codex" => self.sources.agents,
            "shell" => self.sources.shell,
            "calendar" => self.sources.calendar,
            _ => true,
        }
    }

    pub fn roots(&self) -> Vec<PathBuf> {
        self.repo_roots.iter().map(PathBuf::from).collect()
    }
}
