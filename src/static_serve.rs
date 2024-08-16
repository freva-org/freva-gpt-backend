// For statically serving responses

use actix_web::{HttpResponse, Responder};
use const_format::concatcp;
use documented::Documented;
use tracing::{debug, trace};

use crate::chatbot::types::StreamVariant;

// The Ping reponse contains a short description of the server's capabilities.

const VERSION: &str = concat!("Version: ", env!("CARGO_PKG_VERSION"), "\n");
const STREAMVARIANTS: &str = "Streamvariants=User,Assistant,Code,CodeOutput,Image,ServerError,OpenAIError,CodeError,StreamEnd,ServerHint\n"; //TODO: un-hardcode this, instead read the variants from the LLM module
const PING_SPEC: &str = "ping:get,,String\n"; // ping has the get method, no input and outputs String.
const DOCS_SPEC: &str = "docs:get,,String\n"; // docs has the get method, no input and outputs String.
const GETTHREAD_SPEC:&str="getthread:get,thread_id=String&auth_key=String,Json{List{Variant:Streamvariant=String,Content:String}}\n"; // getthread has the get method, takes a threadid and returns a list of Streamvariants and their content.
const STREAMRESPONSE_SPEC:&str="streamresponse:get,thread_id=Optional{String}&input=String&auth_key=String,Stream{Json{Variant:Streamvariant=String,Content:String}}\n"; // streamresponse has the get method, takes an optional threadid and returns a stream of Streamvariants and their content.
const STOP_SPEC: &str = "stop:post+get,thread_id=String&auth_key=String,\n"; // stop can be called with post or get, takes a threadid and returns nothing.
const ALL_SPECS: &str = concatcp!(PING_SPEC, DOCS_SPEC, GETTHREAD_SPEC, STREAMRESPONSE_SPEC, STOP_SPEC);
pub const RESPONSE: &str = concatcp!(VERSION, STREAMVARIANTS, ALL_SPECS);

/// Simply returns a short description of the server's capabilities as well as the backend version.
/// The first line is always "Version: x.y.z" where x.y.z is the version of the backend. This follows the rules of SemVer.
///  
/// The second line describes all Streamvariants that the server can return.
///  
/// From the third line onwards, every line describes an API endpoint. The format is:
/// NAME:METHOD(S),ARGUMENTS,RETURN_TYPE
pub async fn ping() -> impl Responder {
    trace!("Ping request received.");
    HttpResponse::Ok().body(RESPONSE)
}


/// not_found returns a 404 response
pub async fn not_found() -> impl Responder {
    debug!("404 Method Not Found, try /help");
    HttpResponse::NotFound().body("404 Method Not Found, try /help")
}

const STREAMVARIANTS_DOCS: &str = StreamVariant::DOCS;
const PING_DOCS: &str = "\n# PING:\n\nSimply returns a short description of the server's capabilities as well as the backend version.\nThe first line is always \"Version: x.y.z\" where x.y.z is the version of the backend. This follows the rules of SemVer.\n\nThe second line describes all Streamvariants that the server can return.\n\nFrom the third line onwards, every line describes an API endpoint. The format is:\nNAME:METHOD(S),ARGUMENTS,RETURN_TYPE";
const DOCS_DOCS: &str = "\n# DOCS:\n\nReturns the documentation for the API.\n\nTakes no arguments and returns a string with the documentation.";
const GETTHREAD_DOCS: &str = "\n# GETTHREADS:\n\nReturns the content of a thread as a Json of List of Strings. \n\nAs arguments, it takes in a `thread_id` and an `auth_key`.\n\nThe thread id is the unique identifier for the thread, given to the client when the stream started in a ServerHint variant.\n\nThe auth key needs to match the one on the backend for the request to be authorized.\nTo get the auth key, the user needs to contact the backend administrator.\n\nIf the auth key is not given or does not match the one on the backend, an Unauthorized response is returned.\n\nIf the thread id is not given, a BadRequest response is returned.\n\nIf the thread with the given id is not found, a NotFound response is returned.\n\nIf the thread is found but cannot be read or cannot be displayed, an InternalServerError response is returned.";
const STREAMRESPONSE_DOCS: &str = "\n# STREAMRESPONSE:\n\nTakes in a thread_id, an input and an auth_key and returns a stream of StreamVariants and their content.\n\nThe thread_id is the unique identifier for the thread, given to the client when the stream started in a ServerHint variant.\nIf it's empty or not given, a new thread is created.\n\nThe stream consists of StreamVariants and their content. See the different Stream Variants above. \nIf the stream creates a new thread, the new thread_id will be sent as a ServerHint.\nThe stream always ends with a StreamEnd event, unless a server error occurs.\n\nA usual stream cosists mostly of Assistant messages many times a second. This is to give the impression of a real-time conversation.\n\nIf the input is not given, a BadRequest response is returned.\n\nIf the auth_key is not given or does not match the one on the backend, an Unauthorized response is returned.\n\nIf the thread_id does not point to an existing thread, an InternalServerError response is returned.\n\nIf the stream fails due to something else on the backend, an InternalServerError response is returned.";
const STOP_DOCS: &str = "\n# STOP:\n\nStops the conversation with the given thread ID as soon as possible.\n\nTakes in a `thread_id` and an `auth_key`.\nThe thread_id identifies the conversation to stop.\nThe auth_key needs to match the one on the backend for the request to be authorized.\n\nIf the auth key is not given or does not match the one on the backend, an Unauthorized response is returned.\n\nIf the thread id is not given, a BadRequest response is returned.\n\nIf there is an error stopping the conversation, an InternalServerError response is returned.";
const ALL_DOCS: &str = concatcp!(PING_DOCS, DOCS_DOCS, GETTHREAD_DOCS, STREAMRESPONSE_DOCS, STOP_DOCS);
pub const DOCS: &str = concatcp!(VERSION, STREAMVARIANTS_DOCS, ALL_DOCS);

/// Returns the documentation for the API.
/// 
/// Takes no arguments and returns a string with the documentation.
pub async fn docs() -> impl Responder {
    trace!("Docs request received.");
    HttpResponse::Ok().body(DOCS) 
}

/// Simple response to trying to access the old endpoints.
/// Simple answer that it should be accessed through the /api/chatbot/ endpoint.
pub async fn moved_permanently() -> impl Responder {
    HttpResponse::MovedPermanently().body("The Api Endpoints have changed. Instead of using /ping, etc. use /api/chatbot/ping.")
}