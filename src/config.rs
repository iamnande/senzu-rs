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
    /// no guessing where someone keeps their beans
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

/// what the flags and env already settled, before config and defaults
#[derive(Debug, Default)]
pub(crate) struct Overrides {
    pub(crate) beans: Option<PathBuf>,
    pub(crate) agent: Option<String>,
    pub(crate) launch: Option<String>,
}

/// the process environment senzu uses, read once so tests can hand in their own
#[derive(Debug, Default)]
pub(crate) struct Env {
    pub(crate) home: PathBuf,
    pub(crate) in_zellij: bool,
    pub(crate) path: Option<OsString>,
    pub(crate) xdg_config_home: Option<PathBuf>,
    pub(crate) xdg_state_home: Option<PathBuf>,
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
        }
    }

    /// `$XDG_CONFIG_HOME/senzu/config.toml`, else `~/.config/senzu/config.toml`
    pub(crate) fn config_path(&self) -> PathBuf {
        xdg_dir(self.xdg_config_home.as_deref(), &self.home, ".config")
            .join("senzu")
            .join("config.toml")
    }

    /// `$XDG_STATE_HOME/senzu`, else `~/.local/state/senzu`
    pub(crate) fn state_dir(&self) -> PathBuf {
        xdg_dir(self.xdg_state_home.as_deref(), &self.home, ".local/state").join("senzu")
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
}

/// flag > env (clap already merged those) > config > default.
/// the beans dir has no default. the launch default detects: zellij when
/// we're inside one, else exec.
pub(crate) fn resolve(over: Overrides, config: Config, env: &Env) -> Result<Settings, ConfigError> {
    let beans = over
        .beans
        .or(config.beans)
        .map(|p| expand_tilde(p, &env.home))
        .ok_or_else(|| ConfigError::NoBeans(env.config_path()))?;
    let agent = over
        .agent
        .or(config.agent)
        .unwrap_or_else(|| DEFAULT_AGENT.to_owned());
    let detected = if env.in_zellij { "zellij" } else { "exec" };
    let launch = over
        .launch
        .or(config.launch)
        .unwrap_or_else(|| detected.to_owned());
    Ok(Settings {
        beans,
        agent,
        launch,
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
            beans: Some("/from/flag".into()),
            agent: Some("flag-agent".into()),
            launch: Some("exec".into()),
        };
        let got = resolve(over, config("config-agent", "zellij"), &env(false)).unwrap();
        assert_eq!(
            got,
            Settings {
                beans: "/from/flag".into(),
                agent: "flag-agent".into(),
                launch: "exec".into(),
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
            }
        );
    }

    /// only the beans dir set, so everything else is a default
    fn beans_only() -> Overrides {
        Overrides {
            beans: Some("/b".into()),
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
            launch: Some("exec".into()),
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
    fn missing_config_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let got = Config::load(&dir.path().join("nope.toml")).unwrap();
        assert!(got.beans.is_none() && got.agent.is_none() && got.launch.is_none());
    }
}
