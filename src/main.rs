mod bean;
mod cli;
mod config;
mod current;
mod load;
mod output;
mod telemetry;

use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;

use crate::bean::Bean;
use crate::cli::{Cli, Cmd};
use crate::config::{Config, ConfigError, Env, Overrides};
use crate::load::Harvest;
use crate::output::Filter;

/// a slug no bean has
#[derive(Debug, thiserror::Error)]
#[error("no bean named {0:?}")]
struct UnknownSlug(String);

fn main() -> ExitCode {
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
            let harvest = load_and_warn(&settings.beans)?;
            let bean = find(&harvest.beans, &slug)?;
            if json || !tty {
                println!("{}", serde_json::to_string_pretty(bean)?);
            } else {
                print!("{}", output::show(bean, &env.home));
            }
            Ok(ExitCode::SUCCESS)
        }
        Some(Cmd::Open { .. }) => anyhow::bail!("open: not built yet"),
        None if tty => anyhow::bail!("the tui isn't built yet"),
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

fn find<'a>(beans: &'a [Bean], slug: &str) -> Result<&'a Bean, UnknownSlug> {
    beans
        .iter()
        .find(|b| b.slug == slug)
        .ok_or_else(|| UnknownSlug(slug.to_owned()))
}

fn term_width() -> usize {
    crossterm::terminal::size()
        .map(|(cols, _)| usize::from(cols))
        .unwrap_or(100)
}

/// config mistakes are usage mistakes (2), an unknown slug is 3, the rest is 1
fn exit_code(e: &anyhow::Error) -> ExitCode {
    if e.downcast_ref::<ConfigError>().is_some() {
        ExitCode::from(2)
    } else if e.downcast_ref::<UnknownSlug>().is_some() {
        ExitCode::from(3)
    } else {
        ExitCode::FAILURE
    }
}
