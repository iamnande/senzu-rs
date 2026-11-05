//! senzu open: the command, the cwd and the launchers
#![allow(
    clippy::unwrap_used,
    reason = "a test that can't unwrap should fail right there"
)]

mod common;

use std::path::{Path, PathBuf};

use predicates::prelude::*;
use serde_json::{Value, json};

use common::World;

/// `open --dry-run`, parsed
fn dry_run(cmd: &mut assert_cmd::Command, slug: &str) -> Value {
    let out = cmd.args(["open", slug, "--dry-run"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

/// a PATH dir holding fake programs that print what they got instead of doing
/// anything, to stdout and to `<home>/<name>.out`, since zellij's stdout goes
/// nowhere. the real claude and zellij are never on the PATH these tests hand senzu.
fn fake_bin(world: &World, names: &[&str]) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let bin = world.home.path().join("fake-bin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in names {
        let path = bin.join(name);
        let script = format!(
            // builtins only: PATH is just this dir
            "#!/bin/sh\nout=$(echo \"{name} in $PWD\"; printf '%s\\n' \"$@\")\nprintf '%s\\n' \"$out\"\nprintf '%s\\n' \"$out\" > \"$HOME/{name}.out\"\n"
        );
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

fn write_config(home: &Path, body: &str) {
    let dir = home.join(".config/senzu");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), body).unwrap();
}

#[test]
fn dry_run_is_claude_on_the_phase_skill_in_the_beans_parent() {
    let world = World::new();
    let got = dry_run(&mut world.senzu(), "crab-shell");
    assert_eq!(
        got,
        json!({
            "launcher": "exec",
            "existing_tab": false,
            "program": "claude",
            "args": ["--name", "crab-shell", "/senzu-execute crab-shell"],
            "cwd": world.home.path(),
            "session": "crab-shell",
        })
    );
}

#[test]
fn dry_run_opens_in_the_first_worktree_that_exists() {
    let world = World::new();
    let worktree = world.home.path().join("worktrees/crab-shell");
    std::fs::create_dir_all(&worktree).unwrap();
    let mut bean = world.fixture("crab-shell");
    let gone = json!({ "path": "/nowhere/gone", "repo": "crab-rs", "branch": null, "base": null, "note": null });
    let here =
        json!({ "path": worktree, "repo": "crab-rs", "branch": null, "base": null, "note": null });
    bean["repos"] = json!([gone, here]);
    world.write("crab-shell.json", &bean);
    let got = dry_run(&mut world.senzu(), "crab-shell");
    assert_eq!(got["cwd"], json!(worktree));
}

#[test]
fn inside_zellij_the_default_is_a_zellij_pane() {
    let world = World::new();
    let got = dry_run(world.senzu().env("ZELLIJ", "0"), "harden-the-hut");
    assert_eq!(got["launcher"], "zellij");
    assert_eq!(got["program"], "zellij");
}

#[test]
fn a_launch_flag_beats_the_detection() {
    let world = World::new();
    let got = dry_run(
        world.senzu().env("ZELLIJ", "0").args(["--launch", "exec"]),
        "harden-the-hut",
    );
    assert_eq!(got["launcher"], "exec");
}

#[test]
fn unknown_agent_exits_2_and_lists_claude() {
    let world = World::new();
    world
        .senzu()
        .args(["--agent", "nope", "open", "crab-shell", "--dry-run"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "unknown agent \"nope\", known: claude",
        ));
}

#[test]
fn unknown_launch_exits_2() {
    let world = World::new();
    world
        .senzu()
        .args(["--launch", "tmux", "open", "crab-shell", "--dry-run"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("known: exec, zellij"));
}

/// only claude exists, so bogus names show which layer won: the error names it
#[test]
fn agent_flag_beats_env_beats_config() {
    let world = World::new();
    write_config(world.home.path(), "agent = \"from-config\"\n");
    let open = ["open", "crab-shell", "--dry-run"];

    world
        .senzu()
        .args(open)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\"from-config\""));
    world
        .senzu()
        .env("SENZU_AGENT", "from-env")
        .args(open)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\"from-env\""));
    world
        .senzu()
        .env("SENZU_AGENT", "from-env")
        .args(["--agent", "claude"])
        .args(open)
        .assert()
        .success();
}

