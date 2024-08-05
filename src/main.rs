// #![warn(clippy::cargo, clippy::pedantic)]

// Freva-GPT2-backend: Backend for the second version of the Freva-GPT project

use std::time::Duration;

use actix_web::{services, web, App, HttpServer};
use auth::AUTH_KEY;
use clap::Parser;
use dotenvy::dotenv;
use tracing::{error, info, trace};

mod chatbot;
mod cla_parser; // for parsing the command line arguments
mod logging; // for setting up the logger
mod static_serve; // for serving static responses // for the actual chatbot
mod auth; // for basic authentication

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // What the user has passed in the command line
    let args = cla_parser::Args::parse();

    logging::setup_logger(&args);

    // Read from env file. This loads the environment variables from the .env file into `std::env::var`.
    match dotenv() {
        Ok(env_file) => info!("Reading from env file: {:?}", env_file),
        Err(e) => {
            error!("Error reading from env file due to error: {e:?}. Note that the search for the env file starts at pwd, not where the executable lies. Falling back to defaults, may not work!");
            eprintln!("Error reading from env file due to error: {e:?}. Note that the search for the env file starts at pwd, not where the executable lies. Falling back to defaults, may not work!");
        }
    }

    // Server information: host and port
    trace!(
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

    // The lazy static STARTING_MESSAGE_JSON can fail if the prompt or messages cannot be converted to a string. 
    // To make sure that this is caught early, we'll just test it here.
    let _ = chatbot::prompting::STARTING_PROMPT_JSON.clone();
    trace!("Starting messages JSON: {:?}", chatbot::prompting::STARTING_PROMPT_JSON);

    trace!("Ping Response: {:?}", static_serve::RESPONSE);

    // We'll also initialize the authentication here so it's available for the entire server, from the very start.
    let auth_string = match std::env::var("AUTH_KEY"){
        Ok(auth_string) => auth_string,
        Err(e) => {
            error!("Error reading the authentication string from the environment variables: {:?}", e);
            eprintln!("Error reading the authentication string from the environment variables: {:?}", e);
            std::process::exit(1);
        }
    };
    AUTH_KEY.set(auth_string).unwrap_or_else(|_| {
        error!("Error setting the authentication string. Exiting...");
        eprintln!("Error setting the authentication string. Exiting...");
        std::process::exit(1);
    });
    info!("Authentication string set successfully.");
    println!("Authentication string set successfully.");

    info!("Starting server at {host}:{port}");
    println!("Starting server at {host}:{port}");

    // Start the server
    HttpServer::new(|| {
        let services = services![
            web::scope("/ping").route("", web::get().to(static_serve::ping)), // Ping, just reply with a pong
            web::scope("/help").route("", web::get().to(static_serve::ping)), // Also reply with a pong
            web::scope("/stop").route("", web::get().to(chatbot::stop::stop)), // Stop, stop a specific conversation by thread ID.
            web::scope("/stop").route("", web::post().to(chatbot::stop::stop)), // Stop, stop a specific conversation by thread ID. Both post and get are allowed.
            web::scope("/getthread").route("", web::get().to(chatbot::get_thread::get_thread)), // GetThread, get the thread of a specific conversation by thread ID.
            web::scope("/streamresponse")
                .route("", web::get().to(chatbot::stream_response::stream_response)), // StreamResponse, stream the response of a specific conversation by thread ID.
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
