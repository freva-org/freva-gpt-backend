// For basic authorization. 

/// For now, we'll just read the auth key from the environment and check it against the key provided in the request.
pub(crate) static AUTH_KEY: once_cell::sync::OnceCell<String> = once_cell::sync::OnceCell::new();


/// Very simple macro for the API points to call at the beginning to make sure that a request is authorized.
/// If it isn't, it automatically returns the correct response.
macro_rules! authorize_or_fail {
    ($qstring:expr) => {
        use tracing::{debug, error, warn};
        // Read from the variable `qstring`
        match $qstring.get("auth_key") {

            Some(key) => {
                // Try to retrieve the key, it should always work.
                let auth_key = match crate::auth::AUTH_KEY.get() {
                    Some(key) => key,
                    None => {
                        error!("No key found in the environment. Sending 500.");
                        return HttpResponse::InternalServerError().body("No auth key found in the environment; Authorization failed.");
                    }
                };
                // If the key is not the same as the one in the environment, we'll return a 401.
                if key != auth_key {
                    warn!("Unauthorized request.");
                    return HttpResponse::Unauthorized().body("Unauthorized request.");
                }
                // Otherwise, it just worked. 
                debug!("Authorized request.");
            },
            None => {
                // If the key is not found, we'll return a 401.
                warn!("No key provided in the request.");
                return HttpResponse::Unauthorized().body("No key provided in the request. Please set the auth_key in the query parameters.");
            }
        }
    }
}

pub(crate) use authorize_or_fail; // Export the macro for use in other modules.
