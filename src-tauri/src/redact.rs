use std::sync::OnceLock;

use regex::Regex;

use crate::event::{clip, WorkEvent};

const NOISE_DIRS: &[&str] = &[
    "/node_modules/", "/target/", "/dist/", "/build/", "/.next/", "/.git/", "/__pycache__/",
    "/.venv/", "/.turbo/", "/coverage/", "/.cache/", "/DerivedData/",
];
const SECRET_FILE_PATTERNS: &[&str] = &[
    r"(^|/)\.env(\.[^/]*)?$", r"\.pem$", r"\.key$", r"(^|/)auth\.json$", r"(^|/)credentials[^/]*$",
    r"(^|/)id_(rsa|ed25519)[^/]*$", r"\.p12$",
];
const SECRET_TEXT_PATTERNS: &[&str] = &[
    r#"(?i)(api[_-]?key|token|secret|passw(or)?d|bearer)["']?\s*[:=]\s*["']?[^\s"']+"#,
    r"sk-[A-Za-z0-9_-]{20,}",
    r"gh[pousr]_[A-Za-z0-9]{30,}",
    r"AKIA[0-9A-Z]{16}",
    r"xox[abpr]-[A-Za-z0-9-]{10,}",
    r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
    r"(?i)(postgres|mysql|mongodb(\+srv)?|redis)://\S+",
];

/// Privacy settings that apply to every event before it reaches the store.
#[derive(Debug, Clone, Default)]
pub struct Privacy {
    pub excluded_prefixes: Vec<String>,
}

impl Privacy {
    pub fn is_excluded(&self, path: &str) -> bool {
        self.excluded_prefixes.iter().any(|p| path.starts_with(p.as_str()))
    }
}

fn compile(list: &[&str]) -> Vec<Regex> {
    list.iter().map(|p| Regex::new(p).expect("valid redaction regex")).collect()
}

fn secret_files() -> &'static [Regex] {
    static R: OnceLock<Vec<Regex>> = OnceLock::new();
    R.get_or_init(|| compile(SECRET_FILE_PATTERNS))
}

fn secret_text() -> &'static [Regex] {
    static R: OnceLock<Vec<Regex>> = OnceLock::new();
    R.get_or_init(|| compile(SECRET_TEXT_PATTERNS))
}

pub fn is_noise(path: &str) -> bool {
    NOISE_DIRS.iter().any(|d| path.contains(d))
}

pub fn is_secret_file(path: &str) -> bool {
    secret_files().iter().any(|r| r.is_match(path))
}

/// Replaces every secret-looking span with `[redacted]`.
pub fn scrub(text: &str) -> String {
    secret_text().iter().fold(text.to_string(), |acc, r| r.replace_all(&acc, "[redacted]").into_owned())
}

/// Drops noisy or excluded events, hides secret file names and scrubs free text.
/// @returns `None` when the event must not be stored at all.
pub fn redact(mut e: WorkEvent, privacy: &Privacy) -> Option<WorkEvent> {
    if let Some(p) = e.path.as_deref() {
        if privacy.is_excluded(p) || (e.kind == crate::event::kind::FILE_CHANGED && is_noise(p)) {
            return None;
        }
        if is_secret_file(p) {
            e.path = Some("[secret file]".into());
        }
    }
    e.detail = e.detail.map(|d| clip(&scrub(&d), 200));
    Some(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_common_secrets() {
        let out = scrub("set API_KEY=abc123 and sk-ant-0123456789abcdefghijkl then postgres://u:p@h/db");
        assert!(!out.contains("abc123"));
        assert!(!out.contains("sk-ant-0123"));
        assert!(!out.contains("u:p@h"));
    }

    #[test]
    fn hides_env_files_and_drops_noise() {
        assert!(is_secret_file("/x/.env.local"));
        assert!(!is_secret_file("/x/environment.ts"));
        let mut e = WorkEvent::new("fs", "1", chrono::Utc::now(), crate::event::kind::FILE_CHANGED);
        e.path = Some("/r/node_modules/a.js".into());
        assert!(redact(e, &Privacy::default()).is_none());
    }
}
