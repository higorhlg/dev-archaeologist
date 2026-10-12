pub mod claude_code;
pub mod codex;
pub mod git;

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

/// No single agent turn counts for more than three hours, whatever the log says.
pub const MAX_TURN_MS: i64 = 3 * 60 * 60 * 1000;

/// True when a shell command line runs a test suite. The adapter reads the command only
/// to classify it; the command text is never stored.
pub fn is_test_command(cmd: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"(?x)(^|[\s;&|(])(
                (pnpm|npm|yarn|bun)\s+(run\s+)?test |
                (npx\s+|pnpm\s+(exec\s+)?)?(vitest|jest|mocha|playwright\s+test) |
                pytest | python3?\s+-m\s+(pytest|unittest) |
                cargo\s+(test|nextest) | go\s+test | rspec | phpunit | mix\s+test | dotnet\s+test |
                swift\s+test | xcodebuild\s+test | gradle\w*\s+test | mvn\s+test
            )\b",
        )
        .unwrap()
    })
    .is_match(cmd)
}

/// Reads pass and fail counts from a test runner summary. Only the two integers leave
/// this function; the output text is dropped.
pub fn parse_test_counts(output: &str) -> (i64, i64) {
    static PASS: OnceLock<Regex> = OnceLock::new();
    static FAIL: OnceLock<Regex> = OnceLock::new();
    let pass = PASS.get_or_init(|| Regex::new(r"(?i)\b(\d+)\s+(passed|passing)\b|test result: ok\. (\d+) passed").unwrap());
    let fail = FAIL.get_or_init(|| Regex::new(r"(?i)\b(\d+)\s+(failed|failing|failures?)\b").unwrap());
    let last_num = |re: &Regex| {
        re.captures_iter(output)
            .last()
            .and_then(|c| c.iter().skip(1).flatten().find_map(|m| m.as_str().parse::<i64>().ok()))
            .unwrap_or(0)
    };
    (last_num(pass), last_num(fail))
}

/// Flattens a tool result `content` (a string, or a list of `{type:"text", text}` blocks).
pub fn content_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|i| i.get("text").and_then(Value::as_str).or_else(|| i.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A user message is a real prompt only if a person typed it: harness-injected text
/// starts with a tag or a known marker.
pub fn is_human_prompt(text: &str) -> bool {
    let t = text.trim_start();
    !(t.is_empty()
        || t.starts_with('<')
        || t.starts_with("[Request interrupted")
        || t.starts_with("Caveat:")
        || t.starts_with("# AGENTS.md")
        || t.starts_with("Base directory for this skill"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_test_commands() {
        assert!(is_test_command("cd web && pnpm test -- --run"));
        assert!(is_test_command("cargo test -p core"));
        assert!(is_test_command("npx vitest run"));
        assert!(!is_test_command("git status"));
        assert!(!is_test_command("cat latest.txt"));
    }

    #[test]
    fn parses_counts() {
        assert_eq!(parse_test_counts("Tests  3 failed | 41 passed (44)"), (41, 3));
        assert_eq!(parse_test_counts("test result: ok. 12 passed; 0 failed"), (12, 0));
        assert_eq!(parse_test_counts("===== 2 failed, 10 passed in 1.2s ====="), (10, 2));
    }
}
