use async_process::Command;

use itertools::Itertools;
use mongodb::Database;
use tracing::{debug, info, trace, warn};

use crate::{
    chatbot::{
        handle_active_conversations::{conversation_state, get_conversation},
        storage_router::read_thread,
        types::{ConversationState, StreamVariant},
    },
    logging::{silence_logger, undo_silence_logger},
    tool_calls::code_interpreter::{
        execute::execute_code,
        safety_check::{code_is_likely_safe, sanitize_code},
    },
};

#[cfg(debug_assertions)]
const BIN_PATH: &str = "./target/debug/freva-gpt2-backend";
// But when it is run in release mode, the binary is in a different location.
#[cfg(not(debug_assertions))]
const BIN_PATH: &str = "./target/release/freva-gpt2-backend";

/// The main function to execute the code interpreter.
/// Takes in the arguments that were passed to the tool call as well as the id of the tool call (for the output).
/// Returns the output of the code interpreter as a Vector of StreamVariants.
/// Requires the thread_id to be set when used by the frontend. It is used to get the freva_config_path.
/// Also requires the user_id to be set, so that the rw_dir is correctly pointed to.
pub async fn start_code_interpeter(
    arguments: Option<String>,
    id: String,
    thread_id_and_database: Option<(String, Database)>,
    user_id: String,
) -> Vec<StreamVariant> {
    trace!(
        "Running the code interpreter with the following arguments: {:?}",
        arguments
    );

    // We also need to get the freva_config_path from the thread_id.
    let (freva_config_path, thread_id) = match thread_id_and_database.clone() {
        None => {
            info!("Thread_id not set, assuming in testing mode. Not setting freva_config_path.");
            (String::new(), "testing".to_string())
        }
        Some((thread_id, database)) => match conversation_state(&thread_id, database.clone()).await
        {
            None => {
                warn!("No conversation state found while trying to run the code interpreter. Not setting freva_config_path, this WILL break any calls to the code interpreter that require it.");
                (String::new(), thread_id)
            }
            Some(ConversationState::Ended | ConversationState::Stopping) => {
                warn!("Trying to run the code interpreter with a conversation that has already ended. Not executing the code interpreter.");
                return vec![StreamVariant::CodeOutput("The conversation has already ended. Please start a new conversation to use the code interpreter.".to_string(), id)];
            }
            Some(ConversationState::Streaming(freva_config_path)) => (freva_config_path, thread_id),
        },
    };

    // First run the basic safety check.
    if !code_is_likely_safe(&arguments.clone().unwrap_or_default()) {
        // We don't want to give a potential attacker any information about why the code failed.
        return vec![StreamVariant::CodeOutput(
            "A sudden and unexpected error occurred while running the code interpreter. Please try again."
                .to_string(),
            id,
        )];
    }

    // Also retrieve all previous code interpreter inputs to get all libraries that are needed.
    let (previous_code_interpreter_imports, previous_images) = match thread_id_and_database.clone()
    {
        None => (vec![], vec![]),
        Some((thread_id, database)) => {
            retrieve_previous_code_interpreter_imports_and_images(&thread_id, database).await
        }
    };

    // Now, we have to convert the arguments from JSON to a struct.

    // First check whether the arguments are actually present, maybe the LLM forgot to include them.
    let Some(code) = arguments else {
        warn!("No code was found while trying to run the code_interpreter.");
        return vec![StreamVariant::CodeOutput(
            "No code was found while trying to run the code_interpreter. Please try again."
                .to_string(),
            id,
        )];
    };

    // In order to not import twice, which can appearently cause issues, we'll check the code and remove any imports that are already present.
    let imports = sanitize_imports(previous_code_interpreter_imports, &code);
    let imports = imports.join("\n");

    // Now parse the JSON into a struct.
    let mut code = match serde_json::from_str::<CodeInterpreterArguments>(&code) {
        Ok(parsed) => parsed,
        Err(e) => {
            warn!("Error parsing the code interpreter arguments: {:?}", e);
            return vec![StreamVariant::CodeOutput("The Input to the Code Interpreter was malformed and not valid JSON. Please try again.".to_string(), id)];
        }
    };

    let sanitized_code = sanitize_code(imports + &code.code);
    let post_processed_code = post_process(sanitized_code, user_id, thread_id);
    code.code = post_processed_code;

    trace!(
        "Running the code interpreter with the following code: {}",
        code.code
    );

    // The code interpreter also needs the thread_id to retrieve and save the pickle file.
    // We'll pass it as an environment variable to the code interpreter.

    // Instead of just executing the code in this process, we start a new one.
    // This has several advantages:
    // For one, we can actually read the stdout and stderr of the process, which we can't do if we just execute the code in this process.
    // Secondly, the python module likes to crash hard sometimes, so if the code interpreter crashes, it won't take the whole chatbot down with it.
    // The code we use will be the same as in the execute_code function.

    let output = Command::new(BIN_PATH)
        .arg("--code-interpreter")
        .arg(code.code.clone())
        .env("EVALUATION_SYSTEM_CONFIG_FILE", freva_config_path)
        .env(
            "THREAD_ID",
            thread_id_and_database
                .map(|t_a_d| t_a_d.0)
                .unwrap_or_default(),
        ) // Extracts the thread_id from the tuple, or uses an empty string if it is None.
        .output()
        .await; // It's a future now, so we have to await it.

    // for now, we'll just return the output as a string. The code interpreter will later be able to return more complex data.
    match output {
        Ok(output) => {
            // If the code interpreter crashes (non-successful exit code), we'll return an error message.
            if !output.status.success() {
                warn!(
                    "The code interpreter crashed with the following output: {:?}",
                    output
                );
                return vec![StreamVariant::CodeOutput("An unexpected error occurred while running the code interpreter. Please try again.".to_string(), id)];
            }
            // Else, it was successful, and we'll return the output.
            let stdout = String::from_utf8_lossy(&output.stdout);
            trace!("Code interpreter output: {}", stdout);

            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                warn!(
                    "The code interpreter returned the following error output: {}",
                    stderr
                );
            }

            // The stdout can contain an image if the code interpreter has generated one.
            // In that case, we need to extract the image and return it as a separate stream variant.
            let mut images = vec![];
            let mut stdout_without_images = String::new();
            for line in stdout.lines() {
                if line.starts_with("Encoded Image: ") {
                    let encoded_image = line.trim_start_matches("Encoded Image: ");
                    // However, we don't want to return any images that have previously been returned.
                    // So we need to check the past conversation state for images.

                    if previous_images.contains(&encoded_image.to_string()) {
                        debug!("Found an image that has already been returned; skipping.");
                        trace!(
                            "Skipping image that has already been returned: {}",
                            encoded_image
                        );
                        continue; // Skip this image, it has already been returned.
                    }

                    images.push(StreamVariant::Image(encoded_image.to_string()));
                } else {
                    stdout_without_images.push_str(line);
                    stdout_without_images.push('\n');
                }
            }

            // We might get a problem with the output being too long, so we'll limit it to 3500 characters. (1000 was not enough)
            // This is a temporary solution, and we'll have to find a better one later. FIXME
            let stdout_short = if stdout_without_images.len() > 3500 {
                warn!("The code interpreter output was too long. Truncating to 3500 characters.");
                stdout_without_images.chars().take(3500).collect()
            } else {
                stdout_without_images.to_string()
            };

            let stderr_short = if stderr.len() > 3500 {
                warn!("The code interpreter error output was too long. Truncating to 3500 characters.");
                stderr.chars().take(3500).collect()
            } else {
                stderr.to_string()
            };

            // The LLM probably needs both the stdout and stderr, so we'll return both.
            let stdout_stderr = format!("{stdout_short}\n{stderr_short}").trim().to_string(); // Because if the stderr is empty, this would add an unnecessary newline.

            let stdout_stderr = post_process_output(&stdout_stderr, &code.code.clone());
            if stdout_stderr.split_whitespace().next().is_none() {
                // This will check whether it contains only whitespace.
                info!("The code interpreter returned an empty output.");
            }

            let mut ouput_vec = vec![StreamVariant::CodeOutput(stdout_stderr, id)];
            ouput_vec.extend(images); // All the images (most of the time, there will be none and almost all other times it should only be one).
            ouput_vec
        }
        Err(output) => {
            warn!("Error running the code interpreter: {:?}", output);
            vec![StreamVariant::CodeOutput("An unexpected error occurred while running the code interpreter. Please try again.".to_string(), id)]
        }
    }
}

