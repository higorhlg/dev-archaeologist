use serde::{Deserialize, Serialize};

use crate::sessions::Session;

/// A user correction that applies to future sessions. Rules run before the AI label,
/// so a rule always wins over an inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Rule {
    ProjectForPath { prefix: String, project: String },
    LabelForBranch { project: String, branch: String, label: String },
    ExcludeApp { app: String },
}

impl Rule {
    pub fn kind(&self) -> &'static str {
        match self {
            Rule::ProjectForPath { .. } => "project_for_path",
            Rule::LabelForBranch { .. } => "label_for_branch",
            Rule::ExcludeApp { .. } => "exclude_app",
        }
    }

    pub fn project_for(&self, path: &str) -> Option<String> {
        match self {
            Rule::ProjectForPath { prefix, project } if path.starts_with(prefix.as_str()) => Some(project.clone()),
            _ => None,
        }
    }

    pub fn excludes_app(&self, app: &str) -> bool {
        matches!(self, Rule::ExcludeApp { app: a } if a == app)
    }
}

/// Gives an unlabelled session the label of a matching `LabelForBranch` rule.
pub fn apply_label_rules(s: &mut Session, rules: &[Rule]) {
    if s.label.is_some() {
        return;
    }
    for r in rules {
        if let Rule::LabelForBranch { project, branch, label } = r {
            if s.project.as_deref() == Some(project.as_str()) && s.branch.as_deref() == Some(branch.as_str()) {
                s.label = Some(label.clone());
                s.state = "inferred".into();
                s.confidence = Some("high".into());
            }
        }
    }
}
