use std::fmt::Write as _;
use std::path::Path;

use serde::Serialize;

use crate::bean::{Bean, Phase, Priority};

/// what `ls` keeps: the filters the flags asked for
#[derive(Debug, Default)]
pub(crate) struct Filter {
    pub(crate) all: bool,
    pub(crate) phase: Option<Phase>,
    pub(crate) priority: Option<Priority>,
}

impl Filter {
    /// done beans stay out unless `--phase done` asks for them. urgent and high
    /// by default; `--all` or an explicit `--priority` replaces that.
    pub(crate) fn keeps(&self, bean: &Bean) -> bool {
        let phase_ok = match self.phase {
            Some(phase) => bean.phase == phase,
            None => bean.phase != Phase::Done,
        };
        let priority_ok = match self.priority {
            Some(priority) => bean.priority == priority,
            None => self.all || bean.priority <= Priority::High,
        };
        phase_ok && priority_ok
    }
}

/// filtered, then sorted by (priority, slug)
pub(crate) fn select<'a>(beans: &'a [Bean], filter: &Filter) -> Vec<&'a Bean> {
    let mut out: Vec<&Bean> = beans.iter().filter(|b| filter.keeps(b)).collect();
    out.sort_by(|a, b| order(a, b));
    out
}

/// priority first, then slug
pub(crate) fn order(a: &Bean, b: &Bean) -> std::cmp::Ordering {
    (a.priority, &a.slug).cmp(&(b.priority, &b.slug))
}

/// one `ls` row as json
#[derive(Debug, Serialize)]
pub(crate) struct Row<'a> {
    pub(crate) slug: &'a str,
    pub(crate) title: &'a str,
    pub(crate) phase: Phase,
    pub(crate) priority: Priority,
    pub(crate) next: &'a str,
    pub(crate) worktree: Option<&'a Path>,
}

impl<'a> From<&'a Bean> for Row<'a> {
    fn from(b: &'a Bean) -> Self {
        Self {
            slug: &b.slug,
            title: &b.title,
            phase: b.phase,
            priority: b.priority,
            next: &b.next.text,
            worktree: b.worktree(),
        }
    }
}

