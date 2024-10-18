// Routes a tool call to the appropriate function.

use tokio::sync::mpsc;
use tracing::error;

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

        sender
            .send(start_code_interpeter(arguments, id, Some(thread_id)))
            .await
    } else {
        // If the function name is not recognized, we'll return an error message.
        let answer = vec![StreamVariant::CodeOutput(format!("The function '{func_name}' is not recognized. Currently, only \"code_interpreter\" is supported."), id)];
        sender.send(answer).await
    };

    if let Err(e) = senderror {
        error!("Failed to send the answer to the chatbot: {}", e);
    }
}
