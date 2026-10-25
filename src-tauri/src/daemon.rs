use std::collections::BTreeSet;
use std::thread;
use std::time::Duration;

use chrono::Utc;
use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError};

use crate::adapters::git;
use crate::backfill::{repos_to_scan, AgentTail};
use crate::collectors::{files, foreground, shell};
use crate::event::WorkEvent;
use crate::pipeline::Pipeline;
use crate::settings::Settings;

const WRITE_EVERY: Duration = Duration::from_secs(2);
const POLL_EVERY: Duration = Duration::from_secs(60);

/// Starts every enabled collector, one writer and one poller. Returns at once;
/// the threads live as long as the process.
pub fn start(pipeline: Pipeline) -> anyhow::Result<()> {
    let settings = Settings::load(&pipeline.store)?;
    let (tx, rx) = unbounded::<WorkEvent>();
    if settings.sources.app {
        let tx = tx.clone();
        thread::Builder::new().name("foreground".into()).spawn(move || foreground::run_loop(tx))?;
    }
    if settings.sources.fs {
        let tx = tx.clone();
        let repos = git::discover_repos(&settings.roots(), 3);
        thread::Builder::new().name("files".into()).spawn(move || files::run_loop(tx, repos))?;
    }
    if settings.sources.shell {
        let tx = tx.clone();
        thread::Builder::new().name("shell".into()).spawn(move || shell::run_loop(tx))?;
    }
    let p = pipeline.clone();
    thread::Builder::new().name("writer".into()).spawn(move || write_loop(p, rx))?;
    thread::Builder::new().name("poller".into()).spawn(move || poll_loop(pipeline))?;
    Ok(())
}

/// Batches channel events and stores them every two seconds through the pipeline.
fn write_loop(pipeline: Pipeline, rx: Receiver<WorkEvent>) {
    let mut batch = Vec::new();
    loop {
        match rx.recv_timeout(WRITE_EVERY) {
            Ok(e) => {
                batch.push(e);
                batch.extend(rx.try_iter());
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if !batch.is_empty() {
            if let Err(err) = pipeline.ingest(std::mem::take(&mut batch)) {
                eprintln!("dev-arch: write failed: {err:#}");
            }
        }
    }
}

/// Once a minute: reads new agent log lines, new commits and meetings that just ended,
/// then rebuilds every day since the last poll. The first pass catches up on what happened
/// while the app was closed. Then syncs tickets when the interval in Sources has passed, and
/// once a day writes missing journals and last week's report.
fn poll_loop(pipeline: Pipeline) {
    let mut tail = AgentTail::default();
    loop {
        if let Err(err) = catch_up(&pipeline, &mut tail) {
            eprintln!("dev-arch: poll failed: {err:#}");
        }
        if crate::sync::ticket_sync_due(&pipeline.store).unwrap_or(false) {
            if let Err(err) = crate::sync::sync_tickets(&pipeline.store) {
                eprintln!("dev-arch: ticket sync failed: {err:#}");
            }
        }
        match crate::sync::run_daily(&pipeline) {
            Ok(Some(run)) => run.errors.iter().for_each(|e| eprintln!("dev-arch: daily run: {e}")),
            Ok(None) => {}
            Err(err) => eprintln!("dev-arch: daily run failed: {err:#}"),
        }
        thread::sleep(POLL_EVERY);
    }
}

/// How far back a catch-up reads at most, and how far back every poll reads at least.
const MAX_CATCH_UP: chrono::Duration = chrono::Duration::days(90);
const MIN_LOOK_BACK: chrono::Duration = chrono::Duration::days(1);

/// Bumped when adapters start reading new facts from logs already stored, so the next catch-up
/// reads the whole history window again and existing events gain them.
const EVIDENCE_VERSION: i64 = 2;

/// Reads every source since the last successful poll (at least one day back, at most 90),
/// rebuilds the days in between, and records the poll. Before the user ran the history import
/// it reads one day only, so onboarding stays the one place that reads the past. Used by the app's poller and by
/// `dev-arch sync`.
/// @returns the number of new events.
pub fn catch_up(pipeline: &Pipeline, tail: &mut AgentTail) -> anyhow::Result<usize> {
    let now = Utc::now();
    let store = &pipeline.store;
    let evidence_current = store.setting("evidence_version")?.and_then(|v| v.parse::<i64>().ok()) >= Some(EVIDENCE_VERSION);
    let last_poll = store
        .setting("last_poll_ms")?
        .and_then(|v| v.parse::<i64>().ok())
        .and_then(chrono::DateTime::<Utc>::from_timestamp_millis)
        .filter(|_| evidence_current);
    let oldest = if Settings::load(store)?.backfill_done { now - MAX_CATCH_UP } else { now - MIN_LOOK_BACK };
    let rebuild_from = last_poll.unwrap_or(oldest).max(oldest);
    let since = rebuild_from.min(now - MIN_LOOK_BACK);
    let n = poll_once(pipeline, tail, since)?;
    pipeline.rebuild_range(rebuild_from, now)?;
    record_poll(store, now)?;
    Ok(n)
}

/// Marks every source as read up to `at` with the current adapters.
pub fn record_poll(store: &crate::store::Store, at: chrono::DateTime<Utc>) -> anyhow::Result<()> {
    store.set_setting("last_poll_ms", &at.timestamp_millis().to_string())?;
    store.set_setting("evidence_version", &EVIDENCE_VERSION.to_string())
}

/// @returns the number of new events.
fn poll_once(pipeline: &Pipeline, tail: &mut AgentTail, since: chrono::DateTime<Utc>) -> anyhow::Result<usize> {
    let settings = Settings::load(&pipeline.store)?;
    let mut dirs = BTreeSet::new();
    let mut n = 0;
    if settings.sources.agents {
        let (cc, cx) = tail.scan(since)?;
        dirs.extend(cc.iter().chain(cx.iter()).filter_map(|e| e.path.clone()));
        n += pipeline.ingest(cc)?;
        n += pipeline.ingest(cx)?;
    }
    if settings.sources.git {
        for repo in repos_to_scan(&settings, &dirs) {
            if let Ok(scan) = git::scan(&repo, since) {
                n += pipeline.ingest_git(scan)?;
            }
        }
    }
    n += crate::backfill::import_meetings(pipeline, since)?;
    Ok(n)
}
