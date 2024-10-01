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

use tracing::{trace, warn};
use types::ActiveConversation;

/// Because multiple threads need to work together and need to know about the conversations, this static variable holds information about all active conversation.
/// The Lazy and Arc are transparent, it can be accessed by locking the mutex and then accessing the Vec inside.
pub static ACTIVE_CONVERSATIONS: Lazy<Arc<Mutex<Vec<ActiveConversation>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

/// Because we shouldn't have to construct a new OpenAI client for every stream we start, we'll use this static variable to hold the client.
/// The Lazy is transparent, it can be accessed as-is.
static OPENAI_CLIENT: Lazy<async_openai::Client<OpenAIConfig>> = Lazy::new(|| {
    let config = async_openai::config::OpenAIConfig::new();
    async_openai::Client::with_config(config)
});

/// We also need one for the Ollama client, because the API endpoint is dependent on the client.
static OLLAMA_CLIENT: Lazy<async_openai::Client<OpenAIConfig>> = Lazy::new(|| {
    let config = async_openai::config::OpenAIConfig::new().with_api_base("http://localhost:11434/v1").with_api_key("ollama");
    async_openai::Client::with_config(config)
});

/// We might want to talk to ollama. This is to check whether ollama is up. If it is, it'll return "Ollama is running".
/// Timeout is 200 milliseconds; it's on localhost:11434, the delay should be minimal.
pub fn is_ollama_running() -> bool {
    if let Ok(client) = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_millis(200))
        .build()
    {
        let response = client.get("http://localhost:11434/").send();
        if let Ok(response) = response {
            response.status().is_success()
        } else {
            false
        }
    } else {
        false
    }
}

/// Selects the correct client based on the chatbot that is requested.
pub fn select_client(
    chatbot: available_chatbots::AvailableChatbots,
) -> &'static async_openai::Client<OpenAIConfig> {
    match chatbot {
        available_chatbots::AvailableChatbots::OpenAI(_) => {
            trace!("Selecting OpenAI client");
            &OPENAI_CLIENT
        }
        available_chatbots::AvailableChatbots::Ollama(_) => {
            trace!("Selecting Ollama client");
            if !is_ollama_running() {
                warn!("Ollama is not running, but ollama couldn't be found! This might fail!");
            }
            &OLLAMA_CLIENT
        }
    }
}
