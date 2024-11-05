// Handles basic prompting for the chatbot.

use async_openai::types::{ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage};
use once_cell::sync::Lazy;
use std::fs;
use std::io::Read;
use tracing::trace;

// ...existing code...

/// Lazy variable to hold example conversations read from `examples.jsonl`.
pub static EXAMPLE_CONVERSATIONS_FROM_FILE: Lazy<Vec<ChatCompletionRequestMessage>> =
    Lazy::new(|| {
        let mut file = fs::File::open("src/chatbot/prompt_sources/examples.jsonl")
            .expect("Unable to open examples.jsonl");
        let mut content = String::new();
        file.read_to_string(&mut content)
            .expect("Unable to read examples.jsonl");

        trace!("Successfully read from File, content: {}", content);
        let stream_variants = crate::chatbot::thread_storage::extract_variants_from_string(content);
        trace!("Returning number of lines: {}", stream_variants.len());

        crate::chatbot::types::help_convert_sv_ccrm(stream_variants)
    });

/// The starting prompt including all messages, converted to JSON.
pub static STARTING_PROMPT_JSON: Lazy<String> = Lazy::new(|| {
    let temp: Vec<ChatCompletionRequestMessage> = (*STARTING_PROMPT).clone();
    // This should never fail, but if it does, it will do so during initialization.
    serde_json::to_string(&temp).expect("Error converting starting prompt to JSON.")
});

/// All messages that should be added at the start of a new conversation.
/// Consists of a starting prompt and a few example conversations.
pub static STARTING_PROMPT: Lazy<Vec<ChatCompletionRequestMessage>> = Lazy::new(|| {
    let mut messages = vec![ChatCompletionRequestMessage::System(INITIAL_PROMPT.clone())];
    messages.extend(EXAMPLE_CONVERSATIONS_FROM_FILE.clone());
    messages.push(ChatCompletionRequestMessage::System(
        SUMMARY_SYSTEM_PROMPT.clone(),
    ));
    messages
});

/// The basic starting prompt as a const of the correct type.
static STARTING_PROMPT_STR: Lazy<String> = Lazy::new(|| {
    let mut file = fs::File::open("src/chatbot/prompt_sources/starting_prompt.txt")
        .expect("Unable to open starting_prompt.txt");
    let mut content = String::new();
    file.read_to_string(&mut content)
        .expect("Unable to read starting_prompt.txt");
    content
});

/// The Starting prompt, as a static variable.
/// Note that we need to use Lazy because the Type wants a proper String, which isn't const as it requires allocation.
pub static INITIAL_PROMPT: Lazy<ChatCompletionRequestSystemMessage> =
    Lazy::new(|| ChatCompletionRequestSystemMessage {
        name: Some("prompt".to_string()),
        content: async_openai::types::ChatCompletionRequestSystemMessageContent::Text(
            STARTING_PROMPT_STR.clone(),
        ),
    });

/// Some LLMs, especially Llama seem to require another prompt after the example conversations.
static SUMMARY_SYSTEM_PROMPT: Lazy<ChatCompletionRequestSystemMessage> = Lazy::new(|| {
    let mut file = fs::File::open("src/chatbot/prompt_sources/summary_prompt.txt")
        .expect("Unable to open summary_system_prompt.txt");
    let mut content = String::new();
    file.read_to_string(&mut content)
        .expect("Unable to read summary_system_prompt.txt");
    ChatCompletionRequestSystemMessage {
        name: Some("prompt".to_string()),
        content: async_openai::types::ChatCompletionRequestSystemMessageContent::Text(content),
    }
});
