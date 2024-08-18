// For all tool calls that the LLM is able to make.

/// Routes the tool call to the appropriate function.
pub mod route_call;

/// The code interpreter that recieves python code and returns the result
pub mod code_interpreter;

pub static ALL_TOOLS: once_cell::sync::Lazy<Vec<async_openai::types::ChatCompletionTool>> =
    once_cell::sync::Lazy::new(|| vec![code_interpreter::CODE_INTERPRETER_TOOL_TYPE.clone()]);
