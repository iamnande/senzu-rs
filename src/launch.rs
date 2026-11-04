use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::bean::{Bean, Phase};

/// an agent senzu knows how to start. a new agent is a new entry in AGENTS
pub struct Agent {
    pub name: &'static str,
    pub program: &'static str,
    args: fn(&Bean) -> Vec<String>,
}

pub const AGENTS: &[Agent] = &[Agent {
    name: "claude",
    program: "claude",
    args: claude_args,
}];

/// the phase skills are `/iam-<phase>` until the migration renames them `senzu-`
const SKILL_PREFIX: &str = "iam-";

/// open straight on the bean's next skill, so enter skips the router
fn claude_args(b: &Bean) -> Vec<String> {
    vec![
        "--name".into(),
        b.slug.clone(),
        format!("/{SKILL_PREFIX}{} {}", b.phase, b.slug),
    ]
}

/// where the agent opens
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Launcher {
    /// replace this process
    Exec,
    /// a new zellij pane in the worktree, senzu keeps running
    Zellij,
}

const LAUNCHERS: &[(&str, Launcher)] = &[("exec", Launcher::Exec), ("zellij", Launcher::Zellij)];

/// a name that isn't a profile. a usage mistake, so exit 2
#[derive(Debug, thiserror::Error)]
#[error("unknown {kind} {name:?}, known: {known}")]
pub struct UnknownProfile {
    kind: &'static str,
    name: String,
    known: String,
}

pub fn agent(name: &str) -> Result<&'static Agent, UnknownProfile> {
    AGENTS
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| UnknownProfile {
            kind: "agent",
            name: name.to_owned(),
            known: AGENTS.iter().map(|a| a.name).collect::<Vec<_>>().join(", "),
        })
}

pub fn launcher(name: &str) -> Result<Launcher, UnknownProfile> {
    LAUNCHERS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
        .ok_or_else(|| UnknownProfile {
            kind: "launcher",
            name: name.to_owned(),
            known: LAUNCHERS
                .iter()
                .map(|(n, _)| *n)
                .collect::<Vec<_>>()
                .join(", "),
        })
}

/// a launch is data: what to run, where, under which session name
#[derive(Debug, Serialize)]
pub struct Launch {
    pub launcher: Launcher,
    /// the full command, launcher wrapper included
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub session: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("{0} is done, nothing to open")]
    Done(String),
    #[error("{0} not found on PATH")]
    NotOnPath(String),
    #[error("--launch zellij only works inside zellij, try --launch exec")]
    NotInZellij,
    #[error("{program}: {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("zellij exited with {status}: {stderr}")]
    Zellij {
        status: std::process::ExitStatus,
        stderr: String,
    },
}

/// the agent and launcher a launch uses, resolved from flag, env, config or default
#[derive(Clone, Copy)]
pub struct Profile {
    pub agent: &'static Agent,
    pub launcher: Launcher,
}

impl Profile {
    /// unknown names are usage mistakes, caught before anything runs
    pub fn resolve(agent_name: &str, launcher_name: &str) -> Result<Self, UnknownProfile> {
        Ok(Self {
            agent: agent(agent_name)?,
            launcher: launcher(launcher_name)?,
        })
    }
}

/// cwd is the bean's first worktree that exists, else the beans dir's parent
pub fn plan(bean: &Bean, profile: Profile, beans_dir: &Path) -> Result<Launch, LaunchError> {
    let Profile { agent, launcher } = profile;
    if bean.phase == Phase::Done {
        return Err(LaunchError::Done(bean.slug.clone()));
    }
    let cwd = bean
        .worktree()
        .unwrap_or_else(|| fallback_cwd(beans_dir))
        .to_owned();
    let mut argv = vec![agent.program.to_owned()];
    argv.extend((agent.args)(bean));
    if launcher == Launcher::Zellij {
        let mut wrapped: Vec<String> = vec!["zellij".into(), "run".into(), "--cwd".into()];
        wrapped.push(cwd.display().to_string());
        wrapped.extend([
            "--name".into(),
            bean.slug.clone(),
            "--close-on-exit".into(),
            "--".into(),
        ]);
        wrapped.extend(argv);
        argv = wrapped;
    }
    Ok(Launch {
        launcher,
        argv,
        cwd,
        session: bean.slug.clone(),
    })
}

/// where a bean with no worktree opens: the beans dir's parent, usually the
/// repo the beans live in
pub fn fallback_cwd(beans_dir: &Path) -> &Path {
    beans_dir.parent().unwrap_or(beans_dir)
}

/// everything `run` needs from the environment, checked before anything starts
pub struct Host {
    pub path: Option<OsString>,
    pub in_zellij: bool,
}

impl Host {
    pub fn from_process() -> Self {
        Self {
            path: std::env::var_os("PATH"),
            in_zellij: std::env::var_os("ZELLIJ").is_some(),
        }
    }
}

