//! Toggl Track API v9 client: account lookup, projects, and creating time entries.
//! Optional. CSV export works without it.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::reports::{exportable, Report, ReportEntry};
use crate::store::Store;

const BASE: &str = "https://api.track.toggl.com/api/v9";
const KEYCHAIN_SERVICE: &str = "dev-archaeologist";
const KEYCHAIN_USER: &str = "toggl-api-token";

pub fn token() -> Option<String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER).ok().and_then(|e| e.get_password().ok()).filter(|t| !t.trim().is_empty())
}

pub fn save_token(token: &str) -> Result<()> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)?.set_password(token.trim())?;
    Ok(())
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
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(30))).http_status_as_error(false).build().into()
}

fn basic_auth(token: &str) -> String {
    use base64_lite::encode;
    format!("Basic {}", encode(format!("{token}:api_token").as_bytes()))
}

fn read(mut resp: ureq::http::Response<ureq::Body>) -> Result<Value> {
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string()?;
    if !(200..300).contains(&status) {
        bail!("Toggl returned {status}: {}", text.trim().chars().take(200).collect::<String>());
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
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

fn create_entry(token: &str, workspace_id: i64, e: &ReportEntry, project_id: Option<i64>) -> Result<i64> {
    let start = Utc.timestamp_millis_opt(e.start_ts).single().ok_or_else(|| anyhow!("bad start time"))?;
    let body = json!({
        "created_with": "Dev Archaeologist",
        "workspace_id": workspace_id,
        "start": start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "duration": e.duration_min * 60,
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

/// Sends approved entries that were never sent. A sent entry is recorded and never sent twice.
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
        match create_entry(&token, workspace_id, e, project_id) {
            Ok(id) => {
                store.record_toggl_sent(&e.key, id, e.duration_min, &e.description)?;
                result.sent += 1;
            }
            Err(err) => result.failed.push(format!("{} {}: {err:#}", e.day, e.description)),
        }
    }
    Ok(result)
}

/// Standard base64 for the Basic auth header; avoids a dependency for one call site.
mod base64_lite {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(input: &[u8]) -> String {
        let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
        for chunk in input.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[cfg(test)]
    #[test]
    fn matches_rfc4648() {
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }
}