/// Simple struct to ease the conversion from JSON to a struct.
#[derive(serde::Deserialize, Debug)]
struct CodeInterpreterArguments {
    code: String,
}

/// The function that is called when the program is started and the code_interpreter argument is passed.
pub fn run_code_interpeter(arguments: String) -> ! {
    // We'll first initialize the logger.
    let logger = setup_logging(); // can't drop the logger, because we need it to be alive for the whole program.
    debug!(
        "Starting the code interpreter with the following arguments: {}",
        arguments
    );

    // Before executing the code, we'll want to retrieve the Thread_id environment variable.
    // This is needed for the code interpreter to save the pickle file.

    // Debug: Overhead debugging
    if let Ok(overhead_time) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        debug!(
            "The code interpreter has been called. OVERHEAD={}",
            overhead_time.as_nanos()
        );
    }

    let mut thread_id = match std::env::var("THREAD_ID") {
        Err(e) => {
            warn!("Error reading the thread_id environment variable: {:?}", e);
            None
        }
        Ok(thread_id) => Some(thread_id),
    };
    if thread_id == Some(String::new()) {
        thread_id = None;
    }

    let output = execute_code(arguments, thread_id);

    // The LLM wants the output, we'll return it here.
    let output = match output {
        Err(output) | Ok(output) => output, // We'll just return the error message.
    };

    print!("{}", output.trim()); // No trailing newline.

    if let Some(logger) = logger {
        logger.shutdown();
    } // We have to shut down the logger manually

    // Because this is a seperate process, we have to exit it manually.
    std::process::exit(0);
}

