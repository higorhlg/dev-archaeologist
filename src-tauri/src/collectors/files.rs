use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::Utc;
use crossbeam_channel::Sender;
use notify::{EventKind, RecursiveMode, Watcher};

use crate::event::{kind, WorkEvent};
use crate::redact::is_noise;

pub const SOURCE: &str = "fs";
/// One event per file per minute is enough to show activity.
const PER_FILE_COOLDOWN: Duration = Duration::from_secs(60);

/// Watches each repo with FSEvents (through `notify`) and sends `file_changed` events.
/// Noise folders are dropped here, before the channel, to keep the writer quiet.
pub fn run_loop(tx: Sender<WorkEvent>, repos: Vec<PathBuf>) {
    let (raw_tx, raw_rx) = std::sync::mpsc::channel();
    let Ok(mut watcher) = notify::recommended_watcher(raw_tx) else { return };
    for r in &repos {
        let _ = watcher.watch(r, RecursiveMode::Recursive);
    }
    let mut last: HashMap<PathBuf, Instant> = HashMap::new();
    for res in raw_rx {
        let Ok(ev) = res else { continue };
        let change = match ev.kind {
            EventKind::Create(_) => "created",
            EventKind::Modify(_) => "modified",
            EventKind::Remove(_) => "deleted",
            _ => continue,
        };
        for path in ev.paths {
            let p = path.to_string_lossy().into_owned();
            if is_noise(&p) || last.get(&path).is_some_and(|t| t.elapsed() < PER_FILE_COOLDOWN) {
                continue;
            }
            last.insert(path.clone(), Instant::now());
            let now = Utc::now();
            let mut e = WorkEvent::new(SOURCE, format!("{p}:{}", now.timestamp() / 60), now, kind::FILE_CHANGED);
            e.path = Some(p);
            e.detail = Some(change.into());
            let _ = tx.send(e);
        }
    }
}
