use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::adapters::{claude_code, codex, git};
use crate::event::WorkEvent;
use crate::pipeline::Pipeline;
use crate::project::find_repo_root;
use crate::settings::Settings;

/// What a first scan finds before anything is stored. Drives the onboarding screen.
#[derive(Debug, Clone, Serialize)]
pub struct Discovery {
    pub claude_profiles: usize,
    pub claude_sessions: usize,
    pub codex_sessions: usize,
    pub repos: Vec<String>,
    pub shell_history_has_times: bool,
}

pub fn discover(settings: &Settings, since: DateTime<Utc>) -> Discovery {
    let cc_roots = claude_code::default_roots();
    let recent = |files: Vec<PathBuf>| files.into_iter().filter(|f| modified_after(f, since)).count();
    Discovery {
        claude_profiles: cc_roots.len(),
        claude_sessions: recent(claude_code::session_files(&cc_roots)),
        codex_sessions: recent(codex::session_files(&codex::default_roots())),
        repos: git::discover_repos(&settings.roots(), 3).iter().map(|p| p.to_string_lossy().into_owned()).collect(),
        shell_history_has_times: zsh_history_has_times(),
    }
}

fn modified_after(path: &PathBuf, since: DateTime<Utc>) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| DateTime::<Utc>::from(t) >= since)
        .unwrap_or(false)
}

/// Extended history lines look like `: 1712345678:0;cmd`. Only the prefix is checked.
fn zsh_history_has_times() -> bool {
    let Some(path) = dirs::home_dir().map(|h| h.join(".zsh_history")) else { return false };
    let Ok(bytes) = std::fs::read(path) else { return false };
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).into_owned();
    head.lines().take(20).any(|l| l.starts_with(": ") && l.contains(';'))
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct BackfillReport {
    pub claude_code: usize,
    pub codex: usize,
    pub git: usize,
    pub sessions: usize,
}

/// Reads past agent sessions and the commits of every repo they or the roots point at.
pub fn backfill(pipeline: &Pipeline, since: DateTime<Utc>) -> Result<BackfillReport> {
    let settings = Settings::load(&pipeline.store)?;
    let mut report = BackfillReport::default();
    let mut agent_dirs = BTreeSet::new();
    let mut tail = AgentTail::default();
    let (cc, cx) = tail.scan(since)?;
    for e in cc.iter().chain(cx.iter()) {
        if let Some(p) = &e.path {
            agent_dirs.insert(p.clone());
        }
    }
    report.claude_code = pipeline.ingest(cc)?;
    report.codex = pipeline.ingest(cx)?;
    for repo in repos_to_scan(&settings, &agent_dirs) {
        if let Ok(events) = git::scan(&repo, since) {
            report.git += pipeline.ingest(events)?;
        }
    }
    report.sessions = pipeline.rebuild_range(since, Utc::now())?;
    let mut s = Settings::load(&pipeline.store)?;
    s.backfill_done = true;
    s.save(&pipeline.store)?;
    Ok(report)
}

/// Repos under the roots, plus repos that agent sessions worked in.
pub fn repos_to_scan(settings: &Settings, agent_dirs: &BTreeSet<String>) -> Vec<PathBuf> {
    let mut repos: BTreeSet<PathBuf> = git::discover_repos(&settings.roots(), 3).into_iter().collect();
    for d in agent_dirs {
        if let Some(root) = find_repo_root(&PathBuf::from(d)) {
            repos.insert(root);
        }
    }
    repos.into_iter().collect()
}

/// Re-reads agent log files whose size or time changed since the last pass.
#[derive(Default)]
pub struct AgentTail {
    seen: HashMap<PathBuf, (SystemTime, u64)>,
}

impl AgentTail {
    /// @returns `(claude_code_events, codex_events)` from changed files only.
    pub fn scan(&mut self, since: DateTime<Utc>) -> Result<(Vec<WorkEvent>, Vec<WorkEvent>)> {
        let mut cc = Vec::new();
        for f in claude_code::session_files(&claude_code::default_roots()) {
            if self.changed(&f, since) {
                cc.extend(claude_code::parse_file(&f, since).unwrap_or_default());
            }
        }
        let mut cx = Vec::new();
        for f in codex::session_files(&codex::default_roots()) {
            if self.changed(&f, since) {
                cx.extend(codex::parse_file(&f, since).unwrap_or_default());
            }
        }
        Ok((cc, cx))
    }

    fn changed(&mut self, f: &PathBuf, since: DateTime<Utc>) -> bool {
        let Ok(m) = std::fs::metadata(f) else { return false };
        let stamp = (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len());
        if DateTime::<Utc>::from(stamp.0) < since || self.seen.get(f) == Some(&stamp) {
            return false;
        }
        self.seen.insert(f.clone(), stamp);
        true
    }
}
