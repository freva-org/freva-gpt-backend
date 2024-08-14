// Routes a tool call to the appropriate function.

use crate::chatbot::types::StreamVariant;


/// Routes a tool call to the appropriate function.
pub fn route_call(func_name: String, arguments: Option<String>) -> Vec<StreamVariant> {
    // We use a placeholder for now.
    let variant = StreamVariant::CodeOutput(format!("The code interpreter was successfully called, but is not yet implemented. The inputs to this function were: {} ; {:?}. @FrevaGPT, you may tell the user that the tool call was called and interpreted, but not yet implemented.", func_name, arguments));
    vec![variant]
}