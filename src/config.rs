use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Deserialize;

const DEFAULT_AGENT: &str = "claude";
const DEFAULT_WORKTREES: &str = "~/worktrees";

/// ~/.config/senzu/config.toml, every key optional
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) beans: Option<PathBuf>,
    pub(crate) agent: Option<String>,
    pub(crate) launch: Option<String>,
    pub(crate) worktrees: Option<PathBuf>,
    pub(crate) branch_prefix: Option<String>,
    pub(crate) thread_channel: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConfigError {
    #[error("{path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("no beans dir: set --beans, SENZU_BEANS, or beans in {}", .0.display())]
    NoBeans(PathBuf),
}

impl Config {
    /// a missing file is an empty config, anything else unreadable is an error
    pub(crate) fn load(path: &Path) -> Result<Self, ConfigError> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(ConfigError::Read {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        toml::from_str(&raw).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source,
        })
    }
}

/// what the flags and env already settled, before config and defaults,
/// each with the layer that set it
#[derive(Debug, Default)]
pub(crate) struct Overrides {
    pub(crate) beans: Option<(PathBuf, Origin)>,
    pub(crate) agent: Option<(String, Origin)>,
    pub(crate) launch: Option<(String, Origin)>,
    pub(crate) state: Option<(PathBuf, Origin)>,
    pub(crate) worktrees: Option<(PathBuf, Origin)>,
    pub(crate) branch_prefix: Option<(String, Origin)>,
    pub(crate) thread_channel: Option<(String, Origin)>,
}

/// the layer a setting came from. shown as a uri whose scheme is the layer
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin {
    /// `--beans`
    Flag(String),
    /// `SENZU_BEANS`
    Env(String),
    /// the config file
    Config(PathBuf),
    /// nothing set it
    Default,
}

impl Origin {
    /// `flag:--beans`, `env:SENZU_BEANS`, `file:///…/config.toml`, `default:`
    pub(crate) fn uri(&self) -> String {
        match self {
            Self::Flag(name) => format!("flag:{name}"),
            Self::Env(var) => format!("env:{var}"),
            Self::Config(path) => format!("file://{}", path.display()),
            Self::Default => "default:".to_owned(),
        }
    }
}

/// where each setting came from
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Origins {
    pub(crate) beans: Origin,
    pub(crate) agent: Origin,
    pub(crate) launch: Origin,
    pub(crate) state: Origin,
    pub(crate) worktrees: Origin,
    pub(crate) branch_prefix: Origin,
    pub(crate) thread_channel: Origin,
}

/// the process environment senzu uses, read once so tests can hand in their own
#[derive(Debug, Default)]
pub(crate) struct Env {
    pub(crate) home: PathBuf,
    pub(crate) in_zellij: bool,
    pub(crate) path: Option<OsString>,
    pub(crate) xdg_config_home: Option<PathBuf>,
    pub(crate) xdg_state_home: Option<PathBuf>,
    pub(crate) senzu_state: Option<PathBuf>,
}

impl Env {
    #[expect(clippy::disallowed_methods, reason = "the one place env is read")]
    pub(crate) fn from_process() -> Self {
        let path = |var| std::env::var_os(var).map(PathBuf::from);
        Self {
            home: path("HOME").unwrap_or_default(),
            in_zellij: std::env::var_os("ZELLIJ").is_some(),
            path: std::env::var_os("PATH"),
            xdg_config_home: path("XDG_CONFIG_HOME"),
            xdg_state_home: path("XDG_STATE_HOME"),
            senzu_state: path("SENZU_STATE"),
        }
    }

    /// `$XDG_CONFIG_HOME/senzu/config.toml`, else `~/.config/senzu/config.toml`
    pub(crate) fn config_path(&self) -> PathBuf {
        xdg_dir(self.xdg_config_home.as_deref(), &self.home, ".config")
            .join("senzu")
            .join("config.toml")
    }

