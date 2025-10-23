use actix_web::{HttpRequest, Responder};
use documented::docs_const;
use tracing::{debug, trace, warn};

use crate::{
    auth::get_first_matching_field,
    chatbot::mongodb::mongodb_storage::{get_database, update_topic},
};

/// # set_thread_topic
/// Takes in the thread ID and the new topic
/// The topic of that thread will be updated in the database.
///
/// This endpoint also requires authentication.
///
/// If there is an error during the updating, a 500 Internal Server Error response will be returned.

#[docs_const]
pub async fn set_thread_topic(req: HttpRequest) -> impl Responder {
    let qstring = qstring::QString::from(req.query_string());
    let headers = req.headers();

    trace!("Query string: {}", qstring);
    trace!("Headers: {:?}", headers);

    // First try to authorize the user

    let user_id = crate::auth::authorize_or_fail!(qstring, headers);

    // Retrieve the arguments to the request
    let thread_id = get_first_matching_field(
        &qstring,
        headers,
        &["thread_id", "thread-id", "x-thread-id"],
        false,
    )
    .unwrap_or_default();
    let new_topic = get_first_matching_field(&qstring, headers, &["topic", "new_topic"], false);

    let Some(new_topic) = new_topic else {
        warn!("User tried to set thread topic without providing a new topic");
        return actix_web::HttpResponse::BadRequest()
            .body("Missing topic; please set a topic using the query string");
    };

    debug!(
        "User {} wants to set topic of thread {} to {}",
        user_id, thread_id, new_topic
    );

    // Next, we need to establish a connection to the database
    let maybe_vault_url = headers
        .get("x-freva-vault-url")
        .and_then(|h| h.to_str().ok());

    let database = if let Some(vault_url) = maybe_vault_url {
        get_database(vault_url).await
    } else {
        warn!("Vault URL not found");
        Err(actix_web::HttpResponse::BadRequest()
            .body("Vault URL not found. Please provide a non-empty vault URL in the headers."))
    };

    let database = match database {
        Ok(db) => db,
        Err(e) => {
            // If we cannot initialize the database connection, we'll return a 500
            warn!("Error initializing database connection: {:?}", e);
            return e;
        }
    };

    // Send the update
    match update_topic(thread_id, &user_id, new_topic, database).await {
        Ok(()) => {
            debug!("Successfully updated thread topic.");
            actix_web::HttpResponse::Ok().body("Successfully updated thread topic.")
        }
        Err(e) => {
            warn!("Failed to update thread topic: {:?}", e);
            e
        }
    }
}
