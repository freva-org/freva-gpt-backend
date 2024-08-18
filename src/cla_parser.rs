// For parsing the command line arguments

use clap::{self, crate_authors, Parser};

#[derive(Parser, Debug)]
#[command(
    version, // Automatically fills in the version from Cargo.toml
    about = "Freva-GPT2-backend: Backend for the second version of the Freva-GPT project",
    long_about = "Freva-GPT2-backend: Starts the backend server for the Rest-like API to be used by the frontend. Serves the chatbot and manages calls of the code_interpreter.",
    author = crate_authors!(),
)]
pub struct Args {
    /// Make the program verbose, printing debug info too, then trace info. Can be used multiple times.
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Runs the code interpreter with the given code.
    /// For internal use only.
    #[arg(long)]
    pub code_interpreter: Option<String>,
}
