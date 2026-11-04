//! flags, config and the log file, end to end
#![allow(
    clippy::unwrap_used,
    reason = "a test that can't unwrap should fail right there"
)]

mod common;

use std::path::Path;

use predicates::prelude::*;
use tempfile::TempDir;

use common::senzu;

fn write_config(home: &Path, body: &str) {
    let dir = home.join(".config/senzu");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), body).unwrap();
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
    senzu(home.path())
        .arg("--beans")
        .arg(home.path().join("nope"))
        .arg("ls")
        .assert()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("settings resolved").not());
    let log = common::read_logs(home.path());
    assert!(log.contains("settings resolved"), "log was: {log}");
}

#[test]
fn the_log_file_is_named_for_the_day() {
    let home = TempDir::new().unwrap();
    senzu(home.path())
        .arg("--beans")
        .arg(home.path().join("nope"))
        .arg("ls")
        .assert()
        .code(1);
    let files = common::log_files(home.path());
    assert_eq!(files.len(), 1, "{files:?}");
    let name = &files[0];
    // senzu.YYYY-MM-DD.log
    assert!(
        name.starts_with("senzu.")
            && Path::new(name).extension().is_some_and(|e| e == "log")
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
    senzu(home.path())
        .arg("--beans")
        .arg(home.path().join("nope"))
        .arg("ls")
        .assert()
        .code(1);
    let files = common::log_files(home.path());
    // 9 old + today's, pruned to 7. which old ones go is by file creation
    // time, and these were all born in the same instant, so only count them
    assert_eq!(files.len(), 7, "{files:?}");
}

#[test]
fn unmakeable_log_dir_still_runs() {
    let world = common::World::new();
    // a file where the state dir wants to be
    let blocker = world.home.path().join("blocker");
    std::fs::write(&blocker, "").unwrap();
    world
        .senzu()
        .env("XDG_STATE_HOME", &blocker)
        .arg("check")
        .assert()
        .success()
        .stdout("ok 5 beans\n");
}
