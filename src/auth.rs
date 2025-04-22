// For basic authorization.

/// For now, we'll just read the auth key from the environment and check it against the key provided in the request.
pub static AUTH_KEY: once_cell::sync::OnceCell<String> = once_cell::sync::OnceCell::new();

use actix_web::{http::header::HeaderMap, HttpResponse};
use qstring::QString;
/// Very simple macro for the API points to call at the beginning to make sure that a request is authorized.
/// If it isn't, it automatically returns the correct response.
use tracing::{debug, error, warn};

pub fn authorize_or_fail_fn(qstring: &QString, headers: & HeaderMap) -> Result<(), HttpResponse> {

        let Some(auth_key) = crate::auth::AUTH_KEY.get() else {
            error!("No key found in the environment. Sending 500.");
            return Err(HttpResponse::InternalServerError().body("No auth key found in the environment; Authorization failed."));
        };

        // Read from the variable `qstring`
        match (qstring.get("auth_key"), headers.get("Authorization")) {

            (maybe_key, Some(header_val)) => {
                // The user (maybe) sent both an auth_key in the query string and an Authorization header.
                // The header takes priority, but we'll emit a warning if they don't match.

                // The header can be any value, we only allow String.
                let auth_string: String = match header_val.to_str() {
                    Ok(header_val) => header_val.to_string(),
                    Err(e) => {
                        warn!("Authorization header is not a valid UTF-8 string: {}", e);
                        return Err(HttpResponse::BadRequest().body("Authorization header is not a valid UTF-8 string."));
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

                // Now we have the token, we need to check if it matches the key in the environment.
                // And also how it compares to the key in the query string.
                if let Some(query_token) = maybe_key {
                    // If the query string token is present, we'll check whether it equals the token.
                    if token != query_token {
                        warn!("Authorization header and query string auth_key do not match.");
                        
                        // If both don't match, we'll return a 401.
                        if token != auth_key && query_token != auth_key {
                            warn!("Authorization header and query string auth_key do not match.");
                            return Err(HttpResponse::Unauthorized().body("Authorization header and query string auth_key do not match."));
                        };
                        // Else, one of them matches, so we can successfully authorize the request.
                        debug!("Authorization header or query string match auth_key.");
                        Ok(())
                    } else {
                        // They're both the same, which is weird, but Ok. 
                        if token != auth_key {
                            warn!("Authorization header and query string auth_key do not match.");
                            return Err(HttpResponse::Unauthorized().body("Authorization header and query string auth_key do not match."));
                        };
                        debug!("Authorization header and query string match auth_key.");
                        Ok(())
                    }
                } else {
                    // If the query string token is not present, we'll check if the token matches the key in the environment.
                    if token != auth_key {
                        warn!("Authorization header does not match auth_key.");
                        return Err(HttpResponse::Unauthorized().body("Authorization header does not match auth_key."));
                    }
                    debug!("Authorization header matches auth_key.");
                    Ok(())
                }
            },
            (Some(key), None) => {
                // Try to retrieve the key, it should always work.
                // If the key is not the same as the one in the environment, we'll return a 401.
                if key != auth_key {
                    warn!("Unauthorized request.");
                    return Err(HttpResponse::Unauthorized().body("Unauthorized request."));
                }
                // Otherwise, it just worked.
                debug!("Authorized request.");
                Ok(())
            },
            (None, None) => {
                // If the key is not found, we'll return a 401.
                warn!("No key provided in the request.");
                Err(HttpResponse::Unauthorized().body("No key provided in the request. Please set the auth_key in the query parameters."))
            }
        }
    }

// The authorize_or_fail macro is wrapping the function and return the error variant
// if it fails.
macro_rules! authorize_or_fail {
    ($qstring:expr, $headers:expr) => {
        match $crate::auth::authorize_or_fail_fn(&$qstring, $headers) {
            Ok(_) => (),
            Err(e) => return e,
        }
    };
}

pub(crate) use authorize_or_fail; // Export the macro for use in other modules.

