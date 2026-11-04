//! the commands: parse, resolve settings, dispatch. the only module that
//! prints, since stdout is output and the logs go to a file
#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "the cli's output lives here"
)]

use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Cmd};
use crate::config::{self, Config, ConfigError, Env, Overrides};
use crate::load::{self, Harvest};
use crate::output::{self, Filter};
use crate::{current, launch, telemetry, tui};

/// parse the args, run the command, turn any error into a message and an exit code
pub(crate) fn main() -> ExitCode {
    // clap handles --help, --version and usage errors (exit 2) on its own
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("senzu: {e:#}");
            exit_code(&e)
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    let env = Env::from_process();
    let config = Config::load(&env.config_path())?;
    let over = Overrides {
        beans: cli.beans,
        agent: cli.agent,
        launch: cli.launch,
    };
    let _guard = telemetry::init(&env.state_dir());
    let settings = config::resolve(over, config, &env)?;
    tracing::info!(
        beans = %settings.beans.display(),
        agent = %settings.agent,
        launch = %settings.launch,
        "settings resolved"
    );
    let tty = std::io::stdout().is_terminal();

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
            Ok(ExitCode::SUCCESS)
        }
        Some(Cmd::Show { slug, json }) => {
            let bean = load::one(&settings.beans, &slug)?;
            if json || !tty {
                println!("{}", serde_json::to_string_pretty(&bean)?);
            } else {
                print!("{}", output::show(&bean, &env.home));
            }
            Ok(ExitCode::SUCCESS)
        }
        Some(Cmd::Open { slug, dry_run }) => {
            let profile = launch::Profile::resolve(&settings.agent, &settings.launch)?;
            let bean = load::one(&settings.beans, &slug)?;
            let planned = launch::plan(&bean, profile, &settings.beans)?;
            tracing::info!(argv = ?planned.argv, cwd = %planned.cwd.display(), "launch planned");
            if dry_run {
                println!("{}", serde_json::to_string_pretty(&planned)?);
                return Ok(ExitCode::SUCCESS);
            }
            launch::preflight(&planned, profile.agent, &launch::Host::from_process())?;
            launch::run(&planned)?;
            Ok(ExitCode::SUCCESS)
        }
        None if tty => {
            // unknown profiles are a usage mistake, caught before the screen changes
            let profile = launch::Profile::resolve(&settings.agent, &settings.launch)?;
            tui::run(&settings.beans, &env.home, profile)?;
            Ok(ExitCode::SUCCESS)
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
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// `ok N beans`, or one line per problem and exit 1
fn check(dir: &Path) -> anyhow::Result<ExitCode> {
    let harvest = load::load(dir)?;
    warn_markdown(harvest.markdown);
    if harvest.errors.is_empty() {
        println!("ok {} beans", harvest.beans.len());
        return Ok(ExitCode::SUCCESS);
    }
    for e in &harvest.errors {
        println!("{e}");
    }
    Ok(ExitCode::FAILURE)
}

/// load for reading: bad beans are a warning, the good ones carry on
fn load_and_warn(dir: &Path) -> anyhow::Result<Harvest> {
    let harvest = load::load(dir)?;
    warn_markdown(harvest.markdown);
    for e in &harvest.errors {
        eprintln!("senzu: skipping {e}");
    }
    Ok(harvest)
}

/// md beans are the old format, here until the migration clears them out
fn warn_markdown(count: usize) {
    if count > 0 {
        eprintln!("senzu: ignoring {count} markdown beans");
    }
}

fn term_width() -> usize {
    crossterm::terminal::size().map_or(100, |(cols, _)| usize::from(cols))
}

/// config and profile mistakes are usage mistakes (2), an unknown slug is 3, the rest is 1
fn exit_code(e: &anyhow::Error) -> ExitCode {
    if e.downcast_ref::<ConfigError>().is_some()
        || e.downcast_ref::<launch::UnknownProfile>().is_some()
    {
        ExitCode::from(2)
    } else if matches!(e.downcast_ref(), Some(load::OneError::Unknown(_))) {
        ExitCode::from(3)
    } else {
        ExitCode::FAILURE
    }
}
