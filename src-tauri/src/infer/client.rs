//! One Messages API call that must answer in a JSON schema.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::models;

const API_URL: &str = "https://api.anthropic.com/v1/messages";

pub struct StructuredCall<'a> {
    pub model: &'a str,
    pub key: &'a str,
    pub system: &'a str,
    /// The user turn, usually a JSON document.
    pub input: String,
    pub schema: Value,
    pub max_tokens: u32,
    /// `low`, `medium` or `high`.
    pub effort: &'a str,
    pub timeout_secs: u64,
}

/// Sends the call and parses the first text block as `T`. Uses server-side model fallback
/// when the model supports it. A refusal or a cut-off answer is an error.
pub fn call_structured<T: DeserializeOwned>(c: StructuredCall) -> Result<T> {
    let fallback = models::supports_default_fallback(c.model);
    let mut body = json!({
        "model": c.model,
        "max_tokens": c.max_tokens,
        "system": c.system,
        "output_config": {"effort": c.effort, "format": {"type": "json_schema", "schema": c.schema}},
        "messages": [{"role": "user", "content": c.input}],
    });
    if fallback {
        body["fallbacks"] = json!("default");
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(c.timeout_secs)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut req = agent
        .post(API_URL)
        .header("x-api-key", c.key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json");
    if fallback {
        req = req.header("anthropic-beta", models::FALLBACK_BETA);
    }
    let mut resp = req.send_json(&body).context("call the Claude API")?;
    let status = resp.status().as_u16();
    let v: Value = resp.body_mut().read_json()?;
    if status != 200 {
        let msg = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("unknown error");
        bail!("Claude API returned {status}: {msg}");
    }
    match v.get("stop_reason").and_then(Value::as_str) {
        Some("refusal") => bail!("the model declined this request"),
        Some("max_tokens") => bail!("the answer was cut off at max_tokens"),
        _ => {}
    }
    let text = v["content"]
        .as_array()
        .and_then(|blocks| blocks.iter().find(|b| b["type"] == "text"))
        .and_then(|b| b["text"].as_str())
        .ok_or_else(|| anyhow!("no text block in the response"))?;
    Ok(serde_json::from_str(text)?)
}
