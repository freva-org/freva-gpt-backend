use core::fmt;
use std::time::Instant;

use async_openai::types::{
    ChatCompletionRequestAssistantMessage, ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage, ChatCompletionRequestUserMessage
};
use serde::Serialize;
use tracing::trace;

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

/// The different variants of the stream that can be sent to the client.
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "variant", content = "content")] // Makes it so that the variant names are inside the object and the content is held in the content field.
pub enum StreamVariant {
    /// The Prompt for the LLM, as a String; not to be sent to the client.
    Prompt(String),
    /// The Input of the user, as a String
    User(String),
    /// The Output of the Assistant, as a String or Strindelta. Often Markdown.
    Assistant(String),
    /// Code the Assistant generated, as a String or Stringdelta. Python, no formatting.
    Code(String),
    /// The Output of the Code, as a String, verbatim.
    CodeOutput(String),
    /// An image that was generated during the streaming TODO: mark that this is Base64 encoded
    Image(String),
    /// An error that occured on the server(backend) side, as a String
    ServerError(String),
    /// An error that occured on the `OpenAI` side, as a String
    OpenAIError(String),
    /// An error that occured during Code Executing, as a String. Note that this means that trying to start executing the code failed, not that the code itself failed.
    CodeError(String),
    /// The Stream ended. Contains a reason as a String.
    StreamEnd(String),
    //TODO: Add ClientHint variant
}

impl fmt::Display for StreamVariant {
    // A helper function to convert the StreamVariant to a String, will be used later when writing to the thread file.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let result = match self {
            Self::Prompt(s) => format!("Prompt:{s}"),
            Self::User(s) => format!("User:{s}"),
            Self::Assistant(s) => format!("Assistant:{s}"),
            Self::Code(s) => format!("Code:{s}"),
            Self::CodeOutput(s) => format!("CodeOutput:{s}"),
            Self::Image(s) => format!("Image:{s}"),
            Self::ServerError(s) => format!("ServerError:{s}"),
            Self::OpenAIError(s) => format!("OpenAIError:{s}"),
            Self::CodeError(s) => format!("CodeError:{s}"),
            Self::StreamEnd(s) => format!("StreamEnd:{s}"),
        };
        write!(f, "{result:?}")
    }
}

/// A conversation that is not actively streaming, as a List of `StreamVariants`.
pub type Conversation = Vec<StreamVariant>;

/// A helper function to convert the `StreamVariant` to a `ChatCompletionRequestMessage`.
///
/// Converts the `StreamVariant` to a `ChatCompletionRequestMessage`, which is used to send the message to `OpenAI`.
/// This might fail because we can't convert all variants to a `ChatCompletionRequestMessage`.
impl TryInto<ChatCompletionRequestMessage> for StreamVariant {
    type Error = &'static str;

    fn try_into(self) -> Result<ChatCompletionRequestMessage, Self::Error> {
        trace!("Converting StreamVariant to ChatCompletionRequestMessage: {:?}", self);
        match self {
            Self::Prompt(s) => Ok(ChatCompletionRequestMessage::System(
                ChatCompletionRequestSystemMessage {
                    name: Some("Prompt".to_string()),
                    content: s,
                },
            )),
            Self::User(s) => Ok(ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage {
                    name: Some("user".to_string()),
                    content: async_openai::types::ChatCompletionRequestUserMessageContent::Text(s),
                },
            )),
            Self::Assistant(s) => Ok(ChatCompletionRequestMessage::Assistant(
                ChatCompletionRequestAssistantMessage {
                    content: Some(s),
                    name: Some("frevaGPT".to_string()),
                    ..Default::default()
                },
            )),
            Self::Code(s) => Ok(ChatCompletionRequestMessage::Tool(
                async_openai::types::ChatCompletionRequestToolMessage {
                    tool_call_id: "Code Interpreter".to_string(),
                    content: s,
                })
            ),
            Self::CodeOutput(s) => Ok(ChatCompletionRequestMessage::Tool(
                async_openai::types::ChatCompletionRequestToolMessage {
                    tool_call_id: "Code Interpreter Output".to_string(),
                    content: s,
                })
            ),
            Self::Image(_) => Ok(ChatCompletionRequestMessage::System(
                ChatCompletionRequestSystemMessage {
                    name: Some("Image".to_string()),
                    content: "An image was successfully generated, but isn't displayed due to a lack of vision capabilities.".to_string(),
                },
            )),
            Self::CodeError(_) | Self::OpenAIError(_) | Self::ServerError(_) => Err("Error variants should not be passed to the LLM, it doesn't need to know about them."),
            Self::StreamEnd(_) => Err("StreamEnd variants are only for use on the server side, not for the LLM."),
        }
    }
}
