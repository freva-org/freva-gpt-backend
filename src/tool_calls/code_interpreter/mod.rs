// For the code interpreter, this module is responsible for interpreting the code and returning the result.

/// for parsing the input to the Code Interpreter.
pub mod prepare_execution;

/// For checking whether the code that was sent is safe to execute.
/// For now, it's a simple check, but we'll expand on this later.
pub mod safety_check;

/// For executing the code.
pub mod execute;

use async_openai::types::{ChatCompletionTool, ChatCompletionToolType, FunctionObject};
use once_cell::sync::Lazy;
use serde_json::json;
use tracing::{debug, warn};

/// The code interpreter as a tool.
/// Needed for the LLM to understand how to call the code interpreter.
pub static CODE_INTERPRETER_TOOL_TYPE: Lazy<ChatCompletionTool> =
    Lazy::new(|| ChatCompletionTool {
        r#type: ChatCompletionToolType::Function,
        function: CODE_INTERPRETER_FUNCTION.clone(),
    });

static CODE_INTERPRETER_FUNCTION: Lazy<FunctionObject> = Lazy::new(|| {
    FunctionObject {
    name: "code_interpreter".to_string(),
    description: Some(
        "Recieves python code, executes it in python kernel, and returns the result. Supports print statements and returning the last line."
            .to_string(),
    ), 
    parameters: Some(CODE_INTERPRETER_PARAMETER.clone()),
}
});

static CODE_INTERPRETER_PARAMETER: Lazy<serde_json::Value> = Lazy::new(|| {
    json!({
        "type" : "object",
        "properties" : {
            "code" : {
                "type" : "string",
                "description" : "The python code to be executed."
            }
        },
        "required" : ["code"]
    })
});

/// One of the things that the code interpreter needs is the path to the freva config file.
/// This function gets the path and makes sure we have access to it.
pub fn verify_can_access(freva_config_path: String) -> bool {
    // We'll try to read the file to see if we can access it.
    match std::fs::read_to_string(&freva_config_path) {
        Ok(content) => {
            debug!("Successfully read the freva config file: {:?}", content);
            true
        },
        Err(e) => {
            warn!("Error reading the freva config file: {:?}", e);
            false
        }
    }
}