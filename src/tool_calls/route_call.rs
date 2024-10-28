// Routes a tool call to the appropriate function.

use std::{fs::OpenOptions, io::Read};

use fs2::FileExt;
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

        let result = sender
            .send(start_code_interpeter(arguments, id, Some(thread_id)).await)
            .await;

        // Before sending the result, write out the content of tool logger.
        print_and_clear_tool_logs();
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
pub(crate) fn print_and_clear_tool_logs() {
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
                    debug!(
                        "Content of the tool logger file:\n {}",
                        String::from_utf8_lossy(&content)
                    );
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
