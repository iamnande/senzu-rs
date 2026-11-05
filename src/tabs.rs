//! which beans have a zellij tab open, and whether that session has written
//! the bean since. a tab is named after its bean's slug
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use serde::Deserialize;

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
    #[error("zellij list-tabs: {0}")]
    Spawn(std::io::Error),
    #[error("zellij list-tabs exited with {0}")]
    Status(std::process::ExitStatus),
    #[error("zellij list-tabs: {0}")]
    Parse(serde_json::Error),
}

/// the one field we want out of each tab
#[derive(Deserialize)]
struct Tab {
    name: String,
}

/// the tab names in this zellij session
pub(crate) fn names() -> Result<Vec<String>, TabsError> {
    let out = Command::new("zellij")
        .args(["action", "list-tabs", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(TabsError::Spawn)?;
    if !out.status.success() {
        return Err(TabsError::Status(out.status));
    }
    parse(&out.stdout)
}

fn parse(json: &[u8]) -> Result<Vec<String>, TabsError> {
    let tabs: Vec<Tab> = serde_json::from_slice(json).map_err(TabsError::Parse)?;
    Ok(tabs.into_iter().map(|t| t.name).collect())
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
    fn parse_wants_only_the_names() {
        let json = br#"[{"position":0,"name":"senzu","active":true,"tab_id":0},
                        {"position":1,"name":"crab-shell","active":false,"tab_id":3}]"#;
        assert_eq!(parse(json).unwrap(), ["senzu", "crab-shell"]);
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