    /// `$SENZU_STATE`, else `$XDG_STATE_HOME/senzu`, else `~/.local/state/senzu`
    pub(crate) fn state_dir(&self) -> PathBuf {
        self.senzu_state.clone().unwrap_or_else(|| {
            xdg_dir(self.xdg_state_home.as_deref(), &self.home, ".local/state").join("senzu")
        })
    }
}

/// a relative xdg path resolves against cwd, so the same command would read a
/// different config depending on where it runs. ignore it and use home
fn xdg_dir(var: Option<&Path>, home: &Path, fallback: &str) -> PathBuf {
    var.filter(|p| p.is_absolute())
        .map_or_else(|| home.join(fallback), Path::to_owned)
}

/// the settings every command runs with
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Settings {
    /// none until a flag, env or config sets it
    pub(crate) beans: Option<PathBuf>,
    pub(crate) agent: String,
    pub(crate) launch: String,
    /// the logs and the tab markers
    pub(crate) state: PathBuf,
    /// for the skills: where worktrees go
    pub(crate) worktrees: PathBuf,
    /// for the skills: how branch names start. none, no prefix
    pub(crate) branch_prefix: Option<String>,
    /// for the skills: the slack channel for threads. none, no slack
    pub(crate) thread_channel: Option<String>,
    /// which layer set each one
    pub(crate) from: Origins,
}

/// flag > env (clap already merged those) > config > default.
/// the beans dir has no default. the launch default detects: zellij when
/// we're inside one, else exec. state has no config key: the log starts
/// before the config loads, so a config can't say where it goes
pub(crate) fn resolve(over: Overrides, config: Config, env: &Env) -> Settings {
    let file = || Origin::Config(env.config_path());
    // unset is `None`: `senzu config` still shows the rest, the other commands error
    let (beans, beans_from) = over
        .beans
        .or_else(|| config.beans.map(|p| (p, file())))
        .map_or((None, Origin::Default), |(p, from)| {
            // absolute like state: the sidebar gets it too, and the senzu dir is its parent
            let p = expand_tilde(p, &env.home);
            (Some(std::path::absolute(&p).unwrap_or(p)), from)
        });
    let (agent, agent_from) = over
        .agent
        .or_else(|| config.agent.map(|a| (a, file())))
        .unwrap_or_else(|| (DEFAULT_AGENT.to_owned(), Origin::Default));
    let detected = if env.in_zellij { "zellij" } else { "exec" };
    let (launch, launch_from) = over
        .launch
        .or_else(|| config.launch.map(|l| (l, file())))
        .unwrap_or_else(|| (detected.to_owned(), Origin::Default));
    // absolute, since a sidebar gets it handed over and runs somewhere else
    let (state, state_from) = over
        .state
        .unwrap_or_else(|| (env.state_dir(), Origin::Default));
    let state = std::path::absolute(&state).unwrap_or(state);
    let (worktrees, worktrees_from) = over
        .worktrees
        .or_else(|| config.worktrees.map(|p| (p, file())))
        .unwrap_or_else(|| (DEFAULT_WORKTREES.into(), Origin::Default));
    let worktrees = expand_tilde(worktrees, &env.home);
    // absolute like beans: the skills paste it into `git -C <repo> worktree add`
    let worktrees = std::path::absolute(&worktrees).unwrap_or(worktrees);
    let (branch_prefix, branch_prefix_from) = optional(
        over.branch_prefix,
        config.branch_prefix.map(|p| (p, file())),
    );
    let (thread_channel, thread_channel_from) = optional(
        over.thread_channel,
        config.thread_channel.map(|c| (c, file())),
    );
    Settings {
        beans,
        agent,
        launch,
        state,
        worktrees,
        branch_prefix,
        thread_channel,
        from: Origins {
            beans: beans_from,
            agent: agent_from,
            launch: launch_from,
            state: state_from,
            worktrees: worktrees_from,
            branch_prefix: branch_prefix_from,
            thread_channel: thread_channel_from,
        },
    }
}

