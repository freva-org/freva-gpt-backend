// Routes a tool call to the appropriate function.

use std::{fs::OpenOptions, io::Read, time::UNIX_EPOCH};

use fs2::FileExt;
use itertools::Itertools;
use std::io::Write;
use tokio::sync::mpsc;
use tracing::{debug, error};

use crate::chatbot::types::StreamVariant;

use super::code_interpreter::prepare_execution::start_code_interpeter;

/// Routes a tool call to the appropriate function.
pub async fn route_call(
    func_name: String,
    arguments: Option<String>,
    id: String,
    thread_id: String,
    sender: mpsc::Sender<Vec<StreamVariant>>,
) {
    // // Placeholder to disable the code interpreter
    // let variant = StreamVariant::CodeOutput("The code interpreter was successfully called, but is currently disabled. Please wait for the next major version for it to be stabilized. ".to_string(), id);
    // return vec![variant];

    // We currently only support the code interpreter, so we'll check that the name is, in fact, the code interpreter.
    let senderror = if func_name == "code_interpreter" {
        // The functionality lies in the seperate module.

        // Debugging:
        // The code interpreter has a severe overhead that is quite inconsistent. In order to track it down, several points of interest will record when they are reached.
        let routing_pit = std::time::SystemTime::now(); // The point in time when the routing function is reached.

        let result = sender
            .send(start_code_interpeter(arguments, id, Some(thread_id)).await)
            .await;

        let return_pit = std::time::SystemTime::now(); // The point in time when the code interpreter returns.

        // Before sending the result, write out the content of tool logger.
        print_and_clear_tool_logs(routing_pit, return_pit);
        result
    } else {
        // If the function name is not recognized, we'll return an error message.
        let answer = vec![StreamVariant::CodeOutput(format!("The function '{func_name}' is not recognized. Currently, only \"code_interpreter\" is supported."), id)];
        sender.send(answer).await
    };

    if let Err(e) = senderror {
        error!("Failed to send the answer to the chatbot: {}", e);
    }
}

/// Helper function to read and delete the content of the tool logger file.
/// Returns (for debugging) a vector of all points in time that were reached during the code interpreter.
pub(crate) fn print_and_clear_tool_logs(
    routing_pit: std::time::SystemTime,
    return_pit: std::time::SystemTime,
) {
    debug!("Reading and clearing the tool logger file.");
    match OpenOptions::new()
        .read(true)
        .write(true)
        .open("logging_from_tools.log")
    {
        Err(e) => error!("Failed to open the tool logger file: {}", e),
        Ok(mut file) => {
            // To be sure that it doesn't fail, lock the file.
            if let Err(e) = file.lock_exclusive() {
                error!("Failed to lock the tool logger file: {}", e);
                return;
            }

            let mut content = Vec::new();
            if let Err(e) = file.read_to_end(&mut content) {
                error!("Failed to read the tool logger file: {}", e);
            } else {
                // If the content is not empty, log it to this process' logger.
                if !content.is_empty() {
                    let content_as_string = String::from_utf8_lossy(&content);

                    // Add a tab to the beginning of each line to make it more readable and distinguishable.
                    let to_write = content_as_string.replace("\n", "\n\t");
                    debug!("Content of the tool logger file:\n {}", to_write);

                    // Debugging: get all relevant points in time.
                    // They all end in OVERHEAD=XXXXXXX, where XXXXXXX is the time in nanoseconds.
                    let mut pits = Vec::new();
                    pits.push(routing_pit);
                    for line in content_as_string.lines() {
                        if let Some(overhead) = line.split_once("OVERHEAD=") {
                            if let Ok(overhead) = overhead.1.parse::<u64>() {
                                pits.push(UNIX_EPOCH + std::time::Duration::from_nanos(overhead));
                            }
                        }
                    }
                    pits.push(return_pit);

                    // We now have the starting, multiple intermediate, and ending points in time.
                    // Let's log them to the file "debug_overhead.log" (in CSV).
                    // We'll just append to the file, as it's not critical.
                    // Line format: "routing_pit,overhead1,overhead2,...,return_pit".
                    if let Ok(overhead_file) = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("debug_overhead.log")
                    {
                        let mut overhead_file = std::io::BufWriter::new(overhead_file);

                        // write!(overhead_file, "{:?},", routing_pit); // Just debug, don't care about the result.
                        // for pit in pits {
                        //     write!(overhead_file, "{:?},", pit);
                        // }
                        // writeln!(overhead_file, "{:?}", return_pit);
                        // I actually don't want the raw overheads, but the distances between them.
                        // So, I'll calculate the differences.
                        for (pit_1, pit_2) in pits.iter().tuple_windows() {
                            if let Ok(diff) = pit_2.duration_since(*pit_1) {
                                write!(overhead_file, "{:>25},", diff.as_micros()); // Debug only, we can throw away the result.
                            }
                        }
                        writeln!(overhead_file); // New line.
                    }
                }

                // Clear the content of the file.
                if let Err(e) = file.set_len(0) {
                    error!("Failed to clear the tool logger file: {}", e);
                }
                if let Err(e) = file.sync_all() {
                    error!("Failed to sync the tool logger file: {}", e);
                }
            }

            // Unlock the file.
            if let Err(e) = file.unlock() {
                error!("Failed to unlock the tool logger file: {}", e);
                error!("The content of the tool logger file might not be cleared and the file might remain locked.");
            }
        }
    }
}
