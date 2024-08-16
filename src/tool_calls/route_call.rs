// Routes a tool call to the appropriate function.

use crate::chatbot::types::StreamVariant;

use super::code_interpreter::parse_input::run_code_interpeter;


/// Routes a tool call to the appropriate function.
pub fn route_call(func_name: String, arguments: Option<String>, id: String) -> Vec<StreamVariant> {
    // // We use a placeholder for now.
    // let variant = StreamVariant::CodeOutput(format!("The code interpreter was successfully called, but is not yet implemented. The inputs to this function were: {} ; {:?}. @FrevaGPT, you may tell the user that the tool call was called and interpreted, but not yet implemented.", func_name, arguments), id);
    // vec![variant]

    // We currently only support the code interpreter, so we'll check that the name is, in fact, the code interpreter.
    if func_name == "code_interpreter" {
        // The functionality lies in the seperate module.
        
        run_code_interpeter(arguments, id)
    } else {
        // If the function name is not recognized, we'll return an error message.
        vec![StreamVariant::CodeOutput(format!("The function '{}' is not recognized. Currently, only \"code_interpreter\" is supported.", func_name), id)]
    }
}