/// Retrieves all previous code interpreter inputs from the conversation state and also all past images.
/// Returns a string with all the imports, seperated by newlines.
/// The Images are returned as Base64 encoded strings, to be compared with the current images to avoid duplicates.
async fn retrieve_previous_code_interpreter_imports_and_images(
    thread_id: &str,
    database: Database,
) -> (Vec<String>, Vec<String>) {
    // The running conversation is in the global variable.
    let mut this_conversation = get_conversation(thread_id).unwrap_or_default();
    // The past conversation is stored on disk.
    silence_logger();
    let past_conversation = read_thread(thread_id, database).await.unwrap_or_default(); // We don't want to log an error if the file doesn't exist.
    undo_silence_logger();
    this_conversation.extend(past_conversation);

    let mut imports = Vec::<String>::new();
    for variant in this_conversation.clone() {
        if let StreamVariant::Code(code, _) = variant {
            // Split the code into lines and only take the lines that start with "import" or start with "from" AND contain "import".
            // Start the split at the first occurence of "\":\"" to avoid splitting the code itself and to include the first line.
            let rest_code = code.split_once("\":\"").unwrap_or_default().1; // If it doesn't work, use the empty string.
            let code_lines = rest_code.split("\\n"); // It's escaped because it's JSON.
            for line in code_lines {
                if line.starts_with("import")
                    || (line.starts_with("from") && line.contains("import"))
                {
                    trace!("Found import line: {}", line);
                    imports.push(line.to_string());
                }
            }
        }
    }

    // Also extract all images that were returned by the code interpreter.
    let mut images = Vec::<String>::new();
    for variant in this_conversation {
        if let StreamVariant::Image(image) = variant {
            // The images are already Base64 encoded, so we can just push them to the vector.
            trace!("Found image: {}", image);
            images.push(image);
        }
    }

    (imports, images)
}

