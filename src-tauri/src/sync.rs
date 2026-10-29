//! Work that runs without a click: the ticket sync, while the app runs and from `dev-arch sync`.

use anyhow::Result;
use chrono::{Duration, Local, Utc};
use serde::{Deserialize, Serialize};

use crate::reports::{ContextFilter, Grouping, ReportQuery, TicketMode};
use crate::settings::Settings;
use crate::store::Store;
use crate::trackers::find;

/// Days back from today that each ticket sync covers.
const SYNC_DAYS: i64 = 14;
const STATUS_KEY: &str = "ticket_sync";

/// The outcome of the last ticket sync, shown in Reports.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TicketSyncStatus {
    pub at_ms: i64,
    pub from: String,
    pub to: String,
    pub checked: usize,
    pub suggested: usize,
    pub applied: usize,
    pub errors: Vec<String>,
}

pub fn ticket_sync_status(store: &Store) -> Result<Option<TicketSyncStatus>> {
    Ok(store.setting(STATUS_KEY)?.and_then(|s| serde_json::from_str(&s).ok()))
}

/// True when the interval in Sources has passed since the last sync and a tracker is set up.
/// Reads only settings: no Keychain, no `gh`, so the poller can ask every minute.
pub fn ticket_sync_due(store: &Store) -> Result<bool> {
    let settings = Settings::load(store)?;
    let every = settings.trackers.auto_sync_min;
    if every == 0 {
        return Ok(false);
    }
    let last = ticket_sync_status(store)?.map_or(0, |s| s.at_ms);
    Ok(Utc::now().timestamp_millis() - last >= i64::from(every) * 60_000 && any_tracker(&settings.trackers))
}

/// Jira has a site and email, or GitHub is on. A missing token or `gh` login shows up as an
/// empty or failed sync.
fn any_tracker(t: &crate::settings::TrackerSettings) -> bool {
    (!t.jira_site.is_empty() && !t.jira_email.is_empty()) || t.github
}

/// Runs "Find tickets" per session over the work side of the last two weeks, so a ticket one
/// session names is applied to that session only, without the AI tie-break, so it
/// costs nothing, and records the outcome, failed or not, so a failing tracker waits a full
/// interval before the next try. Suggestions the evidence names outright apply at once.
pub fn sync_tickets(store: &Store) -> Result<TicketSyncStatus> {
    let today = Local::now().date_naive();
    let q = ReportQuery {
        from: (today - Duration::days(SYNC_DAYS - 1)).format("%Y-%m-%d").to_string(),
        to: today.format("%Y-%m-%d").to_string(),
        grouping: Grouping::Session,
        tickets: TicketMode::Prefix,
        context: ContextFilter::Work,
        fit_day_min: None, fit_drift_min: None,
    };
    let r = find::find_tickets(store, &q, false).unwrap_or_else(|err| find::FindResult { errors: vec![format!("{err:#}")], ..Default::default() });
    let status = TicketSyncStatus {
        at_ms: Utc::now().timestamp_millis(),
        from: q.from,
        to: q.to,
        checked: r.checked,
        suggested: r.suggested,
        applied: r.applied,
        errors: r.errors,
    };
    store.set_setting(STATUS_KEY, &serde_json::to_string(&status)?)?;
    Ok(status)
}

/// The shortest interval the sync agent runs at.
pub const MIN_AGENT_MIN: u32 = 5;

/// The LaunchAgent that runs `dev-arch sync` while the app is closed.
const AGENT_LABEL: &str = "dev.higorhlg.devarchaeologist.sync";

fn agent_plist_path() -> Result<std::path::PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home folder"))?;
    Ok(home.join("Library/LaunchAgents").join(format!("{AGENT_LABEL}.plist")))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A LaunchAgent that runs `exe sync` at login and every `every_min` minutes, logging to
/// `~/Library/Logs/dev-archaeologist-sync.log`. `PATH` includes Homebrew so `gh` is found.
fn agent_plist(exe: &str, every_min: u32, db: Option<&str>, log: &str) -> String {
    let db_env = db.map(|d| format!("\n    <key>DEV_ARCH_DB</key><string>{}</string>", xml_escape(d))).unwrap_or_default();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{AGENT_LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>{}</string><string>sync</string></array>
  <key>StartInterval</key><integer>{}</integer>
  <key>RunAtLoad</key><true/>
  <key>LowPriorityIO</key><true/>
  <key>ProcessType</key><string>Background</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key><string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin</string>{db_env}
  </dict>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        xml_escape(exe),
        u64::from(every_min.max(MIN_AGENT_MIN)) * 60,
        log = xml_escape(log),
    )
}

