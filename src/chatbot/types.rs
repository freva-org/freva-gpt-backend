use core::fmt;

use async_openai::types::{
    ChatCompletionMessageToolCall, ChatCompletionRequestAssistantMessage,
    ChatCompletionRequestMessage,
    ChatCompletionRequestUserMessage, ChatCompletionToolType, FunctionCall,
};
use documented::Documented;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, trace, warn};

#[derive(Debug, Clone)]
pub enum ConversationState {
    Streaming(String), // The String is the Path to the file of the freva config.
    Stopping,
    Ended,
}

/// When a thread is streaming, it is in the Streaming state. If nothing goes wrong, at the end, it will be in the Ended state.
/// If a request to stop it is sent, another thread will change the state to Stopping.
/// The thread that is streaming will check the state and if it is Stopping, it will stop the streaming and change the state to Ended.
#[derive(Debug, Clone)]
pub struct ActiveConversation {
    pub id: String,

    pub state: ConversationState,

    pub conversation: Conversation,

    pub last_activity: std::time::Instant, // The last time the conversation was active. If the conversation is inactive for too long, it will be ended.

    pub user_id: String, // The ID of the user, as sent from the frontend/client.
}

///
/// # Stream Variants
///
/// The different variants of the stream or Thread that can be sent to the client.
/// They are always sent as JSON strings in the format `{"variant": "variant_name", "content": "content"}`.
///
/// User: The input of the user, as a String.
///
/// Assistant: The output of the Assistant, as a String. Often Markdown, because the LLM can output Markdown.
/// Multiple messages of this variant after each other belong to the same message, but are broken up due to the stream.
///
/// Code: The code that the Assistant generated, as a String. It will be executed on the backend.
/// Currently, only Python is supported. The content is not formatted.
/// Due to how the LLM calls the code_interpreter, it will be contained within a json object in the following format:
/// `{"variant": "Code", "content": "{\"code\":\"LLM Code here\"}"`
///
/// CodeOutput: The output of the code that was executed, as a String. Also not formatted.
///
/// Image: An image that was generated during the conversation, as a String. The image is Base64 encoded.
/// An example of this would be a matplotlib plot. The image format should always be PNG.
///
/// ServerError: An error that occured on the server(backend) side, as a String. Contains the error message.
/// The client should realize that this error occured and handle it accordingly; ServerErrors should immeadiately be followed by a StreamEnd.
///
/// OpenAI Error: An error that occured on the OpenAI side, as a String. Contains the error message.
/// These are often for the rate limits, but can also be for other things, i.E. if the API is down or a tool call took too long.
///
/// CodeError: The Code from the LLM could not be executed or there was some other error while setting up the code execution.
///
/// StreamEnd: The Stream ended. Contains a reason as a String. This is always the last message of a stream.
/// If the last message is not a StreamEnd but the stream ended, it's an error from the server side and needs to be fixed.
///
/// ServerHint: The Server hints something to the client. This is primarily used for giving the thread_id, but also for warnings.
/// The Content is in JSON format, with the key being the hint and the value being the content. Currently, only the keys "thread_id" and "warning" are used.
/// An example for a ServerHint packet would be `{"variant": "ServerHint", "content": "{\"thread_id\":\"1234\"}"}`.
/// That means that the content needs to be parsed as JSON to get the actual content.
#[derive(Debug, Serialize, Deserialize, Clone, Documented, PartialEq, strum::VariantNames)]
#[serde(tag = "variant", content = "content")] // Makes it so that the variant names are inside the object and the content is held in the content field.
pub enum StreamVariant {
    /// The Prompt for the LLM, as JSON; not to be displayed to the user.
    Prompt(String),
    /// The Input of the user, as a String
    User(String),
    /// The Output of the Assistant, as a String or Strindelta. Often Markdown.
    Assistant(String),
    /// Code the Assistant generated, as a String or Stringdelta, as well as the ID of the Tool Call the Code belongs to. Python, no formatting.
    Code(String, String),
    /// The Output of the Code, as a String, verbatim, and the ID of the Tool Call it belongs to.
    CodeOutput(String, String),
    /// An image that was generated during the streaming
    Image(String),
    /// An error that occured on the server(backend) side, as a String
    ServerError(String),
    /// An error that occured on the `OpenAI` side, as a String
    OpenAIError(String),
    /// An error that occured during Code Executing, as a String. Note that this means that trying to start executing the code failed, not that the code itself failed.
    CodeError(String),
    /// The Stream ended. Contains a reason as a String.
    StreamEnd(String),
    /// The Server hints something to the client. Primarily used for giving the thread_id or warning the frontend. May later be used for other things.
    /// The content itself is in JSON format, with the key being the hint and the value being the content.
    ServerHint(String),
}

