// For all the things that is needed in the background for the chatbot to work.

// Relays the files from this folder up

/// Contains all common types and implementations for the chatbot.
pub mod types;

/// Internal use: describes all available chatbots
pub mod available_chatbots;
/// Handles the stop request from the client.
pub mod stop;

/// Returns a thread as a list of strings
pub mod get_thread;

/// Internal use: handles the storing and retrieval of the streamed data
pub mod thread_storage;

/// Streams the response from the chatbot
pub mod stream_response;

/// Handles the logic for storing and using the global conversation state
pub mod handle_active_conversations;

/// Defines the prompts for the chatbot
pub mod prompting;

// Defines a few useful static variables that are used throughout the chatbot.

use std::sync::{Arc, Mutex};

use async_openai::config::OpenAIConfig;
use once_cell::sync::Lazy;

use types::ActiveConversation;

/// Because multiple threads need to work together and need to know about the conversations, this static variable holds information about all active conversation.
/// The Lazy and Arc are transparent, it can be accessed by locking the mutex and then accessing the Vec inside.
pub static ACTIVE_CONVERSATIONS: Lazy<Arc<Mutex<Vec<ActiveConversation>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

/// Because we shouldn't have to construct a new OpenAI client for every stream we start, we'll use this static variable to hold the client.
/// The Lazy is transparent, it can be accessed as-is.
pub static CLIENT: Lazy<async_openai::Client<OpenAIConfig>> = Lazy::new(|| {
    let config = async_openai::config::OpenAIConfig::new();
    async_openai::Client::with_config(config)
});