/// Takes in a list of possible imports and the code that should be run.
/// Returns a sanitized list of the imports to add to the code.
fn sanitize_imports(prev_imports: Vec<String>, code: &str) -> Vec<String> {
    let mut imports = vec![];

    // We'll first check the previous imports.
    for prev_import in prev_imports {
        if !code.contains(&prev_import) {
            imports.push(prev_import);
        }
    }

    // Now we'll check the code itself.
    let code_lines = code.split('\n');
    for line in code_lines {
        if line.starts_with("import") || (line.starts_with("from") && line.contains("import")) {
            imports.push(line.to_string());
        }
    }

    // This newline prevents the imports from accidentally being on the same line
    if !imports.is_empty() {
        imports.push("\n".to_string());
    }
    imports
}

/// Post-processes the code before running it.
/// Adds freva, numpy, matplotlib and xarray imports if they are not already present.
/// Also replaces the user_id and thread_id placeholders with the actual values.
fn post_process(code: String, user_id: String, thread_id: String) -> String {
    let mut code = code;

    // (What should be detected to add it) and (what should be added)
    let libraries = [
        ("freva_client.", "import freva_client\n"),
        ("np.", "import numpy as np\n"),
        ("plt.", "import matplotlib.pyplot as plt\n"),
        ("xr.", "import xarray as xr\n"),
        ("pd.", "import pandas as pd\n"),
        ("ccrs.", "import cartopy.crs as ccrs\n"),
        ("cartopy.", "import cartopy\n"),
        ("cfeature", "import cartopy.feature as cfeature\n"),
    ];

    for (detect, add) in &libraries {
        // If the code contains the detect string, but not the add string, we'll add the add string.
        if code.contains(detect) && !code.contains(add) {
            debug!("Adding the following import to the code: {}", add);
            code = (*add).to_string() + &code;
        }
    }

    // Now we have to replace the user_id and thread_id placeholders with the actual values.
    // They are {user_id} and {thread_id} respectively.
    let replacements = [("{user_id}", user_id), ("{thread_id}", thread_id)];
    for (placeholder, value) in &replacements {
        code = code.replace(placeholder, value);
    }
    trace!("Post-processed code: {}", code);

    code
}