impl fmt::Display for StreamVariant {
    // A helper function to convert the StreamVariant to a String, will be used later when writing to the thread file.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let result = match self {
            Self::Prompt(s) => format!("Prompt:{s}"),
            Self::User(s) => format!("User:{s}"),
            Self::Assistant(s) => format!("Assistant:{s}"),
            Self::Code(s, id) => format!("Code:{s}:{id}"),
            Self::CodeOutput(s, id) => format!("CodeOutput:{s}:{id}"),
            Self::Image(s) => format!("Image:{s}"),
            Self::ServerError(s) => format!("ServerError:{s}"),
            Self::OpenAIError(s) => format!("OpenAIError:{s}"),
            Self::CodeError(s) => format!("CodeError:{s}"),
            Self::StreamEnd(s) => format!("StreamEnd:{s}"),
            Self::ServerHint(s) => format!("ServerHint:{s}"), // It's a JSON string, we can just write it as is.
        };
        write!(f, "{result:?}")
    }
}

/// A conversation that is not actively streaming, as a List of `StreamVariants`.
pub type Conversation = Vec<StreamVariant>;

#[derive(Debug, Clone)]
pub enum ConversionError {
    VariantHide(&'static str), // Some variants are only for the backend, so they should not be converted.
    ParseError(&'static str),  // An error occured during parsing the prompt.
    CodeCall(String, String),  // A Code Call was found, which needs to be handled differently.
}

/// A helper function to convert the `StreamVariant` to a `ChatCompletionRequestMessage`.
///
/// Converts the `StreamVariant` to a `ChatCompletionRequestMessage`, which is used to send the message to `OpenAI`.
/// This might fail because we can't convert all variants to a `ChatCompletionRequestMessage`.
impl TryInto<Vec<ChatCompletionRequestMessage>> for StreamVariant {
    type Error = ConversionError;

    fn try_into(self) -> Result<Vec<ChatCompletionRequestMessage>, Self::Error> {
        trace!(
            "Converting StreamVariant to ChatCompletionRequestMessage: {:?}",
            self
        );
        match self {
            Self::Prompt(s) => {
                // We cannot just put the prompt in the message, since it's not a valid message.
                // It consists of multiple messages, so we'll need to unpack them. 

                let prompt = if let Ok(p) = serde_json::from_str(&s) {
                    trace!("Input prompt: {:?}", s);
                    p
                } else {
                    // it's probably escaped, so we'll unescape it.
                    let s = unescape_string(&s);

                    trace!("Unescaped prompt: {:?}", s);

                    let prompt: Vec<ChatCompletionRequestMessage> = match serde_json::from_str(&s){
                        Ok(p) => p,
                        Err(e) => {
                            error!("Error converting prompt to ChatCompletionRequestMessage: {:?}", e);
                            return Err(ConversionError::ParseError("Error converting prompt to ChatCompletionRequestMessage."));
                        }
                    };
                    prompt
                };

                // For debugging, check whether the prompt is the same as we are currently using.
                if s == crate::chatbot::prompting::STARTING_PROMPT_JSON.to_string() {
                    trace!("Prompt is the same as the starting prompt.");
                } else {
                    warn!("Recieved prompt that is different from the current starting prompt. Did the prompt change?");
                };


                trace!("Prompt: {:?}", prompt);

                Ok(prompt)},
            Self::User(s) => Ok(vec![ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    name: Some("user".to_string()),
                    content: async_openai::types::ChatCompletionRequestUserMessageContent::Text(s),
                },
            )]),
            Self::Assistant(s) => Ok(vec![ChatCompletionRequestMessage::Assistant(
                ChatCompletionRequestAssistantMessage {
                    content: Some(async_openai::types::ChatCompletionRequestAssistantMessageContent::Text(s)),
                    name: Some("frevaGPT".to_string()),
                    ..Default::default()
                },
            )]),
            Self::Code(s, id) => Err(ConversionError::CodeCall(s, id)),
            Self::CodeOutput(s, id) => Ok(vec![ChatCompletionRequestMessage::Tool(
                async_openai::types::ChatCompletionRequestToolMessage {
                    tool_call_id: id,
                    content: async_openai::types::ChatCompletionRequestToolMessageContent::Text(s),
                })
            ]),
            Self::Image(_) => 
            // Ok(vec![ChatCompletionRequestMessage::System(
            //     ChatCompletionRequestSystemMessage {
            //         name: Some("Image".to_string()),
            //         content: async_openai::types::ChatCompletionRequestSystemMessageContent::Text("An image was successfully generated and is being shown to the user.".to_string()),
            //     },
            // )])
            
                    Err(ConversionError::VariantHide("ServerHint variants are only for use on the server side, not for the LLM.")) // TODO: Implement giving the LLM information about the image.
            ,
            Self::CodeError(_) | Self::OpenAIError(_) | Self::ServerError(_) => Err(ConversionError::VariantHide("Error variants should not be passed to the LLM, it doesn't need to know about them.")),
            Self::StreamEnd(_) => Err(ConversionError::VariantHide("StreamEnd variants are only for use on the server side, not for the LLM.")),
            Self::ServerHint(s) => {
                // The content is JSON, we check whether it's valid and that its key is either "thread_id" or "warning".
                let hint: serde_json::Value = match serde_json::from_str(&s) {
                    Ok(h) => h,
                    Err(e) => {
                        warn!("Error parsing ServerHint content, ignoring and passing value to client blindly: {:?}", e);
                        return Err(ConversionError::ParseError("Error parsing ServerHint content."));
                    }
                };
                // We expect the hint to be of type object
                if let serde_json::Value::Object(hint) = hint {
                    if hint.keys().next().is_none() {
                        warn!("ServerHint content is empty! Passing to the client nonetheless.");
                        return Err(ConversionError::ParseError("ServerHint content is empty."));
                    }
                    // We now know that the hint is a non-empty JSON object, we can return it.
                    // TODO: Is this correct? Should we really tell the LLM about the thread_id?
                    // Ok(vec![ChatCompletionRequestMessage::System(
                    //     ChatCompletionRequestSystemMessage {
                    //         name: Some("ServerHint".to_string()),
                    //         content: async_openai::types::ChatCompletionRequestSystemMessageContent::Text(s),
                    //     },
                    // )])
                    Err(ConversionError::VariantHide("ServerHint variants are only for use on the server side, not for the LLM."))
                } else {
                    warn!("ServerHint content is not an object, ignoring and passing value to client blindly.");
                    Err(ConversionError::ParseError("ServerHint content is not an object."))
                }
        }
        }
    }
}

