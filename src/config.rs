use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Deserialize;

const DEFAULT_AGENT: &str = "claude";

/// ~/.config/senzu/config.toml, every key optional
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) beans: Option<PathBuf>,
    pub(crate) agent: Option<String>,
    pub(crate) launch: Option<String>,
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
            Self::Config(path) => url::Url::from_file_path(path)
                .map_or_else(|()| path.display().to_string(), String::from),
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
    pub(crate) beans: PathBuf,
    pub(crate) agent: String,
    pub(crate) launch: String,
    /// the logs and the tab markers
    pub(crate) state: PathBuf,
    /// which layer set each one
    pub(crate) from: Origins,
}

/// flag > env (clap already merged those) > config > default.
/// the beans dir has no default. the launch default detects: zellij when
/// we're inside one, else exec. state has no config key: the log starts
/// before the config loads, so a config can't say where it goes
pub(crate) fn resolve(over: Overrides, config: Config, env: &Env) -> Result<Settings, ConfigError> {
    let file = || Origin::Config(env.config_path());
    let (beans, beans_from) = over
        .beans
        .or_else(|| config.beans.map(|p| (p, file())))
        .map(|(p, from)| (expand_tilde(p, &env.home), from))
        .ok_or_else(|| ConfigError::NoBeans(env.config_path()))?;
    // absolute like state: the sidebar gets it too, and the senzu dir is its parent
    let beans = std::path::absolute(&beans).unwrap_or(beans);
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
    Ok(Settings {
        beans,
        agent,
        launch,
        state,
        from: Origins {
            beans: beans_from,
            agent: agent_from,
            launch: launch_from,
            state: state_from,
        },
    })
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
        }
    }

    #[test]
    fn overrides_beat_config() {
        let over = Overrides {
            beans: Some(flag("/from/flag", "--beans")),
            agent: Some(("flag-agent".into(), Origin::Env("SENZU_AGENT".into()))),
            launch: Some(flag("exec", "--launch")),
            state: Some(flag("/from/flag/state", "--state")),
        };
        let got = resolve(over, config("config-agent", "zellij"), &env(false)).unwrap();
        assert_eq!(
            got,
            Settings {
                beans: "/from/flag".into(),
                agent: "flag-agent".into(),
                launch: "exec".into(),
                state: "/from/flag/state".into(),
                from: Origins {
                    beans: Origin::Flag("--beans".into()),
                    agent: Origin::Env("SENZU_AGENT".into()),
                    launch: Origin::Flag("--launch".into()),
                    state: Origin::Flag("--state".into()),
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
        )
        .unwrap();
        assert_eq!(
            got,
            Settings {
                beans: "/from/config".into(),
                agent: "config-agent".into(),
                launch: "exec".into(),
                state: "/home/crab/.local/state/senzu".into(),
                from: Origins {
                    beans: file(),
                    agent: file(),
                    launch: file(),
                    state: Origin::Default,
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
        let got = resolve(beans_only(), Config::default(), &env(false)).unwrap();
        assert_eq!(
            got,
            Settings {
                beans: "/b".into(),
                agent: "claude".into(),
                launch: "exec".into(),
                state: "/home/crab/.local/state/senzu".into(),
                from: Origins {
                    beans: Origin::Flag("--beans".into()),
                    agent: Origin::Default,
                    launch: Origin::Default,
                    state: Origin::Default,
                },
            }
        );
    }

    #[test]
    fn no_beans_dir_anywhere_is_an_error_naming_the_config() {
        let err = resolve(Overrides::default(), Config::default(), &env(false)).unwrap_err();
        assert!(matches!(err, ConfigError::NoBeans(_)), "{err:?}");
        let msg = err.to_string();
        assert!(
            msg.contains("--beans") && msg.contains("config.toml"),
            "{msg}"
        );
    }

    #[test]
    fn launch_detects_zellij() {
        let got = resolve(beans_only(), Config::default(), &env(true)).unwrap();
        assert_eq!(got.launch, "zellij");
    }

    #[test]
    fn explicit_launch_beats_detection() {
        let over = Overrides {
            launch: Some(flag("exec", "--launch")),
            ..beans_only()
        };
        let got = resolve(over, Config::default(), &env(true)).unwrap();
        assert_eq!(got.launch, "exec");
    }

    #[test]
    fn config_beans_expand_tilde() {
        let cfg = Config {
            beans: Some("~/beans".into()),
            ..Config::default()
        };
        let got = resolve(Overrides::default(), cfg, &env(false)).unwrap();
        assert_eq!(got.beans, PathBuf::from("/home/crab/beans"));
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
        let got = resolve(over, Config::default(), &env(false)).unwrap();
        assert!(got.beans.is_absolute(), "{}", got.beans.display());
        assert!(got.beans.ends_with("beans"), "{}", got.beans.display());
    }

    #[test]
    fn a_relative_state_dir_comes_out_absolute() {
        let over = Overrides {
            state: Some(flag("rel/state", "--state")),
            ..beans_only()
        };
        let got = resolve(over, Config::default(), &env(false)).unwrap();
        assert!(got.state.is_absolute(), "{}", got.state.display());
        assert!(got.state.ends_with("rel/state"), "{}", got.state.display());
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
