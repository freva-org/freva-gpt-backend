use std::io::Write;

use base64::Engine;
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
            }
            Some((rest, last)) => {
                // We'll have to check the last line
                let last_line = last.trim();
                if (last_line.contains('(') || last_line.contains("import"))  && !last_line.contains("plt.show()") {
                    // If the last line contains a "(", it's likely a function call, which we can't evaluate.
                    // If it contains "import", it's likely an import statement, which we also can't evaluate.
                    // The exception is if it's a variable assignment, but we can't really check that.
                    // The exception we do check for is if it's a plt.show() call, which we do support.
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

            // Because we evaluate and don't execute, we have to handle it differently.
            // For example, all LLMs are used to calling plt.show() at the end of their code.
            // It's in all the examples and if you were in a jupyter notebook, you'd need it.
            // But we don't really support it, because we don't do interactive plotting.
            // So instead we just pretend that we do and, if the last line contains a `plt.show()`, we'll convert it to `plt`, which is supported.
            // This is a bit of a hack, but it should work for now.

            let mut last_line = last_line;
            if last_line.contains("plt.show()") {
                // We'll replace plt.show() with plt.
                let new_last_line = last_line.replace("plt.show()", "plt");
                trace!("Replaced plt.show() with plt in the last line.");
                trace!("New last line: {}", new_last_line);
                // We'll replace the last line with the new one.
                last_line = new_last_line;
            }

            debug!("Evaluating the last line.");
            trace!("Last line: {}", last_line);
            // Now the rest of the lines are executed if they exist.
            // Now we don't execute, but evaluate the last line.
            match py.eval_bound(&last_line, Some(&globals), Some(&locals)) {
                Ok(content) => {
                    // We now have a python value in here.
                    // To return it, we can convert it to a string and return it.
                    // But before we do so, we check whether the matplotlib plt module was used.
                    // If it was, we probably want to extract the image and return that too.

                    let maybe_plt = locals.get_item("plt");
                    let image = match maybe_plt {
                        Ok(Some(inner)) => {
                            // If we have a plt module, we'll try to get an image from it.
                            try_get_image(inner)
                        }
                        _ => None,
                    };
                    // We now need to encode the image into the string.
                    if let Some(inner_image) = image {
                        // We'll encode the image as base64.
                        let encoded_image =
                            base64::engine::general_purpose::STANDARD.encode(inner_image);
                        // We'll return the image as a string.
                        Ok(format!(
                            "{}\n\nEncoded Image: {}",
                            content,
                            encoded_image
                        ))
                    } else {
                        Ok(content.to_string())
                    }
                }
                Err(e) => Err(format_pyerr(e, py)),
            }
        } else {
            // If there is no last line, we'll just return an empty string.
            Ok("".to_string())
        }
    });

    trace!("Code execution finished.");

    // Before the output is returned, we should flush the stdout and stderr, in case the python code has printed something without flushing.
    // This is important, as we want to make sure that the output is complete.
    match (std::io::stdout().flush(), std::io::stderr().flush()) {
        (Ok(_), Ok(_)) => {
            // Both flushes were successful.
        }
        (Err(e), Ok(_)) => {
            warn!("Error flushing stdout: {:?}", e);
        }
        (Ok(_), Err(e)) => {
            warn!("Error flushing stderr: {:?}", e);
        }
        (Err(e1), Err(e2)) => {
            warn!("Error flushing stdout: {:?}", e1);
            warn!("Error flushing stderr: {:?}", e2);
        }
    }

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

// Code to save the image from the plt module in a

/// Helper function to try to get an image from the plt module.
/// That means that there is probably a plot that we want to return.
fn try_get_image(plt: Bound<PyAny>) -> Option<Vec<u8>> {
    // I tested this before in a sandbox.
    // First get the string representation of the plt module.
    let name = plt.to_string();
    if name.starts_with("<module 'matplotlib.pyplot") {
        // We most likely have a plt module.
        // But we can't just extract the image from it, we need to save it to a file first.
        // False, we could save it to a python object first, but would be quite difficult and I don't currently see a reason to do so. FIXME: Maybe later?
        match plt.call_method1("savefig", ("/tmp/matplotlib_plt.png",)) {
            Err(e) => {
                // Something went wrong, but we don't know what.
                println!(
                    "Tried to retrieve image from python code, but failed: {:?}",
                    e
                );
            }
            Ok(_) => {
                // The file was saved successfully.
                // Now we can read it and return it.

                // We'll open the file in binary mode.
                match std::fs::read("/tmp/matplotlib_plt.png") {
                    Ok(content) => {
                        // We have the content of the file.
                        // We can now return it.
                        return Some(content);
                    }
                    Err(e) => {
                        // We couldn't read the file.
                        println!("Tried to retrieve image from python code, but failed to read the file: {:?}", e);
                        return None;
                    }
                }
            }
        }
    }
    // If it's not a plt module, we'll just return None.
    None
}
