use pyo3::prelude::*;
use pyo3::types::PyDict;
use tracing::{info, trace, warn};
use tracing_subscriber::fmt::format;

/// Executes the given code within a "jupyter" environment.
/// Not actually, but we support returning the last line of the code.
///
/// REQUIRES: The code has passed the safety checks.
pub fn execute_code(code: String) -> Result<String, String> {
    pyo3::prepare_freethreaded_python(); // For now. We'll have to look into this later. FIXME: remove GIL requirement???

    let output = Python::with_gil(|py| {
        // We need a PyDict to store the local and global variables for the call.
        let locals = PyDict::new_bound(py);
        let globals = PyDict::new_bound(py);

        // Because we want the last line to be returned, we'll execute all but the last line.
        let split = code.rsplit_once("\n");
        // If there is no newline, we'll just execute the code as is.

        if let Some((rest_lines, _)) = split {
            // We'll execute the code in the locals.
            match py.run_bound(rest_lines, Some(&globals), Some(&locals)) {
                Ok(_) => {
                    info!("Code executed successfully.");
                    // But we continue with the last line.
                }
                Err(e) => {
                    return Err(format_pyerr(e, py));
                }
            }
        }

        // The last line is either part of the split or the whole code.
        let last_line = if let Some((_, last)) = split {
            last
        } else {
            code.as_str()
        };

        // Now the rest of the lines are executed if they exist.
        // Now we don't execute, but evaluate the last line.
        match py.eval_bound(last_line, Some(&globals), Some(&locals)) {
            Ok(content) => {
                // We now have a python value in here. We'll just return the string representation.
                // TODO: add support for matplotlib plots and other complex data.
                Ok(content.to_string())
            }
            Err(e) => Err(format_pyerr(e, py)),
        }
    });

    // TDOO: better output?

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
