use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use super::shell_refs::{self, PrEvent};
use super::{content_text, is_human_prompt, is_test_command, parse_test_counts};
use crate::event::{clip, kind, WorkEvent};

pub const SOURCE: &str = "claude_code";

/// A pause longer than this ends an agent turn. A session reopened hours later is new work,
/// not one turn that ran the whole time.
const TURN_GAP: chrono::Duration = chrono::Duration::minutes(15);

/// Every `projects` folder of every Claude Code profile in the home folder
/// (`~/.claude`, `~/.claude-personal`, …).
pub fn default_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else { return vec![] };
    let Ok(entries) = std::fs::read_dir(&home) else { return vec![] };
    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".claude"))
        .map(|e| e.path().join("projects"))
        .filter(|p| p.is_dir())
        .collect()
}

pub fn session_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots
        .iter()
        .flat_map(|r| walkdir::WalkDir::new(r).max_depth(3).into_iter().flatten())
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .map(|e| e.into_path())
        .collect()
}

#[derive(Default)]
struct Turn {
    prompt_uuid: String,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    cwd: Option<String>,
    branch: Option<String>,
}

struct PendingTest {
    ts: DateTime<Utc>,
}

/// Parses one Claude Code session file. Field names match the logs on 2026-10-25:
/// `type`, `timestamp`, `uuid`, `cwd`, `gitBranch`, `message`, `isMeta`, `isSidechain`, `aiTitle`,
/// and `prNumber`, `prRepository` on `pr-link` records.
/// Reads prompts (clipped), tool names, file paths, the Jira key a tool acted on, pull requests
/// the session linked, and test counts. Never tool output text.
pub fn parse_file(path: &Path, since: DateTime<Utc>) -> Result<Vec<WorkEvent>> {
    let session_id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let reader = BufReader::new(std::fs::File::open(path)?);
    let mut out = Vec::new();
    let mut turn = Turn::default();
    let mut pending: HashMap<String, PendingTest> = HashMap::new();
    let mut last_ts: Option<DateTime<Utc>> = None;
    let mut last_cwd: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        let Ok(rec) = serde_json::from_str::<Value>(&line) else { continue };
        let ts = rec.get("timestamp").and_then(Value::as_str).and_then(|t| t.parse::<DateTime<Utc>>().ok());
        if rec.get("type").and_then(Value::as_str) == Some("ai-title") {
            if let (Some(title), Some(t)) = (rec.get("aiTitle").and_then(Value::as_str), last_ts) {
                let mut e = WorkEvent::new(SOURCE, format!("{session_id}:title"), t, kind::AI_TITLE);
                e.detail = Some(clip(title, 120));
                e.path = last_cwd.clone();
                out.push(e);
            }
            continue;
        }
        let Some(ts) = ts else { continue };
        if rec.get("type").and_then(Value::as_str) == Some("pr-link") {
            if ts >= since {
                out.extend(pr_link_event(&rec, &session_id, last_ts.unwrap_or(ts), &last_cwd));
            }
            continue;
        }
        last_ts = Some(ts);
        let cwd = rec.get("cwd").and_then(Value::as_str).map(String::from);
        if cwd.is_some() {
            last_cwd = cwd.clone();
        }
        if ts < since {
            continue;
        }
        if turn.end.is_some_and(|end| ts - end > TURN_GAP) {
            close_turn(&mut turn, &mut out);
        }
        let branch = rec.get("gitBranch").and_then(Value::as_str).filter(|b| !b.is_empty()).map(String::from);
        let uuid = rec.get("uuid").and_then(Value::as_str).unwrap_or_default().to_string();
        let sidechain = rec.get("isSidechain").and_then(Value::as_bool).unwrap_or(false);
        let meta = rec.get("isMeta").and_then(Value::as_bool).unwrap_or(false);
        let content = rec.pointer("/message/content");

        match rec.get("type").and_then(Value::as_str) {
            Some("user") => {
                if let Some(Value::Array(blocks)) = content {
                    for b in blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")) {
                        let id = b.get("tool_use_id").and_then(Value::as_str).unwrap_or_default();
                        if let Some(p) = pending.remove(id) {
                            out.push(test_event(&rec, b, p.ts, id, &cwd, &branch));
                        }
                    }
                }
                let text = match content {
                    Some(Value::String(s)) => Some(s.clone()),
                    Some(Value::Array(blocks)) => blocks
                        .iter()
                        .find(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                        .and_then(|b| b.get("text").and_then(Value::as_str))
                        .map(String::from),
                    _ => None,
                };
                if let Some(text) = text.filter(|t| !meta && !sidechain && is_human_prompt(t)) {
                    close_turn(&mut turn, &mut out);
                    let mut e = WorkEvent::new(SOURCE, uuid.clone(), ts, kind::PROMPT);
                    e.detail = Some(clip(&text, 200));
                    e.path = cwd.clone();
                    e.branch = branch.clone();
                    e.meta = json!({ "session": session_id });
                    out.push(e);
                    turn = Turn { prompt_uuid: uuid, start: Some(ts), end: Some(ts), cwd, branch };
                    continue;
                }
            }
            Some("assistant") => {
                if let Some(Value::Array(blocks)) = content {
                    for b in blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use")) {
                        out.extend(gh_pr_events(b, ts, &cwd));
                        out.push(tool_event(b, ts, &cwd, &branch, &mut pending));
                    }
                }
            }
            _ => {}
        }
        if turn.start.is_some() {
            turn.end = Some(ts);
        }
    }
    close_turn(&mut turn, &mut out);
    Ok(out)
}

