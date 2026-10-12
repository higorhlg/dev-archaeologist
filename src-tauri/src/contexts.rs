//! Work or personal: which side of the user's life a project belongs to.
//! Seeded from the user's git config, so `includeIf` work folders and work emails
//! need no setup.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::project::find_repo_root;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Context {
    Work,
    Personal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextRules {
    /// Folder prefixes that hold work. Seeded from `includeIf "gitdir:…"` entries.
    pub work_paths: Vec<String>,
    /// Folder prefixes that hold personal work, checked before the email rule.
    pub personal_paths: Vec<String>,
    /// A repo whose effective `user.email` is one of these is work.
    pub work_emails: Vec<String>,
    /// The user's choice per project; it wins over every rule.
    pub overrides: BTreeMap<String, Context>,
}

impl Default for ContextRules {
    fn default() -> Self {
        let (work_paths, work_emails) = detect_work_identities();
        Self { work_paths, personal_paths: vec![], work_emails, overrides: BTreeMap::new() }
    }
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn expand(p: &str) -> String {
    p.strip_prefix("~/").map(|rest| home().join(rest).to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string())
}

fn git_config(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !v.is_empty()).then_some(v)
}

/// Reads `includeIf.gitdir:<folder>.path <file>` from the global git config. A folder whose
/// included identity differs from the global one is a work folder.
pub fn detect_work_identities() -> (Vec<String>, Vec<String>) {
    let global = git_config(&["config", "--global", "user.email"]).unwrap_or_default().to_lowercase();
    let Some(list) = git_config(&["config", "--global", "--get-regexp", r"^includeif\.gitdir"]) else {
        return (vec![], vec![]);
    };
    let mut paths = Vec::new();
    let mut emails = Vec::new();
    for line in list.lines() {
        let Some((key, file)) = line.split_once(' ') else { continue };
        let Some(folder) = key.strip_prefix("includeif.gitdir:").and_then(|k| k.strip_suffix(".path")) else { continue };
        let Some(email) = git_config(&["config", "-f", &expand(file), "user.email"]).map(|e| e.to_lowercase()) else { continue };
        if email != global {
            paths.push(expand(folder));
            if !emails.contains(&email) {
                emails.push(email);
            }
        }
    }
    (paths, emails)
}

/// The effective `user.email` of a repo, read once per repo per run.
/// True when `path` is `folder` itself or inside it, whatever trailing slashes either has.
fn is_under(path: &str, folder: &str) -> bool {
    let folder = folder.trim_end_matches('/');
    path == folder || path.starts_with(&format!("{folder}/"))
}

fn repo_email(root: &Path) -> Option<String> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap().get(root) {
        return hit.clone();
    }
    let email = git_config(&["-C", &root.to_string_lossy(), "config", "user.email"]).map(|e| e.to_lowercase());
    cache.lock().unwrap().insert(root.to_path_buf(), email.clone());
    email
}

impl ContextRules {
    /// @param path one path where the project's work happened; `None` for blocks the user
    /// assigned by hand, which come from work hours and count as work.
    pub fn resolve(&self, project: &str, path: Option<&str>) -> Context {
        if let Some(c) = self.overrides.get(project) {
            return *c;
        }
        let Some(path) = path else { return Context::Work };
        let under = |prefixes: &[String]| prefixes.iter().any(|p| is_under(path, &expand(p)));
        if under(&self.personal_paths) {
            return Context::Personal;
        }
        if under(&self.work_paths) {
            return Context::Work;
        }
        let email = find_repo_root(Path::new(path)).and_then(|r| repo_email(&r));
        match email {
            Some(e) if self.work_emails.contains(&e) => Context::Work,
            _ => Context::Personal,
        }
    }
}

/// Context for every project, resolved once from one stored path per project.
pub fn project_contexts(rules: &ContextRules, sample_paths: &HashMap<String, String>, projects: &[String]) -> HashMap<String, Context> {
    projects.iter().map(|p| (p.clone(), rules.resolve(p, sample_paths.get(p).map(String::as_str)))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_beats_folder_and_folder_beats_default() {
        let mut rules = ContextRules { work_paths: vec!["/w/".into()], personal_paths: vec![], work_emails: vec![], overrides: BTreeMap::new() };
        assert_eq!(rules.resolve("api", Some("/w/api/src")), Context::Work);
        assert_eq!(rules.resolve("w", Some("/w")), Context::Work, "the work folder itself");
        assert_eq!(rules.resolve("wx", Some("/wx/a")), Context::Personal, "a sibling with a longer name");
        assert_eq!(rules.resolve("blog", Some("/tmp/no-repo-here")), Context::Personal);
        assert_eq!(rules.resolve("Meetings", None), Context::Work);
        rules.overrides.insert("api".into(), Context::Personal);
        assert_eq!(rules.resolve("api", Some("/w/api/src")), Context::Personal);
    }
}
