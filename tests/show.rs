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
