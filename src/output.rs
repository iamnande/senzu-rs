use std::fmt::Write as _;
use std::path::Path;

use serde::Serialize;

use crate::bean::{Bean, Phase, Priority};
use crate::config::{self, Settings};
use crate::load::Envelope;

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
pub(crate) fn brief(bean: &Bean, home: &Path) -> String {
    format!(
        "{} · {} · {}\nnext: {}\nworktree: {}\n",
        bean.slug,
        bean.phase,
        bean.priority,
        bean.next.text,
        worktree_label(bean, home),
    )
}

/// header fields, where it all lives, open items, the last log line
pub(crate) fn show(envelope: &Envelope<'_>, home: &Path) -> String {
    let bean = envelope.bean;
    let mut out = brief(bean, home);
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
    let _ = writeln!(out, "bean: {}", uri_label(&envelope.uri, home));
    for a in &envelope.attachments {
        let _ = writeln!(out, "{}: {}", a.kind, uri_label(&a.uri, home));
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
fn worktree_label(bean: &Bean, home: &Path) -> String {
    match bean.worktree() {
        Some(path) => tildify(path, home),
        None => "none".to_owned(),
    }
}

/// one setting, for `senzu config --json`. an unset one has a `null` value
#[derive(Debug, Serialize)]
pub(crate) struct SettingRow {
    key: &'static str,
    value: Option<String>,
    from: String,
}

/// a setting's value: a path is shown per output, anything else as is
enum Value<'a> {
    Dir(&'a Path),
    Text(Option<&'a str>),
}

fn settings_list(s: &Settings) -> [(&'static str, Value<'_>, &config::Origin); 7] {
    [
        ("beans", Value::Dir(&s.beans), &s.from.beans),
        ("agent", Value::Text(Some(&s.agent)), &s.from.agent),
        ("launch", Value::Text(Some(&s.launch)), &s.from.launch),
        ("state", Value::Dir(&s.state), &s.from.state),
        ("worktrees", Value::Dir(&s.worktrees), &s.from.worktrees),
        (
            "branch_prefix",
            Value::Text(s.branch_prefix.as_deref()),
            &s.from.branch_prefix,
        ),
        (
            "thread_channel",
            Value::Text(s.thread_channel.as_deref()),
            &s.from.thread_channel,
        ),
    ]
}

/// the settings with their layers. paths go out as plain absolute paths, so a
/// skill can paste one into a command
pub(crate) fn setting_rows(s: &Settings) -> Vec<SettingRow> {
    settings_list(s)
        .into_iter()
        .map(|(key, value, from)| SettingRow {
            key,
            value: match value {
                Value::Dir(p) => Some(p.display().to_string()),
                Value::Text(t) => t.map(str::to_owned),
            },
            from: from.uri(),
        })
        .collect()
}

/// `key  value  from`, in columns, paths as tilde paths, unset as `-`
pub(crate) fn settings_table(s: &Settings, home: &Path) -> String {
    let rows: Vec<_> = settings_list(s)
        .into_iter()
        .map(|(key, value, from)| {
            let value = match value {
                Value::Dir(p) => tildify(p, home),
                Value::Text(t) => t.unwrap_or("-").to_owned(),
            };
            (key, value, from.uri())
        })
        .collect();
    let key_w = rows.iter().map(|(k, _, _)| k.len()).max().unwrap_or(0);
    let value_w = rows
        .iter()
        .map(|(_, v, _)| v.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for (key, value, from) in rows {
        let _ = writeln!(out, "{key:key_w$}  {value:value_w$}  {from}");
    }
    out
}

/// a local file as a tilde path, anything else as the uri
fn uri_label(uri: &url::Url, home: &Path) -> String {
    match uri.to_file_path() {
        Ok(path) if uri.scheme() == "file" => tildify(&path, home),
        _ => uri.to_string(),
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
    use crate::bean::Kind;
    use crate::load::Resolved;
    use url::Url;

    fn envelope(bean: &Bean, attachments: Vec<Resolved>) -> Envelope<'_> {
        Envelope {
            bean,
            uri: Url::parse(&format!("file:///home/crab/beans/{}.json", bean.slug)).unwrap(),
            attachments,
        }
    }

    #[test]
    fn settings_table_lines_up_and_tildes_paths() {
        use crate::config::{Origin, Origins};
        let s = Settings {
            beans: "/home/crab/beans".into(),
            agent: "claude".into(),
            launch: "exec".into(),
            state: "/var/senzu".into(),
            worktrees: "/home/crab/wt".into(),
            branch_prefix: Some("crab/".into()),
            thread_channel: None,
            from: Origins {
                beans: Origin::Env("SENZU_BEANS".into()),
                agent: Origin::Default,
                launch: Origin::Flag("--launch".into()),
                state: Origin::Default,
                worktrees: Origin::Default,
                branch_prefix: Origin::Default,
                thread_channel: Origin::Default,
            },
        };
        assert_eq!(
            settings_table(&s, Path::new("/home/crab")),
            "beans           ~/beans     env:SENZU_BEANS\n\
             agent           claude      default:\n\
             launch          exec        flag:--launch\n\
             state           /var/senzu  default:\n\
             worktrees       ~/wt        default:\n\
             branch_prefix   crab/       default:\n\
             thread_channel  -           default:\n"
        );
    }

    #[test]
    fn show_puts_local_files_as_tilde_paths_and_the_rest_as_uris() {
        let b = bean("crab", Priority::Urgent, "next up");
        let attachments = vec![
            Resolved {
                kind: Kind::Plan,
                uri: Url::parse("file:///home/crab/beans/crab.plan.md").unwrap(),
            },
            Resolved {
                kind: Kind::Artifact,
                uri: Url::parse("https://example.com/artifact/crab").unwrap(),
            },
        ];
        let got = show(&envelope(&b, attachments), Path::new("/home/crab"));
        assert!(
            got.contains(
                "bean: ~/beans/crab.json\nplan: ~/beans/crab.plan.md\nartifact: https://example.com/artifact/crab\n"
            ),
            "{got}"
        );
    }

    fn bean(slug: &str, priority: Priority, next: &str) -> Bean {
        serde_json::from_value(serde_json::json!({
            "schema": 2, "slug": slug, "title": "t", "phase": "plan", "priority": priority,
            "issue": null, "thread": null,
            "next": { "text": next, "draft": null },
        }))
        .unwrap()
    }

    #[test]
    fn table_pads_slugs_into_one_column() {
        let a = bean("crab", Priority::Urgent, "/senzu-plan crab");
        let b = bean("hermit-crab", Priority::High, "/senzu-plan hermit-crab");
        let got = ls_table(&[&a, &b], 200);
        assert_eq!(
            got,
            "crab         urgent   plan     /senzu-plan crab\n\
             hermit-crab  high     plan     /senzu-plan hermit-crab\n"
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
        let got = show(&envelope(&b, vec![]), Path::new("/home/crab"));
        assert!(got.contains("open:\n- [blocks] send it\n"), "{got}");
        assert!(got.ends_with("last: 2026-11-03 plan: second\n"), "{got}");
    }

    #[test]
    fn no_worktree_says_where_it_opens_and_skips_what_isnt_there() {
        let b = bean("crab", Priority::Urgent, "/senzu-plan crab");
        let got = show(&envelope(&b, vec![]), Path::new("/home/crab"));
        assert!(got.contains("worktree: none\n"), "{got}");
        for absent in ["issue:", "priority note:", "artifact:", "open:", "null"] {
            assert!(!got.contains(absent), "{absent} in {got}");
        }
    }
}
