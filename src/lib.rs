//! senzu: browse beans, launch an agent into one.
//!
//! the bean types and the loader are public, so another front end (a web ui)
//! can share them. the rest is the cli, reached through [`run`].
//!
//! an error's message already ends in its cause, so print it with `{}`.
//! anyhow's `{:#}` walks the cause chain and would say it twice.

pub mod bean;
pub mod load;

mod cli;
mod cmd;
mod config;
mod current;
mod launch;
mod output;
mod tabs;
mod telemetry;
mod tui;

/// run the cli with the process's args and env, and say how it went
#[must_use]
pub fn run() -> std::process::ExitCode {
    cmd::main()
}
