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

/// Given a user request, generate a summary to store in the mongodb database
pub mod topic_extraction;

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

/// Returns the latest few threads for a given authenticated user
pub mod get_user_threads;

// Defines a few useful static variables that are used throughout the chatbot.

use std::sync::{Arc, Mutex};

use async_openai::config::OpenAIConfig;
use once_cell::sync::Lazy;

use tracing::{debug, error};
use types::ActiveConversation;

/// Because multiple threads need to work together and need to know about the conversations, this static variable holds information about all active conversation.
/// The Lazy and Arc are transparent, it can be accessed by locking the mutex and then accessing the Vec inside.
pub static ACTIVE_CONVERSATIONS: Lazy<Arc<Mutex<Vec<ActiveConversation>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

/// Because we shouldn't have to construct a new LiteLLM client for every stream we start, we'll use this static variable to hold the client.
/// The Lazy is transparent, it can be accessed as-is.
static LITE_LLM_CLIENT: Lazy<async_openai::Client<OpenAIConfig>> = Lazy::new(|| {
    let config =
        async_openai::config::OpenAIConfig::new().with_api_base(LITE_LLM_ADDRESS.to_string()); // Use the same address as the Ollama client, because of Litellm.
    async_openai::Client::with_config(config)
});

/// The address of the LiteLLM Proxy.
pub static LITE_LLM_ADDRESS: Lazy<String> = Lazy::new(|| {
    println!("LITE_LLM_ADDRESS: {:?}", std::env::var("LITE_LLM_ADDRESS"));
    debug!("LITE_LLM_ADDRESS: {:?}", std::env::var("LITE_LLM_ADDRESS"));
    std::env::var("LITE_LLM_ADDRESS").unwrap_or_else(|_| "http://litellm:4000".to_string())
    // Default to localhost
});

// The Client is reusable, we shouldn't create a new one for every request.
static REQWEST_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(200)) // These are simple ping requests to the LiteLLM Proxy, so we don't need a long timeout.
        .build()
        .expect("Failed to create reqwest client")
});

/// We want to use the LiteLLM Proxy. This is to check whether it is up. If it is, it'll return "I'm alive!".
/// Timeout is 200 milliseconds; it's on another container on the same machine, the delay should be minimal.
pub async fn is_lite_llm_running() -> bool {
    let response = REQWEST_CLIENT
        .get(LITE_LLM_ADDRESS.to_string() + "/health/liveliness")
        .send()
        .await;
    if let Ok(response) = response {
        response.status().is_success()
    } else {
        error!(
            "LiteLLM Proxy could not be reached; the request failed: {:?}",
            response
        );
        false
    }
}
