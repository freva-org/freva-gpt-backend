// Handles the storage and retrieval of conversations.
// In the OpenAI V2, they're called threads, so that's what we'll call them here too.
// Due to us using V1, OpenAI doesn't store the conversations (for us), so we need to do that ourselves.
// They will all be stored at `./threads/THEADID.txt`, where the ThreadID is the ID of the conversation.
// Reading and writing is just manipulating files, so we can use the `std::fs` module.
// Note that the file of a conversation is opened at the start of the stream, so it cannot be read from while it is being written to.

// The File will store the conversation in the JSON lines format, where each line is a JSON object,
// specifying the variant, as serialized by serde_json.

use std::{
    fs::{File, OpenOptions},
    io::{Error, Read, Write},
};

use tracing::{debug, error, info, trace, warn};

use crate::chatbot::types::unescape_string;

use super::types::{Conversation, StreamVariant};

/// Appends events from a stream of a conversation to the file of the conversation.
pub fn append_thread(thread_id: &str, content: Conversation) {
    trace!("Will append content to thread: {:?} (to clean up)", content);
    let mut content = content;
    cleanup_conversation(&mut content);
    trace!("Appending content to thread: {:?}", content);
    // First we have to convert the content to a string.
    if content.is_empty() {
        // weird, but we can just return here
        debug!("Content is empty, not writing anything to file.");
        return;
    }
    let mut to_write = String::new();

    for variant in content {
        let to_push = match serde_json::to_string(&variant) {
            Ok(s) => s, // If it works, we can just use the JSON string.
            Err(e) => {
                // If it doesn't, we can fall back to the old encoding. This is very bad, but we can't just not store the conversation.
                // Besides, according to the signature of `serde_json::to_string`,
                // this should only be able to fail if the type is not infallibly serializable, which StreamVariant is.
                error!(
                    "Error serializing variant to JSON; falling back to old encoding: {:?}",
                    e
                );
                variant.to_string() // This will use the old encoding, see the types.rs file.
            }
        };

        to_write.push_str(to_push.as_str());
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
fn open_thread(thread_id: &str) -> Option<File> {
    trace!("Opening thread with id: {}", thread_id);
    // We'll try to open the file for the conversation.
    match OpenOptions::new()
        .write(true) // Write, don't only read
        .append(true) // Append, don't overwrite
        .create(true) // Create if it doesn't exist
        .open(format!("./threads/{thread_id}.txt"))
    {
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
    let res = extract_variants_from_string(&content);

    trace!("Returning number of lines: {}", res.len());

    Ok(res)
}

pub fn extract_variants_from_string(content: &str) -> Vec<StreamVariant> {
    let lines = content.lines();
    let mut res = Vec::new();
    for line in lines {
        // First try to use json to deserialize the line.
        match serde_json::from_str(line) {
            Ok(variant) => {
                trace!("Successfully deserialized line: {:?}", variant);
                res.push(variant);
                continue;
            }
            Err(e) => {
                // If we can't deserialize the line, we'll assume that it uses the old encoding and try that.
                info!("Error deserializing line, trying old encoding: {:?}", e);
            }
        }

        // For organisational purposes, some lines might be comments (or empty), so we need to skip those.
        if line.trim().is_empty() || line.starts_with("//") {
            trace!("Skipping empty or comment line: {}", line);
            continue;
        }

        let line = line.trim_matches('\"'); // Remove any quotes that might be there.
        let parts = line.split_once(':');
        trace!("Parts: {:?}", parts);
        if let Some(parts) = parts {
            let to_append = match parts {
                ("Prompt", s) => StreamVariant::Prompt(unescape_string(s)),
                ("User", s) => StreamVariant::User(unescape_string(s)),
                ("Assistant", s) => StreamVariant::Assistant(unescape_string(s)),
                ("Code", s) => {
                    if let Some((content, id)) = split_colon_at_end(&unescape_string(s)) {
                        StreamVariant::Code((*content).to_string(), (*id).to_string())
                    } else {
                        warn!("Error splitting Code variant, skipping.");
                        continue;
                    }
                }
                ("CodeOutput", s) => {
                    if let Some((content, id)) = split_colon_at_end(&unescape_string(s)) {
                        StreamVariant::CodeOutput((*content).to_string(), (*id).to_string())
                    } else {
                        warn!("Error splitting CodeOutput variant, skipping.");
                        continue;
                    }
                }
                ("Image", s) => StreamVariant::Image(unescape_string(s)),
                ("ServerError", s) => StreamVariant::ServerError(unescape_string(s)),
                ("OpenAIError", s) => StreamVariant::OpenAIError(unescape_string(s)),
                ("CodeError", s) => StreamVariant::CodeError(unescape_string(s)),
                ("StreamEnd", s) => StreamVariant::StreamEnd(unescape_string(s)),
                ("ServerHint", s) => StreamVariant::ServerHint(unescape_string(s)),
                // If we do find a line that doesn't match any of the above, we can skip it.
                (variant, s) => {
                    warn!(
                        "Unknown variant in conversation file: {}, skipping.",
                        variant
                    );
                    debug!("The content of the line was: {}", s);
                    continue;
                }
            };
            res.push(to_append);
        } else {
            warn!("Error splitting line during parsing, is there no colon? Skipping.");
        }
    }
    res
}

/// Some variants like Code and CodeOutput have more than one field, so this function splits the content at the last colon.
fn split_colon_at_end(s: &str) -> Option<(&str, &str)> {
    let (first, last) = s.rsplit_once(':')?;
    Some((first, last))
}

/// When a conversation is saved, it might be corrupted in some way.
/// For us, this means that every Code variant needs to be followed by a CodeOutput variant
/// after some number of ServerHint variants,
/// and that the very last variant needs to be a StreamEnd variant.
pub fn cleanup_conversation(content: &mut Conversation) {
    // Insert a CodeOutput variant after every Code variant.
    let mut i = 0; // The index of the current variant.
    let mut active_code_id = None; // The ID of the current code variant.
    while i < content.len() {
        match &content[i] {
            StreamVariant::Code(_, id) => {
                active_code_id = Some(id.clone());
            }
            StreamVariant::CodeOutput(_, _) => {
                active_code_id = None;
            }
            StreamVariant::ServerHint(_) => {
                // If we're in a ServerHint, we can just skip it.
                i += 1;
                continue;
            }
            _ => {
                if let Some(id) = active_code_id.take() {
                    // Also resets the active code ID.
                    // If we're in a variant that is not a CodeOutput, but we have an active code ID, we need to insert a CodeOutput variant.
                    content.insert(i, StreamVariant::CodeOutput(String::new(), id));
                    i += 1;
                    continue;
                }
            }
        }
        i += 1;
    }

    // If the last variant is not a StreamEnd variant, we'll need to insert one.
    if let Some(last) = content.last() {
        if !matches!(last, StreamVariant::StreamEnd(_)) {
            content.push(StreamVariant::StreamEnd(
                "Stream ended in a very unexpected manner".to_string(),
            ));
        }
    }
}