/// `slug  priority  phase  next`, columns padded, each line cut to `width`
pub(crate) fn ls_table(beans: &[&Bean], width: usize) -> String {
    let slug_w = beans
        .iter()
        .map(|b| b.slug.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for b in beans {
        let line = format!(
            "{:slug_w$}  {:7}  {:7}  {}",
            b.slug,
            b.priority.to_string(),
            b.phase.to_string(),
            b.next.text,
        );
        out.push_str(&truncate(line.trim_end(), width));
        out.push('\n');
    }
    out
}

/// the 3-line view, for the current stream
pub(crate) fn brief(bean: &Bean, home: &Path, fallback: &Path) -> String {
    format!(
        "{} · {} · {}\nnext: {}\nworktree: {}\n",
        bean.slug,
        bean.phase,
        bean.priority,
        bean.next.text,
        worktree_label(bean, home, fallback),
    )
}

/// header fields, open items, the last log line
pub(crate) fn show(bean: &Bean, home: &Path, fallback: &Path) -> String {
    let mut out = brief(bean, home, fallback);
    let _ = writeln!(out, "title: {}", bean.title);
    if let Some(note) = &bean.priority_note {
        let _ = writeln!(out, "priority note: {note}");
    }
    if let Some(issue) = &bean.issue {
        let _ = writeln!(out, "issue: {issue}");
    }
    for pr in &bean.prs {
        let _ = writeln!(out, "pr: {pr}");
    }
    if let Some(plan) = &bean.plan {
        for (label, path) in [("prd", &plan.prd), ("spec", &plan.spec)] {
            if let Some(path) = path {
                let _ = writeln!(out, "{label}: {}", tildify(path, home));
            }
        }
        if let Some(page) = &plan.page {
            let _ = writeln!(out, "page: {page}");
        }
    }
    if !bean.open.is_empty() {
        out.push_str("open:\n");
        for item in &bean.open {
            let blocks = if item.blocks { "[blocks] " } else { "" };
            let draft = match &item.draft {
                Some(id) => format!(" (draft: {id})"),
                None => String::new(),
            };
            let _ = writeln!(out, "- {blocks}{}{draft}", item.text);
        }
    }
    if let Some(last) = bean.log.last() {
        let _ = writeln!(out, "last: {} {}: {}", last.date, last.phase, last.text);
    }
    out
}

/// the same words the tui uses
fn worktree_label(bean: &Bean, home: &Path, fallback: &Path) -> String {
    match bean.worktree() {
        Some(path) => tildify(path, home),
        None => format!("none, opens in {}", tildify(fallback, home)),
    }
}

/// `/home/crab/x` → `~/x`
pub(crate) fn tildify(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

/// cut to `width` characters, with an ellipsis when something got cut
fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bean(slug: &str, priority: Priority, next: &str) -> Bean {
        serde_json::from_value(serde_json::json!({
            "schema": 1, "slug": slug, "title": "t", "phase": "plan", "priority": priority,
            "issue": null, "thread": null, "plan": null,
            "next": { "text": next, "draft": null },
        }))
        .unwrap()
    }

    #[test]
    fn table_pads_slugs_into_one_column() {
        let a = bean("crab", Priority::Urgent, "/iam-plan crab");
        let b = bean("hermit-crab", Priority::High, "/iam-plan hermit-crab");
        let got = ls_table(&[&a, &b], 200);
        assert_eq!(
            got,
            "crab         urgent   plan     /iam-plan crab\n\
             hermit-crab  high     plan     /iam-plan hermit-crab\n"
        );
    }

    #[test]
    fn table_cuts_lines_to_the_width() {
        let a = bean(
            "crab",
            Priority::Urgent,
            "a very long next step that goes on",
        );
        let got = ls_table(&[&a], 30);
        assert_eq!(got, "crab  urgent   plan     a ver…\n");
        assert_eq!(got.trim_end().chars().count(), 30);
    }

    #[test]
    fn truncate_counts_chars_not_bytes() {
        assert_eq!(truncate("ünïcödé", 7), "ünïcödé");
        assert_eq!(truncate("ünïcödé", 4), "ünï…");
    }

    #[test]
    fn tildify_only_strips_a_real_home() {
        let home = Path::new("/home/crab");
        assert_eq!(tildify(Path::new("/home/crab/beans"), home), "~/beans");
        assert_eq!(tildify(Path::new("/home/crabby"), home), "/home/crabby");
        assert_eq!(tildify(Path::new("/x"), Path::new("")), "/x");
    }

    #[test]
    fn show_lists_open_items_and_the_last_log_line() {
        let mut b = bean("crab", Priority::Urgent, "next up");
        b.open = vec![
            serde_json::from_value(
                serde_json::json!({"text": "send it", "blocks": true, "draft": null}),
            )
            .unwrap(),
        ];
        b.log = vec![
            serde_json::from_value(
                serde_json::json!({"date": "2026-11-01", "phase": "ground", "text": "first"}),
            )
            .unwrap(),
            serde_json::from_value(
                serde_json::json!({"date": "2026-11-03", "phase": "plan", "text": "second"}),
            )
            .unwrap(),
        ];
        let got = show(&b, Path::new("/home/crab"), Path::new("/home/crab/senzu"));
        assert!(got.contains("open:\n- [blocks] send it\n"), "{got}");
        assert!(got.ends_with("last: 2026-11-03 plan: second\n"), "{got}");
    }

    #[test]
    fn no_worktree_says_where_it_opens_and_skips_what_isnt_there() {
        let b = bean("crab", Priority::Urgent, "/iam-plan crab");
        let got = show(&b, Path::new("/home/crab"), Path::new("/home/crab/senzu"));
        assert!(got.contains("worktree: none, opens in ~/senzu\n"), "{got}");
        for absent in ["issue:", "priority note:", "prd:", "page:", "open:", "null"] {
            assert!(!got.contains(absent), "{absent} in {got}");
        }
    }
}