/// Writes and loads the LaunchAgent for this `dev-arch` binary, replacing any earlier one.
/// @returns the plist path.
pub fn install_launch_agent(every_min: u32) -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home folder"))?;
    let log = home.join("Library/Logs/dev-archaeologist-sync.log");
    let path = agent_plist_path()?;
    let db = std::env::var("DEV_ARCH_DB").ok();
    std::fs::create_dir_all(path.parent().unwrap())?;
    remove_launch_agent()?;
    std::fs::write(&path, agent_plist(&exe.to_string_lossy(), every_min, db.as_deref(), &log.to_string_lossy()))?;
    let ok = std::process::Command::new("launchctl").arg("load").arg("-w").arg(&path).status()?.success();
    anyhow::ensure!(ok, "launchctl could not load {}", path.display());
    Ok(path)
}

/// Unloads and deletes the LaunchAgent. Does nothing when there is none.
/// @returns true when one was removed.
pub fn remove_launch_agent() -> Result<bool> {
    let path = agent_plist_path()?;
    if !path.exists() {
        return Ok(false);
    }
    std::process::Command::new("launchctl").arg("unload").arg("-w").arg(&path).status()?;
    std::fs::remove_file(&path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_runs_sync_on_the_interval() {
        let p = agent_plist("/Apps/dev-arch", 30, Some("/tmp/a&b.db"), "/l/sync.log");
        assert!(p.contains("<string>/Apps/dev-arch</string><string>sync</string>"));
        assert!(p.contains("<integer>1800</integer>"));
        assert!(p.contains("/tmp/a&amp;b.db"));
        assert!(agent_plist("/x", 1, None, "/l").contains("<integer>300</integer>"), "at most every 5 minutes");
    }
}

/// Finished days back from today that the automatic journal looks at.
const JOURNAL_DAYS: i64 = 7;
const DAILY_KEY: &str = "daily_run_day";
/// The last week whose full report was written, such as `2026-W43`.
const WEEKLY_KEY: &str = "weekly_report_done";

/// What the once-a-day run did.
#[derive(Debug, Default, Serialize)]
pub struct DailyRun {
    pub journals: Vec<String>,
    pub weekly_report: Option<String>,
    pub errors: Vec<String>,
}

/// Runs the daily automation once per local day, after the history import: journals for
/// finished days without one, then last week's full report when it is not written yet. The day
/// is claimed first, so the app and the sync agent never both run it; a day that fails waits
/// for the next one.
/// @returns `None` when it already ran today.
pub fn run_daily(p: &crate::pipeline::Pipeline) -> Result<Option<DailyRun>> {
    let today = Local::now().date_naive();
    let key = today.format("%Y-%m-%d").to_string();
    let settings = Settings::load(&p.store)?;
    if !settings.backfill_done || !p.store.claim_setting(DAILY_KEY, &key)? {
        return Ok(None);
    }
    let mut run = DailyRun::default();
    if settings.automation.journals && crate::infer::journal::api_key().is_some() {
        write_missing_journals(p, today, &mut run)?;
    }
    if settings.automation.weekly_report {
        let last_monday = crate::weekly::week_start(today) - Duration::days(7);
        let week = crate::weekly::week_name(last_monday);
        if p.store.setting(WEEKLY_KEY)?.as_deref() != Some(week.as_str()) {
            match crate::weekly::write_week(p, last_monday) {
                Ok(path) => {
                    p.store.set_setting(WEEKLY_KEY, &week)?;
                    run.weekly_report = Some(path.to_string_lossy().into_owned());
                }
                Err(err) => run.errors.push(format!("weekly report: {err:#}")),
            }
        }
    }
    Ok(Some(run))
}

/// Journals for the finished days of the past week that have sessions and no journal written
/// after the day ended. A journal written during its own day is rewritten; a confirmed one stays.
fn write_missing_journals(p: &crate::pipeline::Pipeline, today: chrono::NaiveDate, run: &mut DailyRun) -> Result<()> {
    for back in (1..=JOURNAL_DAYS).rev() {
        let date = today - Duration::days(back);
        let day = date.format("%Y-%m-%d").to_string();
        if has_final_journal(&p.store, date, &day)? || p.store.sessions_for_day(&day)?.is_empty() {
            continue;
        }
        match crate::infer::journal::generate(&p.store, &day) {
            Ok(_) => run.journals.push(day),
            Err(err) => run.errors.push(format!("journal {day}: {err:#}")),
        }
    }
    Ok(())
}

/// The day has a confirmed journal, or one written after the day ended.
fn has_final_journal(store: &Store, date: chrono::NaiveDate, day: &str) -> Result<bool> {
    if store.journal(day)?.is_some_and(|j| j.state == "confirmed") {
        return Ok(true);
    }
    let day_end = crate::pipeline::day_bounds(date).1.timestamp_millis();
    Ok(store.journal_written_ms(day)?.is_some_and(|at| at >= day_end))
}
