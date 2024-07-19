use actix_web::{HttpResponse, Responder};
use tracing::{error, info, trace, warn};

use super::thread_storage::read_thread;

/// Returns the content of a thread as a Json of List of Strings
pub async fn get_thread(thread_id: String) -> impl Responder {
    // Instead of retrieving from OpenAI, we need to retrieve from disk since that is where all streamed data is stored.
    let result = match read_thread(thread_id.as_str()) {
        Ok(content) => content,
        Err(e) => {
            // Further handle the error, as we know what possible IO errors can occur.
            trace!("Error reading thread file: {:?}", e);
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    // If the file is not found, we'll return a 404
                    info!("The User requested thread with ID {} that does not exist.", thread_id);
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