/// A helper function to convert the `ChatCompletionRequestMessage` to a `StreamVariant`.
///
/// Again, this might not succeed because not all `ChatCompletionRequestMessage` can be converted to a `StreamVariant`.
impl TryFrom<ChatCompletionRequestMessage> for StreamVariant {
    type Error = &'static str;

    fn try_from(value: ChatCompletionRequestMessage) -> Result<Self, Self::Error> {
        trace!(
            "Converting ChatCompletionRequestMessage to StreamVariant: {:?}",
            value
        );
        match value {
            ChatCompletionRequestMessage::System(content) => {
                // As of currently, the system messages only contain the prompt and the image.
                match content.name {
                    None => {
                        error!("System Message contained no name.");
                        Err("System Message contained no name.")
                    }
                    Some(text) => {
                        match text.as_str() {
                            "Prompt" | "Image" => {
                                let text = match content.content {
                                    async_openai::types::ChatCompletionRequestSystemMessageContent::Text(s) => s,
                                    async_openai::types::ChatCompletionRequestSystemMessageContent::Array(vector) => {
                                        let mut text_vec = vec![]; // buffer the text fragments
                                        for elem in vector {
                                            let async_openai::types::ChatCompletionRequestSystemMessageContentPart::Text(s) = elem;
                                            text_vec.push(s.text);
                                            
                                        }
                                    text_vec.join("\n")
                                    }
                                };
                                Ok(Self::Prompt(text))
                            }
                            _ => Err("Unknown System Message type."),
                        }
                    }
                }
            }
            ChatCompletionRequestMessage::User(content) => {
                match content.content {
                    async_openai::types::ChatCompletionRequestUserMessageContent::Text(s) => {
                        // Standard Text from the User
                        Ok(Self::User(s))
                    }
                    async_openai::types::ChatCompletionRequestUserMessageContent::Array(vector) => {
                        // Unlikely to be used, but we'll handle it.

                        let mut text_vec = vec![];
                        for elem in vector {
                            if let async_openai::types::ChatCompletionRequestUserMessageContentPart::Text(s) = elem {
                            text_vec.push(s.text);
                            } else {
                                error!("User Message Array contained a non-Text variant.");
                                return Err("User Message Array contained a non-Text variant.");
                            }
                        }
                        let concat = text_vec.join("\n");

                        Ok(Self::User(concat))
                    }
                }
            }
            ChatCompletionRequestMessage::Assistant(content) => {
                // This should always be the case
                if content.name != Some("frevaGPT".to_string()) {
                    warn!(
                        "Assistant Message contained an unknown name instead of frevaGPT: {:?}",
                        content.name
                    );
                };

                // There should never be tool or function calls here
                #[allow(deprecated)]
                // We need to match on the function_call, despite it being a deprecated field. This silences that warning.
                if let (Some(_), _) | (_, Some(_)) = (content.tool_calls, content.function_call) {
                    error!("Tried to convert an Assistant Message that contained a tool or function call. This should not happen and is not supported.");
                    Err("Assistant Message contained a tool or function call. This should not happen and is not supported.")
                } else {
                    match content.content {
                        Some(s) => {
                            match s {
                                async_openai::types::ChatCompletionRequestAssistantMessageContent::Text(s) => {
                                    Ok(Self::Assistant(s))
                                },
                                async_openai::types::ChatCompletionRequestAssistantMessageContent::Array(vector) => {
                                    let mut text_vec = vec![];
                                    for elem in vector {
                                        // There are two variants, the text and refusal. We handle text as expected, and inform the user about refusal.
                                        match elem {
                                            async_openai::types::ChatCompletionRequestAssistantMessageContentPart::Text(s) => {
                                                text_vec.push(s.text);
                                            },
                                            async_openai::types::ChatCompletionRequestAssistantMessageContentPart::Refusal(s) => {
                                                warn!("Assistant Message contained a refusal: {:?}", s);
                                                text_vec.push(format!("\n(Assistant refused to generate text: {:?})\n", s));
                                            }
                                        }
                                    }
                                    let concat = text_vec.join("\n");

                                    Ok(Self::Assistant(concat))
                                }
                            }
                        }
                        None => {
                            warn!("Assistant Message contained no content.");
                            Ok(Self::Assistant(String::new()))
                        }
                    }
                }
            }
            ChatCompletionRequestMessage::Tool(content) => {
                // Route the Code Interpreter and Code Interpreter Output to the correct variants.

                // As an API change of this library, it can now also be an Array of Texts.
                let text = match content.content {
                    async_openai::types::ChatCompletionRequestToolMessageContent::Text(s) => s,
                    async_openai::types::ChatCompletionRequestToolMessageContent::Array(vector) => {
                        let mut text_vec = vec![];
                        for elem in vector {
                            let async_openai::types::ChatCompletionRequestToolMessageContentPart::Text(s) = elem;
                            text_vec.push(s.text);
                        }
                        text_vec.join("\n")
                    }
                };

                match content.tool_call_id.as_str() {
                    "Code Interpreter" | "Code Interpreter Output" => {
                        // We also need to check whether the tool_call_id is Code Interpreter or Code Interpreter Output.
                        match content.tool_call_id.as_str() {
                            "Code Interpreter" => Ok(Self::Code(text, content.tool_call_id)),
                            "Code Interpreter Output" => {
                                Ok(Self::CodeOutput(text, content.tool_call_id))
                            }
                            _ => Err("Unknown Tool Call ID."), // This is impossible
                        }
                    }
                    _ => {
                        warn!(
                            "Tool Message contained an unknown tool_call_id: {:?}",
                            content.tool_call_id
                        );
                        // We'll still give it to the assistant, he might need it.
                        // Depending on the implementation of the OpenAI API, this might result in an error from the LLM as we don't answer the tool call.
                        let retval = content.tool_call_id + ": " + &text;
                        Ok(Self::Assistant(retval))
                    }
                }
            }
            ChatCompletionRequestMessage::Function(content) => {
                warn!("Function Message received, this is deprecated and should not be used.");
                // We'll handle it just like an unknown tool call.
                let retval =
                    content.name + ": " + &content.content.unwrap_or("(no content)".to_string());
                Ok(Self::Assistant(retval))
            }
            ChatCompletionRequestMessage::Developer(chat_completion_request_developer_message) => {
                // The Developer message is like a system message, but more explicetly from the developer.
                // From the documentation, it should only be used in the context of reasoning models (o1, o1-mini, o3, o3-mini).
                // I doubt the distinction is useful for us, I'll just treat it as a system message.
                let text = match chat_completion_request_developer_message.content {
                    async_openai::types::ChatCompletionRequestDeveloperMessageContent::Text(s) => s,
                    async_openai::types::ChatCompletionRequestDeveloperMessageContent::Array(vector) => {
                        let mut text_vec = vec![];
                        for elem in vector {
                            text_vec.push(elem.text);
                        }
                        text_vec.join("\n")
                    }
                };
                warn!("Developer Message received, this shouldn't happen. Communication was build with System messages exclusively. Content: {:?}", text);
                Ok(Self::Prompt(text)) 
            },
        }
    }
}

