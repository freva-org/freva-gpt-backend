use actix_web::{HttpRequest, HttpResponse, Responder};
use documented::docs_const;
use tracing::debug;

use crate::chatbot::mongodb_storage::{get_database, read_threads};

/// # getuserthreads
/// Takes in a user_id and returns the latest 10 threads of the user.
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
            // Also try to get the user_id from the query parameters.
            match qstring.get("user_id") {
                Some(user_id) => user_id.to_string(),
                None => {
                    // If the user ID is not found, we'll return a 400
                    return HttpResponse::BadRequest()
                        .body("User ID not found. Please provide a user_id in the query parameters.");
                }
            }
        }
    };
    
    debug!("User ID: {}", user_id);

    // We first need to check whether we have a vault URL to connect to the database from.
    let maybe_vault_url = headers.get("x-freva-vault-url")
        .and_then(|h| h.to_str().ok());

    // If we don't have a vault URL, we'll automatically fall back to the local (testing) database.
    let database = match get_database(maybe_vault_url).await {
        Ok(db) => db,
        Err(e) => {
            debug!("Failed to connect to the database: {:?}", e);
            return HttpResponse::InternalServerError()
                .body("Failed to connect to the database.");
        }
    };

    // Retrieve the latest 10 threads of the user from the database.
    let threads = read_threads(&user_id, database).await;

    debug!("Threads: {:?}", threads);
    HttpResponse::Ok()
        .content_type("application/json")
        .json(threads)

}