fn tool_event(
    b: &Value,
    ts: DateTime<Utc>,
    cwd: &Option<String>,
    branch: &Option<String>,
    pending: &mut HashMap<String, PendingTest>,
) -> WorkEvent {
    let id = b.get("id").and_then(Value::as_str).unwrap_or_default();
    let name = b.get("name").and_then(Value::as_str).unwrap_or("tool");
    let input = b.get("input").cloned().unwrap_or_default();
    if name == "Bash" && input.get("command").and_then(Value::as_str).is_some_and(is_test_command) {
        pending.insert(id.to_string(), PendingTest { ts });
    }
    let mut e = WorkEvent::new(SOURCE, id, ts, kind::TOOL_USE);
    e.detail = Some(name.to_string());
    if let Some((key, action)) = jira_tool_target(name, &input) {
        e.meta = json!({ "ticket": key, "ticket_action": action });
    }
    e.path = input
        .get("file_path")
        .or_else(|| input.get("notebook_path"))
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| cwd.clone());
    e.branch = branch.clone();
    e
}

/// The pull requests a Bash tool call changed or read with `gh pr`.
fn gh_pr_events(b: &Value, ts: DateTime<Utc>, cwd: &Option<String>) -> Vec<WorkEvent> {
    let Some(command) = b.pointer("/input/command").and_then(Value::as_str).filter(|_| b.get("name").and_then(Value::as_str) == Some("Bash"))
    else {
        return Vec::new();
    };
    let call_id = b.get("id").and_then(Value::as_str).unwrap_or_default();
    shell_refs::pr_commands(command)
        .iter()
        .map(|pr| shell_refs::pr_event(PrEvent { source: SOURCE, call_id, pr, ts, path: cwd }))
        .collect()
}

/// A pull request the session opened or pushed to, recorded by Claude Code as a `pr-link`.
/// `detail` is `repo#N` in lowercase, the form tracker keys take. `ts` is the session's last
/// activity before the record, so a link written when a session resumes adds no time of its own.
fn pr_link_event(rec: &Value, session_id: &str, ts: DateTime<Utc>, cwd: &Option<String>) -> Option<WorkEvent> {
    let number = rec.get("prNumber").and_then(Value::as_i64)?;
    let repo = rec.get("prRepository").and_then(Value::as_str)?;
    let name = repo.rsplit('/').next().filter(|n| !n.is_empty())?;
    let mut e = WorkEvent::new(SOURCE, format!("{session_id}:pr:{repo}#{number}"), ts, kind::PR_LINK);
    e.detail = Some(format!("{}#{number}", name.to_lowercase()));
    e.path = cwd.clone();
    e.meta = json!({ "repo": repo.to_lowercase(), "number": number });
    Some(e)
}

/// The Jira key a Jira tool call acted on, and whether it changed the ticket (`write`:
/// transition, edit, comment, worklog) or only read it (`read`). Tools that touch several
/// tickets at once, such as a search or a link between two, name no single ticket.
fn jira_tool_target(name: &str, input: &Value) -> Option<(String, &'static str)> {
    let action = name.rsplit("__").next().unwrap_or(name);
    if !action.to_lowercase().contains("jira") {
        return None;
    }
    let key = input.get("issueIdOrKey").or_else(|| input.get("issueKey")).and_then(Value::as_str)?.trim().to_uppercase();
    if !crate::tickets::is_jira_key(&key) {
        return None;
    }
    let writes = ["transition", "edit", "update", "addcomment", "addworklog"];
    let lower = action.to_lowercase();
    Some((key, if writes.iter().any(|w| lower.starts_with(w)) { "write" } else { "read" }))
}

