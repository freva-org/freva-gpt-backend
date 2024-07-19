// For all the things that is needed in the background for the chatbot to work. 


// Relays the files from this folder up

/// Handles the stop request from the client.
pub mod stop; 

// Because multiple threads need to work together and need to know about the conversations, this static variable holds information about all active conversation.

use std::{sync::{Arc, Mutex}, time::Instant};

use once_cell::sync::Lazy;

pub enum ConversationState{
    Streaming,
    Stopping,
    Ended(Instant),
}

// When a thread is streaming, it is in the Streaming state. If nothing goes wrong, at the end, it will be in the Ended state. 
// If a request to stop it is sent, another thread will change the state to Stopping.
// The thread that is streaming will check the state and if it is Stopping, it will stop the streaming and change the state to Ended.

pub struct ActiveConversation{
    id: String, // Either the id as given by OpenAI or our internal id, maybe an Enum or `either` later

    state: ConversationState,
}

pub static ACTIVE_CONVERSATIONS: Lazy<Arc<Mutex<Vec<ActiveConversation>>>> = Lazy::new(|| Arc::new(Mutex::new(Vec::new())));