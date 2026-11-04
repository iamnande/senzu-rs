//! the shared bits: a throwaway home with the fixture beans in it
#![allow(dead_code, reason = "each test binary uses its own slice of this")]
#![allow(
    clippy::unwrap_used,
    reason = "a test that can't unwrap should fail right there"
)]

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

/// a senzu with a throwaway home, so no real config or log gets touched
pub(crate) fn senzu(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("senzu").unwrap();
    cmd.env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"));
    cmd
}

/// a home with a copy of the fixture beans in it, at `<home>/beans`
pub(crate) struct World {
    pub(crate) home: TempDir,
}

impl World {
    pub(crate) fn new() -> Self {
        let home = TempDir::new().unwrap();
        let beans = home.path().join("beans");
        std::fs::create_dir(&beans).unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beans");
        for entry in std::fs::read_dir(fixtures).unwrap() {
            let path = entry.unwrap().path();
            std::fs::copy(&path, beans.join(path.file_name().unwrap())).unwrap();
        }
        Self { home }
    }

    pub(crate) fn beans(&self) -> PathBuf {
        self.home.path().join("beans")
    }

    /// senzu pointed at this world's beans
    pub(crate) fn senzu(&self) -> Command {
        let mut cmd = senzu(self.home.path());
        cmd.arg("--beans").arg(self.beans());
        cmd
    }

    pub(crate) fn fixture(&self, slug: &str) -> Value {
        let raw = std::fs::read(self.beans().join(format!("{slug}.json"))).unwrap();
        serde_json::from_slice(&raw).unwrap()
    }

    pub(crate) fn write(&self, file: &str, bean: &Value) {
        let raw = serde_json::to_vec_pretty(bean).unwrap();
        std::fs::write(self.beans().join(file), raw).unwrap();
    }

    pub(crate) fn log(&self) -> String {
        read_logs(self.home.path())
    }
}

/// every log file under a home's state dir, oldest first
pub(crate) fn read_logs(home: &Path) -> String {
    let Ok(dir) = std::fs::read_dir(home.join(".local/state/senzu")) else {
        return String::new();
    };
    let mut paths: Vec<PathBuf> = dir.map(|e| e.unwrap().path()).collect();
    paths.sort();
    paths
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect()
}

/// every log line under a home's state dir, parsed
pub(crate) fn log_lines(home: &Path) -> Vec<serde_json::Value> {
    read_logs(home)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// the line where the run's root span closes
pub(crate) fn root_close(home: &Path) -> serde_json::Value {
    log_lines(home)
        .into_iter()
        .find(|l| l["fields"]["message"] == "close" && l["span"]["name"] == "senzu")
        .unwrap()
}

/// the names of the spans that closed during a run
pub(crate) fn closed_spans(home: &Path) -> Vec<String> {
    log_lines(home)
        .into_iter()
        .filter(|l| l["fields"]["message"] == "close")
        .map(|l| l["span"]["name"].as_str().unwrap().to_owned())
        .collect()
}

/// the names of the log files under a home's state dir
pub(crate) fn log_files(home: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(home.join(".local/state/senzu"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}
