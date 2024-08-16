use pyo3::types::PyDict;
use pyo3::prelude::*;
use tracing::{info, trace};




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
                },
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
                return Ok(content.to_string());
            } 
            Err(e) => {
                return Err(format_pyerr(e, py));
            }
        };

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
        let traceback = traceback.to_string();
        info!("Traceback: {}", traceback);
        return traceback;
    },
    None => {
        // That's weird, but we can fall back to __repr__
        if let Ok(repr) = e.value_bound(py).repr() {
            let repr = repr.to_string();
            info!("Error (repr): {}", repr);
            return repr;
        } else {
            // If that doesn't work either, we'll use it as a string, which will always work, but has weird formatting.
            let error = e.to_string();
            info!("Error (to_string): {}", error);
            return error;
        }
    }
}
}