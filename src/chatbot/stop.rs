// Handles the stop request from the client.

use actix_web::{HttpResponse, Responder};
use tracing::{debug, trace, warn};

use crate::chatbot::{ConversationState, ACTIVE_CONVERSATIONS};

// TODO: guarentee panic safety
/// Handles the stop request from the client.
pub async fn stop(thread_id: String) -> impl Responder {
    // Trieds to set the conversation state to Stopping
    debug!("Trying to stop conversation with id: {}", thread_id);

    #[derive(Debug)]
    enum StopResult {
        Found,
        NotFound,
        NotRunning,
        Error(String),
    }

    // We need to lock the mutex for the shortest time possible and can't just return from within the guard,
    // so we need to store the result in a variable and return outside the guard.
    let res = match ACTIVE_CONVERSATIONS.lock() {
        Ok(mut guard) => {
            let mut inner_res = StopResult::NotFound; // default value
            for conversation in guard.iter_mut() {
                if conversation.id == thread_id {
                    // if any conversation has the same id as the one we want to stop
                    inner_res = match conversation.state {
                        ConversationState::Streaming => {
                            // if it's streaming, we want to stop it
                            conversation.state = ConversationState::Stopping;
                            StopResult::Found // and return that we found it
                        }
                        ConversationState::Stopping => StopResult::NotRunning,
                        ConversationState::Ended(_) => StopResult::NotRunning,
                    };
                    break;
                }
            }
            inner_res
        }
        Err(e) => StopResult::Error(format!("Error locking the mutex: {:?}", e)),
    };

    match res {
        StopResult::Found => {
            trace!(
                "Successfully stopped running conversation with threadID {}",
                thread_id
            );
            HttpResponse::Ok().body("Conversation stopped.")
        }
        StopResult::NotFound => HttpResponse::NotFound().body("Conversation not found."),
        StopResult::NotRunning => HttpResponse::BadRequest().body("Conversation not running."),
        StopResult::Error(e) => {
            warn!("Error stopping conversation: {:?}", e);
            HttpResponse::InternalServerError().body("Error stopping conversation.")
        }
    }
}