#[test]
fn done_beans_dont_open() {
    let world = World::new();
    world
        .senzu()
        .args(["open", "old-molt", "--dry-run"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("old-molt is done"));
}

#[test]
fn unknown_slug_exits_3() {
    let world = World::new();
    world
        .senzu()
        .args(["open", "no-such-crab", "--dry-run"])
        .assert()
        .code(3);
}

#[test]
fn no_agent_on_path_exits_1() {
    let world = World::new();
    let empty = fake_bin(&world, &[]);
    world
        .senzu()
        .env("PATH", &empty)
        .args(["open", "crab-shell"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("claude not found on PATH"));
}

#[test]
fn exec_replaces_senzu_with_the_agent() {
    let world = World::new();
    let bin = fake_bin(&world, &["claude"]);
    world
        .senzu()
        .env("PATH", &bin)
        .args(["open", "crab-shell"])
        .assert()
        .success()
        .stdout(format!(
            "claude in {}\n--name\ncrab-shell\n/senzu-execute crab-shell\n",
            world.home.path().display()
        ));
}

/// exec skips every destructor, so only a synchronous log keeps these lines
#[test]
fn exec_keeps_the_log_lines_before_it() {
    let world = World::new();
    let bin = fake_bin(&world, &["claude"]);
    world
        .senzu()
        .env("PATH", &bin)
        .args(["open", "crab-shell"])
        .assert()
        .success();
    let log = world.log();
    assert!(log.contains("launch planned"), "{log}");
}

#[test]
fn zellij_opens_a_new_tab_and_marks_when() {
    let world = World::new();
    let bin = fake_bin(&world, &["claude", "zellij"]);
    let home = world.home.path().display().to_string();
    world
        .senzu()
        .env("PATH", &bin)
        .env("ZELLIJ", "0")
        .args(["open", "harden-the-hut"])
        .assert()
        .success()
        // zellij prints the new tab's id, which would land on top of the tui
        .stdout(predicate::str::is_empty());
    // the fake's list-tabs isn't json, so senzu plans as if no tab is open
    let got = std::fs::read_to_string(world.home.path().join("zellij.out")).unwrap();
    let want = format!(
        "zellij in {home}\naction\nnew-tab\n--cwd\n{home}\n--layout-string\n// the tab senzu opens"
    );
    assert!(got.starts_with(&want), "{got}");
    assert!(
        got.contains(r#"tab name="harden-the-hut" focus=true"#),
        "{got}"
    );
    let marker = world
        .home
        .path()
        .join(".local/state/senzu/tabs/harden-the-hut");
    assert!(marker.is_file(), "no marker at {}", marker.display());
}

#[test]
fn zellij_goes_back_to_an_open_tab() {
    use std::os::unix::fs::PermissionsExt as _;
    let world = World::new();
    let bin = fake_bin(&world, &["claude"]);
    let zellij = bin.join("zellij");
    // lists a tab for the bean, and writes down every other call
    std::fs::write(
        &zellij,
        "#!/bin/sh\nif [ \"$2\" = list-tabs ]; then\n  echo '[{\"tab_id\":4,\"name\":\"harden-the-hut\",\"selectable_tiled_panes_count\":2}]'\n  exit 0\nfi\nprintf '%s\\n' \"$@\" > \"$HOME/zellij.out\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&zellij, std::fs::Permissions::from_mode(0o755)).unwrap();
    world
        .senzu()
        .env("PATH", &bin)
        .env("ZELLIJ", "0")
        .args(["open", "harden-the-hut"])
        .assert()
        .success();
    let got = std::fs::read_to_string(world.home.path().join("zellij.out")).unwrap();
    assert_eq!(got, "action\ngo-to-tab-name\nharden-the-hut\n");
    let marker = world
        .home
        .path()
        .join(".local/state/senzu/tabs/harden-the-hut");
    assert!(!marker.exists(), "going back keeps the first open's time");
}

#[test]
fn a_failed_zellij_says_why() {
    use std::os::unix::fs::PermissionsExt as _;
    let world = World::new();
    let bin = fake_bin(&world, &["claude"]);
    let zellij = bin.join("zellij");
    std::fs::write(
        &zellij,
        "#!/bin/sh\necho noise\necho 'Error: no session' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&zellij, std::fs::Permissions::from_mode(0o755)).unwrap();
    world
        .senzu()
        .env("PATH", &bin)
        .env("ZELLIJ", "0")
        .args(["open", "harden-the-hut"])
        .assert()
        .code(1)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Error: no session"));
}

#[test]
fn zellij_launch_outside_zellij_exits_1() {
    let world = World::new();
    let bin = fake_bin(&world, &["claude", "zellij"]);
    world
        .senzu()
        .env("PATH", &bin)
        .args(["--launch", "zellij", "open", "crab-shell"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "only works inside zellij, try --launch exec",
        ));
}

#[test]
fn the_state_flag_moves_the_marker_the_log_and_the_sidebar() {
    let world = World::new();
    let bin = fake_bin(&world, &["claude", "zellij"]);
    let state = world.home.path().join("elsewhere");
    world
        .senzu()
        .env("PATH", &bin)
        .env("ZELLIJ", "0")
        .arg("--state")
        .arg(&state)
        .args(["open", "harden-the-hut"])
        .assert()
        .success();
    assert!(state.join("tabs/harden-the-hut").is_file());
    let logs = std::fs::read_dir(&state).unwrap().flatten();
    assert!(
        logs.into_iter()
            .any(|f| f.file_name().to_string_lossy().ends_with(".log")),
        "no log in {}",
        state.display()
    );
    let got = std::fs::read_to_string(world.home.path().join("zellij.out")).unwrap();
    let want = format!(r#""--state" "{}""#, state.display());
    assert!(got.contains(&want), "{got}");
}

#[test]
fn senzu_state_in_the_env_works_too() {
    let world = World::new();
    let bin = fake_bin(&world, &["claude", "zellij"]);
    let state = world.home.path().join("from-env");
    world
        .senzu()
        .env("PATH", &bin)
        .env("ZELLIJ", "0")
        .env("SENZU_STATE", &state)
        .args(["open", "harden-the-hut"])
        .assert()
        .success();
    assert!(state.join("tabs/harden-the-hut").is_file());
}

#[test]
fn state_in_the_config_is_an_unknown_key() {
    let world = World::new();
    write_config(world.home.path(), "state = \"/nope\"\n");
    world
        .senzu()
        .args(["open", "crab-shell", "--dry-run"])
        .assert()
        .code(predicate::ne(0))
        .stderr(predicate::str::contains("state"));
}