/// Helper function to convert a Vec<StreamVariant> to a Vec<ChatCompletionRequestMessage>.
/// This is needed because a Code Variant needs to be incorporated into the Assistant CCRM.
pub fn help_convert_sv_ccrm(input: Vec<StreamVariant>) -> Vec<ChatCompletionRequestMessage> {
    let mut all_oai_messages = vec![];
    let mut assistant_message_buffer = None;

    for message in input {
        match std::convert::TryInto::<Vec<ChatCompletionRequestMessage>>::try_into(message) {
            Ok(temp) => {
                for temp in temp {
                    // If this message is an Assistant message, we need to handle the buffer.
                    if let ChatCompletionRequestMessage::Assistant(content) = temp {
                        // If the buffer is not empty, we need to push it to the output.
                        if let Some(buffer) = assistant_message_buffer.clone() {
                            all_oai_messages.push(ChatCompletionRequestMessage::Assistant(buffer));
                        }

                        // We'll set the buffer to the current message.
                        assistant_message_buffer = Some(content);
                    } else {
                        // If it's not an Assistant message, we'll push the buffer to the output and then push the message.
                        if let Some(buffer) = assistant_message_buffer.clone() {
                            all_oai_messages.push(ChatCompletionRequestMessage::Assistant(buffer));
                            assistant_message_buffer = None;
                        }
                        all_oai_messages.push(temp);
                    }
                }
            }
            Err(ConversionError::CodeCall(content, id)) => {
                // We need to use the Code Call to update the content of the buffer, or initialize it.
                let tool_call = ChatCompletionMessageToolCall {
                    id,
                    r#type: ChatCompletionToolType::Function,
                    function: FunctionCall {
                        name: "code_interpreter".to_string(),
                        arguments: content,
                    },
                };
                if let Some(buffer) = assistant_message_buffer.clone() {
                    assistant_message_buffer = Some(
                        // Set the tool call in the buffer.
                        ChatCompletionRequestAssistantMessage {
                            tool_calls: Some(vec![tool_call]),
                            ..buffer
                        },
                    );
                } else {
                    // If the buffer is empty, we'll initialize it.

                    assistant_message_buffer = Some(
                        // Set the tool call in the buffer.
                        ChatCompletionRequestAssistantMessage {
                            tool_calls: Some(vec![tool_call]),
                            content: None,
                            name: Some("frevaGPT".to_string()),
                            ..Default::default() // because else it complain that that field is deprecated.
                        },
                    );
                }
            }
            Err(ConversionError::ParseError(e)) => {
                warn!(
                    "Error parsing StreamVariant to ChatCompletionRequestMessage: {:?}",
                    e
                );
            }
            Err(ConversionError::VariantHide(e)) => {
                debug!("Hiding StreamVariant from LLM: {:?}", e);
            }
        };
    }

    // If the buffer is not empty, we need to push it to the output.
    if let Some(buffer) = assistant_message_buffer {
        all_oai_messages.push(ChatCompletionRequestMessage::Assistant(buffer));
    }

    all_oai_messages
}

