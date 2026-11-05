use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::bean::{Bean, Phase};
use crate::config::Env;
use crate::tabs;

/// an agent senzu knows how to start. a new agent is a new entry in AGENTS
pub(crate) struct Agent {
    pub(crate) name: &'static str,
    pub(crate) program: &'static str,
    args: fn(&Bean) -> Vec<String>,
}

pub(crate) const AGENTS: &[Agent] = &[Agent {
    name: "claude",
    program: "claude",
    args: claude_args,
}];

/// the phase skills are `/senzu-<phase>`
const SKILL_PREFIX: &str = "senzu-";

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
pub(crate) enum Launcher {
    /// replace this process
    Exec,
    /// a new zellij pane in the worktree, senzu keeps running
    Zellij,
}

const LAUNCHERS: &[(&str, Launcher)] = &[("exec", Launcher::Exec), ("zellij", Launcher::Zellij)];

/// a name that isn't a profile. a usage mistake, so exit 2
#[derive(Debug, thiserror::Error)]
#[error("unknown {kind} {name:?}, known: {known}")]
pub(crate) struct UnknownProfile {
    kind: &'static str,
    name: String,
    known: String,
}

pub(crate) fn agent(name: &str) -> Result<&'static Agent, UnknownProfile> {
    AGENTS
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| UnknownProfile {
            kind: "agent",
            name: name.to_owned(),
            known: AGENTS.iter().map(|a| a.name).collect::<Vec<_>>().join(", "),
        })
}

