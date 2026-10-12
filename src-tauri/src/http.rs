//! Shared HTTP pieces for the outside services: Toggl, Jira and GitHub.

use std::time::Duration;

use anyhow::{bail, Result};
use serde_json::Value;

/// An agent that returns error statuses as responses, so callers can read the body.
pub fn agent(timeout_secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_secs)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// @returns the `Authorization` header value for HTTP Basic auth.
pub fn basic_auth(user: &str, secret: &str) -> String {
    format!("Basic {}", base64(format!("{user}:{secret}").as_bytes()))
}

/// Reads a JSON body. A status outside 2xx becomes an error that names the service.
/// @param service the name shown in the error, such as "Jira".
pub fn read_json(mut resp: ureq::http::Response<ureq::Body>, service: &str) -> Result<Value> {
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string()?;
    if !(200..300).contains(&status) {
        bail!("{service} returned {status}: {}", text.trim().chars().take(200).collect::<String>());
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// Standard base64 (RFC 4648) for Basic auth; avoids a dependency for one use.
pub fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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

/// A secret kept in the macOS Keychain under the app's service name.
pub struct KeychainItem(pub &'static str);

impl KeychainItem {
    const SERVICE: &'static str = "dev-archaeologist";

    pub fn get(&self) -> Option<String> {
        keyring::Entry::new(Self::SERVICE, self.0).ok().and_then(|e| e.get_password().ok()).filter(|t| !t.trim().is_empty())
    }

    pub fn set(&self, value: &str) -> Result<()> {
        keyring::Entry::new(Self::SERVICE, self.0)?.set_password(value.trim())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn matches_rfc4648() {
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