/// A simple helper function to "unescape" a string.
/// This is needed because the prompt is escaped when it is sent to the frontend.
pub fn unescape_string(s: &str) -> String {
    s.replace("\\\"", "\"")
        .replace("\\\\", "\\")
        .replace("\\n", "\n")
}

#[cfg(test)]
mod tests {

    use crate::chatbot::prompting::{STARTING_PROMPT, STARTING_PROMPT_JSON};

    // The helper function to convert a StreamVariant to a ChatCompletionRequestMessage
    // has some problems, we'll test it here.
    use super::*;
    #[test]
    fn test_help_convert_sv_ccrm() {
        let input = vec![
            StreamVariant::Prompt(STARTING_PROMPT_JSON.to_string()),
            StreamVariant::ServerHint("{\"thread_id\": \"wLRFKFPcDgRJdZwSFBF82LWulvAaS5MR\"}".to_string()),            
            StreamVariant::User("plot a cirlce".to_string()),
            StreamVariant::Assistant("To plot a circle, we can use the `matplotlib` library to create a simple visualization. Let's create a plot with a circle centered at the origin (0, 0) with a specified radius. I'll use a radius of 1 for this example.\n\nLet's proceed with the code to generate this plot.".to_string()),
            StreamVariant::Code("{\n    \"code\": \"import matplotlib.pyplot as plt\\nimport numpy as np\\n\\n# Create a new figure\\nplt.figure(figsize=(6, 6))\\n\\n# Parameters for the circle\\nradius = 1\\nangle = np.linspace(0, 2 * np.pi, 100)  # 100 points around the circle\\n\\n# Circle coordinates\\nx = radius * np.cos(angle)\\ny = radius * np.sin(angle)\\n\\n# Plot the circle\\nplt.plot(x, y, label='Circle with radius 1', color='blue')\\nplt.xlim(-1.5, 1.5)\\nplt.ylim(-1.5, 1.5)\\nplt.gca().set_aspect('equal')  # Aspect ratio equal\\nplt.title('Plot of a Circle')\\nplt.xlabel('X-axis')\\nplt.ylabel('Y-axis')\\nplt.axhline(0, color='grey', lw=0.5, ls='--')  # Add x-axis\\nplt.axvline(0, color='grey', lw=0.5, ls='--')  # Add y-axis\\nplt.legend()\\nplt.grid()\\nplt.show()  \\n\"\n    }".to_string(), "call_13RrNWNbaziDd34bvPXpdrMV".to_string()),
            StreamVariant::CodeOutput("<module 'matplotlib.pyplot' from '/opt/conda/envs/env/lib/python3.12/site-packages/matplotlib/pyplot.py'>:call_13RrNWNbaziDd34bvPXpdrMV".to_string(), "call_13RrNWNbaziDd34bvPXpdrMV".to_string()),
            StreamVariant::Image("JUST A BASE64 STRING".to_string()),
            StreamVariant::Assistant("The plot above displays a circle centered at the origin (0, 0) with a radius of 1. The axes are set to be equal, ensuring that the circle appears proportional. \n\nIf you want to plot a circle with different parameters or need further visualizations, just let me know!".to_string()),
            StreamVariant::StreamEnd("Generation complete".to_string())
        ];
        let output = help_convert_sv_ccrm(input);
        assert_eq!(output.len(), STARTING_PROMPT.len() + 5); // The length is dependant on the prompt, so we'll have to make it depend on the prompt's length.
        assert_eq!(output[STARTING_PROMPT.len() + 1], ChatCompletionRequestMessage::Assistant(ChatCompletionRequestAssistantMessage {
            content: Some(async_openai::types::ChatCompletionRequestAssistantMessageContent::Text("To plot a circle, we can use the `matplotlib` library to create a simple visualization. Let's create a plot with a circle centered at the origin (0, 0) with a specified radius. I'll use a radius of 1 for this example.\n\nLet's proceed with the code to generate this plot.".to_string())),
            name: Some("frevaGPT".to_string()),
            tool_calls: Some(vec![ChatCompletionMessageToolCall{
                id: "call_13RrNWNbaziDd34bvPXpdrMV".to_string(),
                r#type: ChatCompletionToolType::Function,
                function: FunctionCall{
                    name: "code_interpreter".to_string(),
                    arguments: "{\n    \"code\": \"import matplotlib.pyplot as plt\\nimport numpy as np\\n\\n# Create a new figure\\nplt.figure(figsize=(6, 6))\\n\\n# Parameters for the circle\\nradius = 1\\nangle = np.linspace(0, 2 * np.pi, 100)  # 100 points around the circle\\n\\n# Circle coordinates\\nx = radius * np.cos(angle)\\ny = radius * np.sin(angle)\\n\\n# Plot the circle\\nplt.plot(x, y, label='Circle with radius 1', color='blue')\\nplt.xlim(-1.5, 1.5)\\nplt.ylim(-1.5, 1.5)\\nplt.gca().set_aspect('equal')  # Aspect ratio equal\\nplt.title('Plot of a Circle')\\nplt.xlabel('X-axis')\\nplt.ylabel('Y-axis')\\nplt.axhline(0, color='grey', lw=0.5, ls='--')  # Add x-axis\\nplt.axvline(0, color='grey', lw=0.5, ls='--')  # Add y-axis\\nplt.legend()\\nplt.grid()\\nplt.show()  \\n\"\n    }".to_string()
                }
            }]),
            ..Default::default()
        }));
    }