/// a setting with no default: unset is `None`, from the default layer. empty
/// is unset too, so `SENZU_THREAD_CHANNEL=` can't turn slack on
fn optional(
    over: Option<(String, Origin)>,
    config: Option<(String, Origin)>,
) -> (Option<String>, Origin) {
    let set = |v: &(String, Origin)| !v.0.is_empty();
    over.filter(set)
        .or(config.filter(set))
        .map_or((None, Origin::Default), |(v, from)| (Some(v), from))
}

/// toml has no shell, so `~/beans` in the config means home
fn expand_tilde(path: PathBuf, home: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => home.join(rest),
        Err(_) => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(in_zellij: bool) -> Env {
        Env {
            home: PathBuf::from("/home/crab"),
            in_zellij,
            ..Env::default()
        }
    }

    fn flag<T>(value: impl Into<T>, name: &str) -> (T, Origin) {
        (value.into(), Origin::Flag(name.into()))
    }

    fn file() -> Origin {
        Origin::Config("/home/crab/.config/senzu/config.toml".into())
    }

    fn config(agent: &str, launch: &str) -> Config {
        Config {
            beans: Some(PathBuf::from("/from/config")),
            agent: Some(agent.into()),
            launch: Some(launch.into()),
            ..Config::default()
        }
    }

    #[test]
    fn overrides_beat_config() {
        let over = Overrides {
            beans: Some(flag("/from/flag", "--beans")),
            agent: Some(("flag-agent".into(), Origin::Env("SENZU_AGENT".into()))),
            launch: Some(flag("exec", "--launch")),
            state: Some(flag("/from/flag/state", "--state")),
            ..Overrides::default()
        };
        let got = resolve(over, config("config-agent", "zellij"), &env(false));
        assert_eq!(
            got,
            Settings {
                beans: Some("/from/flag".into()),
                agent: "flag-agent".into(),
                launch: "exec".into(),
                state: "/from/flag/state".into(),
                worktrees: "/home/crab/worktrees".into(),
                branch_prefix: None,
                thread_channel: None,
                from: Origins {
                    beans: Origin::Flag("--beans".into()),
                    agent: Origin::Env("SENZU_AGENT".into()),
                    launch: Origin::Flag("--launch".into()),
                    state: Origin::Flag("--state".into()),
                    worktrees: Origin::Default,
                    branch_prefix: Origin::Default,
                    thread_channel: Origin::Default,
                },
            }
        );
    }

    #[test]
    fn config_beats_defaults() {
        let got = resolve(
            Overrides::default(),
            config("config-agent", "exec"),
            &env(true),
        );
        assert_eq!(
            got,
            Settings {
                beans: Some("/from/config".into()),
                agent: "config-agent".into(),
                launch: "exec".into(),
                state: "/home/crab/.local/state/senzu".into(),
                worktrees: "/home/crab/worktrees".into(),
                branch_prefix: None,
                thread_channel: None,
                from: Origins {
                    beans: file(),
                    agent: file(),
                    launch: file(),
                    state: Origin::Default,
                    worktrees: Origin::Default,
                    branch_prefix: Origin::Default,
                    thread_channel: Origin::Default,
                },
            }
        );
    }

    /// only the beans dir set, so everything else is a default
    fn beans_only() -> Overrides {
        Overrides {
            beans: Some(flag("/b", "--beans")),
            ..Overrides::default()
        }
    }

    #[test]
    fn defaults_fill_the_rest() {
        let got = resolve(beans_only(), Config::default(), &env(false));
        assert_eq!(
            got,
            Settings {
                beans: Some("/b".into()),
                agent: "claude".into(),
                launch: "exec".into(),
                state: "/home/crab/.local/state/senzu".into(),
                worktrees: "/home/crab/worktrees".into(),
                branch_prefix: None,
                thread_channel: None,
                from: Origins {
                    beans: Origin::Flag("--beans".into()),
                    agent: Origin::Default,
                    launch: Origin::Default,
                    state: Origin::Default,
                    worktrees: Origin::Default,
                    branch_prefix: Origin::Default,
                    thread_channel: Origin::Default,
                },
            }
        );
    }

    #[test]
    fn no_beans_dir_anywhere_is_unset() {
        let got = resolve(Overrides::default(), Config::default(), &env(false));
        assert_eq!(got.beans, None);
        assert_eq!(got.from.beans, Origin::Default);
    }

    #[test]
    fn launch_detects_zellij() {
        let got = resolve(beans_only(), Config::default(), &env(true));
        assert_eq!(got.launch, "zellij");
    }

    #[test]
    fn explicit_launch_beats_detection() {
        let over = Overrides {
            launch: Some(flag("exec", "--launch")),
            ..beans_only()
        };
        let got = resolve(over, Config::default(), &env(true));
        assert_eq!(got.launch, "exec");
    }

    #[test]
    fn config_beans_expand_tilde() {
        let cfg = Config {
            beans: Some("~/beans".into()),
            ..Config::default()
        };
        let got = resolve(Overrides::default(), cfg, &env(false));
        assert_eq!(got.beans, Some(PathBuf::from("/home/crab/beans")));
    }

    #[test]
    fn senzu_state_beats_xdg() {
        let mut env = env(false);
        env.xdg_state_home = Some("/xdg".into());
        assert_eq!(env.state_dir(), PathBuf::from("/xdg/senzu"));
        env.senzu_state = Some("/mine".into());
        assert_eq!(env.state_dir(), PathBuf::from("/mine"));
    }

    #[test]
    fn a_relative_beans_dir_comes_out_absolute() {
        let over = Overrides {
            beans: Some(flag("beans", "--beans")),
            ..Overrides::default()
        };
        let got = resolve(over, Config::default(), &env(false));
        let beans = got.beans.unwrap();
        assert!(beans.is_absolute(), "{}", beans.display());
        assert!(beans.ends_with("beans"), "{}", beans.display());
    }

    #[test]
    fn a_relative_state_dir_comes_out_absolute() {
        let over = Overrides {
            state: Some(flag("rel/state", "--state")),
            ..beans_only()
        };
        let got = resolve(over, Config::default(), &env(false));
        assert!(got.state.is_absolute(), "{}", got.state.display());
        assert!(got.state.ends_with("rel/state"), "{}", got.state.display());
    }

    #[test]
    fn a_relative_worktrees_dir_comes_out_absolute() {
        let over = Overrides {
            worktrees: Some(flag("wt", "--worktrees")),
            ..beans_only()
        };
        let got = resolve(over, Config::default(), &env(false));
        assert!(got.worktrees.is_absolute(), "{}", got.worktrees.display());
        assert!(got.worktrees.ends_with("wt"), "{}", got.worktrees.display());
    }

    /// an empty env value is unset, so the config one shows through
    #[test]
    fn an_empty_skill_key_is_unset() {
        let over = Overrides {
            branch_prefix: Some((String::new(), Origin::Env("SENZU_BRANCH_PREFIX".into()))),
            thread_channel: Some(flag("", "--thread-channel")),
            ..beans_only()
        };
        let config = Config {
            branch_prefix: Some("crab/".into()),
            ..Config::default()
        };
        let got = resolve(over, config, &env(false));
        assert_eq!(got.branch_prefix.as_deref(), Some("crab/"));
        assert_eq!(got.from.branch_prefix, file());
        assert_eq!(got.thread_channel, None);
        assert_eq!(got.from.thread_channel, Origin::Default);
    }

    #[test]
    fn origins_read_as_uris() {
        assert_eq!(Origin::Flag("--beans".into()).uri(), "flag:--beans");
        assert_eq!(Origin::Env("SENZU_BEANS".into()).uri(), "env:SENZU_BEANS");
        assert_eq!(file().uri(), "file:///home/crab/.config/senzu/config.toml");
        assert_eq!(Origin::Default.uri(), "default:");
    }

    #[test]
    fn missing_config_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let got = Config::load(&dir.path().join("nope.toml")).unwrap();
        assert!(got.beans.is_none() && got.agent.is_none() && got.launch.is_none());
    }
}
