// For statically serving responses

use actix_web::{HttpResponse, Responder};
use const_format::concatcp;

// The Ping reponse contains a short description of the server's capabilities.
pub async fn ping() -> impl Responder {


    const VERSION: &str = concat!("Version: ", env!("CARGO_PKG_VERSION"), "\n");
    const ROUTES: &str = "ping,getthread,streamreponse,stop\n";
    const STREAMVARIANTS: &str = "User,Assistant,Code,CodeOutput,Image,ServerError,OpenAIError,CodeError,StreamEnd\n"; //TODO: un-hardcode this, instead read the variants from the LLM module
    const RESPONSE: &str = concatcp!(VERSION, ROUTES, STREAMVARIANTS);

    HttpResponse::Ok().body(RESPONSE)
}

// not_found returns a 404 response
pub async fn not_found() -> impl Responder {
    HttpResponse::NotFound().body("404 Not Found")
}
