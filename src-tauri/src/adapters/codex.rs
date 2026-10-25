use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Value};

use super::shell_refs::{self, PrEvent};
use super::{content_text, is_human_prompt, is_test_command, parse_test_counts};
use crate::event::{clip, kind, WorkEvent};

pub const SOURCE: &str = "codex";

pub fn default_roots() -> Vec<PathBuf> {
    dirs::home_dir().map(|h| h.join(".codex").join("sessions")).filter(|p| p.is_dir()).into_iter().collect()
}

pub fn session_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots
        .iter()
        .flat_map(|r| walkdir::WalkDir::new(r).max_depth(4).into_iter().flatten())
        .filter(|e| e.file_name().to_string_lossy().starts_with("rollout-") && e.path().extension().is_some_and(|x| x == "jsonl"))
        .map(|e| e.into_path())
        .collect()
}

/// Parses one Codex rollout file. Record shapes match Codex logs on 2026-10-09:
/// `session_meta`, `turn_context`, `event_msg/task_started|task_complete`,
/// `response_item/message|function_call|function_call_output`. Shell commands add the pull
/// requests the agent opened or acted on with `gh pr`, and the branch it created.
pub fn parse_file(path: &Path, since: DateTime<Utc>) -> Result<Vec<WorkEvent>> {
    let reader = BufReader::new(std::fs::File::open(path)?);
    let mut out = Vec::new();
    let mut cwd: Option<String> = None;
    let mut session = String::new();
    let mut pending_tests: HashMap<String, DateTime<Utc>> = HashMap::new();
    let mut pending_prs: HashMap<String, DateTime<Utc>> = HashMap::new();

    for line in reader.lines().map_while(Result::ok) {
        let Ok(rec) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(ts) = rec.get("timestamp").and_then(Value::as_str).and_then(|t| t.parse::<DateTime<Utc>>().ok()) else {
            continue;
        };
        let p = rec.get("payload").cloned().unwrap_or_default();
        let ptype = p.get("type").and_then(Value::as_str);
        match (rec.get("type").and_then(Value::as_str), ptype) {
            (Some("session_meta"), _) => {
                session = p.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                cwd = p.get("cwd").and_then(Value::as_str).map(String::from).or(cwd);
            }
            (Some("turn_context"), _) => {
                cwd = p.get("cwd").and_then(Value::as_str).map(String::from).or(cwd);
            }
            _ if ts < since => {}
            (Some("event_msg"), Some("task_complete")) => {
                if let Some(e) = turn_event(&p, &session, &cwd) {
                    out.push(e);
                }
            }
            (Some("response_item"), Some("message")) if p.get("role").and_then(Value::as_str) == Some("user") => {
                let text = content_text(p.get("content").unwrap_or(&Value::Null));
                if is_human_prompt(&text) {
                    let id = p.get("id").and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("{session}:{ts}"));
                    let mut e = WorkEvent::new(SOURCE, id, ts, kind::PROMPT);
                    e.detail = Some(clip(&text, 200));
                    e.path = cwd.clone();
                    e.meta = json!({ "session": session });
                    out.push(e);
                }
            }
            (Some("response_item"), Some("function_call" | "custom_tool_call")) => {
                let call_id = p.get("call_id").and_then(Value::as_str).unwrap_or_default().to_string();
                let name = p.get("name").and_then(Value::as_str).unwrap_or("tool");
                let args = p.get("arguments").or_else(|| p.get("input")).map(arg_text).unwrap_or_default();
                if is_test_command(&args) {
                    pending_tests.insert(call_id.clone(), ts);
                }
                if shell_refs::creates_pr(&args) {
                    pending_prs.insert(call_id.clone(), ts);
                }
                for pr in shell_refs::pr_commands(&args) {
                    out.push(shell_refs::pr_event(PrEvent { source: SOURCE, call_id: &call_id, pr: &pr, ts, path: &cwd }));
                }
                let mut e = WorkEvent::new(SOURCE, call_id, ts, kind::TOOL_USE);
                e.detail = Some(name.to_string());
                e.path = cwd.clone();
                e.branch = shell_refs::created_branch(&args);
                out.push(e);
            }
            (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) => {
                let call_id = p.get("call_id").and_then(Value::as_str).unwrap_or_default();
                if pending_tests.remove(call_id).is_some() {
                    out.push(test_event(&p, ts, call_id, &cwd));
                }
                if let Some(at) = pending_prs.remove(call_id) {
                    let output = p.get("output").map(content_text).unwrap_or_default();
                    for pr in shell_refs::pr_urls(&output) {
                        out.push(shell_refs::pr_event(PrEvent { source: SOURCE, call_id, pr: &pr, ts: at, path: &cwd }));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Codex arguments are a JSON string such as `{"cmd": ["bash","-lc","pnpm test"]}`.
fn arg_text(v: &Value) -> String {
    match v {
        Value::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(inner) => inner.to_string(),
            Err(_) => s.clone(),
        },
        other => other.to_string(),
    }
}

fn turn_event(p: &Value, session: &str, cwd: &Option<String>) -> Option<WorkEvent> {
    let started = p.get("started_at").and_then(Value::as_i64)?;
    let start = Utc.timestamp_opt(started, 0).single()?;
    let dur = p.get("duration_ms").and_then(Value::as_i64).unwrap_or(0).clamp(0, super::MAX_TURN_MS);
    let turn = p.get("turn_id").and_then(Value::as_str).unwrap_or_default();
    let mut e = WorkEvent::new(SOURCE, format!("{session}:{turn}:turn"), start, kind::AGENT_TURN);
    e.path = cwd.clone();
    e.meta = json!({ "dur_ms": dur });
    Some(e)
}

fn test_event(p: &Value, ts: DateTime<Utc>, call_id: &str, cwd: &Option<String>) -> WorkEvent {
    let output = p.get("output").map(content_text).unwrap_or_default();
    let (pass, fail) = parse_test_counts(&output);
    let exit_bad = output.contains("Exit code: 1") || output.contains("exit_code\":1");
    let ok = fail == 0 && !exit_bad;
    let mut e = WorkEvent::new(SOURCE, format!("{call_id}:result"), ts, if ok { kind::TEST_PASSED } else { kind::TEST_FAILED });
    e.detail = Some("agent test run".into());
    e.path = cwd.clone();
    e.meta = json!({ "pass": pass, "fail": if ok { 0 } else { fail.max(1) } });
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_lines(name: &str, lines: &[Value]) -> Vec<WorkEvent> {
        let path = std::env::temp_dir().join(format!("devarch-codex-{name}-{}.jsonl", std::process::id()));
        std::fs::write(&path, lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
        let out = parse_file(&path, DateTime::<Utc>::MIN_UTC).unwrap();
        std::fs::remove_file(&path).ok();
        out
    }

    fn call(ts: &str, id: &str, cmd: &str) -> Value {
        let args = json!({ "cmd": cmd }).to_string();
        json!({"timestamp": ts, "type": "response_item", "payload": {"type": "function_call", "name": "exec_command", "call_id": id, "arguments": args}})
    }

    #[test]
    fn reads_pull_requests_and_branches_from_shell_commands() {
        let events = parse_lines("refs", &[
            json!({"timestamp": "2026-10-21T13:00:00Z", "type": "session_meta", "payload": {"id": "s1", "cwd": "/r/insight-ui"}}),
            call("2026-10-21T13:01:00Z", "c1", "git switch -c codex/issue-166-titles"),
            call("2026-10-21T13:02:00Z", "c2", "gh pr create --draft --title 'Titles'"),
            json!({"timestamp": "2026-10-21T13:02:05Z", "type": "response_item", "payload": {"type": "function_call_output", "call_id": "c2", "output": "https://github.com/acme/insight-ui/pull/187\n"}}),
            call("2026-10-21T13:03:00Z", "c3", "gh pr view 2322 --repo acme/web-frontend --json url"),
        ]);
        let branch = events.iter().find(|e| e.source_id == "c1").and_then(|e| e.branch.as_deref());
        assert_eq!(branch, Some("codex/issue-166-titles"));
        let prs: Vec<(&str, &str)> = events
            .iter()
            .filter(|e| e.kind == kind::PR_LINK)
            .map(|e| (e.detail.as_deref().unwrap(), e.meta["action"].as_str().unwrap()))
            .collect();
        assert_eq!(prs, vec![("insight-ui#187", "open"), ("web-frontend#2322", "read")]);
        let created = events.iter().find(|e| e.detail.as_deref() == Some("insight-ui#187")).unwrap();
        assert_eq!(created.ts, "2026-10-21T13:02:00Z".parse::<DateTime<Utc>>().unwrap(), "placed at the command");
    }
}
