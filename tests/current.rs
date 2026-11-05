//! bare senzu with no tty: the stream you're standing in
#![allow(
    clippy::unwrap_used,
    reason = "a test that can't unwrap should fail right there"
)]
#![allow(
    clippy::disallowed_methods,
    reason = "the test hands senzu its own PATH"
)]

mod common;

use std::process::Command;

use serde_json::json;

use common::World;

/// a crab-shell bean whose worktree is a real dir inside the world
fn world_with_worktree() -> (World, std::path::PathBuf) {
    let world = World::new();
    let worktree = world.home.path().join("worktrees/crab-shell");
    std::fs::create_dir_all(worktree.join("src/deep")).unwrap();
    let mut bean = world.fixture("crab-shell");
    bean["repos"][0]["path"] = json!(worktree);
    world.write("crab-shell.json", &bean);
    (world, worktree)
}

#[test]
fn inside_a_worktree_prints_the_three_lines() {
    let (world, worktree) = world_with_worktree();
    world
        .senzu()
        .current_dir(worktree.join("src/deep"))
        .assert()
        .success()
        .stdout(
            "crab-shell · execute · urgent\n\
             next: /senzu-execute crab-shell: checkpoint 3\n\
             worktree: ~/worktrees/crab-shell\n",
        );
}

#[test]
fn nowhere_in_particular_is_no_stream() {
    let world = World::new();
    world
        .senzu()
        .current_dir("/")
        .assert()
        .success()
        .stdout("no stream here\n");
}

#[test]
fn a_branch_named_for_the_slug_matches() {
    let world = World::new();
    let repo = world.home.path().join("some-clone");
    std::fs::create_dir(&repo).unwrap();
    // a clean env, or an inherited GIT_DIR (rebase -x, hooks) points git at
    // the real repo instead of this one
    let git = Command::new("git")
        .args(["init", "-q", "-b", "crab/Tidy-The-Tidepool"])
        .current_dir(&repo)
        .env_clear()
        .env("HOME", world.home.path())
        .env("PATH", std::env::var_os("PATH").unwrap())
        .status()
        .unwrap();
    assert!(git.success());
    world
        .senzu()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .current_dir(&repo)
        .assert()
        .success()
        .stdout(predicates::str::starts_with(
            "tidy-the-tidepool · ground · high\n",
        ));
}
