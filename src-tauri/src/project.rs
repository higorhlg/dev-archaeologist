use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::corrections::Rule;

/// Maps any path to a project key: the name of the main Git repository that holds it.
/// A worktree resolves to its main repository, so `insight-ui-basket` and `insight-ui`
/// share one key. A path outside every repository has no project.
pub struct ProjectResolver {
    cache: Mutex<HashMap<PathBuf, Option<String>>>,
}

impl Default for ProjectResolver {
    fn default() -> Self {
        Self { cache: Mutex::new(HashMap::new()) }
    }
}

impl ProjectResolver {
    /// @param rules user corrections; a `ProjectForPath` rule wins over the Git lookup.
    pub fn resolve(&self, path: &str, rules: &[Rule]) -> Option<String> {
        if let Some(p) = rules.iter().find_map(|r| r.project_for(path)) {
            return Some(p);
        }
        let dir = PathBuf::from(path);
        if let Some(hit) = self.cache.lock().unwrap().get(&dir) {
            return hit.clone();
        }
        let found = find_repo_root(&dir).map(|root| main_repo_name(&root));
        self.cache.lock().unwrap().insert(dir, found.clone());
        found
    }
}

/// Walks up from `start` to the first folder that holds `.git` (a folder or a worktree file).
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut cur = if start.is_file() { start.parent()? } else { start };
    loop {
        if cur.join(".git").exists() {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
        if cur == Path::new("/") || Some(cur) == dirs::home_dir().as_deref() {
            return None;
        }
    }
}

/// For a worktree, `.git` is a file `gitdir: <main>/.git/worktrees/<name>`; follow it.
fn main_repo_name(root: &Path) -> String {
    let dot_git = root.join(".git");
    if dot_git.is_file() {
        if let Ok(text) = std::fs::read_to_string(&dot_git) {
            if let Some(gitdir) = text.trim().strip_prefix("gitdir:") {
                let gitdir = PathBuf::from(gitdir.trim());
                if let Some(main) = gitdir.ancestors().find(|a| a.file_name().is_some_and(|n| n == ".git")) {
                    if let Some(name) = main.parent().and_then(|p| p.file_name()) {
                        return name.to_string_lossy().into_owned();
                    }
                }
            }
        }
    }
    root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}
