//! senzu: browse beans, launch an agent into one.
//!
//! `main.rs` is the cli on top. everything else lives here, so a second
//! front end (a web ui) can share the bean types and the loader.

pub mod bean;
pub mod cli;
pub mod config;
pub mod current;
pub mod launch;
pub mod load;
pub mod output;
pub mod telemetry;
pub mod tui;
