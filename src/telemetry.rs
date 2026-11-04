use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// one file a day, `senzu.2026-11-03.log`, and only the last week kept.
/// the date is utc, so west of greenwich the evening's file is named for tomorrow
const KEEP_DAYS: usize = 7;

/// hold this until exit: dropping it flushes the file
pub struct Guard {
    _file: Option<WorkerGuard>,
}

/// logs go to a file and nowhere else, so stdout stays output. a dir we
/// can't make means no file layer, and the cli carries on without a diary.
pub fn init(state_dir: &Path) -> Guard {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let appender = std::fs::create_dir_all(state_dir).ok().and_then(|()| {
        RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix("senzu")
            .filename_suffix("log")
            .max_log_files(KEEP_DAYS)
            .build(state_dir)
            .ok()
    });
    let (file_layer, file_guard) = match appender {
        Some(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            // a line per closed span, with how long it took
            let layer = tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_span_events(FmtSpan::CLOSE);
            (Some(layer), Some(guard))
        }
        None => (None, None),
    };

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .try_init();

    Guard { _file: file_guard }
}
