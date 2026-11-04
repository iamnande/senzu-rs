use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
