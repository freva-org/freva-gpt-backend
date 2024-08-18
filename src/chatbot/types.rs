use core::fmt;
use std::time::Instant;

use async_openai::types::{
    ChatCompletionMessageToolCall, ChatCompletionRequestAssistantMessage,
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestUserMessage, ChatCompletionToolType, FunctionCall,
};
use documented::Documented;
use serde::Serialize;
use tracing::{debug, error, trace, warn};

#[derive(Debug, Clone)]
pub enum ConversationState {
    Streaming,
    Stopping,
    Ended(Instant),
}

/// When a thread is streaming, it is in the Streaming state. If nothing goes wrong, at the end, it will be in the Ended state.
/// If a request to stop it is sent, another thread will change the state to Stopping.
/// The thread that is streaming will check the state and if it is Stopping, it will stop the streaming and change the state to Ended.
#[derive(Debug, Clone)]
pub struct ActiveConversation {
    pub id: String, // Either the id as given by OpenAI or our internal id, maybe an Enum or `either` later. It's just an identified for while it's streaming, mainly for the stop request.

    pub state: ConversationState,

    pub conversation: Conversation,
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
#[derive(Debug, Serialize, Clone, Documented, strum::VariantNames)]
#[serde(tag = "variant", content = "content")] // Makes it so that the variant names are inside the object and the content is held in the content field.
pub enum StreamVariant {
    /// The Prompt for the LLM, as JSON; not to be sent to the client.
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

                // Sometimes `s` is escaped, so we'll need to unescape it. 
                let prompt = if let Ok(p) = serde_json::from_str(&s) {
                    trace!("Input prompt: {:?}", s);
                    p
                } else {
                    // it's probably escaped, so we'll unescape it.
                    let s = s.replace("\\\"", "\"");
                    let s = s.replace("\\\\", "\\");

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
                    content: Some(s),
                    name: Some("frevaGPT".to_string()),
                    ..Default::default()
                },
            )]),
            Self::Code(s, id) => Err(ConversionError::CodeCall(s, id)),
            Self::CodeOutput(s, id) => Ok(vec![ChatCompletionRequestMessage::Tool(
                async_openai::types::ChatCompletionRequestToolMessage {
                    tool_call_id: id,
                    content: s,
                })
            ]),
            Self::Image(_) => Ok(vec![ChatCompletionRequestMessage::System(
                ChatCompletionRequestSystemMessage {
                    name: Some("Image".to_string()),
                    content: "An image was successfully generated and is being shown to the user.".to_string(),
                },
            )]),
            Self::CodeError(_) | Self::OpenAIError(_) | Self::ServerError(_) => Err(ConversionError::VariantHide("Error variants should not be passed to the LLM, it doesn't need to know about them.")),
            Self::StreamEnd(_) => Err(ConversionError::VariantHide("StreamEnd variants are only for use on the server side, not for the LLM.")),
            Self::ServerHint(s) => {
                // We do check that only the thread_id is sent. 
                // let first_part = s.splitn(2, ':').collect::<Vec<&str>>()[0];
                let (first_part, _) = s.split_once(':').unwrap_or(("",""));
                if first_part != "thread_id" && first_part != "warning" {
                    warn!("ServerHint contained an unknown key: {:?}", first_part);
                    Err(ConversionError::ParseError("ServerHint contained an unknown key."))
            } else {
                // The ServerHint is only used to send the thread_id to the client, so we don't need to send it to OpenAI.
                Err(ConversionError::VariantHide("ServerHint variants are only for use on the server side, not for the LLM."))
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
                if Some("Prompt".to_string()) == content.name {
                    Ok(Self::Prompt(content.content))
                } else if Some("Image".to_string()) == content.name {
                    Ok(Self::Image(content.content))
                } else {
                    Err("Unknown System Message type.")
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
                        // let text_vec = vector.into_iter().map(|x| if let async_openai::types::ChatCompletionRequestMessageContentPart::Text(s) = x {
                        //         Ok(s.text)
                        //     } else {
                        //         error!("User Message Array contained a non-Text variant.");
                        //         Err("User Message Array contained a non-Text variant.")
                        //     }).collect::<Vec<_>>();

                        let mut text_vec = vec![];
                        for elem in vector {
                            if let async_openai::types::ChatCompletionRequestMessageContentPart::Text(s) = elem {
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
                if let (Some(_), _) | (_, Some(_)) = (content.tool_calls, content.function_call) {
                    error!("Tried to convert an Assistant Message that contained a tool or function call. This should not happen and is not supported.");
                    Err("Assistant Message contained a tool or function call. This should not happen and is not supported.")
                } else {
                    match content.content {
                        Some(s) => Ok(Self::Assistant(s)),
                        None => {
                            warn!("Assistant Message contained no content.");
                            Ok(Self::Assistant(String::new()))
                        }
                    }
                }
            }
            ChatCompletionRequestMessage::Tool(content) => {
                // Route the Code Interpreter and Code Interpreter Output to the correct variants.
                if content.tool_call_id == "Code Interpreter" {
                    Ok(Self::Code(content.content, content.tool_call_id))
                } else if content.tool_call_id == "Code Interpreter Output" {
                    Ok(Self::CodeOutput(content.content, content.tool_call_id))
                } else {
                    warn!(
                        "Tool Message contained an unknown tool_call_id: {:?}",
                        content.tool_call_id
                    );
                    // We'll still give it to the assistant, he might need it.
                    let retval = content.tool_call_id + ": " + &content.content;
                    Ok(Self::Assistant(retval))
                }
            }
            ChatCompletionRequestMessage::Function(content) => {
                warn!("Function Message received, this is deprecated and should not be used.");
                // We'll handle it just like an unknown tool call.
                let retval =
                    content.name + ": " + &content.content.unwrap_or("(no content)".to_string());
                Ok(Self::Assistant(retval))
            }
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
                if let Some(buffer) = assistant_message_buffer.clone() {
                    let tool_call = ChatCompletionMessageToolCall {
                        id,
                        r#type: ChatCompletionToolType::Function,
                        function: FunctionCall {
                            name: "code_interpreter".to_string(),
                            arguments: content,
                        },
                    };
                    assistant_message_buffer = Some(
                        // Set the tool call in the buffer.
                        ChatCompletionRequestAssistantMessage {
                            tool_calls: Some(vec![tool_call]),
                            ..buffer
                        },
                    )
                } else {
                    // If the buffer is empty, we'll initialize it.
                    let tool_call = ChatCompletionMessageToolCall {
                        id,
                        r#type: ChatCompletionToolType::Function,
                        function: FunctionCall {
                            name: "code_interpreter".to_string(),
                            arguments: content,
                        },
                    };
                    assistant_message_buffer = Some(
                        // Set the tool call in the buffer.
                        ChatCompletionRequestAssistantMessage {
                            tool_calls: Some(vec![tool_call]),
                            content: None,
                            name: Some("frevaGPT".to_string()),
                            ..Default::default() // because else it complain that that field is deprecated.
                        },
                    )
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
    if let Some(buffer) = assistant_message_buffer.clone() {
        all_oai_messages.push(ChatCompletionRequestMessage::Assistant(buffer));
    }

    all_oai_messages
}
