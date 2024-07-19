// Freva-GPT2-backend: Backend for the second version of the Freva-GPT project

use actix_web::{services, web, App, HttpServer};
use clap::Parser;
use dotenvy::dotenv;
use tracing::{error, info, trace};

mod chatbot;
mod cla_parser; // for parsing the command line arguments
mod logging; // for setting up the logger
mod static_serve; // for serving static responses // for the actual chatbot

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // What the user has passed in the command line
    let args = cla_parser::Args::parse();

    logging::setup_logger(&args);

    // Read from env file. This loads the environment variables from the .env file into `std::env::var`.
    match dotenv() {
        Ok(env_file) => info!("Reading from env file: {:?}", env_file),
        Err(e) => {
            error!("Error reading from env file due to error: {:?}. Note that the search for the env file starts at pwd, not where the executable lies. Falling back to defaults, may not work!", e);
            eprintln!("Error reading from env file due to error: {:?}. Note that the search for the env file starts at pwd, not where the executable lies. Falling back to defaults, may not work!", e);
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

    info!("Starting server at {}:{}", host, port);
    println!("Starting server at {}:{}", host, port);

    // Start the server
    HttpServer::new(|| {
        let services = services![
            web::scope("/ping").route("", web::get().to(static_serve::ping)), // Ping, just reply with a pong
            web::scope("/stop").route("", web::post().to(chatbot::stop::stop)), // Stop, stop a specific conversation by thread ID.
            web::scope("/getthread").route("", web::get().to(chatbot::get_thread::get_thread)), // GetThread, get the thread of a specific conversation by thread ID.
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
    .run()
    .await
}
