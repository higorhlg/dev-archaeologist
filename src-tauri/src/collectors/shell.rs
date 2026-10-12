use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

use chrono::{TimeZone, Utc};
use crossbeam_channel::Sender;
use serde::Deserialize;
use serde_json::json;

use crate::event::{kind, WorkEvent};

pub const SOURCE: &str = "shell";

/// Command names that run a test suite on their own. With name-only capture,
/// `pnpm test` is just `pnpm`, so only dedicated runners count as tests.
const TEST_RUNNERS: &[&str] = &["pytest", "vitest", "jest", "mocha", "rspec", "phpunit", "nextest"];

pub fn socket_path() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".dev-arch").join("shell.sock")
}

/// The zsh hook sends one JSON line per command: name, exit code, folder and times.
#[derive(Debug, Deserialize)]
struct HookLine {
    cmd: String,
    exit: i64,
    cwd: String,
    t0: f64,
    t1: f64,
}

/// Listens on `~/.dev-arch/shell.sock` and turns each hook line into an event.
pub fn run_loop(tx: Sender<WorkEvent>) {
    let path = socket_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else { return };
    for stream in listener.incoming().flatten() {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if let Some(e) = parse_line(&line) {
                let _ = tx.send(e);
            }
        }
    }
}

fn parse_line(line: &str) -> Option<WorkEvent> {
    let h: HookLine = serde_json::from_str(line).ok()?;
    let name = h.cmd.split_whitespace().next()?.rsplit('/').next()?.to_string();
    let ts = Utc.timestamp_millis_opt((h.t0 * 1000.0) as i64).single()?;
    let is_test = TEST_RUNNERS.contains(&name.as_str());
    let k = match (is_test, h.exit) {
        (true, 0) => kind::TEST_PASSED,
        (true, _) => kind::TEST_FAILED,
        _ => kind::SHELL_COMMAND,
    };
    let mut e = WorkEvent::new(SOURCE, format!("{}:{}", h.t0, name), ts, k);
    e.detail = Some(name);
    e.path = Some(h.cwd);
    e.meta = json!({ "exit": h.exit, "dur_ms": ((h.t1 - h.t0) * 1000.0) as i64, "fail": if is_test && h.exit != 0 { 1 } else { 0 } });
    Some(e)
}

/// The hook script the user sources from `~/.zshrc`. It sends the first word only.
pub const ZSH_HOOK: &str = include_str!("../../../shell/dev-arch.zsh");

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_only_the_command_name() {
        let e = super::parse_line(r#"{"cmd":"pnpm","exit":1,"cwd":"/r","t0":1760000000.5,"t1":1760000003.0}"#).unwrap();
        assert_eq!(e.detail.as_deref(), Some("pnpm"));
        assert_eq!(e.kind, "shell_command");
        assert_eq!(e.meta_i64("dur_ms"), Some(2500));
    }
}
