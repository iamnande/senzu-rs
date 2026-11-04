//! senzu ls: what shows, in what order, in what shape
#![allow(
    clippy::unwrap_used,
    reason = "a test that can't unwrap should fail right there"
)]

mod common;

use std::collections::BTreeSet;

use predicates::prelude::*;
use serde_json::Value;

use common::World;

/// run ls (piped, so json) and pull out the slugs in order
fn slugs(world: &World, args: &[&str]) -> Vec<String> {
    let out = world.senzu().arg("ls").args(args).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let rows: Vec<Value> = serde_json::from_slice(&out.stdout).unwrap();
    rows.iter()
        .map(|r| r["slug"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn default_is_urgent_and_high_and_no_done() {
    let world = World::new();
    assert_eq!(
        slugs(&world, &[]),
        ["crab-shell", "harden-the-hut", "tidy-the-tidepool"]
    );
}

/// kelp-budget sorts before tidy-the-tidepool by name, after it by priority
#[test]
fn sorts_by_priority_before_slug() {
    let world = World::new();
    let got = slugs(&world, &["--all"]);
    assert_eq!(got[2..], ["tidy-the-tidepool", "kelp-budget"]);
}

#[test]
fn all_brings_back_medium_but_not_done() {
    let world = World::new();
    assert_eq!(
        slugs(&world, &["--all"]),
        [
            "crab-shell",
            "harden-the-hut",
            "tidy-the-tidepool",
            "kelp-budget"
        ]
    );
}

#[test]
fn phase_filters_and_can_ask_for_done() {
    let world = World::new();
    assert_eq!(slugs(&world, &["--phase", "plan"]), ["harden-the-hut"]);
    assert_eq!(slugs(&world, &["--phase", "done"]), Vec::<String>::new());
    assert_eq!(slugs(&world, &["--phase", "done", "--all"]), ["old-molt"]);
}

#[test]
fn priority_replaces_the_urgent_and_high_default() {
    let world = World::new();
    assert_eq!(slugs(&world, &["--priority", "medium"]), ["kelp-budget"]);
}

#[test]
fn piped_rows_have_the_documented_fields() {
    let world = World::new();
    let out = world.senzu().arg("ls").output().unwrap();
    let rows: Vec<Value> = serde_json::from_slice(&out.stdout).unwrap();
    // Value keeps keys sorted, so compare as sets
    let keys: BTreeSet<&str> = rows[0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let want: BTreeSet<&str> = ["slug", "title", "phase", "priority", "next", "worktree"].into();
    assert_eq!(keys, want);
    assert_eq!(rows[0]["priority"], "urgent");
    assert_eq!(rows[0]["worktree"], Value::Null);
}

#[test]
fn one_bad_bean_warns_and_the_rest_still_list() {
    let world = World::new();
    std::fs::write(world.beans().join("broken.json"), "{ nope").unwrap();
    let out = world.senzu().arg("ls").assert().success();
    let out =
        out.stderr(predicate::str::contains("broken.json").and(predicate::str::contains("line 1")));
    let rows: Vec<Value> = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(rows.len(), 3);
}

/// a field from a newer senzu is a warning when reading, not a missing bean
#[test]
fn an_unknown_field_still_lists_with_a_warning() {
    let world = World::new();
    let mut bean = world.fixture("harden-the-hut");
    bean["vibes"] = "immaculate".into();
    world.write("harden-the-hut.json", &bean);
    let out = world
        .senzu()
        .arg("ls")
        .assert()
        .success()
        .stderr(predicate::str::contains("unknown field `vibes`"));
    let rows: Vec<Value> = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert!(
        rows.iter().any(|r| r["slug"] == "harden-the-hut"),
        "{rows:?}"
    );
}
