use actix_web::{HttpRequest, HttpResponse, Responder};
use documented::docs_const;
use tracing::{debug, warn};

use crate::{
    auth::get_first_matching_field,
    chatbot::mongodb_storage::{get_database, read_threads},
};

/// # getuserthreads
/// Takes in a vault_url and returns the latest 10 threads of the user. Requires Authentication.
///
/// If the vault_url is missing or empty, an UnprocessableEntity response is returned.
///
/// If the user cannot be authenticated, an Unauthorized response is returned.
///
/// If the database cannot be connected to, a ServiceUnavailable response is returned.
#[docs_const]
pub async fn get_user_threads(req: HttpRequest) -> impl Responder {
    let qstring = qstring::QString::from(req.query_string());
    let headers = req.headers();

    debug!("Query string: {:?}", qstring);
    // debug!("Headers: {:?}", headers);

    // First try to authorize the user.
    let user_id = crate::auth::authorize_or_fail!(qstring, headers);

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
