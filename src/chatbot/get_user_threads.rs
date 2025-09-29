use actix_web::{HttpRequest, HttpResponse, Responder};
use documented::docs_const;
use tracing::{debug, warn};

use crate::{
    auth::{get_first_matching_field, ALLOW_FALLBACK_OLD_AUTH},
    chatbot::mongodb_storage::{get_database, read_threads},
};

/// # getuserthreads
/// Takes in a vault_url and returns the latest 10 threads of the user. Requires Authentication.
///
/// Supports the fallback authentication that is disabled by default by sending user_id.
///
/// If the user cannot be authenticated, an Unauthorized response is returned.
///
/// If the user_id is not given or cannot be derived from the OID token, a BadRequest response is returned.
///
/// If the database cannot be connected to, a ServiceUnavailable response is returned.
#[docs_const]
pub async fn get_user_threads(req: HttpRequest) -> impl Responder {
    let qstring = qstring::QString::from(req.query_string());
    let headers = req.headers();

    debug!("Query string: {:?}", qstring);
    // debug!("Headers: {:?}", headers);

    // First try to authorize the user.
    let maybe_username = crate::auth::authorize_or_fail!(qstring, headers);
    let user_id = match maybe_username {
        Some(user_id) => user_id,
        None => {
            // Also try to get the user_id from the query parameters, if allowed.
            if ALLOW_FALLBACK_OLD_AUTH {
                match qstring.get("user_id") {
                    Some(user_id) => user_id.to_string(),
                    None => {
                        // If the user ID is not found, we'll return a 422
                        return HttpResponse::UnprocessableEntity().body(
                            "User ID not found. Please provide a user_id in the query parameters.",
                        );
                    }
                }
            } else {
                return HttpResponse::UnprocessableEntity().body(
                    "Could not determine User ID. Please authenticate using OpenID Connect.",
                );
            }
        }
    };

    debug!("User ID: {}", user_id);

    // We first need to check whether we have a vault URL to connect to the database from.
    let maybe_vault_url = get_first_matching_field(
        &qstring,
        headers,
        &[
            "x-freva-vault-url",
            "x-vault-url",
            "vault-url",
            "vault_url",
            "freva_vault_url",
        ],
        true,
    );

    let Some(vault_url) = maybe_vault_url else {
        warn!("The User requested a stream without a vault URL.");
        return HttpResponse::UnprocessableEntity()
            .body("Vault URL not found. Please provide a non-empty vault URL in the headers.");
    };

    let database = match get_database(vault_url).await {
        Ok(db) => db,
        Err(e) => {
            debug!("Failed to connect to the database: {:?}", e);
            return HttpResponse::ServiceUnavailable().body("Failed to connect to the database.");
        }
    };

    // Retrieve the latest 10 threads of the user from the database.
    let threads = read_threads(&user_id, database).await;

    debug!("Threads: {:?}", threads);
    HttpResponse::Ok()
        .content_type("application/json")
        .json(threads)
}
