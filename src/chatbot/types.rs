use core::fmt;
use std::time::Instant;

use serde::Serialize;

pub enum ConversationState {
    Streaming,
    Stopping,
    Ended(Instant),
}

// When a thread is streaming, it is in the Streaming state. If nothing goes wrong, at the end, it will be in the Ended state.
// If a request to stop it is sent, another thread will change the state to Stopping.
// The thread that is streaming will check the state and if it is Stopping, it will stop the streaming and change the state to Ended.

pub struct ActiveConversation {
    pub(crate) id: String, // Either the id as given by OpenAI or our internal id, maybe an Enum or `either` later. It's just an identified for while it's streaming, mainly for the stop request.

    pub state: ConversationState,

    pub(crate) conversation: Conversation,
}

/// The different variants of the stream that can be sent to the client.
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "variant", content = "content")] // Makes it so that the variant names are inside the object and the content is held in the content field.
pub enum StreamVariant {
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
    /// The Stream ended. May contain a reason as a String.
    StreamEnd(String),
}

impl fmt::Display for StreamVariant {
    // A helper function to convert the StreamVariant to a String, will be used later when writing to the thread file.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let result = match self {
            StreamVariant::User(s) => format!("User:{s}"),
            StreamVariant::Assistant(s) => format!("Assistant:{s}"),
            StreamVariant::Code(s) => format!("Code:{s}"),
            StreamVariant::CodeOutput(s) => format!("CodeOutput:{s}"),
            StreamVariant::Image(s) => format!("Image:{s}"),
            StreamVariant::ServerError(s) => format!("ServerError:{s}"),
            StreamVariant::OpenAIError(s) => format!("OpenAIError:{s}"),
            StreamVariant::CodeError(s) => format!("CodeError:{s}"),
            StreamVariant::StreamEnd(s) => format!("StreamEnd:{s}"),
        };
        write!(f, "{result:?}")
    }
}

/// A conversation that is not actively streaming, as a List of `StreamVariants`.
pub type Conversation = Vec<StreamVariant>;
