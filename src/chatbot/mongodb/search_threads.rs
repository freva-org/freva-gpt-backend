use actix_web::{HttpRequest, HttpResponse, Responder};
use documented::docs_const;
use tracing::{debug, warn};

use crate::chatbot::mongodb::mongodb_storage::{get_database, query_by_topic, query_by_variant};

/// Searches the threads in the database by a given user ID.
/// Supports specifying how many results should be used and pagination.
///
/// The search query is contained inside the `query` parameter.
/// It searches in the topic field of the threads.  
///
/// The `num_threads` and `page` parameters can be used to specify how many results should be returned and which page (0-based) should be returned.
#[docs_const]
pub async fn search_threads(req: HttpRequest) -> impl Responder {
    let qstring = qstring::QString::from(req.query_string());
    let headers = req.headers();

    debug!("Query string: {:?}", qstring);
    debug!("Headers: {:?}", headers);

    // In order to search threads, the user needs to be authenticated.
    let maybe_username = crate::auth::authorize_or_fail!(qstring, headers);

    let Some(user_id) = maybe_username else {
        warn!("Failed to authenticate user");
        return HttpResponse::Unauthorized().body("Could not authenticate, missing user ID.");
    };

    // Now the query
    let query = qstring
        .get("query")
        .or_else(|| headers.get("query").and_then(|h| h.to_str().ok()));

    let Some(query) = query else {
        warn!("Failed to get query");
        return HttpResponse::BadRequest().body("Missing query parameter.");
    };

    let query = query.to_lowercase();

    // Instead of only searching the topic, we want to be able to search for the user input, AI response, code input or code output.
    // The user can do this by prefixing their search with "user:", "ai:", "code_input:" or "code_output:". (I'll also add some aliases)

    let query = {
        // The query can either be a simple topic query, if no colon is found, or a variant query if there is a colon.
        let parts = query.split_once(':');
        match parts {
            Some((prefix, content)) => match prefix.trim() {
                // If the prefix (before the colon) is recognized, search for a variant instead.
                "user" | "u" | "input" | "me" | "question" | "request" | "i" | "benutzer"
                | "eingabe" | "chris" | "sebastian" | "bianca" | "gizem" | "etor" => {
                    Err(("User", content))
                }
                "ai" | "a" | "assistant" | "frevagpt" | "freva-gpt" | "freva_gpt" | "answer"
                | "ki" | "assistent" | "computer " => Err(("Assistant", content)),
                "code_input" | "ci" | "code" | "codeinput" | "python" | "py" => {
                    Err(("Code", content))
                }
                "code_output" | "co" | "codeoutput" | "output" | "ausgabe" | "ergebnis" => {
                    Err(("CodeOutput", content))
                }
                _ => Ok(query), // This fails silently, which isn't that good, but it's easiest for the frontend. TODO: Maybe ask whether it should be an error instead.
            },
            None => Ok(query),
        }
    };

    // Get the num_threads and page
    let num_threads = qstring
        .get("num_threads")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(10);
    let page = qstring
        .get("page")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    // We need to get the database before we can query.
    let maybe_vault_url = headers
        .get("x-freva-vault-url")
        .and_then(|h| h.to_str().ok());

    let database = if let Some(vault_url) = maybe_vault_url {
        get_database(vault_url).await
    } else {
        warn!("Failed to get vault URL");
        return HttpResponse::BadRequest()
            .body("Vault URL not found. Please provide a vault URL in the headers.");
    };

    let database = match database {
        Ok(db) => db,
        Err(e) => {
            warn!("Failed to get database: {:?}", e);
            return HttpResponse::InternalServerError().body("Failed to get database.");
        }
    };

    let result = match query {
        Ok(topic) => query_by_topic(&user_id, &topic, num_threads, page, database).await,
        Err((variant, content)) => {
            // Pass it along
            query_by_variant(&user_id, variant, content, num_threads, page, database).await
        }
    };

    match result {
        Ok(threads) => HttpResponse::Ok().json(threads),
        Err(e) => {
            warn!("Failed to query threads: {:?}", e);
            HttpResponse::InternalServerError().body("Failed to query threads.")
        }
    }
}
