// #![warn(clippy::cargo, clippy::pedantic)] // Enable for more warnings.

// Freva-GPT2-backend: Backend for the second version of the Freva-GPT project

use std::time::Duration;

use actix_web::{services, web, App, HttpServer};
use clap::Parser;
use dotenvy::dotenv;
use tool_calls::code_interpreter::prepare_execution::run_code_interpeter;
use tracing::{debug, error, info};

mod auth; // for basic authentication
mod chatbot; // for the actual chatbot
mod cla_parser; // for parsing the command line arguments
mod logging; // for setting up the logger
mod static_serve; // for serving static responses 
mod tool_calls; // for the tool calls
mod runtime_checks; // for the runtime checks

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // What the user has passed in the command line
    let args = cla_parser::Args::parse();

    // If we are in code_interpreter mode, run the code interpreter and have it exit without starting the server.
    if let Some(code) = &args.code_interpreter {
        run_code_interpeter(code.clone());
    }

    print!("Setting up the logger... ");
    let _logger = logging::setup_logger(&args); // store in a variable to keep the logger alive. If it drops, the logger will stop logging.
    println!("Success!");

    // Read from env file. This loads the environment variables from the .env file into `std::env::var`.
    match dotenv() {
        Ok(env_file) => info!("Reading from env file: {:?}", env_file),
        Err(e) => {
            error!("Error reading from env file due to error: {e:?}. Note that the search for the env file starts at pwd, not where the executable lies. Falling back to defaults, may not work!");
            eprintln!("Error reading from env file due to error: {e:?}. Note that the search for the env file starts at pwd, not where the executable lies. Falling back to defaults, may not work!");
        }
    }

    // Server information: host and port
    debug!(
        "Reading host and port from environment variables: {:?}:{:?}",
        std::env::var("HOST"),
        std::env::var("PORT")
    );
    let port = std::env::var("PORT").unwrap_or_else(|_| "8502".to_string());
    let port = port.parse::<u16>().unwrap_or_else(|_| {
        error!("Error parsing port number. Falling back to default port 8502");
        eprintln!("Error parsing port number. Falling back to default port 8502");
        8502
    });
    let host = std::env::var("HOST").unwrap_or_else(|_| "localhost".to_string());

    // Run all runtime checks
    runtime_checks::run_runtime_checks();

    info!("Starting server at {host}:{port}");
    println!("Starting server at {host}:{port}");

    // Start the server
    HttpServer::new(|| {
        let services = services![
            web::scope("/api/chatbot")
                .route("/ping", web::get().to(static_serve::ping)) // Ping, return a short description of the API. 
                .route("/help", web::get().to(static_serve::ping)) // Ping, return a short description of the API. 
                .route("/stop", web::get().to(chatbot::stop::stop)) // Stop, stop a specific conversation by thread ID.
                .route("/stop", web::post().to(chatbot::stop::stop)) // Stop, stop a specific conversation by thread ID. Both post and get are allowed.
                .route("/docs", web::get().to(static_serve::docs)) // Docs, return the documentation of the API.
                .route("/getthread", web::get().to(chatbot::get_thread::get_thread)) // GetThread, get the thread of a specific conversation by thread ID.
                .route("/streamresponse", web::get().to(chatbot::stream_response::stream_response)) // StreamResponse, stream the response of a specific conversation by thread ID.
            ,
            // Also, for convenience, all old points without the /api/chatbot, give a "moved permanently" to the new location.
            web::scope("/ping").route("", actix_web::web::get().to(static_serve::moved_permanently)),
            web::scope("/help").route("", actix_web::web::get().to(static_serve::moved_permanently)),
            web::scope("/stop").route("", actix_web::web::get().to(static_serve::moved_permanently)),
            web::scope("/stop").route("", actix_web::web::post().to(static_serve::moved_permanently)),
            web::scope("/docs").route("", actix_web::web::get().to(static_serve::moved_permanently)),
            web::scope("/getthread").route("", actix_web::web::get().to(static_serve::moved_permanently)),
            web::scope("/streamresponse").route("", actix_web::web::get().to(static_serve::moved_permanently)),
        ];
        App::new()
            .service(services)
            .default_service(web::route().to(static_serve::not_found))
    })
    .bind((host, port))
    .unwrap_or_else(|_| {
        error!("Error binding to the address. Exiting...");
        eprintln!("Error binding to the address. Exiting...");
        std::process::exit(1);
    })
    .keep_alive(Duration::from_secs(75)) // Long keep-alive time to prevent the server from closing the connection too early.
    // But as far as I can see, we will always have the problem that the stream length is capped at the keep-alive time...
    // If the keep-alive time is too short, we risk the connection being closed before the stream is finished.
    // If it's too long, there might be a lot of open connections that are not being used.
    .run()
    .await
}
