use pyo3::prelude::*;
use pyo3::types::PyDict;
use tracing::{debug, info, trace, warn};

/// Executes the given code within a "jupyter" environment.
/// Not actually, but we support returning the last line of the code.
///
/// REQUIRES: The code has passed the safety checks.
pub fn execute_code(code: String) -> Result<String, String> {
    trace!("Preparing python interpreter for code execution.");
    pyo3::prepare_freethreaded_python(); // For now. We'll have to look into this later. FIXME: remove GIL requirement???

    trace!("Starting GIL block.");
    let output = Python::with_gil(|py| {
        // We need a PyDict to store the local and global variables for the call.
        let locals = PyDict::new_bound(py);
        let globals = PyDict::new_bound(py);

        // Because we want the last line to be returned, we'll execute all but the last line.
        let (rest_lines, last_line) = match code.trim().rsplit_once("\n") {
            // We need to decide how to split up the code. If it's just one line, we put it into the last line, since that's ouput is evaluated by us.
            // If that's not the case, we'll split it up into the last line and the rest of the code.
            // That is, unless the last line is not just a variable, but a function call or something else that doesn't return a value.
            None => {
                // If there is no newline, we'll just execute the whole code.
                (Some(code), None)
            },
            Some((rest, last)) => {
                // We'll have to check the last line
                let last_line = last.trim();
                if last_line.contains('(') || last_line.contains("import") {
                    // If the last line contains a "(", it's likely a function call, which we can't evaluate.
                    // If it contains "import", it's likely an import statement, which we also can't evaluate.
                    // We'll just execute the whole code.
                    (Some(code), None)
                } else {
                    // Otherwise, we'll split it up.
                    (Some(rest.to_string()), Some(last_line.to_string()))
                }

            }
        };

        if let Some(rest_lines) = rest_lines {
            debug!("Executing all but the last line.");
            trace!("Executing code: {}", rest_lines);
            // We'll execute the code in the locals.
            match py.run_bound(&rest_lines, Some(&globals), Some(&locals)) {
                Ok(_) => {
                    info!("Code executed successfully.");
                    // But we continue with the last line.
                }
                Err(e) => {
                    return Err(format_pyerr(e, py));
                }
            }
        }

        if let Some(last_line) = last_line {
            debug!("Evaluating the last line.");
            trace!("Last line: {}", last_line);
            // Now the rest of the lines are executed if they exist.
            // Now we don't execute, but evaluate the last line.
            match py.eval_bound(&last_line, Some(&globals), Some(&locals)) {
                Ok(content) => {
                    // We now have a python value in here. We'll just return the string representation.
                    // TODO: add support for matplotlib plots and other complex data.
                    Ok(content.to_string())
                }
                Err(e) => Err(format_pyerr(e, py)),
            }
        }
        else {
            // If there is no last line, we'll just return an empty string.
            Ok("".to_string())
        }
    });

    trace!("Code execution finished.");

    output
}

/// Helper function to turn a PyErr into a string for the LLM
fn format_pyerr(e: PyErr, py: Python) -> String {
    // The type is "PyErr", which we will just just use to get the traceback.
    trace!("Error executing code: {:?}", e);
    match e.traceback_bound(py) {
        Some(traceback) => {
            // We'll just return the traceback for now.
            match traceback.format() {
                Ok(tb_string) => {
                    info!("Traceback: {}", tb_string);
                    format!("{}{}", tb_string, e)
                }
                Err(inner_e) => {
                    // If we can't get the traceback, we shouldn't just return the error message, because that's about not being able to get the traceback.
                    // Instead, we'll fall back to just the Python error message.
                    warn!("Error getting traceback: {:?}", inner_e);
                    format!("(An error occured; no traceback available)\n{}", e)
                }
            }
        }
        None => {
            // That's weird and should never happen, but we can fall back to just printing e.
            warn!("No traceback found for error: {:?}", e);
            format!("(An error occured; no traceback available)\n{}", e)
        }
    }
}
