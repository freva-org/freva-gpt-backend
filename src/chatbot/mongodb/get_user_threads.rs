use actix_web::{HttpRequest, HttpResponse, Responder};
use documented::docs_const;
use tracing::{debug, trace, warn};

use crate::{
    auth::ALLOW_FALLBACK_OLD_AUTH,
    chatbot::mongodb::mongodb_storage::{get_database, read_threads_and_num},
};

/// # getuserthreads
/// Takes in a user_id and returns the latest n threads of the user.
/// n is an optional parameter that defaults to 10.
/// if a page number (0-based) is passed, it instead paginates and uses that page number
///
/// The user should ideally authenticate themselves using the OpenID Connect token.
/// Alternatively, the user_id can be passed in as a query parameter.
/// This might be removed in the future.
///
/// If the user cannot be authenticated, an Unauthorized response is returned.
///
/// If the user_id is not given or cannot be derived from the OID token, a BadRequest response is returned.
#[docs_const]
pub async fn get_user_threads(req: HttpRequest) -> impl Responder {
    let qstring = qstring::QString::from(req.query_string());
    let headers = req.headers();

    debug!("Query string: {:?}", qstring);
    debug!("Headers: {:?}", headers);

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
    let maybe_vault_url = headers
        .get("x-freva-vault-url")
        .and_then(|h| h.to_str().ok());

    let Some(vault_url) = maybe_vault_url else {
        warn!("The User requested a stream without a vault URL.");
        return HttpResponse::UnprocessableEntity()
            .body("Vault URL not found. Please provide a non-empty vault URL in the headers.");
    };

    // If we don't have a vault URL, we'll automatically fall back to the local (testing) database.
    let database = match get_database(vault_url).await {
        Ok(db) => db,
        Err(e) => {
            debug!("Failed to connect to the database: {:?}", e);
            return HttpResponse::ServiceUnavailable().body("Failed to connect to the database.");
        }
    };

    // Try to get n from the qstring
    let n = match qstring.get("num_threads") {
        Some(n) => {
            debug!("Parsed num_threads: {}", n);
            n.parse::<u8>().unwrap_or(10)
        }
        None => 10,
    };
    trace!("Final num_threads: {}", n);

    let page = qstring.get("page").and_then(|p| p.parse::<u8>().ok());

    // Retrieve the latest n threads of the user from the database.
    let threads = read_threads_and_num(&user_id, database, n, page).await;

    debug!("Threads: {:?}", threads);
    HttpResponse::Ok()
        .content_type("application/json")
        .json(threads)
}
