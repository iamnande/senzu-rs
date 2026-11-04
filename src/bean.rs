use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

// terms: a fact is sourced, a decision is settled, an open item is unresolved,
// a draft is work product (an email, a script, a map), a log entry is something that happened.

/// one stream of work, one `<slug>.json` in the beans dir
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bean {
    pub slug: String,
    pub title: String,
    pub phase: Phase,
    pub priority: Priority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority_note: Option<String>,
    pub issue: Option<String>,
    pub thread: Option<String>,
    #[serde(default)]
    pub repos: Vec<Repo>,
    pub plan: Option<Plan>,
    #[serde(default)]
    pub prs: Vec<String>,
    pub next: Next,
    #[serde(default)]
    pub context: Vec<Fact>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub open: Vec<Open>,
    #[serde(default)]
    pub drafts: Vec<Draft>,
    #[serde(default)]
    pub log: Vec<LogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    pub text: String,
    pub source: String,
    pub date: Date,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub text: String,
    pub by: String,
    pub date: Date,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Open {
    pub text: String,
    pub blocks: bool,
    pub draft: Option<String>,
}

/// a skill call for most beans, a dated chore for life ones
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Next {
    pub text: String,
    pub draft: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub id: String,
    pub title: String,
    pub date: Date,
    /// markdown
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repo {
    /// absolute. the first one that exists is where the agent launches
    pub path: PathBuf,
    pub repo: String,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub prd: Option<PathBuf>,
    pub spec: Option<PathBuf>,
    pub page: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogEntry {
    pub date: Date,
    pub phase: Phase,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Ground,
    Plan,
    Execute,
    Refine,
    Pr,
    Rollout,
    Done,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Ground => "ground",
            Self::Plan => "plan",
            Self::Execute => "execute",
            Self::Refine => "refine",
            Self::Pr => "pr",
            Self::Rollout => "rollout",
            Self::Done => "done",
        };
        f.write_str(name)
    }
}

/// derive order is sort order: urgent sorts first
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Urgent,
    High,
    Medium,
    Low,
    None,
}

impl Bean {
    /// every rule serde can't check on its own. empty means the bean is good
    pub fn problems(&self, stem: &str) -> Vec<String> {
        let mut out = Vec::new();
        if self.slug != stem {
            out.push(format!("slug {:?} doesn't match the file name", self.slug));
        }
        if !is_slug(&self.slug) {
            out.push(format!("slug {:?} isn't lowercase-kebab", self.slug));
        }
        for repo in &self.repos {
            if !repo.path.is_absolute() {
                out.push(format!("repo path {:?} isn't absolute", repo.path));
            }
        }
        let mut ids = HashSet::new();
        for draft in &self.drafts {
            if !is_slug(&draft.id) {
                out.push(format!("draft id {:?} isn't lowercase-kebab", draft.id));
            }
            if !ids.insert(draft.id.as_str()) {
                out.push(format!("draft id {:?} is used twice", draft.id));
            }
        }
        let refs = self
            .open
            .iter()
            .filter_map(|o| o.draft.as_deref())
            .chain(self.next.draft.as_deref());
        for id in refs {
            if !ids.contains(id) {
                out.push(format!("draft {id:?} is referenced but doesn't exist"));
            }
        }
        out
    }
}

/// `^[a-z0-9]+(-[a-z0-9]+)*$`, without pulling in a regex crate
pub fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        for good in ["senzu-cli", "a", "r2-d2", "2026-11-03"] {
            assert!(is_slug(good), "{good} should be a slug");
        }
        for bad in ["", "-a", "a-", "a--b", "Senzu", "a_b", "a b", "ä"] {
            assert!(!is_slug(bad), "{bad:?} shouldn't be a slug");
        }
    }

    #[test]
    fn priority_sorts_urgent_first() {
        let mut got = vec![Priority::None, Priority::High, Priority::Urgent];
        got.sort();
        assert_eq!(got, vec![Priority::Urgent, Priority::High, Priority::None]);
    }
}
