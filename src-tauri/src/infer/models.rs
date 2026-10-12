//! The Claude models the AI features can use, read from the Models API.

use std::time::Duration;

use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::Value;

const MODELS_URL: &str = "https://api.anthropic.com/v1/models?limit=100";

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    /// Short guidance shown under the picker; empty for models the app has no note for.
    pub note: String,
}

/// Notes for the current models, so the picker can say what each one costs and suits.
fn note_for(id: &str) -> &'static str {
    match id {
        "claude-sonnet-5-5" => "Default. Strong writing at $2 / $10 per million tokens.",
        "claude-opus-5-5" => "Best judgement for labels and summaries. $4 / $20.",
        "claude-haiku-5-5" => "Cheapest at $0.10 / $0.50, with plainer writing.",
        "claude-fable-5-1" => "Most capable and slowest. $10 / $50.",
        _ => "",
    }
}

/// Used when the Models API cannot be reached, so the picker never comes up empty.
pub fn known_models() -> Vec<ModelInfo> {
    [
        ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
        ("claude-opus-5-5", "Claude Opus 5.5"),
        ("claude-haiku-5-5", "Claude Haiku 5.5"),
        ("claude-fable-5-1", "Claude Fable 5.1"),
    ]
    .into_iter()
    .map(|(id, name)| ModelInfo { id: id.into(), display_name: name.into(), note: note_for(id).into() })
    .collect()
}

/// Models the key can use that support structured outputs, which the journal needs.
/// Falls back to `known_models` when the request fails.
pub fn available_models(key: &str) -> Vec<ModelInfo> {
    fetch(key).ok().filter(|m| !m.is_empty()).unwrap_or_else(known_models)
}

fn fetch(key: &str) -> Result<Vec<ModelInfo>> {
    let agent: ureq::Agent =
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).http_status_as_error(false).build().into();
    let mut resp = agent.get(MODELS_URL).header("x-api-key", key).header("anthropic-version", "2023-06-01").call()?;
    if resp.status().as_u16() != 200 {
        bail!("Models API returned {}", resp.status());
    }
    let v: Value = resp.body_mut().read_json()?;
    Ok(v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m.pointer("/capabilities/structured_outputs/supported").and_then(Value::as_bool).unwrap_or(true))
        .filter_map(|m| {
            let id = m["id"].as_str()?.to_string();
            Some(ModelInfo {
                display_name: m["display_name"].as_str().unwrap_or(&id).to_string(),
                note: note_for(&id).into(),
                id,
            })
        })
        .collect())
}

/// Models that accept `fallbacks: "default"`: a request a safety classifier declines
/// re-runs on another model on the server instead of failing.
pub fn supports_default_fallback(model: &str) -> bool {
    matches!(model, "claude-sonnet-5-5" | "claude-opus-5-5" | "claude-opus-5" | "claude-fable-5-1")
}

pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
