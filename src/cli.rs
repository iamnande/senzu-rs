use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::bean::{Phase, Priority};

/// senzu: browse beans, launch an agent into one
#[derive(Debug, Parser)]
#[command(name = "senzu", version, about)]
pub struct Cli {
    /// the beans dir
    #[arg(long, global = true, env = "SENZU_BEANS")]
    pub beans: Option<PathBuf>,

    /// the agent profile to launch
    #[arg(long, global = true, env = "SENZU_AGENT")]
    pub agent: Option<String>,

    /// how the agent opens: exec or zellij
    #[arg(long, global = true, env = "SENZU_LAUNCH")]
    pub launch: Option<String>,

    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
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
        /// print the argv and cwd as json, launch nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// validate every bean
    Check,
}
