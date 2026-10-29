//! Toggl Track API v9 client: account lookup, projects, and creating time entries.
//! Optional. CSV export works without it.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::http::{self, KeychainItem};
use crate::reports::{exportable, Piece, Report, ReportEntry};
use crate::store::Store;

const BASE: &str = "https://api.track.toggl.com/api/v9";
const TOKEN: KeychainItem = KeychainItem("toggl-api-token");

pub fn token() -> Option<String> {
    TOKEN.get()
}

pub fn save_token(token: &str) -> Result<()> {
    TOKEN.set(token)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TogglAccount {
    pub email: String,
    pub workspace_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TogglProjectInfo {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub active: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct SendResult {
    pub sent: usize,
    pub already_sent: usize,
    pub failed: Vec<String>,
}

fn agent() -> ureq::Agent {
    http::agent(30)
}

fn basic_auth(token: &str) -> String {
    http::basic_auth(token, "api_token")
}

fn read(resp: ureq::http::Response<ureq::Body>) -> Result<Value> {
    http::read_json(resp, "Toggl")
}

fn get(token: &str, path: &str) -> Result<Value> {
    read(agent().get(format!("{BASE}{path}")).header("Authorization", basic_auth(token)).call()?)
}

pub fn account(token: &str) -> Result<TogglAccount> {
    let me = get(token, "/me")?;
    Ok(TogglAccount {
        email: me["email"].as_str().unwrap_or_default().to_string(),
        workspace_id: me["default_workspace_id"].as_i64().ok_or_else(|| anyhow!("Toggl account has no default workspace"))?,
    })
}

pub fn projects(token: &str, workspace_id: i64) -> Result<Vec<TogglProjectInfo>> {
    let v = get(token, &format!("/workspaces/{workspace_id}/projects?active=true&per_page=200"))?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

fn create_entry(token: &str, workspace_id: i64, e: &ReportEntry, piece: Piece, project_id: Option<i64>) -> Result<i64> {
    let start = Utc.timestamp_millis_opt(piece.start_ts).single().ok_or_else(|| anyhow!("bad start time"))?;
    let body = json!({
        "created_with": "Dev Archaeologist",
        "workspace_id": workspace_id,
        "start": start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "duration": piece.duration_min * 60,
        "description": e.description,
        "billable": e.billable,
        "tags": e.tags,
        "project_id": project_id,
    });
    let resp = agent()
        .post(format!("{BASE}/workspaces/{workspace_id}/time_entries"))
        .header("Authorization", basic_auth(token))
        .send_json(&body)?;
    read(resp)?["id"].as_i64().ok_or_else(|| anyhow!("Toggl did not return an entry id"))
}

/// Creates one Toggl entry per piece of `e`.
/// @returns the first Toggl entry's id.
fn create_pieces(token: &str, workspace_id: i64, e: &ReportEntry, project_id: Option<i64>) -> Result<i64> {
    let ids = e.blocks().into_iter().map(|piece| create_entry(token, workspace_id, e, piece, project_id)).collect::<Result<Vec<_>>>()?;
    ids.first().copied().ok_or_else(|| anyhow!("entry has no time"))
}

/// Sends approved entries that were never sent. An entry split around meetings becomes one Toggl
/// entry per piece. A sent entry is recorded and never sent twice.
/// Projects resolve by mapped id, else by a case-insensitive name match in the workspace.
pub fn send(store: &Store, report: &Report, workspace_id: i64) -> Result<SendResult> {
    let token = token().ok_or_else(|| anyhow!("Add a Toggl API token in Sources first"))?;
    let settings = crate::settings::Settings::load(store)?;
    let by_name: HashMap<String, i64> =
        projects(&token, workspace_id)?.into_iter().map(|p| (p.name.to_lowercase(), p.id)).collect();
    let mut result = SendResult::default();
    for e in exportable(report) {
        if e.sent {
            result.already_sent += 1;
            continue;
        }
        let mapped_id = e.project.as_ref().and_then(|p| settings.toggl.project_map.get(p)).and_then(|m| m.id);
        let project_id = mapped_id.or_else(|| by_name.get(&e.toggl_project.to_lowercase()).copied());
        match create_pieces(&token, workspace_id, e, project_id) {
            Ok(id) => {
                store.record_toggl_sent(&e.key, id, e.duration_min, &e.description)?;
                result.sent += 1;
            }
            Err(err) => result.failed.push(format!("{} {}: {err:#}", e.day, e.description)),
        }
    }
    Ok(result)
}
