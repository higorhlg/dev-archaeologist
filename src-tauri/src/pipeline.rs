use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Utc};

use crate::achievements;
use crate::corrections::{apply_label_rules, Rule};
use crate::event::{kind, WorkEvent};
use crate::project::ProjectResolver;
use crate::redact::{redact, Privacy};
use crate::sessions::{cluster_sessions, gaps::find_gaps};
use crate::settings::Settings;
use crate::store::Store;

/// The one write path for events: resolve the project, apply corrections, redact, store.
#[derive(Clone)]
pub struct Pipeline {
    pub store: Store,
    resolver: Arc<ProjectResolver>,
}

impl Pipeline {
    pub fn new(store: Store) -> Self {
        Self { store, resolver: Arc::new(ProjectResolver::default()) }
    }

    /// @returns the number of new events stored.
    pub fn ingest(&self, events: Vec<WorkEvent>) -> Result<usize> {
        let rules = self.store.rules()?;
        let settings = Settings::load(&self.store)?;
        let privacy = Privacy { excluded_prefixes: settings.excluded_paths.clone() };
        let ready: Vec<WorkEvent> = events
            .into_iter()
            .filter(|e| settings.source_enabled(&e.source))
            .filter(|e| !is_excluded_app(e, &rules))
            .map(|mut e| {
                if e.project.is_none() {
                    e.project = e.path.as_deref().and_then(|p| self.resolver.resolve(p, &rules));
                }
                if e.project.is_none() && is_agent(&e) {
                    e.project = e.path.as_deref().and_then(workspace_folder);
                }
                e
            })
            .filter_map(|e| redact(e, &privacy))
            .collect();
        self.store.insert_events(&ready)
    }

    /// Re-groups one local day into sessions and re-runs the achievement rules.
    /// Sessions the user edited, confirmed or excluded stay as they are.
    pub fn rebuild_day(&self, day: NaiveDate) -> Result<usize> {
        let (start, end) = day_bounds(day);
        let events = self.store.events_between(start.timestamp_millis(), end.timestamp_millis())?;
        let key = day.format("%Y-%m-%d").to_string();
        let rules = self.store.rules()?;
        let mut sessions = cluster_sessions(&key, &events);
        for s in &mut sessions {
            apply_label_rules(s, &rules);
        }
        self.store.replace_day_sessions(&key, &sessions)?;
        self.rebuild_gaps(day, &key, &events)?;
        let history_start = start - Duration::days(30);
        let prior = self.store.events_between(history_start.timestamp_millis(), start.timestamp_millis())?;
        self.store.replace_achievements(&key, &achievements::detect(&key, &events, &prior))?;
        Ok(sessions.len())
    }

    /// Finds the gaps inside work hours against the stored sessions, which include the
    /// sessions the user made by hand. No schedule means no gaps.
    fn rebuild_gaps(&self, day: NaiveDate, key: &str, events: &[WorkEvent]) -> Result<()> {
        let gaps = match Settings::load(&self.store)?.schedule {
            Some(schedule) => find_gaps(day, &self.store.sessions_for_day(key)?, events, &schedule),
            None => vec![],
        };
        self.store.replace_day_gaps(key, &gaps)
    }

    /// Rebuilds every day that holds events in `[from, to]`.
    pub fn rebuild_range(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<usize> {
        let mut day = from.with_timezone(&Local).date_naive();
        let last = to.with_timezone(&Local).date_naive();
        let mut n = 0;
        while day <= last {
            n += self.rebuild_day(day)?;
            day = day.succ_opt().unwrap();
        }
        Ok(n)
    }
}

fn is_agent(e: &WorkEvent) -> bool {
    e.source == crate::adapters::claude_code::SOURCE || e.source == crate::adapters::codex::SOURCE
}

/// An agent started outside any repo works in a folder; that folder names the project,
/// unless it is the home folder itself.
fn workspace_folder(path: &str) -> Option<String> {
    let p = std::path::Path::new(path);
    (p.is_dir() && Some(p) != dirs::home_dir().as_deref())
        .then(|| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .flatten()
}

fn is_excluded_app(e: &WorkEvent, rules: &[Rule]) -> bool {
    e.kind == kind::APP_ACTIVE && e.detail.as_deref().is_some_and(|app| rules.iter().any(|r| r.excludes_app(app)))
}

/// Start and end of a local calendar day, in UTC.
pub fn day_bounds(day: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
    let start = Local.from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap()).earliest().unwrap();
    let end = Local.from_local_datetime(&day.succ_opt().unwrap().and_hms_opt(0, 0, 0).unwrap()).earliest().unwrap();
    (start.with_timezone(&Utc), end.with_timezone(&Utc))
}
