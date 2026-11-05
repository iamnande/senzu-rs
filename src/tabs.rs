//! which beans have a zellij tab open, and whether that session has written
//! the bean since. a tab is named after its bean's slug
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use serde::Deserialize;

use crate::config::Env;
use crate::launch;

/// a bean with a live tab
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Live {
    /// the bean changed since the tab opened, or we can't tell when it opened
    Written,
    /// nothing written since the tab opened. closing it now loses the session's work
    Quiet,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum TabsError {
    #[error("zellij not found on PATH")]
    NotOnPath,
    #[error("zellij {0}: {1}")]
    Spawn(String, std::io::Error),
    #[error("zellij {0} exited with {1}")]
    Status(String, std::process::ExitStatus),
    #[error("zellij list-tabs: {0}")]
    Parse(serde_json::Error),
}

/// what we want out of each tab zellij lists
#[derive(Debug, Deserialize)]
pub(crate) struct Tab {
    #[serde(rename = "tab_id")]
    pub(crate) id: u64,
    pub(crate) name: String,
    /// the tiled panes you can focus, so not the bars
    #[serde(rename = "selectable_tiled_panes_count")]
    pub(crate) panes: usize,
}

/// the tabs in this zellij session
pub(crate) fn list(env: &Env) -> Result<Vec<Tab>, TabsError> {
    let out = zellij(env, &["list-tabs", "--json", "--panes"])?;
    parse(&out)
}

/// close one tab, and everything in it
pub(crate) fn close(env: &Env, id: u64) -> Result<(), TabsError> {
    zellij(env, &["close-tab-by-id", &id.to_string()]).map(drop)
}

/// the open tab names, for planning a launch. none outside zellij, and none
/// when zellij won't say, which costs at worst a second tab for the bean
pub(crate) fn open_names(env: &Env) -> Vec<String> {
    if !env.in_zellij {
        return Vec::new();
    }
    match list(env) {
        Ok(tabs) => tabs.into_iter().map(|t| t.name).collect(),
        Err(e) => {
            tracing::info!(error = %e, outcome = "planned as if no tabs are open", "list tabs");
            Vec::new()
        }
    }
}

/// `zellij action <args>`, found on our PATH like the launch is, so a test's
/// fake stands in and the real one never runs by accident
fn zellij(env: &Env, args: &[&str]) -> Result<Vec<u8>, TabsError> {
    let what = args.first().copied().unwrap_or_default();
    let program =
        launch::find_on_path("zellij", env.path.as_deref()).ok_or(TabsError::NotOnPath)?;
    let out = Command::new(program)
        .arg("action")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| TabsError::Spawn(what.to_owned(), e))?;
    if !out.status.success() {
        return Err(TabsError::Status(what.to_owned(), out.status));
    }
    Ok(out.stdout)
}

fn parse(json: &[u8]) -> Result<Vec<Tab>, TabsError> {
    serde_json::from_slice(json).map_err(TabsError::Parse)
}

/// the launcher touches this when it opens a tab, so its mtime is the open time
pub(crate) fn marker(state_dir: &Path, slug: &str) -> PathBuf {
    state_dir.join("tabs").join(slug)
}

/// where the beans and the tab markers live
#[derive(Clone, Copy)]
pub(crate) struct Dirs<'a> {
    pub(crate) beans: &'a Path,
    pub(crate) state: &'a Path,
}

/// the beans among `slugs` with a tab in `tabs`, and whether each has been written
pub(crate) fn live<'a>(
    tabs: &[String],
    slugs: impl IntoIterator<Item = &'a str>,
    dirs: Dirs,
) -> HashMap<String, Live> {
    slugs
        .into_iter()
        .filter(|slug| tabs.iter().any(|t| t == slug))
        .map(|slug| {
            let opened = mtime(&marker(dirs.state, slug));
            let written = mtime(&dirs.beans.join(format!("{slug}.json")));
            let state = match (opened, written) {
                (Some(opened), Some(written)) if written <= opened => Live::Quiet,
                _ => Live::Written,
            };
            (slug.to_owned(), state)
        })
        .collect()
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[cfg(test)]
mod tests {
    use std::fs::{File, FileTimes};
    use std::time::Duration;

    use super::*;

    #[test]
    fn parse_takes_ids_names_and_pane_counts() {
        let json = br#"[{"position":0,"name":"senzu","active":true,"tab_id":0,"selectable_tiled_panes_count":1},
                        {"position":1,"name":"crab-shell","active":false,"tab_id":3,"selectable_tiled_panes_count":2}]"#;
        let got: Vec<_> = parse(json)
            .unwrap()
            .into_iter()
            .map(|t| (t.id, t.name, t.panes))
            .collect();
        assert_eq!(got, [(0, "senzu".into(), 1), (3, "crab-shell".into(), 2)]);
        assert!(parse(b"not json").is_err());
    }

    /// set a file's mtime to `secs` after the epoch
    fn touch(path: &Path, secs: u64) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = File::create(path).unwrap();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        file.set_times(FileTimes::new().set_modified(at)).unwrap();
    }

    #[test]
    fn live_marks_tabs_quiet_until_the_bean_changes() {
        let beans = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let tabs = vec![
            "senzu".to_owned(),
            "crab-shell".into(),
            "harden-the-hut".into(),
            "kelp".into(),
        ];
        // opened at 100, bean last written at 50: quiet
        touch(&marker(state.path(), "crab-shell"), 100);
        touch(&beans.path().join("crab-shell.json"), 50);
        // opened at 100, written at 150: written
        touch(&marker(state.path(), "harden-the-hut"), 100);
        touch(&beans.path().join("harden-the-hut.json"), 150);
        // a tab opened by hand has no marker, so we can't say it's quiet
        touch(&beans.path().join("kelp.json"), 50);

        let slugs = ["crab-shell", "harden-the-hut", "kelp", "tidy-the-tidepool"];
        let dirs = Dirs {
            beans: beans.path(),
            state: state.path(),
        };
        let got = live(&tabs, slugs, dirs);
        assert_eq!(got.len(), 3, "no tab, not live: {got:?}");
        assert_eq!(got["crab-shell"], Live::Quiet);
        assert_eq!(got["harden-the-hut"], Live::Written);
        assert_eq!(got["kelp"], Live::Written);
    }
}