/// everything that can be known wrong before starting anything: the agent and,
/// for zellij, the multiplexer on PATH and us inside it
pub fn preflight(launch: &Launch, agent: &Agent, host: &Host) -> Result<(), LaunchError> {
    let on_path = |p: &str| find_on_path(p, host.path.as_deref()).is_some();
    if !on_path(agent.program) {
        return Err(LaunchError::NotOnPath(agent.program.to_owned()));
    }
    if launch.launcher == Launcher::Zellij {
        if !host.in_zellij {
            return Err(LaunchError::NotInZellij);
        }
        if !on_path("zellij") {
            return Err(LaunchError::NotOnPath("zellij".into()));
        }
    }
    Ok(())
}

/// exec never returns on success. zellij returns once the pane is open.
/// zellij gets no stdin or stdout, since it would print its pane id over the
/// tui, and its stderr comes back in the error. call `preflight` first
#[tracing::instrument(name = "launch", skip_all, fields(session = %launch.session, launcher = ?launch.launcher))]
pub fn run(launch: &Launch) -> Result<(), LaunchError> {
    let (program, args) = launch.argv.split_first().expect("argv has a program");
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(&launch.cwd);
    let spawn = |source| LaunchError::Spawn {
        program: program.clone(),
        source,
    };
    match launch.launcher {
        Launcher::Exec => {
            use std::os::unix::process::CommandExt as _;
            Err(spawn(cmd.exec()))
        }
        Launcher::Zellij => {
            let out = cmd
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output()
                .map_err(spawn)?;
            if out.status.success() {
                return Ok(());
            }
            let stderr = String::from_utf8_lossy(&out.stderr);
            Err(LaunchError::Zellij {
                status: out.status,
                // the last line is usually the one that says what went wrong
                stderr: stderr.trim().lines().last().unwrap_or("").to_owned(),
            })
        }
    }
}

/// `which`, without the crate: the first executable file named `program` on PATH
fn find_on_path(program: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    std::env::split_paths(path?)
        .map(|dir| dir.join(program))
        .find(|p| {
            p.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(launcher: Launcher) -> Profile {
        Profile {
            agent: agent("claude").unwrap(),
            launcher,
        }
    }

    fn bean(phase: &str, repo: Option<&Path>) -> Bean {
        let repos = match repo {
            Some(p) => {
                serde_json::json!([{ "path": p, "repo": "crab-rs", "branch": null, "base": null, "note": null }])
            }
            None => serde_json::json!([]),
        };
        serde_json::from_value(serde_json::json!({
            "slug": "crab-shell", "title": "t", "phase": phase, "priority": "urgent",
            "issue": null, "thread": null, "plan": null, "repos": repos,
            "next": { "text": "n", "draft": null },
        }))
        .unwrap()
    }

    #[test]
    fn claude_opens_on_the_phase_skill() {
        let b = bean("execute", None);
        let got = plan(&b, profile(Launcher::Exec), Path::new("/s/beans")).unwrap();
        assert_eq!(
            got.argv,
            ["claude", "--name", "crab-shell", "/iam-execute crab-shell"]
        );
        assert_eq!(got.session, "crab-shell");
    }

    #[test]
    fn no_worktree_falls_back_to_the_beans_parent() {
        let b = bean("plan", Some(Path::new("/nowhere/at/all")));
        let got = plan(&b, profile(Launcher::Exec), Path::new("/s/beans")).unwrap();
        assert_eq!(got.cwd, Path::new("/s"));
    }

    #[test]
    fn an_existing_worktree_wins() {
        let dir = tempfile::tempdir().unwrap();
        let b = bean("plan", Some(dir.path()));
        let got = plan(&b, profile(Launcher::Exec), Path::new("/s/beans")).unwrap();
        assert_eq!(got.cwd, dir.path());
    }

    #[test]
    fn zellij_wraps_the_agent_in_a_named_pane() {
        let b = bean("plan", None);
        let got = plan(&b, profile(Launcher::Zellij), Path::new("/s/beans")).unwrap();
        assert_eq!(
            got.argv,
            [
                "zellij",
                "run",
                "--cwd",
                "/s",
                "--name",
                "crab-shell",
                "--close-on-exit",
                "--",
                "claude",
                "--name",
                "crab-shell",
                "/iam-plan crab-shell",
            ]
        );
    }

    #[test]
    fn done_beans_dont_launch() {
        let b = bean("done", None);
        let got = plan(&b, profile(Launcher::Exec), Path::new("/s/beans"));
        assert!(matches!(got, Err(LaunchError::Done(_))));
    }

    #[test]
    fn unknown_names_list_the_known_ones() {
        let err = agent("codex").err().unwrap().to_string();
        assert_eq!(err, "unknown agent \"codex\", known: claude");
        let err = launcher("tmux").err().unwrap().to_string();
        assert_eq!(err, "unknown launcher \"tmux\", known: exec, zellij");
    }

    #[test]
    fn path_lookup_wants_an_executable_file() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("crab");
        std::fs::write(&exe, "").unwrap();
        let path = Some(dir.path().as_os_str());
        assert!(find_on_path("crab", path).is_none(), "not executable yet");
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(find_on_path("crab", path), Some(exe));
        assert!(find_on_path("crab", None).is_none());
    }
}
