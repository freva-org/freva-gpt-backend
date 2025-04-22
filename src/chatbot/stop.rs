// Handles the stop request from the client.

use actix_web::{HttpRequest, HttpResponse, Responder};
use documented::docs_const;
use tracing::{debug, trace, warn};

use super::{types::ConversationState, ACTIVE_CONVERSATIONS};

// TODO: guarentee panic safety

/// # Stop
/// Stops the conversation with the given thread ID as soon as possible.
///
/// Takes in a `thread_id` and an `auth_key`.
/// The thread_id identifies the conversation to stop.
/// The auth_key needs to match the one on the backend for the request to be authorized.
///
/// If the auth key is not given or does not match the one on the backend, an Unauthorized response is returned.
///
/// If the thread id is not given, a BadRequest response is returned.
///
/// If there is an error stopping the conversation, an InternalServerError response is returned.
#[docs_const]
pub async fn stop(req: HttpRequest) -> impl Responder {
    #[derive(Debug)]
    enum StopResult {
        Found,
        NotFound,
        NotRunning,
        Error(String),
    }
    let qstring = qstring::QString::from(req.query_string());
    let headers = req.headers();

    // First try to authorize the user.
    crate::auth::authorize_or_fail!(qstring, headers);

    // Try to get the thread ID from the request's query parameters.
    let thread_id = match qstring.get("thread_id") {
        None | Some("") => {
            // If the thread ID is not found, we'll return a 400
            warn!("The User requested a stop without a thread ID.");
            return HttpResponse::BadRequest()
                .body("Thread ID not found. Please provide a thread_id in the query parameters.");
        }
        Some(thread_id) => thread_id,
    };
    // Trieds to set the conversation state to Stopping
    debug!("Trying to stop conversation with id: {}", thread_id);

    // We need to lock the mutex for the shortest time possible and can't just return from within the guard,
    // so we need to store the result in a variable and return outside the guard.
    let result = match ACTIVE_CONVERSATIONS.lock() {
        Ok(mut guard) => {
            let mut inner_res = StopResult::NotFound;
            for conversation in guard.iter_mut() {
                if conversation.id == thread_id {
                    // if any conversation has the same id as the one we want to stop
                    inner_res = match conversation.state {
                        ConversationState::Streaming(_) => {
                            // if it's streaming, we want to stop it
                            conversation.state = ConversationState::Stopping;
                            StopResult::Found // and return that we found it
                        }
                        ConversationState::Stopping | ConversationState::Ended => {
                            StopResult::NotRunning
                        }
                    };
                    break;
                }
            }
            inner_res
        }
        Err(e) => StopResult::Error(format!("Error locking the mutex: {e:?}")),
    };

    match result {
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
