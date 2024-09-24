use actix_web::{HttpRequest, HttpResponse, Responder};
use documented::docs_const;
use qstring::QString;
use tracing::{debug, error, info, trace, warn};

use crate::chatbot::types::StreamVariant;

use super::thread_storage::read_thread;

/// # Get Thread
/// Returns the content of a thread as a Json of List of Strings.
///
/// As arguments, it takes in a `thread_id` and an `auth_key`.
///
/// The thread id is the unique identifier for the thread, given to the client when the stream started in a ServerHint variant.
///
/// The auth key needs to match the one on the backend for the request to be authorized.
/// To get the auth key, the user needs to contact the backend administrator.
///
/// If the auth key is not given or does not match the one on the backend, an Unauthorized response is returned.
///
/// If the thread id is not given, a BadRequest response is returned.
///
/// If the thread with the given id is not found, a NotFound response is returned.
///
/// If the thread is found but cannot be read or cannot be displayed, an InternalServerError response is returned.
#[docs_const] // writes the docstring into a variable called GET_THREAD_DOCS
pub async fn get_thread(req: HttpRequest) -> impl Responder {
    let qstring = QString::from(req.query_string());

    // First try to authorize the user.
    crate::auth::authorize_or_fail!(qstring);

    // Try to get the thread ID from the request's query parameters.
    let thread_id = match qstring.get("thread_id") {
        None | Some("") => {
            // If the thread ID is not found, we'll return a 400
            warn!("The User requested a thread without a thread ID.");
            return HttpResponse::BadRequest()
                .body("Thread ID not found. Please provide a thread_id in the query parameters.");
        }
        Some(thread_id) => thread_id,
    };

    // Instead of retrieving from OpenAI, we need to retrieve from disk since that is where all streamed data is stored.
    let result = match read_thread(thread_id) {
        Ok(content) => content,
        Err(e) => {
            // Further handle the error, as we know what possible IO errors can occur.
            debug!("Error reading thread file: {:?}", e);
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    // If the file is not found, we'll return a 404
                    info!(
                        "The User requested thread with ID {} that does not exist.",
                        thread_id
                    );
                    return HttpResponse::NotFound().body("Thread not found.");
                }
                std::io::ErrorKind::PermissionDenied => {
                    // If the file is found but we may not access it, it's a server error.
                    warn!("Permission denied reading thread file: {:?}", e);
                    return HttpResponse::InternalServerError()
                        .body("Permission denied reading thread file.");
                }
                _ => {
                    // If it's anything else, we'll just return a generic error.
                    error!("Error reading thread file: {:?}", e);
                    return HttpResponse::InternalServerError().body("Error reading thread file.");
                }
            }
        }
    };

    let result = post_process(result);

    // We can now return the content as a JSON response using serde_json

    let json = match serde_json::to_string(&result) {
        Ok(json) => json,
        Err(e) => {
            // If we can't serialize the content, we'll return a generic error.
            error!("Error serializing thread content: {:?}", e);
            return HttpResponse::InternalServerError().body("Error serializing thread content.");
        }
    };

    trace!("Returning thread content: {}", json);
    HttpResponse::Ok().body(json)
}

/// Post-processes the Vector of Stream Variants to be sent to the user.
/// For now, this only removes the prompt variant.
fn post_process(v: Vec<StreamVariant>) -> Vec<StreamVariant> {
    v.into_iter()
        .filter(|x| !matches!(x, StreamVariant::Prompt(_)))
        .collect()
}