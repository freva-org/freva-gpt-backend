// Handles the storage and retrieval of conversations.
// In the OpenAI V2, they're called threads, so that's what we'll call them here too.
// Due to us using V1, OpenAI doesn't store the conversations (for us), so we need to do that ourselves.
// They will all be stored at `./threads/THEADID.txt`, where the ThreadID is the ID of the conversation.
// Reading and writing is just manipulating files, so we can use the `std::fs` module.
// Note that the file of a conversation is opened at the start of the stream, so it cannot be read from while it is being written to.

// The file content will be structured as follows:
// STREAMVARIANT:CONTENT
// So for example, if the user asks "What is the capital of France?" and the assistant responds "Paris", the file
// will contain
// User:What is the capital of France?
// Assistant:Paris
// StreamEnd:Success

use std::{
    fs::{File, OpenOptions},
    io::{Error, Read, Write},
};

use tracing::{debug, error, trace, warn};

use super::
    types::{Conversation, StreamVariant}
;

/// Appends events from a stream of a conversation to the file of the conversation.
pub fn append_thread(thread_id: &str, content: Conversation) {
    trace!("Appending content to thread: {:?}", content);
    // First we have to convert the content to a string.
    if content.is_empty() {
        // weird, but we can just return here
        debug!("Content is empty, not writing anything to file.");
        return;
    }
    let mut to_write = String::new();

    for variant in content {
        to_write.push_str(&variant.to_string());
        to_write.push('\n');
    }

    trace!("Writing to file: {}", to_write);

    // Open File and write to it
    let Some(mut file) = open_thread(thread_id) else {
        // If we can't open the file, we'll just print the error and continue.
        // This is not a critical error, as the conversation is still running, but is bad because it means something is wrong with the filesystem.
        warn!("Error opening conversation file, not writing to file.");
        return;
    };

    // Then we write it to the file.
    match file.write_all(to_write.as_bytes()) {
        Ok(()) => trace!("Successfully wrote to file."),
        Err(e) => {
            // If we can't write to the file, we'll just print the error and continue.
            // This is not a critical error, as the conversation is still running, but is bad because it means something is wrong with the filesystem.
            warn!("Error writing conversation to file: {:?}", e);
        }
    }
}

/// Opens a file for a conversation and returns a file handle.
pub fn open_thread(thread_id: &str) -> Option<File> {
    trace!("Opening thread with id: {}", thread_id);
    // We'll try to open the file for the conversation.
    match OpenOptions::new()
        .write(true) // Write, don't only read
        .append(true) // Append, don't overwrite
        .create(true) // Create if it doesn't exist
        .open(format!("./threads/{thread_id}.txt"))
    {
        // We want to only append to the file and also to create it if it doesn't exist.
        Ok(file) => {
            trace!("Successfully opened file for conversation.");
            Some(file)
        }
        Err(e) => {
            // If we can't open the file, we'll just print the error and continue.
            // This is not a critical error, as the conversation is still running, but is bad because it means something is wrong with the filesystem and we can't store the conversation.
            warn!("Error opening conversation file: {:?}", e);
            None
        }
    }
}

/// Reads a file for a conversation and returns the content.
/// Returns the Read content as a Vec of `StreamVariants` or the IO Error that occured.
/// # Errors
/// Returns the IO Errors that occured while reading the file.
pub fn read_thread(thread_id: &str) -> Result<Conversation, Error> {
    trace!("Reading thread with id: {}", thread_id);

    let content = match OpenOptions::new()
        .read(true)
        .open(format!("./threads/{thread_id}.txt"))
    {
        Ok(mut file) => {
            // we can open the file
            let mut content = String::new();
            match file.read_to_string(&mut content) {
                Ok(_) => {
                    trace!("Successfully read file for conversation.");
                }
                Err(e) => {
                    // If we can't read the file, we'll have to error out.
                    error!(
                        "Error reading conversation file, sending error to client: {:?}",
                        e
                    );
                    return Err(e);
                }
            }
            content
        }
        Err(e) => {
            // If we can't open the file, we'll have to error out again, as the client expects the conversation to be there.
            error!(
                "Error opening conversation file, sending error to client: {:?}",
                e
            );
            return Err(e);
        }
    };

    trace!("Successfully read from File, content: {}", content);

    // We now need to "split" at the first colon, so we can get the variant and the content.
    let lines = content.lines();
    let mut res = Vec::new();

    for line in lines {
        let line = line.trim_matches('\"'); // Remove any quotes that might be there.
        let parts = line.splitn(2, ':').collect::<Vec<&str>>();
        trace!("Parts: {:?}", parts);
        let to_append = match parts.as_slice() {
            ["Prompt", s] => StreamVariant::Prompt((*s).to_string()),
            ["User", s] => StreamVariant::User((*s).to_string()),
            ["Assistant", s] => StreamVariant::Assistant((*s).to_string()),
            ["Code", s] => StreamVariant::Code((*s).to_string()),
            ["CodeOutput", s] => StreamVariant::CodeOutput((*s).to_string()),
            ["Image", s] => StreamVariant::Image((*s).to_string()),
            ["ServerError", s] => StreamVariant::ServerError((*s).to_string()),
            ["OpenAIError", s] => StreamVariant::OpenAIError((*s).to_string()),
            ["CodeError", s] => StreamVariant::CodeError((*s).to_string()),
            ["StreamEnd", s] => StreamVariant::StreamEnd((*s).to_string()),
            ["ServerHint", s] => StreamVariant::ServerHint((*s).to_string()),
            // If the line is empty, this will be the empty slice, so we need to cover that case.
            [] => {
                warn!("Empty line in conversation file, skipping.");
                continue;
            }
            // If we do find a line that doesn't match any of the above, we can skip it.
            [variant, s] => {
                warn!(
                    "Unknown variant in conversation file: {}, skipping.",
                    variant
                );
                debug!("The content of the line was: {}", s);
                continue;
            }
            // The splitn should always return a slice of length 0,1 or 2, so we can expect this
            _ => unreachable!("Splitn called with 2 as a limit returned a slice of length > 2"),
        };
        res.push(to_append);
    }

    trace!("Returning number of lines: {}", res.len());

    Ok(res)
}

