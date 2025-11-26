use mongodb::Database;
use rand::Rng;
use tracing::{debug, error, trace, warn};

use crate::chatbot::{
    types::{ActiveConversation, ConversationState},
    ACTIVE_CONVERSATIONS,
};

use super::types::StreamVariant;

/// Helper function to generate an ID.
/// Mostly for creating conversation IDs.
/// TODO: move to other module?
pub fn generate_id() -> String {
    trace!("Generating new ID.");
    rand::rng()
        .sample_iter(rand::distr::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

/// Helper function to return an ID for a new conversation.
pub fn new_conversation_id() -> String {
    trace!("Generating new conversation ID.");
    let value = generate_id();

    // If this value is already in use, we'll just try again.
    let result = match ACTIVE_CONVERSATIONS.lock() {
        Ok(guard) => {
            // If we can lock the mutex, we can check if the value is already in use.
            if guard.iter().any(|x| x.id == value) {
                warn!("Generated conversation ID is already in use, trying again.");
                None
            } else {
                Some(value)
            }
        }
        Err(e) => {
            error!(
                "Error locking the mutex, falling back to hoping the value is unique: {:?}",
                e
            );
            Some(value)
        }
    };

    match result {
        Some(value) => value,
        None => new_conversation_id(), // Try again
    }
}

/// Adds the given Stream Variants to the conversation with the given ID
/// or creates a new conversation if the ID is not found.
pub fn add_to_conversation(
    thread_id: &str,
    variant: Vec<StreamVariant>,
    freva_config_path: String,
    user_id: String,
) {
    trace!("Adding to conversation with id: {}", thread_id);

    match ACTIVE_CONVERSATIONS.lock() {
        Ok(mut guard) => {
            // If we can lock the mutex, we can check if the value is already in use.
            if let Some(conversation) = guard.iter_mut().find(|x| x.id == thread_id) {
                // If we find the conversation, we'll add the variant to it.
                conversation.conversation.extend(variant);
                conversation.last_activity = std::time::Instant::now(); // ALso update the last activity.
            } else {
                // If we don't find the conversation, we'll create a new one.
                guard.push(ActiveConversation {
                    id: thread_id.to_string(),
                    conversation: variant,
                    state: ConversationState::Streaming(freva_config_path),
                    last_activity: std::time::Instant::now(),
                    user_id,
                });
            }
        }
        Err(e) => {
            error!("Error locking the mutex: {:?}", e);
        }
    }
}

/// Returns the state of the conversation, if possible
pub async fn conversation_state(thread_id: &str, database: Database) -> Option<ConversationState> {
    trace!("Checking the state of conversation with id: {}", thread_id);

    let mut to_save = None;

    let return_val = match ACTIVE_CONVERSATIONS.lock() {
        Ok(mut guard) => {
            // For debugging, log the length of the active conversations.
            trace!("Number of active conversations: {}", guard.len());
            //DEBUG
            // println!("Number of active conversations: {}", guard.len());
            // If we can lock the mutex, we can check if the value is already in use.
            let return_val = if let Some(conversation) = guard.iter().find(|x| x.id == thread_id) {
                // If we find the conversation, we'll check if it's stopped.
                Some(conversation.state.clone())
            } else {
                // If the conversation is not found, we'll return false.
                warn!("Conversation with id: {} not found.", thread_id);
                None
            };
            // Before returning, we'll clean up stale conversations.
            to_save = Some(cleanup_conversations(&mut guard));
            return_val
        }
        Err(e) => {
            error!("Error locking the mutex: {:?}", e);
            None
        }
    };

    // In order to not save the conversations while the mutex is locked, we'll save it here.
    if let Some(conversations) = to_save {
        for conversation in conversations {
            save_conversation(conversation, database.clone()).await;
        }
    }

    return_val
}

/// Ends the conversation with the given ID, setting the state to Ended.
pub fn end_conversation(thread_id: &str) {
    trace!("Ending conversation with id: {}", thread_id);

    match ACTIVE_CONVERSATIONS.lock() {
        Ok(mut guard) => {
            // If we can lock the mutex, we can check if the value is already in use.
            if let Some(conversation) = guard.iter_mut().find(|x| x.id == thread_id) {
                // If we find the conversation, we'll set the state to Ended.
                conversation.state = ConversationState::Ended;
            }
        }
        Err(e) => {
            error!("Error locking the mutex: {:?}", e);
        }
    }
}

/// Removes the conversation with the given ID, clearing it from the active conversations and writing it to disk.
pub async fn save_and_remove_conversation(thread_id: &str, database: Database) {
    trace!("Removing conversation with id: {}", thread_id);

    // We extract the conversation from the global variable to minimize the time we lock the mutex.
    let conversation = match ACTIVE_CONVERSATIONS.lock() {
        Ok(mut guard) => {
            // If we can lock the mutex, we can check if the value is already in use.
            guard
                .iter()
                .position(|x| x.id == thread_id)
                .map(|index| guard.remove(index))
        }
        Err(e) => {
            error!("Error locking the mutex: {:?}", e);
            None
        }
    };

    if let Some(conversation) = conversation {
        save_conversation(conversation, database).await;
    }
}

/// Helper function to save a conversation to disk.
async fn save_conversation(conversation: ActiveConversation, database: Database) {
    debug!("Writing conversation to disk.");

    // Before we'll write it to disk, we'll fold all the consecutive Assistant messages into one.

    let new_conversation = concat_variants(conversation.conversation);

    crate::chatbot::storage_router::append_thread(
        &conversation.id,
        &conversation.user_id,
        new_conversation,
        database,
    )
    .await;
}

/// The assistant and code messages are streamed, so the variants that come from OpenAI contain only one or a few tokens of the message.
/// This function takes a vector of StreamVariants and concatenates consecutive Assistant messages and the Code messages.
///
/// So instead of having multiple variants like this: "Assistant": "He", "Assistant": "llo", "Assistant": "!"
/// we'll have one variant like this: "Assistant": "Hello!". The same goes for the Code messages.
fn concat_variants(input: Vec<StreamVariant>) -> Vec<StreamVariant> {
    let mut output = Vec::new();
    let mut assistant_buffer = String::new();
    let mut code_buffer = (String::new(), String::new(), String::new()); // content; id; name

    for variant in input {
        match variant {
            StreamVariant::Assistant(message) => {
                assistant_buffer.push_str(&message);
            }
            StreamVariant::Code(message, id, name) => {
                // We don't expect two tool calls to be right after one another, so we won't recheck the id or name.
                code_buffer.0.push_str(&message);
                code_buffer.1 = id;
                code_buffer.2 = name;
            }
            _ => {
                // If it's not an assistant or code message, we'll push the buffers to the output.
                if !assistant_buffer.is_empty() {
                    output.push(StreamVariant::Assistant(assistant_buffer.clone()));
                    assistant_buffer.clear();
                }
                if !code_buffer.0.is_empty() {
                    output.push(StreamVariant::Code(
                        code_buffer.0.clone(),
                        code_buffer.1.clone(),
                        code_buffer.2.clone(),
                    ));
                    code_buffer.0.clear();
                    code_buffer.1.clear();
                    code_buffer.2.clear();
                }
                output.push(variant);
            }
        }
    }

    // Edge case: theoretically, all conversations should end with a StreamEnd, but if it doesn't, we'd drop the last assistant message, unless we add it here.
    // The same goes for the code messages.
    // Actually, this happens often, because we need to restart the stream after a tool call.
    if !assistant_buffer.is_empty() {
        output.push(StreamVariant::Assistant(assistant_buffer));
    }
    if !code_buffer.0.is_empty() {
        output.push(StreamVariant::Code(
            code_buffer.0,
            code_buffer.1,
            code_buffer.2,
        ));
    }

    output
}

/// Returns the conversation with the given thread_ID, if it exists.
pub fn get_conversation(thread_id: &str) -> Option<Vec<StreamVariant>> {
    trace!("Getting conversation with id: {}", thread_id);

    // The conversation is stored in the global variable, so we'll lock the mutex to access it.
    let found_conversation = match ACTIVE_CONVERSATIONS.lock() {
        Ok(guard) => {
            // If we can lock the mutex, we can check if the value is already in use.
            if let Some(conversation) = guard.iter().find(|x| x.id == thread_id) {
                // If we find the conversation, we'll return it.
                Some(conversation.conversation.clone())
            } else {
                // If the conversation is not found, we'll return false.
                warn!("Conversation with id: {} not found.", thread_id);
                None
            }
        }
        Err(e) => {
            error!("Error locking the mutex: {:?}", e);
            None
        }
    };

    // Because the conversation is stored in the global variable, it might not have concatinated the assistant and code messages yet.
    found_conversation.map(concat_variants) // If the conversation is found, we'll concatenate the messages, else we'll return None.
}

static MAX_INACTIVE_TIME: std::time::Duration = std::time::Duration::from_secs(3 * 60); // 3 minutes

/// Cleans up all stae conversations to avoid the ACTIVE_CONVERSATIONS vector from growing indefinitely.
/// The vector grows because when a client loses connection, the stream ends shortly after, so the cleanup doesn't happen.
fn cleanup_conversations(guard: &mut Vec<ActiveConversation>) -> Vec<ActiveConversation> {
    // Store the conversations that need to be saved, because we shouldn't save them while the mutex is locked.
    let mut to_save = Vec::new();
    guard.retain(|x| {
        if x.last_activity.elapsed() > MAX_INACTIVE_TIME {
            debug!(
                "Removing conversation with id: {} because it's inactive.",
                x.id
            );
            trace!("Conversation: {:?}", x);
            // TODO, FIXME: this currently doesn't clean up conversations that used the code_interpreter, because the heartbeat is currently not working and the
            // This will be fixed once the heartbeat is working, but this is a temporary fix.
            if x.conversation
                .last()
                .is_some_and(|x| matches!(x, StreamVariant::ServerHint(_)))
            {
                trace!("Conversation used the code_interpreter, not removing.");
                return true;
            }
            // If the conversation is inactive, we'll save it to disk and remove it from the active conversations.
            to_save.push(x.clone());
            false
        } else {
            true
        }
    });
    to_save
}

/// This function is run when the frontend sends an edit-input.
/// It generates a new thread_id and manages the python_pickles file.
pub fn switch_to_new_thread_id(thread_id: &str) -> String {
    trace!(
        "Switching to new thread_id for conversation with id: {}",
        thread_id
    );

    // The conversation wasn't started yet at this point in the code, so we'll just create a new conversation with the new thread_id.
    // This will happen automatically when this function returns a new thread_id.

    let new_thread_id = new_conversation_id();

    // We need to copy the python_pickles file to the new thread_id. This previously only happened within python.
    // Both files lie in `python_pickles/{thread_id}.pickle` and `python_pickles/{new_thread_id}.pickle`.
    let old_path = format!("python_pickles/{thread_id}.pickle");
    let new_path = format!("python_pickles/{new_thread_id}.pickle");
    if let Err(e) = std::fs::copy(&old_path, &new_path) {
        if matches!(e.kind(), std::io::ErrorKind::NotFound) {
            // If the error is not that the file doesn't exist, we log it as an error.
            // If it is, we just ignore it, because it means the file didn't exist in the first place.
            // This can happen if the user never used the code_interpreter or if the file was deleted manually.
            // In this case, a new file will be created when the user uses the code_interpreter again.
            trace!("File not found, ignoring.");
        } else {
            error!(
                "Error copying python_pickles file from {} to {}: {:?}",
                old_path, new_path, e
            );
        }
    } else {
        trace!(
            "Copied python_pickles file from {} to {}",
            old_path,
            new_path
        );
    }

    // Return the new thread_id.
    new_thread_id
}
