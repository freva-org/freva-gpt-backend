// For the code interpreter, this module is responsible for interpreting the code and returning the result.

/// for parsing the input to the Code Interpreter.
pub mod parse_input;

/// For checking whether the code that was sent is safe to execute.
/// For now, it's a simple check, but we'll expand on this later.
pub mod safety_check;

/// For executing the code.
pub mod execute;

use async_openai::types::{ChatCompletionTool, ChatCompletionToolType, FunctionObject};
use once_cell::sync::Lazy;
use serde_json::json;

/// The code interpreter as a tool.
/// Needed for the LLM to understand how to call the code interpreter.
pub static CODE_INTERPRETER_TOOL_TYPE: Lazy<ChatCompletionTool> =
    Lazy::new(|| ChatCompletionTool {
        r#type: ChatCompletionToolType::Function,
        function: CODE_INTERPRETER_FUNCTION.clone(),
    });

static CODE_INTERPRETER_FUNCTION: Lazy<FunctionObject> = Lazy::new(|| FunctionObject {
    name: "code_interpreter".to_string(),
    description: Some(
        "Recieves python code, executes it in a jupyter environment, and returns the result."
            .to_string(),
    ), // This is technically a lie, but we simulate the main thing about the jupyter notebook: the last line is returned.
    parameters: Some(CODE_INTERPRETER_PARAMETER.clone()),
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
