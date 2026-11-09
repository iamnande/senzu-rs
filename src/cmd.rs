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

use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory as _, FromArgMatches as _};

use crate::cli::{Cli, Cmd};
use crate::config::{self, Config, ConfigError, Env, Origin, Overrides};
use crate::load::{self, Harvest};
use crate::output::{self, Filter};
use crate::{current, launch, tabs, telemetry, tui};

/// parse, then log, then config, so config errors land in the log. a usage
/// error logs to the env's state dir, since the parse never got to --state.
/// one root span per run, closed with its exit code
pub(crate) fn main() -> ExitCode {
    let env = Env::from_process();
    // matches kept, so a setting can say whether a flag or the env set it
    let parsed = Cli::command().try_get_matches().and_then(|matches| {
        Cli::from_arg_matches(&matches)
            .map(|cli| (cli, matches))
            .map_err(|e| e.format(&mut Cli::command()))
    });
    let state = parsed.as_ref().ok().and_then(|(cli, _)| cli.state.clone());
    telemetry::init(&state.unwrap_or_else(|| env.state_dir()));
    let (mut cli, matches) = match parsed {
        Ok(parsed) => parsed,
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
    let over = overrides(&mut cli, &matches);
    let code = run(Parsed { cli, over }, &env, tty).unwrap_or_else(|e| {
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
        Some(Cmd::Config { .. }) => "config",
        None if tty => "tui",
        None => "current",
    }
}

/// a flag or env value, tagged with which of the two set it. the names come
/// from the arg itself, so they can't drift from `cli.rs`
fn layered<T>(value: Option<T>, matches: &ArgMatches, id: &str) -> Option<(T, Origin)> {
    let value = value?;
    let cmd = Cli::command();
    let arg = cmd.get_arguments().find(|a| a.get_id() == id);
    let env_var = arg.and_then(|a| a.get_env());
    let from = match (matches.value_source(id), env_var) {
        (Some(ValueSource::EnvVariable), Some(var)) => {
            Origin::Env(var.to_string_lossy().into_owned())
        }
        _ => Origin::Flag(format!(
            "--{}",
            arg.and_then(|a| a.get_long()).unwrap_or(id)
        )),
    };
    Some((value, from))
}

/// takes the settings out of the cli, each tagged with its layer
fn overrides(cli: &mut Cli, matches: &ArgMatches) -> Overrides {
    Overrides {
        beans: layered(cli.beans.take(), matches, "beans"),
        agent: layered(cli.agent.take(), matches, "agent"),
        launch: layered(cli.launch.take(), matches, "launch"),
        state: layered(cli.state.take(), matches, "state"),
        worktrees: layered(cli.worktrees.take(), matches, "worktrees"),
        branch_prefix: layered(cli.branch_prefix.take(), matches, "branch_prefix"),
        thread_channel: layered(cli.thread_channel.take(), matches, "thread_channel"),
    }
}

/// the cli, with its settings already taken out as overrides
struct Parsed {
    cli: Cli,
    over: Overrides,
}

fn run(parsed: Parsed, env: &Env, tty: bool) -> anyhow::Result<u8> {
    let Parsed { cli, over } = parsed;
    let settings = tracing::info_span!("config").in_scope(|| {
        let config = Config::load(&env.config_path())?;
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
        Some(Cmd::Config { json }) => {
            if json || !tty {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&output::setting_rows(&settings))?
                );
            } else {
                print!("{}", output::settings_table(&settings, &env.home));
            }
            Ok(0)
        }
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
