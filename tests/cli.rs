use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// a senzu with a throwaway home, so no real config or log gets touched
fn senzu(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("senzu").unwrap();
    cmd.env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"));
    cmd
}

/// the same, pointed at a beans dir that isn't there
fn senzu_nowhere(home: &Path) -> Command {
    let mut cmd = senzu(home);
    cmd.arg("--beans").arg(home.join("nope"));
    cmd
}

fn write_config(home: &Path, body: &str) {
    let dir = home.join(".config/senzu");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), body).unwrap();
}

/// the names of the log files under a home's state dir
fn log_files(home: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(home.join(".local/state/senzu"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// every log file under a home's state dir, oldest first
fn read_logs(home: &Path) -> String {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(home.join(".local/state/senzu"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect()
}

#[test]
fn unknown_config_key_exits_2_and_names_it() {
    let home = TempDir::new().unwrap();
    write_config(home.path(), "agent = \"claude\"\nnope = 1\n");
    senzu(home.path())
        .arg("ls")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("config.toml"))
        .stderr(predicate::str::contains("nope"));
}

#[test]
fn no_beans_dir_set_exits_2_and_says_how_to_set_one() {
    let home = TempDir::new().unwrap();
    senzu(home.path())
        .arg("ls")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no beans dir"))
        .stderr(predicate::str::contains("SENZU_BEANS"));
}

#[test]
fn logs_go_to_the_file_not_stdout() {
    let home = TempDir::new().unwrap();
    senzu_nowhere(home.path())
        .arg("ls")
        .assert()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("settings resolved").not());
    let log = read_logs(home.path());
    assert!(log.contains("settings resolved"), "log was: {log}");
}

#[test]
fn the_log_file_is_named_for_the_day() {
    let home = TempDir::new().unwrap();
    senzu_nowhere(home.path()).arg("ls").assert().code(1);
    let files = log_files(home.path());
    assert_eq!(files.len(), 1, "{files:?}");
    let name = &files[0];
    // senzu.YYYY-MM-DD.log
    assert!(
        name.starts_with("senzu.")
            && name.ends_with(".log")
            && name.len() == "senzu.2026-11-03.log".len(),
        "{name}"
    );
}

#[test]
fn only_a_week_of_logs_is_kept() {
    let home = TempDir::new().unwrap();
    let dir = home.path().join(".local/state/senzu");
    std::fs::create_dir_all(&dir).unwrap();
    for day in 1..=9 {
        std::fs::write(dir.join(format!("senzu.2026-10-{day:02}.log")), "old\n").unwrap();
    }
    senzu_nowhere(home.path()).arg("ls").assert().code(1);
    let files = log_files(home.path());
    // 9 old + today's, pruned to 7. which old ones go is by file creation
    // time, and these were all born in the same instant, so only count them
    assert_eq!(files.len(), 7, "{files:?}");
}

#[test]
fn unmakeable_log_dir_still_runs() {
    let home = TempDir::new().unwrap();
    // a file where the state dir wants to be
    std::fs::write(home.path().join("blocker"), "").unwrap();
    senzu_nowhere(home.path())
        .env("XDG_STATE_HOME", home.path().join("blocker"))
        .arg("ls")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not built yet"));
}
