// For basic authorization.

/// For now, we'll just read the auth key from the environment and check it against the key provided in the request.
pub static AUTH_KEY: once_cell::sync::OnceCell<String> = once_cell::sync::OnceCell::new();

/// Same with whether or not guests should be allowed to access the streaming API.
pub static ALLOW_GUESTS: once_cell::sync::OnceCell<bool> = once_cell::sync::OnceCell::new();

use actix_web::{http::header::HeaderMap, HttpResponse};
use qstring::QString;
/// Very simple macro for the API points to call at the beginning to make sure that a request is authorized.
/// If it isn't, it automatically returns the correct response.
/// If a username was found in the token check, it will be returned.
use tracing::{debug, error, info, trace, warn};

pub async fn authorize_or_fail_fn(
    qstring: &QString,
    headers: &HeaderMap,
) -> Result<Option<String>, HttpResponse> {
    let Some(auth_key) = crate::auth::AUTH_KEY.get() else {
        error!("No key found in the environment. Sending 500.");
        return Err(HttpResponse::InternalServerError()
            .body("No auth key found in the environment; Authorization failed."));
    };

    // The new authentication system requires the client to (usually over nginx) send a vault url in the headers.
    // Against this URL, the token will be checked.
    let vault_url = headers
        .get("x-freva-vault-url")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let vault_url = if let Some(url) = vault_url {
        // Success case
        debug!("Vault URL found in headers: {}", url);
        url
    } else {
        // Because we expect all requests to go through nginx or to be done manually, we expect the vault URL to be present.
        warn!("No vault URL found in headers, expecting incorrect or malicous usage.");
        return Err(HttpResponse::BadRequest()
            .body("Authentication not successful; please use the nginx proxy.")); // Deliberately vague
    };

    // Read from the variable `qstring`
    match (qstring.get("auth_key"), headers.get("Authorization").or(headers.get("x-freva-user-token"))) {
        (maybe_key, Some(header_val)) => {
            // The user (maybe) sent both an auth_key in the query string and an Authorization header.
            // The header takes priority, but we'll emit a warning if they don't match.

            // The header can be any value, we only allow String.
            let auth_string: String = match header_val.to_str() {
                Ok(header_val) => header_val.to_string(),
                Err(e) => {
                    warn!("Authorization header is not a valid UTF-8 string: {}", e);
                    return Err(HttpResponse::BadRequest()
                        .body("Authorization header is not a valid UTF-8 string."));
                }
            };
            debug!("Authorization header: {}", auth_string);
            debug!("Query string auth_key: {:?}", maybe_key);
            // The Authentication header is a Bearer token, so we need to extract the token from it.
            let token = match auth_string.strip_prefix("Bearer ") {
                Some(token) => token,
                None => {
                    warn!("Authorization header is not a Bearer token.");
                    return Err(HttpResponse::BadRequest().body("Authorization header is not a Bearer token. Please use the Bearer token format."));
                }
            };

            let token_check = get_username_from_token(token, &vault_url).await;

            // Depending on whether the token was valid or not, check the query string token.
            match token_check {
                Ok(username) => {
                    debug!("Token check successful, returning username: {}", username);
                    Ok(Some(username))
                }
                Err(tokencheck_error) => {
                    // Now we have the token, we need to check whether it is valid.
                    info!("Token check failed, checking query string auth_key.");
                    if let Some(query_token) = maybe_key {
                        // If the query string token is present, we'll check whether it equals the token.

                        // If both don't match, we'll return a 401.
                        if query_token != auth_key {
                            warn!(
                                "Authorization header and query string auth_key do not authorize. Sending error: {:?}", tokencheck_error
                            );
                            return Err(tokencheck_error);
                        };
                        // Else, the query string token is valid. This will be removed in the future.
                        debug!("Query string matches auth_key, authenticating without username.");
                        Ok(None)
                    } else {
                        // If the query string token is not present, we've also run out of authentication options.

                        warn!("Authorization header is not valid and no query string auth_key provided. Sending error: {:?}", tokencheck_error);
                        Err(tokencheck_error)
                    }
                }
            }
        }
        (Some(key), None) => {
            // If the key is not the same as the one in the environment, we'll return a 401.
            if key != auth_key {
                warn!("Unauthorized request.");
                return Err(HttpResponse::Unauthorized().body("Unauthorized request."));
            }
            // Otherwise, it just worked.
            debug!("Authorized request, no username.");
            Ok(None)
        }
        (None, None) => {
            // If the key is not found, we'll return a 401.
            warn!("No key provided in the request.");
            Err(HttpResponse::Unauthorized().body(
                "No key provided in the request. Please set the auth_key in the query parameters.",
            ))
        }
    }
}