pub(crate) fn launcher(name: &str) -> Result<Launcher, UnknownProfile> {
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
pub(crate) struct Launch {
    pub(crate) launcher: Launcher,
    /// the bean already has a tab, so this goes back to it
    pub(crate) existing_tab: bool,
    /// what runs: the agent, or zellij making or finding its tab
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
    pub(crate) cwd: PathBuf,
    pub(crate) session: String,
    /// touched right before a new tab opens, so the sidebars know when it did
    #[serde(skip)]
    pub(crate) marker: Option<PathBuf>,
}

/// what a plan needs from around the bean
#[derive(Clone, Copy)]
pub(crate) struct Site<'a> {
    pub(crate) beans_dir: &'a Path,
    /// this senzu, run again as the new tab's sidebar
    pub(crate) exe: &'a Path,
    /// the zellij tabs open now, by name
    pub(crate) tabs: &'a [String],
    /// where the tab markers live
    pub(crate) state_dir: &'a Path,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum LaunchError {
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
pub(crate) struct Profile {
    pub(crate) agent: &'static Agent,
    pub(crate) launcher: Launcher,
}

impl Profile {
    /// unknown names are usage mistakes, caught before anything runs
    pub(crate) fn resolve(agent_name: &str, launcher_name: &str) -> Result<Self, UnknownProfile> {
        Ok(Self {
            agent: agent(agent_name)?,
            launcher: launcher(launcher_name)?,
        })
    }
}

/// cwd is the bean's first worktree that exists, else the beans dir's parent
/// for zellij, a bean with a tab goes back to it, and one without gets a new
/// tab: the sidebar on the left, the agent on the right
pub(crate) fn plan(bean: &Bean, profile: Profile, site: Site) -> Result<Launch, LaunchError> {
    let Profile { agent, launcher } = profile;
    let slug = &bean.slug;
    if bean.phase == Phase::Done {
        return Err(LaunchError::Done(slug.clone()));
    }
    let cwd = bean
        .worktree()
        .unwrap_or_else(|| fallback_cwd(site.beans_dir))
        .to_owned();
    let mut planned = Launch {
        launcher,
        existing_tab: false,
        program: agent.program.to_owned(),
        args: (agent.args)(bean),
        cwd,
        session: slug.clone(),
        marker: None,
    };
    if launcher == Launcher::Exec {
        return Ok(planned);
    }
    let args: Vec<String> = if site.tabs.contains(slug) {
        planned.existing_tab = true;
        vec!["action".into(), "go-to-tab-name".into(), slug.clone()]
    } else {
        let sidebar: Vec<String> = [
            site.exe.display().to_string(),
            "--beans".into(),
            site.beans_dir.display().to_string(),
            "--state".into(),
            site.state_dir.display().to_string(),
            "--agent".into(),
            agent.name.into(),
            "--launch".into(),
            "zellij".into(),
            "--sidebar".into(),
            "--tab".into(),
            slug.clone(),
        ]
        .into();
        let mut agent_cmd = vec![planned.program.clone()];
        agent_cmd.extend(planned.args.iter().cloned());
        planned.marker = Some(tabs::marker(site.state_dir, slug));
        vec![
            "action".into(),
            "new-tab".into(),
            "--cwd".into(),
            planned.cwd.display().to_string(),
            "--layout-string".into(),
            tab_layout(slug, &sidebar, &agent_cmd),
        ]
    };
    planned.program = "zellij".into();
    planned.args = args;
    Ok(planned)
}

/// the sidebar plus zellij's pane frame
const TAB_SIDEBAR_COLS: u16 = 38;

/// the tab, with `{slots}` to fill
const TAB_LAYOUT: &str = include_str!("tab.kdl");

/// the agent takes focus and its pane closes when it exits, then the sidebar
/// sees it gone and closes the tab. q closes the sidebar pane, and the agent
/// takes the room. with the agent alone, zellij closes the tab when it exits
fn tab_layout(slug: &str, sidebar: &[String], agent: &[String]) -> String {
    let name = kdl(slug);
    let cols = TAB_SIDEBAR_COLS.to_string();
    let sidebar = command_node(sidebar);
    let agent = command_node(agent);
    fill(
        TAB_LAYOUT,
        &[
            ("name", &name),
            ("sidebar_cols", &cols),
            ("sidebar", &sidebar),
            ("agent", &agent),
        ],
    )
}

/// swap each `{slot}` for its value in one pass, so a value that happens to
/// hold `{agent}` stays as written. kdl's own braces aren't slots, so they stay
fn fill(template: &str, slots: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some((before, after)) = rest.split_once('{') {
        out.push_str(before);
        let filled = after.split_once('}').and_then(|(slot, tail)| {
            slots
                .iter()
                .find(|(name, _)| *name == slot)
                .map(|(_, value)| (*value, tail))
        });
        if let Some((value, tail)) = filled {
            out.push_str(value);
            rest = tail;
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// this binary, for the new tab's sidebar. after `cargo install` replaces it,
/// linux names the running one `<path> (deleted)`, and the new one sits at the
/// plain path, which is the one to run
pub(crate) fn this_exe() -> std::io::Result<PathBuf> {
    std::env::current_exe().map(undeleted)
}

fn undeleted(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|p| p.strip_suffix(" (deleted)")) {
        Some(p) => PathBuf::from(p),
        None => path,
    }
}

/// `command="p" { args "a" "b"; }`, or just the command when there are no args
fn command_node(cmd: &[String]) -> String {
    let Some((program, args)) = cmd.split_first() else {
        return String::new();
    };
    let program = kdl(program);
    if args.is_empty() {
        return format!("command={program}");
    }
    let args: Vec<String> = args.iter().map(|a| kdl(a)).collect();
    format!("command={program} {{ args {}; }}", args.join(" "))
}

/// a quoted kdl string
fn kdl(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// where a bean with no worktree opens: the beans dir's parent, usually the
/// repo the beans live in
pub(crate) fn fallback_cwd(beans_dir: &Path) -> &Path {
    beans_dir.parent().unwrap_or(beans_dir)
}

/// everything that can be known wrong before starting anything: the agent and,
/// for zellij, the multiplexer on PATH and us inside it
pub(crate) fn preflight(launch: &Launch, agent: &Agent, env: &Env) -> Result<(), LaunchError> {
    let on_path = |p: &str| find_on_path(p, env.path.as_deref()).is_some();
    if !on_path(agent.program) {
        return Err(LaunchError::NotOnPath(agent.program.to_owned()));
    }
    if launch.launcher == Launcher::Zellij {
        if !env.in_zellij {
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
pub(crate) fn run(launch: &Launch) -> Result<(), LaunchError> {
    let mut cmd = Command::new(&launch.program);
    cmd.args(&launch.args).current_dir(&launch.cwd);
    let spawn = |source| LaunchError::Spawn {
        program: launch.program.clone(),
        source,
    };
    match launch.launcher {
        Launcher::Exec => {
            use std::os::unix::process::CommandExt as _;
            Err(spawn(cmd.exec()))
        }
        Launcher::Zellij => {
            if let Some(marker) = &launch.marker {
                // without it the tab just never reads as unwritten, so carry on
                if let Err(e) = touch(marker) {
                    tracing::error!(error = %e, marker = %marker.display(), "tab marker");
                }
            }
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

/// make the file if it's missing, and set its mtime to now either way
fn touch(path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::File::create(path)?.set_modified(std::time::SystemTime::now())
}

/// `which`, without the crate: the first executable file named `program` on PATH
pub(crate) fn find_on_path(program: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
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
            "schema": 1, "slug": "crab-shell", "title": "t", "phase": phase, "priority": "urgent",
            "issue": null, "thread": null, "plan": null, "repos": repos,
            "next": { "text": "n", "draft": null },
        }))
        .unwrap()
    }

    fn site(tabs: &'static [String]) -> Site<'static> {
        Site {
            beans_dir: Path::new("/s/beans"),
            exe: Path::new("/bin/senzu"),
            tabs,
            state_dir: Path::new("/state"),
        }
    }

    #[test]
    fn claude_opens_on_the_phase_skill() {
        let b = bean("execute", None);
        let got = plan(&b, profile(Launcher::Exec), site(&[])).unwrap();
        assert_eq!(got.program, "claude");
        assert_eq!(
            got.args,
            ["--name", "crab-shell", "/senzu-execute crab-shell"]
        );
        assert_eq!(got.session, "crab-shell");
    }

    #[test]
    fn no_worktree_falls_back_to_the_beans_parent() {
        let b = bean("plan", Some(Path::new("/nowhere/at/all")));
        let got = plan(&b, profile(Launcher::Exec), site(&[])).unwrap();
        assert_eq!(got.cwd, Path::new("/s"));
    }

    #[test]
    fn an_existing_worktree_wins() {
        let dir = tempfile::tempdir().unwrap();
        let b = bean("plan", Some(dir.path()));
        let got = plan(&b, profile(Launcher::Exec), site(&[])).unwrap();
        assert_eq!(got.cwd, dir.path());
    }

    #[test]
    fn zellij_opens_a_tab_with_the_sidebar_and_the_agent() {
        let b = bean("plan", None);
        let got = plan(&b, profile(Launcher::Zellij), site(&[])).unwrap();
        assert_eq!(got.program, "zellij");
        assert!(!got.existing_tab);
        assert_eq!(
            got.marker.as_deref(),
            Some(Path::new("/state/tabs/crab-shell"))
        );
        let [action, new_tab, cwd_flag, cwd, layout_flag, layout] = got.args.as_slice() else {
            panic!("{:?}", got.args);
        };
        assert_eq!(
            [action, new_tab, cwd_flag, cwd, layout_flag],
            ["action", "new-tab", "--cwd", "/s", "--layout-string"]
        );
        assert!(
            layout.contains(r#"tab name="crab-shell" focus=true"#),
            "{layout}"
        );
        assert!(
            layout.contains(
                r#"pane size=38 name="senzu" close_on_exit=true command="/bin/senzu" { args "--beans" "/s/beans" "--state" "/state" "--agent" "claude" "--launch" "zellij" "--sidebar" "--tab" "crab-shell"; }"#
            ),
            "{layout}"
        );
        assert!(
            layout.contains(
                r#"pane name="crab-shell" focus=true close_on_exit=true command="claude" { args "--name" "crab-shell" "/senzu-plan crab-shell"; }"#
            ),
            "{layout}"
        );
        assert!(layout.contains("zellij:tab-bar") && layout.contains("zellij:status-bar"));
    }

    #[test]
    fn zellij_goes_back_to_a_tab_thats_open() {
        static TABS: std::sync::LazyLock<Vec<String>> =
            std::sync::LazyLock::new(|| vec!["senzu".into(), "crab-shell".into()]);
        let b = bean("plan", None);
        let got = plan(&b, profile(Launcher::Zellij), site(&TABS)).unwrap();
        assert!(got.existing_tab);
        assert_eq!(got.marker, None, "the open time stays the first open's");
        assert_eq!(got.args, ["action", "go-to-tab-name", "crab-shell"]);
    }

    #[test]
    fn kdl_strings_escape_quotes_and_backslashes() {
        assert_eq!(kdl(r#"a "b" \c"#), r#""a \"b\" \\c""#);
        assert_eq!(kdl("one\ntwo"), r#""one\ntwo""#);
        assert_eq!(command_node(&["x".into()]), r#"command="x""#);
    }

    #[test]
    fn done_beans_dont_launch() {
        let b = bean("done", None);
        let got = plan(&b, profile(Launcher::Exec), site(&[]));
        assert!(matches!(got, Err(LaunchError::Done(_))));
    }

    #[test]
    fn fill_swaps_slots_once_and_leaves_kdl_braces() {
        let got = fill(
            "pane {sidebar} {\n    {agent}\n} {nope}",
            &[("sidebar", "\"{agent}\""), ("agent", "claude")],
        );
        assert_eq!(got, "pane \"{agent}\" {\n    claude\n} {nope}");
    }

    #[test]
    fn the_tab_layout_has_no_slots_left() {
        let layout = tab_layout("crab-shell", &["/bin/senzu".into()], &["claude".into()]);
        for slot in ["{name}", "{sidebar_cols}", "{sidebar}", "{agent}"] {
            assert!(!layout.contains(slot), "{slot} left in:\n{layout}");
        }
    }

    #[test]
    fn a_replaced_binary_runs_the_new_one() {
        let got = undeleted(PathBuf::from("/home/crab/.cargo/bin/senzu (deleted)"));
        assert_eq!(got, Path::new("/home/crab/.cargo/bin/senzu"));
        let got = undeleted(PathBuf::from("/home/crab/.cargo/bin/senzu"));
        assert_eq!(got, Path::new("/home/crab/.cargo/bin/senzu"));
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
