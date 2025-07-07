// Handles basic prompting for the chatbot.

use async_openai::types::{ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage};
use once_cell::sync::Lazy;
use std::fs;
use std::io::Read;
use tracing::{debug, error, trace};

/// The basic starting prompt as a const of the correct type.
static STARTING_PROMPT_STR: Lazy<String> = Lazy::new(|| {
    let mut file = fs::File::open("src/chatbot/prompt_sources/starting_prompt.txt")
        .expect("Unable to open starting_prompt.txt");
    let mut content = String::new();
    file.read_to_string(&mut content)
        .expect("Unable to read starting_prompt.txt");
    content
});

/// The entire Example conversation file as a String.
static EXAMPLE_CONVERSATIONS_STR: Lazy<String> = Lazy::new(|| {
    let mut file = fs::File::open("src/chatbot/prompt_sources/examples.jsonl")
        .expect("Unable to open examples.jsonl");
    let mut content = String::new();
    file.read_to_string(&mut content)
        .expect("Unable to read examples.jsonl");
    trace!("Successfully read from File, content: {}", content);
    content
});

/// The summary system prompt, as a static string.
static SUMMARY_SYSTEM_PROMPT_STR: Lazy<String> = Lazy::new(|| {
    let mut file = fs::File::open("src/chatbot/prompt_sources/summary_prompt.txt")
        .expect("Unable to open summary_system_prompt.txt");
    let mut content = String::new();
    file.read_to_string(&mut content)
        .expect("Unable to read summary_system_prompt.txt");
    trace!("Successfully read from File, content: {}", content);
    content
});

/// The Starting prompt, as a static variable for the async_openai library.
/// Note that we need to use Lazy because the Type wants a proper String, which isn't const as it requires allocation.
pub static STARTING_PROMPT_CCRM: Lazy<ChatCompletionRequestSystemMessage> =
    Lazy::new(|| ChatCompletionRequestSystemMessage {
        name: Some("prompt".to_string()),
        content: async_openai::types::ChatCompletionRequestSystemMessageContent::Text(
            STARTING_PROMPT_STR.clone(),
        ),
    });

/// Function that holds the example conversations as a type that the async_openai library can use.
/// Doesn't template anymore, so the user_id and thread_id are not used.
fn example_conversations_ccrm() -> Vec<ChatCompletionRequestMessage> {
    let content = EXAMPLE_CONVERSATIONS_STR.clone();

    let stream_variants = crate::chatbot::thread_storage::extract_variants_from_string(&content);
    trace!("Returning number of lines: {}", stream_variants.len());

    crate::chatbot::types::help_convert_sv_ccrm(stream_variants, false) // The example conversations shouldn't contain images, but if they do, we don't want to send them.
}

/// Some LLMs, especially Llama seem to require another prompt after the example conversations.
static SUMMARY_SYSTEM_PROMPT_CCRM: Lazy<ChatCompletionRequestSystemMessage> = Lazy::new(|| {
    let content = SUMMARY_SYSTEM_PROMPT_STR.clone();
    ChatCompletionRequestSystemMessage {
        name: Some("prompt".to_string()),
        content: async_openai::types::ChatCompletionRequestSystemMessageContent::Text(content),
    }
});

/// All messages that should be added at the start of a new conversation.
/// Consists of a starting prompt and a few example conversations.
fn entire_prompt_ccrm() -> Vec<ChatCompletionRequestMessage> {
    let mut messages = vec![ChatCompletionRequestMessage::System(
        STARTING_PROMPT_CCRM.clone(),
    )];
    messages.extend(example_conversations_ccrm());
    messages.push(ChatCompletionRequestMessage::System(
        SUMMARY_SYSTEM_PROMPT_CCRM.clone(),
    ));
    messages
}

/// Function that returns the entire prompt as a JSON string.
pub fn get_entire_prompt_json(user_id: &str, thread_id: &str) -> String {
    recursively_create_dir_at_rw_dir(user_id, thread_id);
    // This function is a placeholder for now, but will in a few hours be used to
    // Properly template the content of the starting prompt.
    // For now, it just returns the JSON string of the starting prompt.
    let ep_crrm = entire_prompt_ccrm();

    let result =
        serde_json::to_string(&ep_crrm).expect("Error converting starting prompt to JSON.");
    // Safety: The conversion currently has no paths to error. However, if it does, the first call before the server is started will fail, causing the server to not start.
    // Note that the templating makes it not pure, but if one templating is correct, and everything is alphanumeric, the rest should be too.

    trace!("Returning starting prompt JSON: {}", result);
    result
}

pub fn get_entire_prompt(user_id: &str, thread_id: &str) -> Vec<ChatCompletionRequestMessage> {
    recursively_create_dir_at_rw_dir(user_id, thread_id);
    // Note that this function allows for the user_id and thread_id to be non-alphanumeric, as it is not used in the JSON parsing.
    let result = entire_prompt_ccrm();

    trace!("Returning templated starting prompt: {:?}", result);
    result
}

/// Every time a prompt is requested, the folder at rw_dir needs to be created because else, some python functions
/// might not find it. (We cannot expect all the functions to alwas recursively create the folders)
fn recursively_create_dir_at_rw_dir(user_id: &str, thread_id: &str) {
    trace!(
        "Creating rw_dir for user_id: {}, thread_id: {}",
        user_id,
        thread_id
    );
    let rw_dir = format!("rw_dir/{user_id}/{thread_id}");
    let path = std::path::Path::new(&rw_dir);
    if path.exists() {
        trace!("rw_dir already exists: {}", rw_dir);
    } else {
        let result = std::fs::create_dir_all(path);
        if let Err(e) = result {
            debug!("Failed to create rw_dir: {e}. Python might have trouble storing data.");
            // The directory might not be created if the user_id contains non-alphanumeric characters.
            // We'll try again, this time with a sanitized user_id.
            let sanitized_user_id = user_id
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>();
            let sanitized_rw_dir = format!("rw_dir/{sanitized_user_id}/{thread_id}");
            let sanitized_path = std::path::Path::new(&sanitized_rw_dir);
            if sanitized_path.exists() {
                trace!("Sanitized rw_dir already exists: {}", sanitized_rw_dir);
            } else if let Err(e) = std::fs::create_dir_all(sanitized_path) {
                error!(
                    "Failed to create sanitized rw_dir: {}. Python might have trouble storing data.",
                    e
                );
            } else {
                trace!("Sanitized rw_dir created: {}", sanitized_rw_dir);
            }
            return;
        }
        trace!("rw_dir created: {}", rw_dir);
    }
}
