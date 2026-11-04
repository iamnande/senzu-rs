//! the bean schema: one stream of work per `<slug>.json`. these types are the
//! format, so the skills that write beans, `senzu check` and anything else
//! that reads them share this one definition.
//!
//! every bean says which `schema` it was written for. only a breaking change
//! (a field renamed, removed or retyped) bumps it. a new field doesn't: an
//! older senzu reads past it with a warning, and `check` flags it.
//!
//! terms: a fact is sourced, a decision is settled, an open item is
//! unresolved, a draft is work product (an email, a script, a map), a log
//! entry is something that happened.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

/// the schema this senzu reads. a bean with any other one is refused
pub const SCHEMA: u32 = 1;

/// one stream of work, one `<slug>.json` in the beans dir
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bean {
    /// the schema it was written for, see [`SCHEMA`]
    pub schema: u32,
    /// lowercase-kebab, and the same as the file name
    pub slug: String,
    /// one line on what the stream is
    pub title: String,
    /// where the stream is in the senzu phases
    pub phase: Phase,
    /// how soon it matters, which is also the sort order
    pub priority: Priority,
    /// why the priority is what it is, when that needs saying
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority_note: Option<String>,
    /// a tracker id, as a plain string. senzu doesn't talk to trackers
    pub issue: Option<String>,
    /// a permalink to the thread the stream lives in, wherever that is
    pub thread: Option<String>,
    /// the repos it touches. the first one that exists is where the agent opens
    #[serde(default)]
    pub repos: Vec<Repo>,
    /// where the prd, spec and review page are
    pub plan: Option<Plan>,
    /// pull request urls
    #[serde(default)]
    pub prs: Vec<String>,
    /// what happens next
    pub next: Next,
    /// what's known, and from where
    #[serde(default)]
    pub context: Vec<Fact>,
    /// what's settled, and by whom
    #[serde(default)]
    pub decisions: Vec<Decision>,
    /// what's still unresolved
    #[serde(default)]
    pub open: Vec<Open>,
    /// work product the stream carries, linked from `open` and `next`
    #[serde(default)]
    pub drafts: Vec<Draft>,
    /// what happened, oldest first
    #[serde(default)]
    pub log: Vec<LogEntry>,
}

/// something known, with where it came from
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// the fact
    pub text: String,
    /// who or what said so: "the crab", "checked", "the old script"
    pub source: String,
    /// when it was true
    pub date: Date,
}

/// something settled
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    /// the decision
    pub text: String,
    /// who made it
    pub by: String,
    /// when
    pub date: Date,
}

/// something unresolved
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Open {
    /// the question or task
    pub text: String,
    /// whether the stream can't move until it's done
    pub blocks: bool,
    /// the id of a draft that goes with it
    pub draft: Option<String>,
}

/// what happens next: a skill call for most beans, a dated chore for life ones
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Next {
    /// the step
    pub text: String,
    /// the id of a draft that goes with it
    pub draft: Option<String>,
}

/// work product: an email, a script, a map
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    /// lowercase-kebab, unique in the bean, what `open` and `next` link by
    pub id: String,
    /// a name to show
    pub title: String,
    /// when it was written
    pub date: Date,
    /// markdown
    pub body: String,
}

/// a repo the stream works in
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    /// absolute. the first one that exists is where the agent launches
    pub path: PathBuf,
    /// a short name: "senzu-rs", "crab-rs"
    pub repo: String,
    /// the branch the work is on
    pub branch: Option<String>,
    /// what the branch is based on: "origin/main 244ffc6"
    pub base: Option<String>,
    /// anything else worth knowing about it
    pub note: Option<String>,
}

/// where the plan docs are
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// the prd
    pub prd: Option<PathBuf>,
    /// the tech spec
    pub spec: Option<PathBuf>,
    /// a rendered review page
    pub page: Option<String>,
    /// anything else worth knowing about it
    pub note: Option<String>,
}

/// something that happened
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// when
    pub date: Date,
    /// the phase it happened in
    pub phase: Phase,
    /// what happened
    pub text: String,
}

/// the senzu phases, in order
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// figuring out what the problem is
    Ground,
    /// the prd and tech spec
    Plan,
    /// building it
    Execute,
    /// making it right
    Refine,
    /// the pull request
    Pr,
    /// shipping it
    Rollout,
    /// nothing left to do
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
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// first, ahead of everything
    Urgent,
    /// soon
    High,
    /// when there's room
    Medium,
    /// someday
    Low,
    /// unranked: the triage queue, rank these
    None,
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Urgent => "urgent",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::None => "none",
        };
        f.write_str(name)
    }
}

impl Bean {
    /// the first repo path that exists on this machine
    #[must_use]
    pub fn worktree(&self) -> Option<&Path> {
        self.repos
            .iter()
            .map(|r| r.path.as_path())
            .find(|p| p.is_dir())
    }

    /// every rule serde can't check on its own. empty means the bean is good
    #[must_use]
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
                out.push(format!("repo path {} isn't absolute", repo.path.display()));
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
#[must_use]
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
