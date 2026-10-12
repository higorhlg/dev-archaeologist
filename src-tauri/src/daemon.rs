use std::collections::BTreeSet;
use std::thread;
use std::time::Duration;

use chrono::{Local, Utc};
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
/// then rebuilds today.
fn poll_loop(pipeline: Pipeline) {
    let mut tail = AgentTail::default();
    loop {
        let since = Utc::now() - chrono::Duration::days(1);
        if let Err(err) = poll_once(&pipeline, &mut tail, since) {
            eprintln!("dev-arch: poll failed: {err:#}");
        }
        thread::sleep(POLL_EVERY);
    }
}

fn poll_once(pipeline: &Pipeline, tail: &mut AgentTail, since: chrono::DateTime<Utc>) -> anyhow::Result<()> {
    let settings = Settings::load(&pipeline.store)?;
    let mut dirs = BTreeSet::new();
    if settings.sources.agents {
        let (cc, cx) = tail.scan(since)?;
        dirs.extend(cc.iter().chain(cx.iter()).filter_map(|e| e.path.clone()));
        pipeline.ingest(cc)?;
        pipeline.ingest(cx)?;
    }
    if settings.sources.git {
        for repo in repos_to_scan(&settings, &dirs) {
            if let Ok(events) = git::scan(&repo, since) {
                pipeline.ingest(events)?;
            }
        }
    }
    crate::backfill::import_meetings(pipeline, since)?;
    let today = Local::now().date_naive();
    pipeline.rebuild_day(today)?;
    Ok(())
}