    #[test]
    fn test_help_convert_sv_ccrm_real_data() {
        // Instead of using constructed data, we'll actually read the data from a file.
        // In this case, from a file that, when read, triggered the error this test is supposed to catch.
        let input = crate::chatbot::thread_storage::read_thread("testthread"); // Always read from disk
        assert!(
            input.is_ok(),
            "Error reading test thread file: {:?}",
            input.err()
        );
        let input = input.expect("Error reading test thread file. Did you copy over the `testthread.txt` file to the threads folder?");
        let output = help_convert_sv_ccrm(input);
        assert_eq!(output.len(), 36);

        // "Assistant:To create an annual mean temperature global map plot for the year 2023 using the provided dataset, we will follow these steps:\n\n1. Load the temperature data for 2023.\n2. Calculate the annual mean temperature for that year.\n3. Create a global map plot of the mean temperature.\n\nLet's start by loading the temperature data and calculating the annual mean temperature for 2023."
        // "Code: {\r\n        \"code\": \"import xarray as xr\\nimport numpy as np\\nimport matplotlib.pyplot as plt\\n\\n# Load the specified dataset for the year 2023\\ntemperature_data = xr.open_dataset('/work/bm1159/XCES/data4xces/reanalysis/reanalysis/ECMWF/IFS/ERA5/mon/atmos/tas/r1i1p1/tas_Amon_reanalysis_era5_r1i1p1_20240101-20241231.nc')\\n\\n# Calculate the annual mean temperature for the year 2023\\ntemperature_mean_2023 = temperature_data['tas'].mean(dim='time')\\n\\n# Extract latitude and longitude for plotting\\nlon = temperature_data['lon']\\nlat = temperature_data['lat']\\n\\n# Create a global map plot of the mean temperature\\nplt.figure(figsize=(12, 6))\\nplt.contourf(lon, lat, temperature_mean_2023, levels=np.linspace(250, 310, 61), cmap='coolwarm', extend='both')\\nplt.colorbar(label='Mean Temperature (K)')\\nplt.title('Annual Mean Temperature (K) for 2023')\\nplt.xlabel('Longitude')\\nplt.ylabel('Latitude')\\nplt.show()\"\r\n    }:call_OgWOIoYgje39a1akMKmRyXeL"
        assert_eq!(output[33], ChatCompletionRequestMessage::Assistant(ChatCompletionRequestAssistantMessage {
            content: None,
            name: Some("frevaGPT".to_string()),
            tool_calls: Some(vec![ChatCompletionMessageToolCall{
                id: "call_7utCmjpQd9Jhys17aVRCyDFo".to_string(),
                r#type: ChatCompletionToolType::Function,
                function: FunctionCall{
                    name: "code_interpreter".to_string(),
                    arguments: "{\"code\":\"4 * 3\"}".to_string()
                }
            }]),
            ..Default::default()
        }));

        // The conversation doesn't do ServerHints anymore, so we'll check assistant without tool calls.
        assert_eq!(
            output[26],
            ChatCompletionRequestMessage::Assistant(ChatCompletionRequestAssistantMessage {
                content: Some(async_openai::types::ChatCompletionRequestAssistantMessageContent::Text("The exact size of the dataset is approximately 4500.61 MB.".to_string())),
                name: Some("frevaGPT".to_string()),
                tool_calls: None,
                ..Default::default()
            })
        );
    }
}
