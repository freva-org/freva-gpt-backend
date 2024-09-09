use std::process::Command;

use tracing::{trace, warn};

use crate::{
    chatbot::types::StreamVariant,
    tool_calls::code_interpreter::{
        execute::execute_code,
        safety_check::{code_is_likely_safe, sanitize_code},
    },
};

/// The main function to execute the code interpreter.
/// Takes in the arguments that were passed to the tool call as well as the id of the tool call (for the output).
/// Returns the output of the code interpreter as a Vector of StreamVariants.
pub fn start_code_interpeter(arguments: Option<String>, id: String) -> Vec<StreamVariant> {
    trace!(
        "Running the code interpreter with the following arguments: {:?}",
        arguments
    );

    // First run the basic safety check.
    if !code_is_likely_safe(&arguments.clone().unwrap_or_default()) {
        // We don't want to give a potential attacker any information about why the code failed.
        return vec![StreamVariant::CodeOutput(
            "A sudden and unexpected error occurred while running the code interpreter. Please try again."
                .to_string(),
            id,
        )];
    }

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

    // Now parse the JSON into a struct.
    let mut code = match serde_json::from_str::<CodeInterpreterArguments>(&code) {
        Ok(parsed) => parsed,
        Err(e) => {
            warn!("Error parsing the code interpreter arguments: {:?}", e);
            return vec![StreamVariant::CodeOutput("The Input to the Code Interpreter was malformed and not valid JSON. Please try again.".to_string(), id)];
        }
    };

    let sanitized_code = sanitize_code(code.code);
    code.code = sanitized_code;

    trace!(
        "Running the code interpreter with the following code: {}",
        code.code
    );

    // let output = execute_code(code.code);

    // Instead of just executing the code in this process, we start a new one.
    // This has several advantages:
    // For one, we can actually read the stdout and stderr of the process, which we can't do if we just execute the code in this process.
    // Secondly, the python module likes to crash hard sometimes, so if the code interpreter crashes, it won't take the whole chatbot down with it.
    // The code we use will be the same as in the execute_code function.

    let output = Command::new("./target/debug/freva-gpt2-backend")
        .arg("--code-interpreter")
        .arg(code.code)
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
            let stdout_stderr = format!("{stdout_short}\n{stderr_short}");
            if stdout_stderr.is_empty() {
                warn!("The code interpreter returned an empty output.");
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
    // For testing currently. TODO.
    // println!("Running the code interpreter with the following arguments: {}", arguments);

    let output = execute_code(arguments);

    // The LLM wants the output, we'll return it here.
    let output = match output {
        Err(output) | Ok(output) => output, // We'll just return the error message.
    };

    print!("{}", output.trim()); // No trailing newline.

    // Because this is a seperate process, we have to exit it manually.
    std::process::exit(0);
}
