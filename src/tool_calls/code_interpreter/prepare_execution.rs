use std::process::Command;

use tracing::{debug, info, trace, warn};

use crate::{
    chatbot::{
        handle_active_conversations::{conversation_state, get_conversation},
        thread_storage::read_thread,
        types::{ConversationState, StreamVariant},
    },
    tool_calls::code_interpreter::{
        execute::execute_code,
        safety_check::{code_is_likely_safe, sanitize_code},
    },
};

/// The main function to execute the code interpreter.
/// Takes in the arguments that were passed to the tool call as well as the id of the tool call (for the output).
/// Returns the output of the code interpreter as a Vector of StreamVariants.
/// Requires the thread_id to be set when used by the frontend. It is used to get the freva_config_path.
pub fn start_code_interpeter(
    arguments: Option<String>,
    id: String,
    thread_id: Option<String>,
) -> Vec<StreamVariant> {
    trace!(
        "Running the code interpreter with the following arguments: {:?}",
        arguments
    );

    // We also need to get the freva_config_path from the thread_id.
    let freva_config_path = match thread_id.clone() {
        None => {
            info!("Thread_id not set, assuming in testing mode. Not setting freva_config_path.");
            "".to_string()
        }
        Some(thread_id) => match conversation_state(&thread_id) {
            None => {
                warn!("No conversation state found while trying to run the code interpreter. Not setting freva_config_path, this WILL break any calls to the code interpreter that require it.");
                "".to_string()
            }
            Some(ConversationState::Ended) | Some(ConversationState::Stopping) => {
                warn!("Trying to run the code interpreter with a conversation that has already ended. Not executing the code interpreter.");
                return vec![StreamVariant::CodeOutput("The conversation has already ended. Please start a new conversation to use the code interpreter.".to_string(), id)];
            }
            Some(ConversationState::Streaming(freva_config_path)) => freva_config_path,
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
    let previous_code_interpreter_imports = match thread_id.clone() {
        None => vec![],
        Some(thread_id) => retrieve_previous_code_interpreter_imports(&thread_id),
    };

    // Now, we have to convert the arguments from JSON to a struct.

    // First check whether the arguments are actually present, maybe the LLM forgot to include them.
    let code = if let Some(content) = arguments {
        content
    } else {
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
    let post_processed_code = post_process(sanitized_code);
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

    let output = Command::new("./target/debug/freva-gpt2-backend")
        .arg("--code-interpreter")
        .arg(code.code)
        .env("EVALUATION_SYSTEM_CONFIG_FILE", freva_config_path)
        .env("THREAD_ID", thread_id.unwrap_or_default())
        .output();

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
                    images.push(StreamVariant::Image(encoded_image.to_string()));
                } else {
                    stdout_without_images.push_str(line);
                    stdout_without_images.push('\n');
                }
            }

            // We might get a problem with the output being too long, so we'll limit it to 2000 characters. (1000 was not enough)
            // This is a temporary solution, and we'll have to find a better one later. FIXME
            let stdout_short = if stdout_without_images.len() > 2000 {
                warn!("The code interpreter output was too long. Truncating to 2000 characters.");
                stdout_without_images.chars().take(2000).collect()
            } else {
                stdout_without_images.to_string()
            };

            let stderr_short = if stderr.len() > 2000 {
                warn!("The code interpreter error output was too long. Truncating to 2000 characters.");
                stderr.chars().take(2000).collect()
            } else {
                stderr.to_string()
            };

            // The LLM probably needs both the stdout and stderr, so we'll return both.
            let stdout_stderr = format!("{stdout_short}\n{stderr_short}").trim().to_string(); // Because if the stderr is empty, this would add an unnecessary newline.
            if stdout_stderr.is_empty() {
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
pub fn run_code_interpeter(arguments: String) {
    // Before executing the code, we'll want to retrieve the Thread_id environment variable.
    // This is needed for the code interpreter to save the pickle file.

    let mut thread_id = match std::env::var("THREAD_ID") {
        Err(e) => {
            warn!("Error reading the thread_id environment variable: {:?}", e);
            None
        }
        Ok(thread_id) => Some(thread_id),
    };
    if thread_id == Some("".to_string()) {
        thread_id = None;
    }

    let output = execute_code(arguments, thread_id);

    // The LLM wants the output, we'll return it here.
    let output = match output {
        Err(output) | Ok(output) => output, // We'll just return the error message.
    };

    print!("{}", output.trim()); // No trailing newline.

    // Because this is a seperate process, we have to exit it manually.
    std::process::exit(0);
}

/// Retrieves all previous code interpreter inputs from the conversation state.
/// Returns a string with all the imports, seperated by newlines.
fn retrieve_previous_code_interpreter_imports(thread_id: &str) -> Vec<String> {
    // The running conversation is in the global variable.
    let mut this_conversation = get_conversation(thread_id).unwrap_or_default();
    // The past conversation is stored on disk.
    let past_conversation = read_thread(thread_id, true).unwrap_or_default(); // We don't want to log an error if the file doesn't exist.
    this_conversation.extend(past_conversation);

    let mut imports = Vec::<String>::new();
    for variant in this_conversation {
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
    imports
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
    let code_lines = code.split("\n");
    for line in code_lines {
        if line.starts_with("import") || (line.starts_with("from") && line.contains("import")) {
            imports.push(line.to_string());
        }
    }

    imports
}

/// Post-processes the code before running it.
/// Adds freva, numpy, matplotlib and xarray imports if they are not already present.
fn post_process(code: String) -> String {
    let mut code = code;

    // (What should be detected to add it) and (what should be added)
    let libraries = [
        ("freva.", "import freva\n"),
        ("np.", "import numpy as np\n"),
        ("plt.", "import matplotlib.pyplot as plt\n"),
        ("xr.", "import xarray as xr\n"),
        ("pd.", "import pandas as pd\n"),
    ];

    for (detect, add) in libraries.iter() {
        // If the code contains the detect string, but not the add string, we'll add the add string.
        if code.contains(detect) && !code.contains(add) {
            debug!("Adding the following import to the code: {}", add);
            code = add.to_string() + &code;
        }
    }

    code
}
