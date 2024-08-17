// Routes a tool call to the appropriate function.

use crate::chatbot::types::StreamVariant;

use super::code_interpreter::parse_input::start_code_interpeter;

/// Routes a tool call to the appropriate function.
pub fn route_call(func_name: String, arguments: Option<String>, id: String) -> Vec<StreamVariant> {
    // // We use a placeholder for now.
    // let variant = StreamVariant::CodeOutput("The code interpreter was successfully called, but is currently disabled. Please wait for the next major version for it to be stabilized. ".to_string(), id);
    // return vec![variant];

    // We currently only support the code interpreter, so we'll check that the name is, in fact, the code interpreter.
    if func_name == "code_interpreter" {
        // The functionality lies in the seperate module.

        // match catch_unwind(|| run_code_interpeter(arguments, id.clone())) { // I know this is a bad idea, but something goes wrong in C++, and I don't know what. It doesn't catch C++ aborts, but does catch Rust Panics Safety: Not really.
        //     Ok(variants) => variants,
        //     Err(e) => {
        //         // If the code interpreter panics, we'll return an error message.
        //         error!("The code interpreter panicked: {:?}", e);
        //         vec![StreamVariant::CodeOutput(format!("The code interpreter panicked: {:?}", e), id)] // This should never ever happen. If it does, we have a serious problem.
        //     }
        // }
        start_code_interpeter(arguments, id.clone())
    } else {
        // If the function name is not recognized, we'll return an error message.
        vec![StreamVariant::CodeOutput(format!("The function '{}' is not recognized. Currently, only \"code_interpreter\" is supported.", func_name), id)]
    }
}
