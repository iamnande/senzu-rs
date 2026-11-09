//! senzu config: each setting, its value, and the layer that set it
#![allow(
    clippy::unwrap_used,
    reason = "a test that can't unwrap should fail right there"
)]

mod common;

use serde_json::{Value, json};
use url::Url;

use common::World;

fn config_json(cmd: &mut assert_cmd::Command) -> Vec<Value> {
    let out = cmd.args(["config", "--json"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn a_flag_says_flag_and_the_env_says_env() {
    let world = World::new();
    let got = config_json(world.senzu().env("SENZU_AGENT", "crab"));
    let beans = world.beans().display().to_string();
    assert_eq!(
        got[0],
        json!({ "key": "beans", "value": beans, "from": "flag:--beans" })
    );
    assert_eq!(
        got[1],
        json!({ "key": "agent", "value": "crab", "from": "env:SENZU_AGENT" })
    );
}

#[test]
fn a_config_file_names_itself() {
    let world = World::new();
    let dir = world.home.path().join(".config/senzu");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), "launch = \"exec\"\n").unwrap();
    let got = config_json(&mut world.senzu());
    let file = Url::from_file_path(dir.join("config.toml"))
        .unwrap()
        .to_string();
    assert_eq!(
        got[2],
        json!({ "key": "launch", "value": "exec", "from": file })
    );
}

#[test]
fn the_skill_keys_come_from_the_config_file() {
    let world = World::new();
    let dir = world.home.path().join(".config/senzu");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "worktrees = \"~/w t\"\nbranch_prefix = \"crab/\"\nthread_channel = \"#tidepool\"\n",
    )
    .unwrap();
    let got = config_json(&mut world.senzu());
    let file = Url::from_file_path(dir.join("config.toml"))
        .unwrap()
        .to_string();
    // a plain path, not a uri: no `file://`, no `%20`, no trailing slash
    let wt = world.home.path().join("w t").display().to_string();
    assert_eq!(
        got[4..],
        [
            json!({ "key": "worktrees", "value": wt, "from": file }),
            json!({ "key": "branch_prefix", "value": "crab/", "from": file }),
            json!({ "key": "thread_channel", "value": "#tidepool", "from": file }),
        ]
    );
}

#[test]
fn an_unset_skill_key_is_null() {
    let world = World::new();
    let got = config_json(&mut world.senzu());
    assert_eq!(
        got[6],
        json!({ "key": "thread_channel", "value": null, "from": "default:" })
    );
}
