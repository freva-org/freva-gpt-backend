// For statically serving responses

use actix_web::{HttpResponse, Responder};
use const_format::concatcp;
use tracing::{debug, trace};

// The Ping reponse contains a short description of the server's capabilities.


const VERSION: &str = concat!("Version: ", env!("CARGO_PKG_VERSION"), "\n");
const STREAMVARIANTS: &str = "Streamvariants=User,Assistant,Code,CodeOutput,Image,ServerError,OpenAIError,CodeError,StreamEnd,ServerHint\n"; //TODO: un-hardcode this, instead read the variants from the LLM module
const PING_SPEC:&str ="ping:get,,String\n"; // ping has the get method, no input and outputs String.
const GETTHREAD_SPEC:&str="getthread:get,thread_id=String&auth_key=String,Json{List{Variant:Streamvariant=String,Content:String}}\n"; // getthread has the get method, takes a threadid and returns a list of Streamvariants and their content.
const STREAMRESPONSE_SPEC:&str="streamresponse:get,thread_id=Optional{String}&input=String&auth_key=String,Stream{Json{Variant:Streamvariant=String,Content:String}}\n"; // streamresponse has the get method, takes an optional threadid and returns a stream of Streamvariants and their content.
const STOP_SPEC:&str = "stop:post+get,thread_id=String&auth_key=String,\n"; // stop can be called with post or get, takes a threadid and returns nothing.
const ALL_SPECS :&str = concatcp!(PING_SPEC,GETTHREAD_SPEC,STREAMRESPONSE_SPEC,STOP_SPEC);
pub const RESPONSE: &str = concatcp!(VERSION, STREAMVARIANTS, ALL_SPECS);

pub async fn ping() -> impl Responder {
    trace!("Ping request received.");
    HttpResponse::Ok().body(RESPONSE)
}

// not_found returns a 404 response
pub async fn not_found() -> impl Responder {
    debug!("404 Method Not Found, try /help");
    HttpResponse::NotFound().body("404 Method Not Found, try /help")
}
