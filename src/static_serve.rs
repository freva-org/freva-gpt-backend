// For statically serving responses

use std::env;

use actix_web::{HttpResponse, Responder};
use const_format::concatcp;
use documented::{docs_const, Documented};
use once_cell::sync::Lazy;
use strum::VariantNames;
use tracing::{debug, trace};

use crate::chatbot::{
    available_chatbots_endpoint::AVAILABLE_CHATBOTS_ENDPOINT_DOCS, get_thread::GET_THREAD_DOCS,
    get_user_threads::GET_USER_THREADS_DOCS, stop::STOP_DOCS,
    stream_response::STREAM_RESPONSE_DOCS, types::StreamVariant,
};

/// The valid methods for an endpoint.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum EndpointMethods {
    Get,
    Post,
}

/// The specification for an endpoint, for the ping endpoint.
#[derive(Debug, serde::Serialize)]
struct EndpointSpec {
    name: &'static str,
    methods: &'static [EndpointMethods],
    params: serde_json::map::Map<String, serde_json::Value>,
    return_type: serde_json::Value,
}

static PING_SPEC: Lazy<EndpointSpec> = Lazy::new(|| {
    EndpointSpec {
    name: "ping",
    return_type: serde_json::Value::String("json{version:string,streamvariants:list{string},endpoints:list{name:string,methods:string,params:list{json},returntype:json}}".to_string()),
    params: serde_json::Map::new(),                             // no params
    methods: &[EndpointMethods::Get],
}
});

static DOCS_SPEC: Lazy<EndpointSpec> = Lazy::new(|| EndpointSpec {
    name: "docs",
    return_type: serde_json::Value::String("string".to_string()), // Just for manual inspection
    params: serde_json::Map::new(),                               // no params
    methods: &[EndpointMethods::Get],
});

static GETTHREAD_SPEC: Lazy<EndpointSpec> = Lazy::new(|| EndpointSpec {
    name: "getthread",
    return_type: serde_json::Value::String(
        "json{list{variant:streamvariant=string,content:string}}".to_string(),
    ),
    params: serde_json::Map::from_iter(vec![
        (
            "thread_id".to_string(),
            serde_json::Value::String("string".to_string()),
        ),
        (
            "auth_key".to_string(),
            serde_json::Value::String("string".to_string()),
        ),
    ]),
    methods: &[EndpointMethods::Get],
});

static STREAMRESPONSE_SPEC: Lazy<EndpointSpec> = Lazy::new(|| EndpointSpec {
    name: "streamresponse",
    return_type: serde_json::Value::String(
        "stream{json{variant:streamvariant=string,content:string}}".to_string(),
    ),
    params: serde_json::Map::from_iter(vec![
        (
            "thread_id".to_string(),
            serde_json::Value::String("optional{string}".to_string()),
        ),
        (
            "input".to_string(),
            serde_json::Value::String("string".to_string()),
        ),
        (
            "auth_key".to_string(),
            serde_json::Value::String("string".to_string()),
        ),
    ]),
    methods: &[EndpointMethods::Get],
});

static STOP_SPEC: Lazy<EndpointSpec> = Lazy::new(|| EndpointSpec {
    name: "stop",
    return_type: serde_json::Value::String(String::new()),
    params: serde_json::Map::from_iter(vec![
        (
            "thread_id".to_string(),
            serde_json::Value::String("string".to_string()),
        ),
        (
            "auth_key".to_string(),
            serde_json::Value::String("string".to_string()),
        ),
    ]),
    methods: &[EndpointMethods::Get, EndpointMethods::Post],
});

const VERSION: &str = env!("CARGO_PKG_VERSION");

// Thanks to strum, there's StreamVariant::VARIANTS;
static STREAMVARIANTS: Lazy<serde_json::Value> = Lazy::new(|| {
    serde_json::Value::Array(
        StreamVariant::VARIANTS
            .iter()
            .map(|v| serde_json::Value::String((*v).to_string()))
            .collect(),
    )
});

// The entire response object contains the version, streamvariants and all the endpoint specs as a JSON object.
static RESPONSE: Lazy<serde_json::Value> = Lazy::new(|| {
    serde_json::Value::Object(serde_json::Map::from_iter(vec![
        (
            "version".to_string(),
            serde_json::Value::String(VERSION.to_string()),
        ),
        ("streamvariants".to_string(), STREAMVARIANTS.clone()),
        (
            "endpoints".to_string(),
            serde_json::Value::Array(vec![
                serde_json::to_value(&*PING_SPEC).expect("Unable to serialize JSON"),
                serde_json::to_value(&*DOCS_SPEC).expect("Unable to serialize JSON"),
                serde_json::to_value(&*GETTHREAD_SPEC).expect("Unable to serialize JSON"),
                serde_json::to_value(&*STREAMRESPONSE_SPEC).expect("Unable to serialize JSON"),
                serde_json::to_value(&*STOP_SPEC).expect("Unable to serialize JSON"),
            ]),
        ),
    ]))
});

pub static RESPONSE_STRING: Lazy<String> =
    Lazy::new(|| serde_json::to_string_pretty(&*RESPONSE).expect("Unable to serialize JSON"));

/// # Ping
/// Simply returns a short description of the server's capabilities as well as the backend version.
/// This is in the JSON format and contains three keys: the version, streamvariants and all the endpoint specs in a list.
///
/// The version number is in the form of "Version: x.y.z" where x.y.z is the version of the backend. This follows the rules of SemVer.
///
/// The Endpoints all have four keys: name, methods, params and return_type.
///
/// This endpoint can be used to check whether the server is running and whether the interal model of the client matches the server's.
#[docs_const] // constructs the documentation for this function into PING_DOCS
pub async fn ping() -> impl Responder {
    trace!("Ping request received.");
    HttpResponse::Ok().body(RESPONSE_STRING.to_string())
}

/// not_found returns a 404 response
pub async fn not_found() -> impl Responder {
    debug!("404 Method Not Found, try /help");
    HttpResponse::NotFound().body("404 Method Not Found, try /help")
}

const STREAMVARIANTS_DOCS: &str = StreamVariant::DOCS;
// The other docs come from the other modules, directly above the functions.
const ALL_DOCS: &str = concatcp!(
    "\n\n",
    PING_DOCS,
    "\n\n",
    DOCS_DOCS,
    "\n\n",
    GET_THREAD_DOCS,
    "\n\n",
    STREAM_RESPONSE_DOCS,
    "\n\n",
    GET_USER_THREADS_DOCS,
    "\n\n",
    STOP_DOCS,
    "\n\n",
    AVAILABLE_CHATBOTS_ENDPOINT_DOCS,
);
pub const DOCS: &str = concatcp!("Version: ", VERSION, STREAMVARIANTS_DOCS, ALL_DOCS);

/// # Docs
/// Returns the documentation for the API.
///
/// Takes no arguments and returns a string with the documentation.
#[docs_const] // constructs the documentation for this function into DOCS_DOCS
pub async fn docs() -> impl Responder {
    trace!("Docs request received.");
    HttpResponse::Ok().body(DOCS)
}

/// Simple response to trying to access the old endpoints.
/// Simple answer that it should be accessed through the /api/chatbot/ endpoint.
pub async fn moved_permanently() -> impl Responder {
    HttpResponse::MovedPermanently()
        .body("The Api Endpoints have changed. Instead of using /ping, etc. use /api/chatbot/ping.")
}