fn test_event(
    rec: &Value,
    block: &Value,
    started: DateTime<Utc>,
    tool_id: &str,
    cwd: &Option<String>,
    branch: &Option<String>,
) -> WorkEvent {
    let output = block.get("content").map(content_text).unwrap_or_default();
    let (pass, fail) = parse_test_counts(&output);
    let is_error = block.get("is_error").and_then(Value::as_bool).unwrap_or(false);
    let ok = !is_error && fail == 0;
    let ts = rec.get("timestamp").and_then(Value::as_str).and_then(|t| t.parse().ok()).unwrap_or(started);
    let mut e =
        WorkEvent::new(SOURCE, format!("{tool_id}:result"), ts, if ok { kind::TEST_PASSED } else { kind::TEST_FAILED });
    e.detail = Some("agent test run".into());
    e.path = cwd.clone();
    e.branch = branch.clone();
    e.meta = json!({ "pass": pass, "fail": if ok { 0 } else { fail.max(1) } });
    e
}

/// An agent turn runs from a human prompt to the last record before the next prompt.
fn close_turn(turn: &mut Turn, out: &mut Vec<WorkEvent>) {
    if let (Some(start), Some(end)) = (turn.start, turn.end) {
        let mut e = WorkEvent::new(SOURCE, format!("{}:turn", turn.prompt_uuid), start, kind::AGENT_TURN);
        e.path = turn.cwd.clone();
        e.branch = turn.branch.clone();
        e.meta = json!({ "dur_ms": (end - start).num_milliseconds().clamp(0, super::MAX_TURN_MS) });
        out.push(e);
    }
    *turn = Turn::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_lines(name: &str, lines: &[Value]) -> Vec<WorkEvent> {
        let path = std::env::temp_dir().join(format!("devarch-cc-{name}-{}.jsonl", std::process::id()));
        std::fs::write(&path, lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
        let out = parse_file(&path, DateTime::<Utc>::MIN_UTC).unwrap();
        std::fs::remove_file(&path).ok();
        out
    }

    #[test]
    fn reads_pr_links_once_per_pull_request() {
        let link = |ts: &str| json!({"type": "pr-link", "prNumber": 210, "prRepository": "acme/Shop-Admin", "timestamp": ts});
        let events = parse_lines("pr", &[
            json!({"type": "user", "timestamp": "2026-10-21T13:00:00Z", "uuid": "u1", "cwd": "/r/shop-admin", "message": {"content": "open the PR"}}),
            link("2026-10-21T13:59:21Z"),
            link("2026-10-21T13:59:32Z"),
        ]);
        let prs: Vec<&WorkEvent> = events.iter().filter(|e| e.kind == kind::PR_LINK).collect();
        assert_eq!(prs.len(), 2, "the store keeps one: both share a source id");
        assert_eq!(prs[0].source_id, prs[1].source_id);
        assert_eq!(prs[0].detail.as_deref(), Some("shop-admin#210"));
        assert_eq!(prs[0].ts, "2026-10-21T13:00:00Z".parse::<DateTime<Utc>>().unwrap(), "placed at the last activity");
        assert_eq!(prs[0].path.as_deref(), Some("/r/shop-admin"));
    }

    #[test]
    fn tags_jira_tool_calls_with_their_ticket() {
        let target = |name: &str, input: Value| jira_tool_target(name, &input);
        assert_eq!(target("mcp__claude_ai_Atlassian__transitionJiraIssue", json!({"issueIdOrKey": "SHOP-12"})), Some(("SHOP-12".into(), "write")));
        assert_eq!(target("mcp__claude_ai_Atlassian__addCommentToJiraIssue", json!({"issueIdOrKey": "shop-12"})), Some(("SHOP-12".into(), "write")));
        assert_eq!(target("mcp__claude_ai_Atlassian__getJiraIssue", json!({"issueIdOrKey": "SHOP-12"})), Some(("SHOP-12".into(), "read")));
        assert_eq!(target("mcp__claude_ai_Atlassian__getJiraIssue", json!({"issueIdOrKey": "10042"})), None);
        assert_eq!(target("mcp__claude_ai_Atlassian__searchJiraIssuesUsingJql", json!({"jql": "key = SHOP-12"})), None);
        assert_eq!(target("Read", json!({"issueIdOrKey": "SHOP-12"})), None);
    }
}
