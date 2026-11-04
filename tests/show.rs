//! senzu show: one bean, read on its own
#![allow(
    clippy::unwrap_used,
    reason = "a test that can't unwrap should fail right there"
)]

mod common;

use predicates::prelude::*;
use serde_json::Value;

use common::World;

#[test]
fn unknown_slug_exits_3() {
    let world = World::new();
    world
        .senzu()
        .args(["show", "no-such-crab"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("no-such-crab"));
}

/// crab-shell sets every field, so a lossless round trip gives back the file
#[test]
fn json_round_trips_the_whole_bean() {
    let world = World::new();
    let out = world
        .senzu()
        .args(["show", "crab-shell", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let got: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(got, world.fixture("crab-shell"));
}

#[test]
fn piped_show_is_json_without_asking() {
    let world = World::new();
    let out = world
        .senzu()
        .args(["show", "kelp-budget"])
        .output()
        .unwrap();
    let got: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(got["slug"], "kelp-budget");
}

/// show reads only its own file, so a broken neighbor stays quiet
#[test]
fn a_bad_neighbor_doesnt_bother_show() {
    let world = World::new();
    std::fs::write(world.beans().join("broken.json"), "{").unwrap();
    world
        .senzu()
        .args(["show", "kelp-budget"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

#[test]
fn a_bad_bean_exits_1_and_says_why() {
    let world = World::new();
    let mut bean = world.fixture("kelp-budget");
    bean["slug"] = "not-kelp".into();
    world.write("kelp-budget.json", &bean);
    world
        .senzu()
        .args(["show", "kelp-budget"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("kelp-budget.json"))
        .stderr(predicate::str::contains("doesn't match the file name"));
}

/// a slug that isn't kebab-case never becomes a path
#[test]
fn a_slug_cant_walk_out_of_the_dir() {
    let world = World::new();
    let outside = world.home.path().join("escape.json");
    std::fs::copy(world.beans().join("kelp-budget.json"), &outside).unwrap();
    world.senzu().args(["show", "../escape"]).assert().code(3);
}
