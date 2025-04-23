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

/// Internal use: handles the storing and retrieval of the streamed data in a mongoDB database
pub mod mongodb_storage;

/// Streams the response from the chatbot
pub mod stream_response;

/// Routes requests to the storage backend (disk or mongoDB)
pub mod storage_router;

/// Handles the logic for storing and using the global conversation state
pub mod handle_active_conversations;

/// Defines the prompts for the chatbot
pub mod prompting;

/// The endpoint for returning the available chatbots
pub mod available_chatbots_endpoint;

/// Internally used to handle the heartbeat that is happening while the code interpreter is running.
pub mod heartbeat;

// Defines a few useful static variables that are used throughout the chatbot.

use std::sync::{Arc, Mutex};

use async_openai::config::OpenAIConfig;
use once_cell::sync::Lazy;

use tracing::{debug, trace, warn};
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
    let address = OLLAMA_ADDRESS.clone();
    let api_base = format!("{address}/v1"); // format doesn't automatically dereference the variable, so we need to do it manually.
    let config = async_openai::config::OpenAIConfig::new()
        .with_api_base(api_base)
        .with_api_key("ollama");
    async_openai::Client::with_config(config)
});

/// The Google-based chatbot needs a seperate API key that is set in the .env file.
static GOOGLE_CLIENT: Lazy<async_openai::Client<OpenAIConfig>> = Lazy::new(|| {
    let key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY is not set in the .env file.");
    let config = async_openai::config::OpenAIConfig::new().with_api_key(key);
    async_openai::Client::with_config(config)
});

/// The address of the Ollama server.
static OLLAMA_ADDRESS: Lazy<String> = Lazy::new(|| {
    println!("OLLAMA_ADDRESS: {:?}", std::env::var("OLLAMA_ADDRESS"));
    debug!("OLLAMA_ADDRESS: {:?}", std::env::var("OLLAMA_ADDRESS"));
    std::env::var("OLLAMA_ADDRESS").unwrap_or_else(|_| "http://localhost:11434".to_string())
    // Default to localhost
});

/// We might want to talk to ollama. This is to check whether ollama is up. If it is, it'll return "Ollama is running".
/// Timeout is 200 milliseconds; it's on localhost:11434, the delay should be minimal.
pub async fn is_ollama_running() -> bool {
    if let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(200))
        .build()
    {
        let response = client.get(OLLAMA_ADDRESS.to_string()).send().await;
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
pub async fn select_client(
    chatbot: available_chatbots::AvailableChatbots,
) -> &'static async_openai::Client<OpenAIConfig> {
    match chatbot {
        available_chatbots::AvailableChatbots::OpenAI(_) => {
            trace!("Selecting OpenAI client");
            &OPENAI_CLIENT
        }
        available_chatbots::AvailableChatbots::Ollama(_) => {
            trace!("Selecting Ollama client");
            if !is_ollama_running().await {
                warn!("Ollama is not running, but ollama couldn't be found! This might fail!");
            }
            &OLLAMA_CLIENT
        }
        available_chatbots::AvailableChatbots::Google(_) => {
            trace!("Selecting Google client");
            warn!("Gemini API is currently not available in the EU region. This will most likely fail!");
            &GOOGLE_CLIENT
        }
    }
}
