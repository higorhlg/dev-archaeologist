use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::reports::{Grouping, TicketMode};
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReportPrefs {
    /// `this_week` | `last_week` | `this_month` | `last_month`
    pub range: String,
    pub grouping: Grouping,
    pub tickets: TicketMode,
}

impl Default for ReportPrefs {
    fn default() -> Self {
        Self { range: "this_week".into(), grouping: Grouping::TicketDay, tickets: TicketMode::Prefix }
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
}

impl Default for SourceToggles {
    fn default() -> Self {
        Self { app: true, window_titles: false, fs: true, git: true, agents: true, shell: true }
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
            _ => true,
        }
    }

    pub fn roots(&self) -> Vec<PathBuf> {
        self.repo_roots.iter().map(PathBuf::from).collect()
    }
}
