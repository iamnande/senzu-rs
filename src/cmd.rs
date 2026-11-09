//! the commands: parse, resolve settings, dispatch. the only module that
//! prints, since stdout is output and the logs go to a file
#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "the cli's output lives here"
)]

use std::io::IsTerminal as _;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser as _;

use crate::cli::{Cli, Cmd};
use crate::config::{self, Config, ConfigError, Env, Overrides};
use crate::load::{self, Harvest};
use crate::output::{self, Filter};
use crate::{current, launch, tabs, telemetry, tui};

/// parse, then log, then config, so config errors land in the log. a usage
/// error logs to the env's state dir, since the parse never got to --state.
/// one root span per run, closed with its exit code
pub(crate) fn main() -> ExitCode {
    let env = Env::from_process();
    let parsed = Cli::try_parse();
    let state = parsed.as_ref().ok().and_then(|cli| cli.state.clone());
    telemetry::init(&state.unwrap_or_else(|| env.state_dir()));
    let cli = match parsed {
        Ok(cli) => cli,
        Err(e) => {
            // --help and --version come through here too, and aren't failures
            if e.use_stderr() {
                tracing::error!(kind = ?e.kind(), "usage");
            }
            e.exit();
        }
    };
    let tty = std::io::stdout().is_terminal();
    let root = tracing::info_span!(
        "senzu",
        cmd = cmd_name(cli.cmd.as_ref(), tty),
        version = env!("CARGO_PKG_VERSION"),
        exit_code = tracing::field::Empty,
    );
    let _root = root.enter();
    let code = run(cli, &env, tty).unwrap_or_else(|e| {
        tracing::error!(error = %e, "failed");
        eprintln!("senzu: {e}");
        exit_code(&e)
    });
    root.record("exit_code", code);
    ExitCode::from(code)
}

fn cmd_name(cmd: Option<&Cmd>, tty: bool) -> &'static str {
    match cmd {
        Some(Cmd::Ls { .. }) => "ls",
        Some(Cmd::Show { .. }) => "show",
        Some(Cmd::Open { .. }) => "open",
        Some(Cmd::Check) => "check",
        None if tty => "tui",
        None => "current",
    }
}

fn run(cli: Cli, env: &Env, tty: bool) -> anyhow::Result<u8> {
    let settings = tracing::info_span!("config").in_scope(|| {
        let config = Config::load(&env.config_path())?;
        let over = Overrides {
            beans: cli.beans,
            agent: cli.agent,
            launch: cli.launch,
            state: cli.state,
        };
        let settings = config::resolve(over, config, env)?;
        tracing::info!(
            beans = %settings.beans.display(),
            agent = %settings.agent,
            launch = %settings.launch,
            state = %settings.state.display(),
            "settings resolved"
        );
        anyhow::Ok(settings)
    })?;

    match cli.cmd {
        Some(Cmd::Check) => check(&settings.beans),
        Some(Cmd::Ls {
            all,
            phase,
            priority,
            json,
        }) => {
            let harvest = load_and_warn(&settings.beans)?;
            let filter = Filter {
                all,
                phase,
                priority,
            };
            let picked = output::select(&harvest.beans, &filter);
            if json || !tty {
                let rows: Vec<output::Row> = picked.into_iter().map(Into::into).collect();
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                print!("{}", output::ls_table(&picked, term_width()));
            }
            Ok(0)
        }
        Some(Cmd::Show { slug, json }) => {
            let found = load::one(&settings.beans, &slug)?;
            warn_unknown(&found);
            let envelope = found.envelope()?;
            if json || !tty {
                println!("{}", serde_json::to_string_pretty(&envelope)?);
            } else {
                print!("{}", output::show(&envelope, &env.home));
            }
            Ok(0)
        }
        Some(Cmd::Open { slug, dry_run }) => {
            let profile = launch::Profile::resolve(&settings.agent, &settings.launch)?;
            let bean = found(load::one(&settings.beans, &slug)?);
            let exe = launch::this_exe()?;
            let open_tabs = if profile.launcher == launch::Launcher::Zellij {
                tabs::open_names(env)
            } else {
                Vec::new()
            };
            let site = launch::Site {
                beans_dir: &settings.beans,
                exe: &exe,
                tabs: &open_tabs,
                state_dir: &settings.state,
            };
            let planned = launch::plan(&bean, profile, site)?;
            tracing::info!(program = %planned.program, args = ?planned.args, cwd = %planned.cwd.display(), "launch planned");
            if dry_run {
                println!("{}", serde_json::to_string_pretty(&planned)?);
                return Ok(0);
            }
            launch::preflight(&planned, profile.agent, env)?;
            launch::run(&planned)?;
            Ok(0)
        }
        None if tty => {
            // unknown profiles are a usage mistake, caught before the screen changes
            let profile = launch::Profile::resolve(&settings.agent, &settings.launch)?;
            let layout = if cli.sidebar {
                tui::Layout::Sidebar
            } else {
                tui::Layout::Full
            };
            let opts = tui::Opts {
                profile,
                layout,
                tab: cli.tab.as_deref(),
            };
            let dirs = tabs::Dirs {
                beans: &settings.beans,
                state: &settings.state,
            };
            tui::run(dirs, env, opts)?;
            Ok(0)
        }
        None => {
            let harvest = load_and_warn(&settings.beans)?;
            let cwd = std::env::current_dir()?;
            let here = current::matches(&harvest.beans, &cwd);
            if here.is_empty() {
                println!("no stream here");
            }
            for bean in here {
                print!("{}", output::brief(bean, &env.home));
            }
            Ok(0)
        }
    }
}

/// `ok N beans`, or one line per problem and exit 1
fn check(dir: &Path) -> anyhow::Result<u8> {
    let harvest = load::load(dir)?;
    warn_markdown(harvest.markdown);
    // check is the strict one: a field senzu doesn't know fails it
    if harvest.errors.is_empty() && harvest.unknown.is_empty() {
        println!("ok {} beans", harvest.beans.len());
        return Ok(0);
    }
    for e in harvest.errors.iter().chain(&harvest.unknown) {
        println!("{e}");
    }
    Ok(1)
}

/// load for reading: bad beans are a warning, the good ones carry on
fn load_and_warn(dir: &Path) -> anyhow::Result<Harvest> {
    let harvest = load::load(dir)?;
    warn_markdown(harvest.markdown);
    for e in &harvest.errors {
        eprintln!("senzu: skipping {e}");
    }
    for e in &harvest.unknown {
        eprintln!("senzu: {e}");
    }
    Ok(harvest)
}

/// a bean for reading: unknown fields are a warning, not a failure
fn found(found: load::Found) -> crate::bean::Bean {
    warn_unknown(&found);
    found.bean
}

fn warn_unknown(found: &load::Found) {
    for e in &found.unknown {
        eprintln!("senzu: {e}");
    }
}

/// md beans are the old format
fn warn_markdown(count: usize) {
    if count > 0 {
        eprintln!("senzu: ignoring {count} markdown beans");
    }
}

fn term_width() -> usize {
    crossterm::terminal::size().map_or(100, |(cols, _)| usize::from(cols))
}

/// config and profile mistakes are usage mistakes (2), an unknown slug is 3, the rest is 1
fn exit_code(e: &anyhow::Error) -> u8 {
    if e.downcast_ref::<ConfigError>().is_some()
        || e.downcast_ref::<launch::UnknownProfile>().is_some()
    {
        2
    } else if matches!(e.downcast_ref(), Some(load::OneError::Unknown(_))) {
        3
    } else {
        1
    }
}
