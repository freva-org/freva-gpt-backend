use crate::cla_parser; // imports the cla_parser module for the Args struct

pub fn setup_logger(args: &cla_parser::Args) {
    let (file, filename) = generate_log_file(); // generates the log file base on the current time in nanoseconds

    tracing_subscriber::fmt()
        .with_max_level(match &args.verbose {
            0 => tracing::Level::INFO,
            1 => tracing::Level::DEBUG,
            _ => tracing::Level::TRACE, // more than 1 verbose flag
        })
        .with_writer(file)
        .init();

    tracing::info!("Logger initialized successfully.");
    println!("Logger initialized successfully. Logs will be written to {filename}");
}

fn generate_log_file() -> (std::fs::File, String) {
    // We want to log all our messages to a file that by convention is named after the current amount of nanoseconds since the Unix epoch.
    // Since it's inside a docker container, we just write to "/logs/log_NS.txt".
    // Maybe later we can also write to stdout?
    let time_ns =
        if let Ok(time) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            time.as_nanos()
        } else {
            // We can't fail here, but if we do, we'll just return a 404.
            eprintln!("Error getting the current time in nanoseconds.");
            404
        };

    // let log_file = format!("/logs/log_{}.txt", time_ns);
    let log_file = format!("./logs/log_{time_ns}.txt"); // TODO: Change this back! In production, this needs to be correct.

    let file = std::fs::File::create(&log_file).expect("Error creating the log file. Either the system clock moved backwards or the file system is full.");

    // Returns the file handle and the filename
    (file, log_file)
}
