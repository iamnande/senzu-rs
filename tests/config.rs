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
