use std::path::Path;
use std::process::Command;

use crate::bean::Bean;

/// the beans this dir belongs to: by worktree path first, then by branch name
pub(crate) fn matches<'a>(beans: &'a [Bean], cwd: &Path) -> Vec<&'a Bean> {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_owned());
    let by_path: Vec<&Bean> = beans
        .iter()
        .filter(|b| {
            b.repos.iter().any(|r| {
                let path = r.path.canonicalize().unwrap_or_else(|_| r.path.clone());
                cwd.starts_with(path)
            })
        })
        .collect();
    if !by_path.is_empty() {
        return by_path;
    }
    match branch(&cwd) {
        Some(branch) => by_branch(beans, &branch),
        None => Vec::new(),
    }
}

/// a bean whose slug shows up in the branch, ignoring case
fn by_branch<'a>(beans: &'a [Bean], branch: &str) -> Vec<&'a Bean> {
    let branch = branch.to_lowercase();
    beans
        .iter()
        .filter(|b| branch.contains(b.slug.as_str()))
        .collect()
}

/// symbolic-ref works on a branch with no commits yet, where rev-parse doesn't.
/// a detached head or no git at all means no branch.
fn branch(cwd: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["symbolic-ref", "--short", "-q", "HEAD"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8(out.stdout).ok()?;
    Some(name.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bean(slug: &str) -> Bean {
        serde_json::from_value(serde_json::json!({
            "slug": slug, "title": "t", "phase": "plan", "priority": "high",
            "issue": null, "thread": null, "plan": null,
            "next": { "text": "n", "draft": null },
        }))
        .unwrap()
    }

    #[test]
    fn branch_match_ignores_case() {
        let beans = [bean("crab-shell"), bean("kelp")];
        let got = by_branch(&beans, "crab/CRAB-Shell");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].slug, "crab-shell");
    }
}
