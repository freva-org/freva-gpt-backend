use std::ffi::CString;
use std::io::Write;

use base64::Engine;
use pyo3::types::{PyDict, PyTuple};
use pyo3::{prelude::*, types::PyList};
use tracing::{debug, info, trace, warn};

/// Executes the given code within a "jupyter" environment.
/// Not actually, but we support returning the last line of the code.
///
/// REQUIRES: The code has passed the safety checks.
pub fn execute_code(code: String, thread_id: Option<String>) -> Result<String, String> {
    trace!("Preparing python interpreter for code execution.");
    pyo3::prepare_freethreaded_python();
    // Fixed: Martin told me that the "global" interpreter lock, is, in fact, not global, but per process.
    // Because I moved the execution to another process to prevent catastrophic crashes, nothing should be able to interfere with the GIL.

    // Debug: Overhead debugging
    if let Ok(overhead_time) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        debug!(
            "The python environment was prepared. OVERHEAD={}",
            overhead_time.as_nanos()
        );
    }

    // Before we start the GIL block, we can decide whether or not we should try to extract a plot from matplotlib.
    // Importing it is not enough to make sure that we need to extract a plot, as consequent calls to the code interpreter always contain
    // previously imported modules.
    // Instead, we'll look at whether or not the plt object was modified.
    let should_extract_plot = {
        if !code.contains("matplotlib.pyplot") {
            // This is a clear sign that we don't need to extract a plot.
            false
        } else {
            // The plt module is modified iif any line starts with plt. or plt. is used.
            // This is a bit of a hack, but it should work for now.
            code.lines()
                .any(|line| line.trim().starts_with("plt.") || line.trim().starts_with("plt "))
        }
    };

    // Lastly, because the backend manually extracts the plot from the plt module,
    // we need to make sure that at no point, plt.show() is actually called.
    // To be sure that if a traceback hits, the LLM doesn't get confused, we'll have to replace it with an info message.
    // A similar situation is when plt.close() is called, as we cannot extract the plot after that.
    let code = code.lines()
        .map(|line| {
            if line.trim().starts_with("plt.show()") {
                // We'll replace plt.show() with an info message.
                // This is a bit of a hack, but it should work for now.
                "# plt.show() was called here, but due to the backend being non-interactive, it was intercepted at execution.".to_string()
            } else if line.trim().starts_with("plt.close()") {
                // We'll replace plt.close() with an info message.
                // This is a bit of a hack, but it should work for now.
                "# plt.close() was called here, but for the backend to extract the plot, it was intercepted at execution.".to_string()
            }
            else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    trace!("Starting GIL block.");
    let output = Python::with_gil(|py| {
        // We need a PyDict to store the local and global variables for the call.
        let locals = match try_read_locals(py, thread_id.clone()) {
            Some(locals) => locals,
            None => PyDict::new(py),
        };
        let globals = PyDict::new(py);

        // Debug: Overhead debugging
        if let Ok(overhead_time) =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        {
            debug!(
                "The local variables were loaded. OVERHEAD={}",
                overhead_time.as_nanos()
            );
        }

        let mut result = {
            // Because we want the last line to be returned, we'll execute all but the last line.
            let (rest_lines, last_line) = match code.trim().rsplit_once('\n') {
                // We need to decide how to split up the code. If it's just one line, we put it into the last line, since that's ouput is evaluated by us.
                // If that's not the case, we'll split it up into the last line and the rest of the code.
                // That is, unless the last line is not just a variable, but a function call or something else that doesn't return a value.
                None => {
                    // If there is no newline, we'll just eval the whole code, unless an import is present.
                    // If an import is present, we'll have to execute it instead.
                    if should_eval(&code, py) {
                        (None, Some(code))
                    } else {
                        (Some(code), None)
                    }
                }
                Some((rest, last)) => {
                    // We'll have to check the last line
                    if should_eval(last, py) {
                        // We'll split it up.
                        (Some(rest.to_string()), Some(last.to_string()))
                    } else {
                        (Some(code), None)
                    }
                }
            };

            // Debug: Overhead debugging
            if let Ok(overhead_time) =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            {
                debug!(
                    "It was decided whether or not the code should be evaluated or executed. OVERHEAD={}",
                    overhead_time.as_nanos()
                );
            }

            if let Some(rest_lines) = rest_lines {
                debug!("Executing all but the last line.");
                trace!("Executing code: {}", rest_lines);
                // We'll execute the code in the locals.
                let rest_lines_cstr = CString::new(rest_lines);
                match rest_lines_cstr {
                    Ok(rest_lines_cstr) => {
                        match py.run(&rest_lines_cstr, Some(&globals), Some(&locals)) {
                            Ok(()) => {
                                info!("Code executed successfully.");
                                // But we continue with the last line.
                            }
                            Err(e) => {
                                // Also store the locals to a pickle file so they aren't lost
                                if let Some(thread_id) = thread_id {
                                    save_to_pickle_file(py, &locals, &thread_id);
                                }
                                return Err(format_pyerr(&e, py));
                            }
                        }
                    }
                    Err(e) => {
                        // If we couldn't convert the code to a C string, we'll just return an error.
                        // This should never happen, but we'll just return an error.
                        warn!("Error converting code to C string: {:?}", e);
                        return Err(format!("Error converting code to C string: {e}"));
                    }
                }
            }

            // Debug: Overhead debugging
            if let Ok(overhead_time) =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            {
                debug!(
                    "All but (maybe) the last line were executed. OVERHEAD={}",
                    overhead_time.as_nanos()
                );
            }

            if let Some(last_line) = last_line {
                // Previously, plt.show() was expected to always be on the last line.
                // This has now changed, whether or not a plot should be extracted is detected before the GIL block.

                debug!("Evaluating the last line.");
                trace!("Last line: {}", last_line);
                // Now the rest of the lines are executed if they exist.
                // Now we don't execute, but evaluate the last line.
                let last_line_cstr = match CString::new(last_line) {
                    Ok(last_line_cstr) => last_line_cstr,
                    Err(e) => {
                        // If we couldn't convert the code to a C string, we'll just return an error.
                        // This should never happen, but we'll just return an error.
                        warn!("Error converting code to C string: {:?}", e);
                        return Err(format!("Error converting code to C string: {e}"));
                    }
                };
                match py.eval(&last_line_cstr, Some(&globals), Some(&locals)) {
                    Ok(content) => {
                        // We now have a python value in here.
                        // To return it, we can convert it to a string and return it.

                        // If we got nothing to return (in python, that would be None), we'll just return an empty string.
                        if content.is_none() {
                            Ok(String::new()) // else, this would say "None"
                        } else {
                            Ok(content.to_string())
                        }
                    }
                    Err(e) => Err(format_pyerr(&e, py)),
                }
            } else {
                // If there is no last line, we'll just return an empty string.
                Ok(String::new())
            }
        };

        // Debug: Overhead debugging
        if let Ok(overhead_time) =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        {
            debug!(
                "The code has finished executing. OVERHEAD={}",
                overhead_time.as_nanos()
            );
        }

        if should_extract_plot {
            // Output the plot if it was created.
            let maybe_plt = locals.get_item("plt");
            let image = match maybe_plt {
                Ok(Some(inner)) => {
                    // If we have a plt module, we'll try to get an image from it.
                    try_get_image(&inner)
                }
                _ => None,
            };
            // We now need to encode the image into the string.
            if let Some(inner_image) = image {
                // We'll encode the image as base64.
                let encoded_image = base64::engine::general_purpose::STANDARD.encode(inner_image);
                // We'll return the image as a string, in the format the other side of the LLM expects.
                let to_append = format!("\n\nEncoded Image: {encoded_image}");
                // This needs to be appended to the result, so we can return it.
                if let Ok(ref mut res) = result {
                    res.push_str(&to_append);
                } else {
                    // If the result is an error, we don't want to append the image to it.
                    warn!("Error executing code, but we still got an image: {to_append}");
                }
            }
        }

        // Before returning the result, we'll have to flush stdout and stderr IN PYTHON.

        let flush = py.run(
            &CString::new("import sys;sys.stdout.flush();sys.stderr.flush()")
                .expect("Constant CString failed conversion"),
            Some(&globals),
            Some(&locals),
        );
        if flush.is_err() {
            warn!("Error flushing stdout and stderr: {:?}", flush);
        }

        // Additionally, we'll save the locals to a pickle file.
        // But that's only possible if we have a thread_id.
        if let Some(thread_id) = thread_id {
            save_to_pickle_file(py, &locals, &thread_id);
        }

        result
    });

    trace!("Code execution finished.");

    // Before the output is returned, we should flush the stdout and stderr, in case the python code has printed something without flushing.
    // This is important, as we want to make sure that the output is complete.
    match (std::io::stdout().flush(), std::io::stderr().flush()) {
        (Ok(()), Ok(())) => {
            // Both flushes were successful.
        }
        (Err(e), Ok(())) => {
            warn!("Error flushing stdout: {:?}", e);
        }
        (Ok(()), Err(e)) => {
            warn!("Error flushing stderr: {:?}", e);
        }
        (Err(e1), Err(e2)) => {
            warn!("Error flushing stdout: {:?}", e1);
            warn!("Error flushing stderr: {:?}", e2);
        }
    }

    output
}

/// Helper function to decide whether a line should be evaluated or executed.
/// Statements like 2+2 or list expressions should be evaluated,
/// while function calls, imports, and variable assignments should be executed.
fn should_eval(line: &str, py: Python) -> bool {
    // Imports, function calls, and variable assignments should be executed.
    // However, outputting multiple variables via a tuple should be evaluated.
    // let negative = line.contains("import") || (line.contains("(") && !line.starts_with("(")) || line.contains("=");
    // let exceptions = line.contains("plt.show()") || line.contains("item()") || line.contains("freva.databrowser.metadata_search(");
    // !negative || exceptions

    // Never, ever try to eval if the last line is indented, that will lead to an indentation
    // error.
    if line.starts_with(' ') || line.starts_with('\t') {
        return false;
    }

    // New approach: Python has the ast library, which we can use to parse the line and decide whether it should be evaluated.

    let to_check = CString::new(format!(
        r#"import ast
should_eval = None
try:
    node = ast.parse("{line}")
    # Only one node is allowed
    correct_node = node.body[-1] if node.body else None
    should_eval = isinstance(correct_node, ast.Expr)
except Exception:
    should_eval = False
    "#
    ))
    .expect("Constant CString failed conversion");
    let locals = PyDict::new(py);
    let globals = PyDict::new(py);

    match py.run(&to_check, Some(&globals), Some(&locals)) {
        Ok(()) => {
            let should_eval = locals.get_item("should_eval");
            if let Ok(Some(should_eval)) = should_eval {
                let is_true = should_eval.is_truthy(); // If there was an error in is_truthy, we'll assume false.
                debug!("Should the line be evaluated? {:?}", is_true);
                matches!(is_true, Ok(true))
            } else {
                // If we couldn't get the value, we'll just return false.
                warn!(
                    "Error checking whether the line should be evaluated: {:?}",
                    should_eval
                );
                false
            }
        }
        Err(e) => {
            // If we couldn't run the code, we'll just return false.
            warn!(
                "Error checking whether the line should be evaluated: {:?}",
                e
            );
            false
        }
    }
}

/// Helper function to turn a PyErr into a string for the LLM
fn format_pyerr(e: &PyErr, py: Python) -> String {
    // The type is "PyErr", which we will just just use to get the traceback.
    trace!("Error executing code: {:?}", e);
    if let Some(traceback) = e.traceback(py) {
        // We'll just return the traceback for now.
        match traceback.format() {
            Ok(tb_string) => {
                info!("Traceback: {tb_string}");
                format!("{e}\n{tb_string}") // Writing the error first means that the error message is at the top, so cutting the message off will still show the error.
            }
            Err(inner_e) => {
                // If we can't get the traceback, we shouldn't just return the error message, because that's about not being able to get the traceback.
                // Instead, we'll fall back to just the Python error message.
                warn!("Error getting traceback: {inner_e:?}");
                format!("(An error occured; no traceback available)\n{e}")
            }
        }
    } else {
        // That's weird and should never happen, but we can fall back to just printing e.
        warn!("No traceback found for error: {e:?}");
        format!("(An error occured; no traceback available)\n{e}")
    }
}

// Code to save the image from the plt module in a

/// Helper function to try to get an image from the plt module.
/// That means that there is probably a plot that we want to return.
fn try_get_image(plt: &Bound<PyAny>) -> Option<Vec<u8>> {
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
                println!("Tried to retrieve image from python code, but failed: {e:?}",);
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
                        println!("Tried to retrieve image from python code, but failed to read the file: {e:?}");
                        return None;
                    }
                }
            }
        }
    }
    // If it's not a plt module, we'll just return None.
    None
}

