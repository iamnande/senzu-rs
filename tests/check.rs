//! senzu check: every rule a bean can break
#![allow(
    clippy::unwrap_used,
    reason = "a test that can't unwrap should fail right there"
)]
#![allow(
    clippy::disallowed_methods,
    reason = "the git tests hand git and senzu their own PATH"
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
fn a_plan_beside_its_bean_is_not_a_markdown_bean() {
    let world = World::new();
    std::fs::write(world.beans().join("crab-shell.plan.md"), "# crab-shell\n").unwrap();
    world
        .senzu()
        .arg("check")
        .assert()
        .success()
        .stdout("ok 5 beans\n")
        .stderr(predicate::str::contains("ignoring 1 markdown beans"));
}

/// git in the beans dir, with a clean env so an inherited `GIT_DIR` can't
/// point it at a real repo
fn git(world: &World, args: &[&str]) {
    git_in(world, &world.beans(), args);
}

fn git_in(world: &World, dir: &std::path::Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("HOME", world.home.path())
        .env("PATH", std::env::var_os("PATH").unwrap())
        .status()
        .unwrap();
    assert!(ok.success(), "git {args:?}");
}

#[test]
fn a_plan_git_tracks_fails_and_is_named() {
    let world = World::new();
    std::fs::write(world.beans().join("crab-shell.plan.md"), "# crab-shell\n").unwrap();
    git(&world, &["init", "-q"]);
    git(&world, &["add", "crab-shell.plan.md"]);
    world
        .senzu()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .arg("check")
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "crab-shell.plan.md: git tracks it, and plans stay local",
        ));
}

/// the beans can be tracked, just not the plan beside them
#[test]
fn tracked_beans_and_an_untracked_plan_pass() {
    let world = World::new();
    std::fs::write(world.beans().join("crab-shell.plan.md"), "# crab-shell\n").unwrap();
    git(&world, &["init", "-q"]);
    git(&world, &["add", "*.json"]);
    world
        .senzu()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .arg("check")
        .assert()
        .success()
        .stdout("ok 5 beans\n");
}

/// a hook in another repo hands senzu its index. check reads the beans dir's own
#[test]
fn an_inherited_index_cant_name_another_repos_plan() {
    let world = World::new();
    let other = world.home.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("crab-other.plan.md"), "# crab-other\n").unwrap();
    git_in(&world, &other, &["init", "-q"]);
    git_in(&world, &other, &["add", "crab-other.plan.md"]);
    git(&world, &["init", "-q"]);
    world
        .senzu()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("GIT_INDEX_FILE", other.join(".git/index"))
        .arg("check")
        .assert()
        .success()
        .stdout("ok 5 beans\n");
}

#[test]
fn a_plan_with_no_bean_warns_and_passes() {
    let world = World::new();
    std::fs::write(world.beans().join("crab-gone.plan.md"), "# crab-gone\n").unwrap();
    world
        .senzu()
        .arg("check")
        .assert()
        .success()
        .stdout("ok 5 beans\n")
        .stderr(predicate::str::contains(
            "crab-gone.plan.md: a plan with no bean beside it",
        ));
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
    let spans = common::closed_spans(world.home.path());
    assert!(spans.iter().any(|s| s == "load"), "{spans:?}");
    assert!(!spans.iter().any(|s| s == "parse"), "{spans:?}");
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
    let spans = common::closed_spans(world.home.path());
    assert!(spans.iter().any(|s| s == "parse"), "{spans:?}");
}

/// senzu reads beans, it never touches them
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
    bean["schema"] = json!(3);
    bean["phase"] = json!({ "renamed": "in schema 3" });
    rejects(
        "harden-the-hut.json",
        &bean,
        "schema 3, this senzu reads 2, upgrade senzu",
    );
}

/// a schema 1 bean still carries `plan`, so it's refused with a pointer
#[test]
fn rejects_a_schema_1_bean_and_says_to_migrate() {
    let mut bean = World::new().fixture("harden-the-hut");
    bean["schema"] = json!(1);
    bean["plan"] = json!({ "prd": "/nowhere/prd.md" });
    rejects(
        "harden-the-hut.json",
        &bean,
        "schema 1, this senzu reads 2, the bean needs migrating",
    );
}

#[test]
fn rejects_a_stored_plan_attachment() {
    let mut bean = World::new().fixture("harden-the-hut");
    bean["attachments"] = json!([{ "kind": "plan", "uri": "harden-the-hut.plan.md" }]);
    rejects(
        "harden-the-hut.json",
        &bean,
        "plans are found beside the bean",
    );
}

#[test]
fn rejects_an_attachment_kind_it_doesnt_know() {
    let mut bean = World::new().fixture("harden-the-hut");
    bean["attachments"] = json!([{ "kind": "artefact", "uri": "https://example.com" }]);
    rejects("harden-the-hut.json", &bean, "unknown variant `artefact`");
}

#[test]
fn rejects_an_attachment_uri_that_isnt_one() {
    let mut bean = World::new().fixture("harden-the-hut");
    bean["attachments"] = json!([{ "kind": "link", "uri": "https://" }]);
    rejects(
        "harden-the-hut.json",
        &bean,
        "attachment uri \"https://\" isn't a uri",
    );
}
