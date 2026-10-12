use std::process::Command;
use std::time::Duration;

use chrono::Utc;
use crossbeam_channel::Sender;
use serde_json::json;

use crate::event::{kind, WorkEvent};

pub const SOURCE: &str = "app";
const POLL: Duration = Duration::from_secs(5);
const HEARTBEAT_SECS: i64 = 60;
const IDLE_AFTER_SECS: f64 = 120.0;

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(source_state: i32, event_type: u32) -> f64;
}

/// Seconds since the last keyboard or mouse input, for the whole login session.
#[cfg(target_os = "macos")]
pub fn idle_seconds() -> f64 {
    const COMBINED_SESSION_STATE: i32 = 0;
    const ANY_INPUT_EVENT: u32 = u32::MAX;
    unsafe { CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, ANY_INPUT_EVENT) }
}

#[cfg(not(target_os = "macos"))]
pub fn idle_seconds() -> f64 {
    0.0
}

/// The name and bundle id of the frontmost app, from `lsappinfo`. Window titles are not read.
pub fn front_app() -> Option<(String, String)> {
    let asn = run(&["front"])?;
    let info = run(&["info", "-only", "name", "-only", "bundleid", asn.trim()])?;
    let field = |key: &str| {
        info.lines()
            .find_map(|l| l.split_once(&format!("\"{key}\"=")).map(|(_, v)| v.trim().trim_matches('"').to_string()))
    };
    Some((field("LSDisplayName")?, field("CFBundleIdentifier").unwrap_or_default()))
}

fn run(args: &[&str]) -> Option<String> {
    let out = Command::new("lsappinfo").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Polls the frontmost app and idle time. Sends `app_active` on a switch and once a minute
/// while the user is active, and `idle_start` / `idle_end` around pauses.
pub fn run_loop(tx: Sender<WorkEvent>) {
    let mut last_app = String::new();
    let mut last_sent = 0i64;
    let mut idle = false;
    loop {
        let now = Utc::now();
        let idle_for = idle_seconds();
        if !idle && idle_for >= IDLE_AFTER_SECS {
            idle = true;
            let started = now - chrono::Duration::milliseconds((idle_for * 1000.0) as i64);
            let _ = tx.send(WorkEvent::new(SOURCE, format!("idle:{}", started.timestamp()), started, kind::IDLE_START));
        } else if idle && idle_for < IDLE_AFTER_SECS {
            idle = false;
            let _ = tx.send(WorkEvent::new(SOURCE, format!("wake:{}", now.timestamp()), now, kind::IDLE_END));
        }
        if !idle {
            if let Some((name, bundle)) = front_app() {
                if name != last_app || now.timestamp() - last_sent >= HEARTBEAT_SECS {
                    let mut e = WorkEvent::new(SOURCE, format!("{}:{}", now.timestamp(), name), now, kind::APP_ACTIVE);
                    e.detail = Some(name.clone());
                    e.meta = json!({ "bundle": bundle });
                    let _ = tx.send(e);
                    last_app = name;
                    last_sent = now.timestamp();
                }
            }
        }
        std::thread::sleep(POLL);
    }
}