/// Recives a token, checks it against the URL provided in the header and returns the username.
async fn get_username_from_token(token: &str, vault_url: &str) -> Result<String, HttpResponse> {
    debug!("Checking token: {}", token);
    debug!("Using vault URL: {}", vault_url);

    // // Also, for development, if the AUTH_URL is set to NO_AUTH, we'll skip the check. This is so I don't have to
    // // set up a server to check the token against.
    // if auth_url == "NO_AUTH" {
    //     debug!("Skipping token check, AUTH_URL is set to NO_AUTH.");
    //     return Ok("testing".to_string()); // "testing" is the username for development.
    // }

    // If the URL is set, we'll send a GET request to it with the token in the header.
    let client = reqwest::Client::new();
    let response = client
        .get(vault_url.to_string() + "/auth/v2/userinfo") // The endpoint is at "/auth/v2/userinfo"
        .header("Authorization", format!("Bearer {}", token));
    let response = response.send().await;

    let result = match response {
        Ok(res) => {
            if res.status().is_success() {
                // If the response is successful, we'll return the username.
                let content = res
                    .text()
                    .await
                    .unwrap_or_else(|_| "Empty JSON!".to_string())
                    .trim()
                    .to_owned(); // just signals that an error happened
                debug!("Token check successful, content: {}", content);
                content
            } else {
                // If the response is not successful, we'll return a 401.
                warn!("Token check failed, status code: {}", res.status());
                return Err(HttpResponse::Unauthorized().body("Token check failed."));
            }
        }
        Err(e) => {
            // If there was an error sending the request, we'll return a 500.
            error!("Error sending token check request: {}", e);
            return Err(
                HttpResponse::InternalServerError().body("Error sending token check request.")
            );
        }
    };

    // The result is a JSON object with the username and some other stuff, but we only care about the username.
    let username = match serde_json::from_str::<serde_json::Value>(&result) {
        Ok(json) => {
            // If the JSON is valid, we'll return the username.
            match json["username"].as_str() {
                Some(username) => username.to_string(),
                None => {
                    // If the username is not found, we'll return a 500.
                    error!("Username not found in token check response.");
                    return Err(HttpResponse::InternalServerError()
                        .body("Username not found in token check response."));
                }
            }
        }
        Err(e) => {
            // If the JSON is not valid, we'll return a 500.
            error!("Error parsing token check response: {}", e);
            return Err(
                HttpResponse::InternalServerError().body("Error parsing token check response.")
            );
        }
    };
    debug!("Token check successful, username: {}", username);
    Ok(username)
}

/// Receives the vault URL and returns the URL to the MongoDB database to use.
pub async fn get_mongodb_uri(vault_url: &str) -> Result<String, HttpResponse> {
    // The vault URL will be contained in the answer to the request to the vault. (No endpoint or authentication needed.)
    debug!("Getting MongoDB URL from vault: {}", vault_url);
    let client = reqwest::Client::new();
    let response = client
        .get(vault_url) 
        .send()
        .await;

    // Extract the result or fail
    let result = match response {
        Ok(res) => {
            if res.status().is_success() {
                // If the response is successful, we'll return the MongoDB URL.
                let content = res
                    .text()
                    .await;
                debug!("Response from vault: {:?}", content);
                let content = match content {
                    Ok(text) => text.trim().to_owned(),
                    Err(e) => {
                        error!("Error reading response text: {}", e);
                        return Err(HttpResponse::InternalServerError()
                            .body("Error reading response text from vault."));
                    }
                };
                debug!("Vault response: {}", content);
                content
            } else {
                // If the response is not successful, we'll return a 500.
                warn!("Failed to get MongoDB URL, status code: {}", res.status());
                return Err(HttpResponse::InternalServerError()
                    .body("Failed to get MongoDB URL."));
            }
        }
        Err(e) => {
            // If there was an error sending the request, we'll return a 500.
            error!("Error sending request to vault: {}", e);
            return Err(HttpResponse::InternalServerError()
                .body("Error sending request to vault."));
        }
    };

    // The result is a JSON object containing a bunch of stuff, but we only care about the MongoDB URL ("mongodb.url").
    let mongodb_url = match serde_json::from_str::<serde_json::Value>(&result) {
        Ok(json) => {
            // If the JSON is valid, we'll return the MongoDB URL.
            match json["mongodb.url"].as_str() {
                Some(url) => url.to_string(),
                None => {
                    // If the MongoDB URL is not found, we'll return a 500.
                    error!("MongoDB URL not found in vault response.");
                    return Err(HttpResponse::InternalServerError()
                        .body("MongoDB URL not found in vault response."));
                }
            }
        }
        Err(e) => {
            // If the JSON is not valid, we'll return a 500.
            error!("Error parsing vault response: {}", e);
            return Err(HttpResponse::InternalServerError()
                .body("Error parsing vault response."));
        }
    };
    debug!("MongoDB URL: {}", mongodb_url);
    Ok(mongodb_url)

}


/// The authorize_or_fail macro is wrapping the function and return the error variant
/// if it fails. If it succeeds because a good authentication token was given via header, the 
/// username is returned. If the token was given via query string, None is returned.
macro_rules! authorize_or_fail {
    ($qstring:expr, $headers:expr) => {
        match $crate::auth::authorize_or_fail_fn(&$qstring, $headers).await {
            Ok(maybe_username) => maybe_username,
            Err(e) => return e,
        }
    };
}

pub(crate) use authorize_or_fail;

/// Whether or not a username is considered a guest.
pub fn is_guest(username: &str) -> bool {
    trace!("Checking if username '{}' is a guest.", username);
    // If the ALLOW_GUESTS is true, we just allow all usernames.
    if let Some(allow_guests) = crate::auth::ALLOW_GUESTS.get() {
        if *allow_guests {
            return true;
        }
    } else {
        warn!("ALLOW_GUESTS is not set, this should not happen! defaulting to false.");
    }

    // Usernames are by default guests, unless they follow one of these patterns:
    // "kXXXXXX" (where X is a digit) or "bXXXXXX" (where X is a digit).
    // "testing" is also considered a non-guest
    if username == "testing" {
        return false;
    }
    if (username.starts_with('k') || username.starts_with('b')) && username.len() == 7 && username[1..].chars().all(|c| c.is_ascii_digit()) {
        return false; // It's a valid user ID, not a guest.
    }
    // If it doesn't match any of the above patterns, it's a guest.
    debug!("Username '{}' is considered a guest.", username);
    true
}