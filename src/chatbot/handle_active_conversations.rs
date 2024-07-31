use rand::Rng;
use tracing::{error, trace, warn, debug};

use crate::chatbot::{types::{ActiveConversation, ConversationState}, ACTIVE_CONVERSATIONS};

use super::types::StreamVariant;


/// Helper function to return an ID for a new conversation.
/// Currently unused, the thread IDs come from the frontend.
pub fn new_conversation_id() -> String {
    trace!("Generating new conversation ID.");
    let value = rand::thread_rng()
        .sample_iter(rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    // If this value is already in use, we'll just try again.
    match ACTIVE_CONVERSATIONS.lock() {
        Ok(guard) => {
            // If we can lock the mutex, we can check if the value is already in use.
            if guard.iter().any(|x| x.id == value) {
                warn!("Generated conversation ID is already in use, trying again.");
                return new_conversation_id();
            }
            value
        }
        Err(e) => {
            error!(
                "Error locking the mutex, falling back to hoping the value is unique: {:?}",
                e
            );
            value
        }
    }
}

/// Adds the given Stream Variants to the conversation with the given ID
/// or creates a new conversation if the ID is not found.
pub fn add_to_conversation(thread_id: &str, variant: Vec<StreamVariant>) {
    trace!("Adding to conversation with id: {}", thread_id);

    match ACTIVE_CONVERSATIONS.lock() {
        Ok(mut guard) => {
            // If we can lock the mutex, we can check if the value is already in use.
            if let Some(conversation) = guard.iter_mut().find(|x| x.id == thread_id) {
                // If we find the conversation, we'll add the variant to it.
                conversation.conversation.append(&mut variant.clone());
            } else {
                // If we don't find the conversation, we'll create a new one.
                guard.push(ActiveConversation {
                    id: thread_id.to_string(),
                    conversation: variant,
                    state: ConversationState::Streaming,
                });
            }
        }
        Err(e) => {
            error!("Error locking the mutex: {:?}", e);
        }
    }
}

/// Returns the state of the conversation, if possible
pub fn conversation_state(thread_id: &str) -> Option<ConversationState> {
    trace!("Checking the state of conversation with id: {}", thread_id);

    match ACTIVE_CONVERSATIONS.lock() {
        Ok(mut guard) => {
            // If we can lock the mutex, we can check if the value is already in use.
            if let Some(conversation) = guard.iter_mut().find(|x| x.id == thread_id) {
                // If we find the conversation, we'll check if it's stopped.
                Some(conversation.state.clone())
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
    }

}

/// Ends the conversation with the given ID, setting the state to Ended.
pub fn end_conversation(thread_id: &str) {
    trace!("Ending conversation with id: {}", thread_id);

    match ACTIVE_CONVERSATIONS.lock() {
        Ok(mut guard) => {
            // If we can lock the mutex, we can check if the value is already in use.
            if let Some(conversation) = guard.iter_mut().find(|x| x.id == thread_id) {
                // If we find the conversation, we'll set the state to Ended.
                conversation.state = ConversationState::Ended(std::time::Instant::now());
            }
        }
        Err(e) => {
            error!("Error locking the mutex: {:?}", e);
        }
    }
}

/// removes the conversation with the given ID, clearing it from the active conversations and writing it to disk.
pub fn remove_conversation(thread_id: &str) {
    trace!("Removing conversation with id: {}", thread_id);

    // We extract the conversation from the global variable to minimize the time we lock the mutex.
    let conversation = match ACTIVE_CONVERSATIONS.lock() {
        Ok(mut guard) => {
            // If we can lock the mutex, we can check if the value is already in use.
            guard.iter().position(|x| x.id == thread_id).map(|index| guard.remove(index))
        }
        Err(e) => {
            error!("Error locking the mutex: {:?}", e);
            None
        }
    };

    if let Some(conversation) = conversation {
        // If we found the conversation, we'll write it to disk.
        trace!("Removed conversation with thread_id: {}: {:?}", thread_id, conversation);
        debug!("Writing conversation to disk.");

        // Before we'll write it to disk, we'll fold all the consecutive Assistant messages into one.

        let mut new_conversation = Vec::new();

        let mut assistant_buffer = String::new();

        for variant in conversation.conversation {
            match variant {
                StreamVariant::Assistant(message) => { // If it's an assistant message, we'll append it to the current buffer;
                    assistant_buffer.push_str(&message);
                }
                _ => { // It's not an assistant message
                    if !assistant_buffer.is_empty() { // if the assistant buffer contains something, we'll push it to the new conversation.
                        new_conversation.push(StreamVariant::Assistant(assistant_buffer.clone()));
                        assistant_buffer.clear(); // and then clear the buffer so the next message can be appended.
                    }
                    new_conversation.push(variant);
                }
            }
        }

        // Edge case: theoretically, all conversations should end with a StreamEnd, but if it doesn't, we'd drop the last assistant message, unless we add it here.
        if !assistant_buffer.is_empty() {
            new_conversation.push(StreamVariant::Assistant(assistant_buffer));
        }


        crate::chatbot::thread_storage::append_thread(thread_id, new_conversation);
    }
}