/// Helper function to read the locals from the pickled file.
/// (Also the only function where I use the question mark operator.)
fn try_read_locals(py: Python, thread_id: Option<String>) -> Option<Bound<PyDict>> {
    // If the thread_id is None, we don't even have to try to read the file.
    let thread_id = thread_id?; // Unwrap the thread_id.
    let pickleable_path = format!("python_pickles/{thread_id}.pickle");

    // Check if pickled files exist
    if !std::path::Path::new(&pickleable_path).exists() {
        return None;
    }

    // The Python code to read the pickle file to a variable named loaded_vars.
    let code = CString::new(format!(
        r"import dill

loaded_vars = []

# Load picklable variables
with open('{pickleable_path}', 'rb') as f:
    # We load all variables from the file.
    # If there is only 1, it's the locals, if there are more, we assume that they are all locals.
    while True:
        try:
            var = dill.load(f)
            loaded_vars.append(var)
        except EOFError:
            break # We reached the end of the file, so we can stop loading variables.

# Now, if there is not exactly one variable, we assume that they are all locals.
if len(loaded_vars) == 1:
    loaded_vars = loaded_vars[0] # We assume that this is the locals.
else:
    # If there are multiple variables, we assume that they are all locals.
    # We will just return them as a dictionary.
    # It is currently a list of dictionaries, and we want to convert it to a single dictionary.
    loaded_vars = {{k: v for d in loaded_vars for k, v in d.items()}}

# Now, 'loaded_vars' contains all the variables to be used as locals
"
    ))
    .expect("Constant CString failed conversion");
    let temp_locals = PyDict::new(py);

    // Run the code; if it doesn't work, we'll just return None.
    py.run(&code, Some(&PyDict::new(py)), Some(&temp_locals))
        .ok()?;

    // Now we have the loaded_vars in the locals.
    // We have to load that into the locals in rust, so we can use it.
    let loaded_vars = temp_locals.get_item("loaded_vars").ok()??;

    // We expect the loaded_vars to be a dictionary, so we'll try to convert it to one.
    let locals = loaded_vars.downcast_into::<PyDict>().ok()?;

    // For debugging, log the names of the variables.
    let keys = locals.keys();
    for k in keys {
        trace!("Loaded variable: {:?}", k);
    }

    Some(locals)
}

