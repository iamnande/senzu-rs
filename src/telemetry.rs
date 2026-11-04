use std::path::Path;

use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

/// one file a day, `senzu.2026-11-03.log`, and only the last week kept.
/// the date is utc, so west of greenwich the evening's file is named for tomorrow
const KEEP_DAYS: usize = 7;

/// json lines to a file, never stdout, each with its span list. writes are
/// synchronous since exec and `process::exit` skip destructors, and a buffered
/// writer would drop the last lines. no state dir, no logs
pub(crate) fn init(state_dir: &Path) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let file = std::fs::create_dir_all(state_dir).ok().and_then(|()| {
        RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix("senzu")
            .filename_suffix("log")
            .max_log_files(KEEP_DAYS)
            .build(state_dir)
            .ok()
    });
    let layer = file.map(|file| {
        tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .with_span_events(FmtSpan::CLOSE)
            .with_writer(file)
    });
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .try_init();
}
