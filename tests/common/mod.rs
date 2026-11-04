#![allow(dead_code)] // each test binary uses its own slice of this

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

/// a senzu with a throwaway home, so no real config or log gets touched
pub fn senzu(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("senzu").unwrap();
    cmd.env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"));
    cmd
}

/// a home with a copy of the fixture beans in it, at `<home>/beans`
pub struct World {
    pub home: TempDir,
}

impl World {
    pub fn new() -> Self {
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

    pub fn beans(&self) -> PathBuf {
        self.home.path().join("beans")
    }

    /// senzu pointed at this world's beans
    pub fn senzu(&self) -> Command {
        let mut cmd = senzu(self.home.path());
        cmd.arg("--beans").arg(self.beans());
        cmd
    }

    pub fn fixture(&self, slug: &str) -> Value {
        let raw = std::fs::read(self.beans().join(format!("{slug}.json"))).unwrap();
        serde_json::from_slice(&raw).unwrap()
    }

    pub fn write(&self, file: &str, bean: &Value) {
        let raw = serde_json::to_vec_pretty(bean).unwrap();
        std::fs::write(self.beans().join(file), raw).unwrap();
    }

    pub fn log(&self) -> String {
        read_logs(self.home.path())
    }
}

/// every log file under a home's state dir, oldest first
pub fn read_logs(home: &Path) -> String {
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

/// the names of the log files under a home's state dir
pub fn log_files(home: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(home.join(".local/state/senzu"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}
