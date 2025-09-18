use actix_web::{HttpRequest, HttpResponse, Responder};
use documented::docs_const;
use tracing::trace;

/// # Available Chatbots
///
/// Statically returns the list of available chatbots as JSON.
/// Requires the auth key to be correct or an Authentication header with OpenIDConnect to be present.
///
/// The String representations of the chatbots can then be used at the '/streamresponse' endpoint
/// to start a conversation with a specific chatbot. If no chatbot is specified, the first one
/// in the list will be used.
#[docs_const]
pub async fn available_chatbots_endpoint(req: HttpRequest) -> impl Responder {
    let qstring = qstring::QString::from(req.query_string());
    let headers = req.headers();

    trace!("Query string: {:?}", qstring);

    // First try to authorize the user.
    let _maybe_username = crate::auth::authorize_or_fail!(qstring, headers);

    // The user wants a list of Strings, not the enum.
    let chatbot_string_list = crate::chatbot::available_chatbots::AVAILABLE_CHATBOTS
        .iter()
        .map(|chatbot| chatbot.clone().into())
        .collect::<Vec<String>>();

    HttpResponse::Ok().json(chatbot_string_list)
}
