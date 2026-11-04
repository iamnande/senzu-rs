//! senzu check: every rule a bean can break
#![allow(
    clippy::unwrap_used,
    reason = "a test that can't unwrap should fail right there"
)]

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::SystemTime;

use predicates::prelude::*;
use serde_json::json;

use common::World;

#[test]
fn ok_on_the_fixtures_and_skips_markdown() {
    let world = World::new();
    world
        .senzu()
        .arg("check")
        .assert()
        .success()
        .stdout("ok 5 beans\n")
        .stderr(predicate::str::contains("ignoring 1 markdown beans"));
}

#[test]
fn one_bad_bean_fails_and_is_named() {
    let world = World::new();
    std::fs::write(world.beans().join("broken.json"), "{ nope").unwrap();
    world
        .senzu()
        .arg("check")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("broken.json"))
        .stdout(predicate::str::contains("line 1"));
}

#[test]
fn missing_dir_exits_1() {
    let world = World::new();
    common::senzu(world.home.path())
        .arg("--beans")
        .arg(world.home.path().join("nope"))
        .arg("check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("beans dir not found"));
}

/// write a broken copy of a fixture, check names the file and the problem
fn rejects(file: &str, bean: &serde_json::Value, needle: &str) {
    let world = World::new();
    world.write(file, bean);
    world
        .senzu()
        .arg("check")
        .assert()
        .code(1)
        .stdout(predicate::str::contains(file))
        .stdout(predicate::str::contains(needle));
}

#[test]
fn rejects_an_unknown_field() {
    let mut bean = World::new().fixture("harden-the-hut");
    bean["vibes"] = json!("immaculate");
    rejects("harden-the-hut.json", &bean, "unknown field `vibes`");
}

#[test]
fn rejects_a_slug_that_isnt_the_file_name() {
    let bean = World::new().fixture("harden-the-hut");
    rejects("hut.json", &bean, "doesn't match the file name");
}

#[test]
fn rejects_a_slug_that_isnt_kebab() {
    let mut bean = World::new().fixture("harden-the-hut");
    bean["slug"] = json!("Hut_Two");
    rejects("Hut_Two.json", &bean, "isn't lowercase-kebab");
}

#[test]
fn rejects_a_relative_repo_path() {
    let mut bean = World::new().fixture("crab-shell");
    bean["repos"][0]["path"] = json!("worktrees/crab-shell");
    rejects("crab-shell.json", &bean, "isn't absolute");
}

#[test]
fn rejects_a_duplicate_draft_id() {
    let mut bean = World::new().fixture("crab-shell");
    let draft = bean["drafts"][0].clone();
    bean["drafts"].as_array_mut().unwrap().push(draft);
    rejects("crab-shell.json", &bean, "used twice");
}

#[test]
fn rejects_a_dangling_open_draft_ref() {
    let mut bean = World::new().fixture("crab-shell");
    bean["open"][0]["draft"] = json!("ghost-email");
    rejects("crab-shell.json", &bean, "\"ghost-email\" is referenced");
}

#[test]
fn rejects_a_dangling_next_draft_ref() {
    let mut bean = World::new().fixture("harden-the-hut");
    bean["next"]["draft"] = json!("ghost-map");
    rejects("harden-the-hut.json", &bean, "\"ghost-map\" is referenced");
}

#[test]
fn rejects_a_date_that_isnt_one() {
    let mut bean = World::new().fixture("crab-shell");
    bean["log"][0]["date"] = json!("2026-13-45");
    rejects("crab-shell.json", &bean, "crab-shell.json");
}

#[test]
fn the_log_gets_a_load_span_but_not_one_per_bean() {
    let world = World::new();
    world.senzu().arg("check").assert().success();
    let log = world.log();
    assert!(log.contains("load{dir="), "log was: {log}");
    assert!(!log.contains("parse{file="), "log was: {log}");
}

#[test]
fn debug_brings_back_the_parse_spans() {
    let world = World::new();
    world
        .senzu()
        .env("RUST_LOG", "debug")
        .arg("check")
        .assert()
        .success();
    let log = world.log();
    assert!(log.contains("parse{file="), "log was: {log}");
}

/// R12: senzu reads beans, it never touches them
#[test]
fn the_beans_dir_is_untouched() {
    let world = World::new();
    let snapshot = || -> BTreeMap<PathBuf, (Vec<u8>, SystemTime)> {
        std::fs::read_dir(world.beans())
            .unwrap()
            .map(|e| {
                let path = e.unwrap().path();
                let meta = std::fs::metadata(&path).unwrap();
                let body = std::fs::read(&path).unwrap();
                (path, (body, meta.modified().unwrap()))
            })
            .collect()
    };
    let before = snapshot();
    world.senzu().arg("check").assert().success();
    assert_eq!(before, snapshot());
}

#[test]
fn rejects_an_unknown_field_deep_in_the_bean() {
    let mut bean = World::new().fixture("crab-shell");
    bean["open"][0]["vibes"] = json!("immaculate");
    rejects("crab-shell.json", &bean, "unknown field `open.0.vibes`");
}

#[test]
fn rejects_a_bean_with_no_schema() {
    let mut bean = World::new().fixture("harden-the-hut");
    bean.as_object_mut().unwrap().remove("schema");
    rejects("harden-the-hut.json", &bean, "missing field `schema`");
}

/// a newer schema may not fit the types, so it's refused before they're tried
#[test]
fn rejects_a_newer_schema_and_says_to_upgrade() {
    let mut bean = World::new().fixture("harden-the-hut");
    bean["schema"] = json!(2);
    bean["phase"] = json!({ "renamed": "in schema 2" });
    rejects(
        "harden-the-hut.json",
        &bean,
        "schema 2, this senzu reads 1, upgrade senzu",
    );
}
