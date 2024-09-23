use flexi_logger::{
    style, Age, Cleanup, Criterion, FileSpec, LevelFilter, Logger, LoggerHandle, Naming,
};

use crate::cla_parser; // imports the cla_parser module for the Args struct

pub fn setup_logger(args: &cla_parser::Args) -> LoggerHandle {
    let loglevel = match args.verbose {
        0 => LevelFilter::Info,
        1 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    let logger = Logger::with(loglevel)
        .log_to_file(
            FileSpec::default()
                .directory("./logs")
                .basename("log")
                .suffix("txt"),
        )
        .format(format_log_message)
        .set_palette("b1;3;2;4;6".to_string())
        .rotate(
            Criterion::Age(Age::Hour),
            Naming::Timestamps,
            Cleanup::KeepLogFiles(7 * 24),
        ) // rotate every hour, keep logs for a week
        .write_mode(flexi_logger::WriteMode::Async) // write logs asynchronously to support tracing from multiple threads
        .duplicate_to_stderr(flexi_logger::Duplicate::Warn) // duplicate warnings and errors to stderr
        .start()
        .expect("Error initializing the logger."); // And fail if we can't initialize the logger.

    // to keep the logger alive, we need to return it to the main thread and keep it alive there

    tracing::info!("Logger initialized successfully.");
    logger
}

/// Custom log message formatter: [timestamp]:[level] (module:line) message
fn format_log_message(
    write: &mut dyn std::io::Write,
    now: &mut flexi_logger::DeferredNow,
    record: &flexi_logger::Record,
) -> std::io::Result<()> {
    let level = record.level();
    write!(
        write,
        "[{}]:{} ({}:{}) {}",
        now.format("%Y-%m-%d %H:%M:%S%.6f"),
        style(level).paint(format!("{:7}", format!("[{}]", level))), // paint the level in a color
        record.module_path().unwrap_or("<unnamed>"),                 // Module from tracing
        record.line().unwrap_or(0), // line number can help with debugging
        record.args()
    ) // the actual message
}
