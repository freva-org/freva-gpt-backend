use std::sync::{Mutex, OnceLock};

use flexi_logger::{
    style, Age, Cleanup, Criterion, FileSpec, LevelFilter, LogSpecification, Logger, LoggerHandle,
    Naming,
};

use crate::cla_parser; // imports the cla_parser module for the Args struct

// Stores the logger in a global variable to keep it alive.
static LOGGER: OnceLock<Mutex<LoggerHandle>> = OnceLock::new();

pub fn setup_logger(args: &cla_parser::Args) {
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

    // to keep the logger alive, we'll store it in a global variable
    if LOGGER.set(Mutex::new(logger)).is_err() {
        eprintln!("Error storing the logger in the global variable, logging will not work.");
        std::process::exit(1);
    }

    tracing::info!("Logger initialized successfully.");
}

/// Custom log message formatter: [timestamp]:[level] (module:line) message
pub fn format_log_message(
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

/// Temporarily sets the log level to error.
/// Useful for temporarily silencing the logger if a function is too verbose.
pub fn silence_logger() {
    match LOGGER.get() {
        None => {
            eprintln!("Logger not initialized.");
            std::process::exit(1);
        }
        Some(logger) => {
            let mut logger_guard = match logger.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    eprintln!("Error locking the logger: {e:?}");
                    std::process::exit(1);
                }
            };
            logger_guard.push_temp_spec(LogSpecification::off());
        }
    }
}

/// Resets the log level to what it was before silence_logger was called.
pub fn undo_silence_logger() {
    match LOGGER.get() {
        None => {
            eprintln!("Logger not initialized.");
            std::process::exit(1);
        }
        Some(logger) => {
            let mut logger_guard = match logger.lock() {
                Ok(guard) => guard,
                Err(e) => {
                    eprintln!("Error locking the logger: {e:?}");
                    std::process::exit(1);
                }
            };
            logger_guard.pop_temp_spec();
        }
    }
}
