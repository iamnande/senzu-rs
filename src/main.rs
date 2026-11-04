mod bean;
mod cli;
mod config;
mod load;
mod telemetry;

use std::path::Path;
use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Cmd};
use crate::config::{Config, ConfigError, Env, Overrides};

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

    let name = match cli.cmd {
        Some(Cmd::Check) => return check(&settings.beans),
        None => "senzu",
        Some(Cmd::Ls { .. }) => "ls",
        Some(Cmd::Show { .. }) => "show",
        Some(Cmd::Open { .. }) => "open",
    };
    anyhow::bail!("{name}: not built yet")
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

/// md beans are the old format, here until the migration clears them out
fn warn_markdown(count: usize) {
    if count > 0 {
        eprintln!("senzu: ignoring {count} markdown beans");
    }
}

/// config mistakes are usage mistakes (2), everything else is a plain failure (1)
fn exit_code(e: &anyhow::Error) -> ExitCode {
    if e.downcast_ref::<ConfigError>().is_some() {
        ExitCode::from(2)
    } else {
        ExitCode::FAILURE
    }
}
