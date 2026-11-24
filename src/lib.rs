//! senzu is _the_ MHQ developer interface.
//!
//! the cli is accessed via [`run`], from `main.rs`.
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

/// iho, silver - away! (plz gimme args and env; sometimes, i vomit errors)
#[must_use]
pub fn run() -> std::process::ExitCode {
    cmd::main()
}