/// Post-processes the output before returning it.
/// Gives hints for SyntaxErrors and Tracebacks.
fn post_process_output(output: &str, code: &str) -> String {
    let mut output = output.to_string();

    // The line we are looking for is formatted like this: "SyntaxError: invalid syntax # or other error #  (<string>, line 1)"
    // If we find it, we want to insert the line that caused the error.

    // Loop over all lines. If one starts with "SyntaxError", we'll return it.
    let mut synerr_line = None;
    for line in output.lines() {
        if line.starts_with("SyntaxError") || line.starts_with("IndentationError"){
            synerr_line = Some(line);
            break;
        }
    }
    match synerr_line {
        None => {
            // If we don't find the line, we don't process the output further here.
        }
        Some(line) => {
            // We have the line, now we have to extract the line number.
            let line_number_str = line
                .split("line ")
                .nth(1)
                .unwrap_or_default()
                .split(')')
                .next()
                .unwrap_or_default();

            // If the line number is empty, we can't do anything.
            if line_number_str.is_empty() {
                return output;
            }

            // We want it as a number, so we can use it to read from the code.
            let line_number = match line_number_str.parse::<usize>() {
                Ok(line_number) => line_number,
                Err(e) => {
                    warn!(
                        "Error parsing the line number from the SyntaxError: {:?}",
                        e
                    );
                    return output;
                }
            };

            // Now we can construct the hint. It should look like this, if the syntax error occured at line 3:
            // Hint: the error occured on line 3:
            // 2: (previous line)
            // 3: > (line that caused the error) <
            // 4: (next line)

            add_hint_to_output(line_number, code, &mut output);
        }
    }

    // Note that it's not possible to have both a traceback and a syntax error in the same output.

    // Now, we implement a similar hint for tracebacks.
    // The structure of a traceback is (for us):
    // Valuetype("Error message")
    // Traceback (most recent call last):
    // File "<string>", line (this_line), in <module>
    // ...
    // (Because I write the error before the traceback, so that when the output is too long, the error is still visible.)

    // The goal is to also write a hint for the line that caused the error.

    let mut traceback_line = None;
    for (line, next_line) in output.lines().tuple_windows() {
        if line.starts_with("Traceback") {
            traceback_line = Some(next_line);
            break;
        }
    }

    match traceback_line {
        None => {
            // If we don't find the line, we don't process the output further here.
        }
        Some(line) => {
            // We have the line, now we have to extract the line number.
            let line_number_str = line // "File "<string>", line (this_line), in <module>"
                .split("line ") // ["File \"<string>\", ", " (this_line), in <module>"]
                .nth(1) // " (this_line), in <module>"
                .unwrap_or_default()
                .split(',') // [" (this_line)", " in <module>"]
                .next() // " (this_line)"
                .unwrap_or_default()
                .trim(); // "(this_line)"

            // If the line number is empty, we can't do anything.
            if line_number_str.is_empty() {
                return output;
            }

            // We want it as a number, so we can use it to read from the code.
            let line_number = match line_number_str.parse::<usize>() {
                Ok(line_number) => line_number,
                Err(e) => {
                    warn!("Error parsing the line number from the Traceback: {:?}", e);
                    return output;
                }
            };

            // Now we can construct the hint. It should look like this, if the traceback occured at line 3:
            // Hint: the error occured on line 3:
            // 2: (previous line)
            // 3: > (line that caused the error) <
            // 4: (next line)

            add_hint_to_output(line_number, code, &mut output);
        }
    }
    output
}

// Small helper function to add the hint to the output.
fn add_hint_to_output(line_number: usize, code: &str, output: &mut String) {
    let mut hint = String::new();
    hint.push_str("Hint: the error occured on line ");
    hint.push_str(&line_number.to_string());
    hint.push('\n');

    // Now we have to extract the line that caused the error.
    let prev_line = code
        .lines()
        .nth(line_number.wrapping_sub(2)) // wrapping because usize can't be negative.
        .unwrap_or_default();
    // This will then simply be an empty string.
    let error_line = code
        .lines()
        .nth(line_number.wrapping_sub(1))
        .unwrap_or_default();
    let next_line = code.lines().nth(line_number).unwrap_or_default();

    if line_number != 1 {
        hint.push_str(&(line_number - 1).to_string());
        hint.push_str(": ");
        hint.push_str(prev_line);
        hint.push('\n');
    }

    hint.push_str(&(line_number).to_string());
    hint.push_str(": > ");
    hint.push_str(error_line);
    hint.push_str(" <");
    hint.push('\n');

    if line_number != code.lines().count() {
        hint.push_str(&(line_number + 1).to_string());
        hint.push_str(": ");
        hint.push_str(next_line);
    }

    output.push_str("\n\n");
    output.push_str(&hint);
}

/// Helper function that initializes logging to the logging file.
fn setup_logging() -> Option<flexi_logger::LoggerHandle> {
    let result = flexi_logger::Logger::with(flexi_logger::LevelFilter::Trace)
        .log_to_file(
            flexi_logger::FileSpec::default()
                .basename("logging_from_tools")
                .suppress_timestamp(), // Don't use timestamps, only one file is created.
        )
        .append() // Append to the file, don't overwrite it.
        .format(crate::logging::format_log_message)
        .start();
    // Since we have nothing to print if this fails, we'll just ignore the error.
    result.ok()
}
