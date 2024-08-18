// Routes a tool call to the appropriate function.

use crate::chatbot::types::StreamVariant;

use super::code_interpreter::parse_input::start_code_interpeter;

/// Routes a tool call to the appropriate function.
pub fn route_call(func_name: String, arguments: Option<String>, id: String) -> Vec<StreamVariant> {
    // Placeholder to disable the code interpreter
    // let variant = StreamVariant::CodeOutput("The code interpreter was successfully called, but is currently disabled. Please wait for the next major version for it to be stabilized. ".to_string(), id);
    // return vec![variant];

    // We currently only support the code interpreter, so we'll check that the name is, in fact, the code interpreter.
    if func_name == "code_interpreter" {
        // The functionality lies in the seperate module.

        start_code_interpeter(arguments, id.clone())
    } else {
        // If the function name is not recognized, we'll return an error message.
        vec![StreamVariant::CodeOutput(format!("The function '{}' is not recognized. Currently, only \"code_interpreter\" is supported.", func_name), id)]
    }
}