/// Helper function to save the locals to a pickle file.
fn save_to_pickle_file(py: Python, locals: &Bound<PyDict>, thread_id: &str) {
    trace!("Saving the locals to a pickle file.");

    // We want to save the result of databrowser searches, but they are unpickleable.
    // By default, they contain metadata besides the result, which can be useful.
    // I couldn't find a way to keep the metadata, but the results can simply be extracted by running it through a list().

    // First we filter the locals to only include the ones that are actually serializable.
    // We'll execute some python code to do that.
    let code = CString::new(format!(
        r"import dill # like pickle, but can handle >2GB variables
from types import ModuleType
import freva_client

local_items = locals().copy()
pickleable_vars = {{}}
unpickleable_vars = {{}}

for key, value in local_items.items():
    try:
        if isinstance(value, ModuleType):
            # We shouldn't pickle modules, so we'll just skip them.
            unpickleable_vars[key] = [None, value]
            continue
        if isinstance(value, freva_client.query.databrowser):
            # We cannot store it as a databrowser result, but we can store it as a list
            pickleable_vars[key] = list(value)
            continue # We don't want to store it twice
        dill.dumps(value)
        pickleable_vars[key] = value
    except Exception as e:
        # We'd like to hint that we can't pickle this variable, but printing would show it to the LLM.
        # So instead we store it in a variable that we access later in Rust.
        unpickleable_vars[key] = [e,value]
        pass # we'll just assume that it's something we can't handle like a module

# In order to be consistent to the new standard, we need at least two variables to store, so they aren't confused with the locals.
if len(pickleable_vars) == 0:
    pickleable_vars['empty'] = None
if len(pickleable_vars) == 1:
    pickleable_vars['empty2'] = None

# Save picklable variables
with open('python_pickles/{thread_id}.pickle', 'wb') as f:
    # Loop over all the variables and pickle them individually.
    # This is necessary because dill can't tell which variables are pickleable and which aren't.
    # If we try to pickle them all at once, it will fail if one of them is not pickleable.
    for key, value in pickleable_vars.items():
        # We use dill.dump to save the variables to the file.
        try:
            dill.dump({{key: value}}, f)
        except Exception as e:
            # If we can't pickle the variable, we'll just skip it.
            # We'll store the exception in the unpickleable_vars dictionary.
            unpickleable_vars[key] = [e, value]"
    )).expect("Constant CString failed conversion");
    let locals = locals.clone();

    // We'll run the code.
    match py.run(&code, Some(&PyDict::new(py)), Some(&locals)) {
        Ok(()) => {
            // The code executed successfully.
            trace!("Successfully saved the locals to a pickle file.");
        }
        Err(e) => {
            // The code didn't execute successfully.
            warn!("Error saving the locals to a pickle file: {:?}", e);
            println!("Error saving the locals to a pickle file: {e:?}",);
        }
    }

    // Now we'll check if there are any variables that we couldn't pickle.
    let unpickleable_vars = locals.get_item("unpickleable_vars").ok().flatten();
    if let Some(Ok(unpick)) = unpickleable_vars.map(|x| x.downcast_into::<PyDict>()) {
        trace!("Unpickleable variables found.");
        // We'll log the names of the variables that we couldn't pickle.
        let items = unpick.items();
        for k in items {
            trace!("Unpickleable variable: {:?}", k);
            // Try to get the exception
            let tuple = k
                .downcast_into::<PyTuple>()
                .ok()
                .and_then(|x| x.get_item(1).ok()); // 0th item is the key, 1st is the value
            let exception = tuple
                .and_then(|x| x.downcast_into::<PyList>().ok())
                .and_then(|x| x.get_item(0).ok());
            if let Some(exception) = exception {
                // We'll log the exception.
                trace!("Exception: {:?}", exception.repr());
            }
        }
    }
    // Also trace print all the variables that we could pickle.
    let pickleable_vars = locals.get_item("pickleable_vars").ok().flatten();
    if let Some(Ok(pick)) = pickleable_vars.map(|x| x.downcast_into::<PyDict>()) {
        trace!("Pickleable variables found.");
        let items = pick.items();
        for k in items {
            trace!("Stored variable: {:?}", k);
        }
    }
}
