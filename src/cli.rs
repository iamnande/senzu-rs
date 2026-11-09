use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::bean::{Phase, Priority};

/// senzu: browse beans, launch an agent into one
#[derive(Debug, Parser)]
#[command(name = "senzu", version, about)]
pub(crate) struct Cli {
    /// the beans dir
    #[arg(long, global = true, env = "SENZU_BEANS")]
    pub(crate) beans: Option<PathBuf>,

    /// the agent profile to launch
    #[arg(long, global = true, env = "SENZU_AGENT")]
    pub(crate) agent: Option<String>,

    /// how the agent opens: exec or zellij
    #[arg(long, global = true, env = "SENZU_LAUNCH")]
    pub(crate) launch: Option<String>,

    /// where the logs and tab markers go. no config key, the log starts first
    #[arg(long, global = true, env = "SENZU_STATE")]
    pub(crate) state: Option<PathBuf>,

    /// the tui as a narrow list beside an agent, for the zellij tabs
    #[arg(long)]
    pub(crate) sidebar: bool,

    /// the bean whose tab this sidebar lives in. it starts selected, and the
    /// tab closes once the agent beside it exits
    #[arg(long, requires = "sidebar")]
    pub(crate) tab: Option<String>,

    #[command(subcommand)]
    pub(crate) cmd: Option<Cmd>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Cmd {
    /// one line per open stream, urgent and high first
    Ls {
        /// every priority, not just urgent and high
        #[arg(long)]
        all: bool,
        /// only this phase (done included, if you ask for it)
        #[arg(long)]
        phase: Option<Phase>,
        /// only this priority, in place of urgent and high
        #[arg(long)]
        priority: Option<Priority>,
        /// json, even on a tty
        #[arg(long)]
        json: bool,
    },
    /// one bean, in full
    Show {
        slug: String,
        /// json, even on a tty
        #[arg(long)]
        json: bool,
    },
    /// launch the agent in a bean's worktree
    Open {
        slug: String,
        /// print the command and cwd as json, launch nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// validate every bean
    Check,
    /// each setting, its value, and the layer that set it
    Config {
        /// json, even on a tty
        #[arg(long)]
        json: bool,
    },
}
