use tracing::{trace, warn};

use crate::{chatbot::types::StreamVariant, tool_calls::code_interpreter::{execute::execute_code, safety_check::code_is_likely_safe}};


/// The main function to execute the code interpreter.
/// Takes in the arguments that were passed to the tool call as well as the id of the tool call (for the output).
/// Returns the output of the code interpreter as a Vector of StreamVariants.
pub fn run_code_interpeter(arguments: Option<String>, id: String) -> Vec<StreamVariant> {

    trace!("Running the code interpreter with the following arguments: {:?}", arguments);

    // First run the basic safety check.
    if !code_is_likely_safe(&arguments.clone().unwrap_or_default()) {
        // We don't want to give a potential attacker any information about why the code failed.
        return vec![StreamVariant::CodeOutput("An unexpected error occurred while running the code interpreter. Please try again.".to_string(), id)];
    }

    // Now, we have to convert the arguments from JSON to a struct.

    // First check whether the arguments are actually present, maybe the LLM forgot to include them.
    let code = if let Some(content) = arguments {
        content
    } else {
        warn!("No code was found while trying to run the code_interpreter.");
        return vec![StreamVariant::CodeOutput("No code was found while trying to run the code_interpreter. Please try again.".to_string(), id)];
    };

    // Now parse the JSON into a struct.
    let code  = match serde_json::from_str::<CodeInterpreterArguments>(&code) {
        Ok(parsed) => parsed,
        Err(e) => {
            warn!("Error parsing the code interpreter arguments: {:?}", e);
            return vec![StreamVariant::CodeOutput("The Input to the Code Interpreter was malformed and not valid JSON. Please try again.".to_string(), id)];
        }
    };

    trace!("Running the code interpreter with the following code: {}", code.code);

    let output = execute_code(code.code);
    
    // for now, we'll just return the output as a string. The code interpreter will later be able to return more complex data.
    let output = match output {
        Ok(output) => output,
        Err(output) => output,
    };
    vec![StreamVariant::CodeOutput(output, id)] 
}

/// Simple struct to ease the conversion from JSON to a struct.
#[derive(serde::Deserialize)]
#[derive(Debug)]
struct CodeInterpreterArguments {
    code: String,
}