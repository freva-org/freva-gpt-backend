use tracing::{debug, error, info, trace, warn};

use crate::{
    auth::AUTH_KEY,
    chatbot::{self, stream_response::STREAM_STOP_CONTENT, types::StreamVariant},
    static_serve,
};

/// Check that the setup is correct for the runtime to run:
/// - Initializes lazy variables to make sure they don't fail later.
/// - Checks Auth setup.
/// - Runs a few basic tests agains the code interpreter.
pub async fn run_runtime_checks() {
    // The lazy static STARTING_MESSAGE_JSON can fail if the prompt or messages cannot be converted to a string.
    // To make sure that this is caught early, we'll just test it here.
    let _ = chatbot::prompting::STARTING_PROMPT_JSON.clone();
    trace!(
        "Starting messages JSON: {:?}",
        chatbot::prompting::STARTING_PROMPT_JSON
    );

    trace!("Ping Response: {:?}", static_serve::RESPONSE_STRING);

    // The lazy static STREAM_STOP_CONTENT can also fail, so we need to test it here.
    let _ = STREAM_STOP_CONTENT.clone();

    // The heartbeat module also has a lazy static variable that we should initialize here.
    {
        let guard = chatbot::heartbeat::SYSINFO.read().await;
        debug!("System information: {:?}", guard.0);
    }

    // We'll also initialize the authentication here so it's available for the entire server, from the very start.
    print!("Checking the authentication string... ");
    info!("Checking the authentication string...");
    let auth_string = match std::env::var("AUTH_KEY") {
        Ok(auth_string) => auth_string,
        Err(e) => {
            error!("Error reading the authentication string from the environment variables: {e:?}",);
            eprintln!(
                "Error reading the authentication string from the environment variables: {e:?}"
            );
            std::process::exit(1);
        }
    };
    AUTH_KEY.set(auth_string).unwrap_or_else(|_| {
        error!("Error setting the authentication string. Exiting...");
        eprintln!("Error setting the authentication string. Exiting...");
        std::process::exit(1);
    });
    info!("Authentication string set successfully.");
    println!("Success!");

    // Run the basic checks for the code interpreter.
    // Note that those checks need to be runtime, not compiletime, as the code interpreter calles the binary itself.
    print!("Running runtime checks including library checks for the code interpreter... ");
    info!("Running runtime checks including library checks for the code interpreter.");
    check_assignments();
    check_two_plus_two();
    check_print();
    check_print_noflush();
    check_print_two();
    check_imports();
    println!("Success!");
    info!("Runtime checks for the code interpreter were successful and all required libraries are available.");

    // Also check that the code interpreter can handle hard and soft crashes.
    print!("Checking whether the code interpreter can handle crashes... ");
    info!("Checking whether the code interpreter can handle crashes.");
    check_hard_crash();
    check_soft_crash();
    println!("Success!");
    info!("The code interpreter can handle crashes.");

    // Also check that required directories exist.
    if check_directory("/app/logs")
        & check_directory("/app/threads")
        & check_directory("/app/python_pickles")
    {
        println!("All required directories exist and are readable.");
        info!("All required directories exist and are readable.");
    } else {
        println!("Some required directories are missing or not readable");
        error!("Some required directories are missing or not readable");
    }
    if !check_directory("/data/inputFiles") {
        println!("The test data is not accessable. This means that the test data will not be available for the runtime.");
        warn!("The test data is not accessable. This means that the test data will not be available for the runtime.");
    }

    // Check that the syntax error catching works.
    check_syntax_error();
    check_syntax_error_surround();
}

/// Checks that the code interpreter can calculate 2+2.
/// It's a very basic check to make sure that the code interpreter is working.
fn check_two_plus_two() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "2+2"}"#.to_string()),
        "test".to_string(),
        None,
    );
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "4".to_string(),
            "test".to_string()
        )]
    );
}

/// Checks that the code interpreter can handle printing.
fn check_print() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "print('Hello World!', flush=True)"}"#.to_string()),
        "test".to_string(),
        None,
    );
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "Hello World!".to_string(),
            "test".to_string()
        )]
    );
}

/// We also make sure that the code interpreter doesn't have to flush the stdout.
fn check_print_noflush() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "print('Hello World!')"}"#.to_string()),
        "test".to_string(),
        None,
    );
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "Hello World!".to_string(),
            "test".to_string()
        )]
    );
}

/// There was a weird error, this is to check that two print statements are correctly handled...
/// I don't exactly know why this error occurs, but it's a good test.
fn check_print_two() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "print('Hello')\nprint('World!')"}"#.to_string()),
        "test".to_string(),
        None,
    );
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "Hello\nWorld!".to_string(),
            "test".to_string()
        ),]
    );
}

/// Check whether simple assignments work.
fn check_assignments() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "a = 2"}"#.to_string()),
        "test".to_string(),
        None,
    );
    // The output should be empty, as we're not printing anything.
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "".to_string(),
            "test".to_string()
        )]
    );
}

/// Checks that all wanted libraries can be imported.
fn check_imports() {
    let libraries = [
        "xarray",
        "tzdata",
        "six",
        "shapely",
        "pytz",
        "shapefile", // This is the pyshp library, but it's called shapefile
        "pyproj",
        "pyparsing",
        "PIL", // This is the pillow library, but it's called pil
        "pandas",
        "packaging",
        "numpy",
        "netCDF4",
        "matplotlib",
        "kiwisolver",
        "fontTools", // Case sensitive
        "cycler",
        "contourpy",
        "cftime",
        "certifi",
        "cartopy", // lowercase
    ];
    for library in &libraries {
        check_single_import(library);
    }
}

/// Checks that the code interpreter can import one specific library.
fn check_single_import(library: &str) {
    let formatted_import_code =
        format!(r#"{{"code": "import {library};print(\"success!\", flush=True)"}}"#);
    debug!(formatted_import_code);
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(formatted_import_code),
        "test".to_string(),
        None,
    );
    assert!(output.len() == 1);
    assert_eq!(
        output[0],
        StreamVariant::CodeOutput("success!".to_string(), "test".to_string())
    );
}

/// Checks that the code interpreter can run code that crashes python hard with crashing itself.
pub fn check_hard_crash() {
    let _ = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "exit()"}"#.to_string()),
        "test".to_string(),
        None,
    );
    // If we reach this point, the code interpreter did not crash.
}

/// Checks that the code interpreter can handle simple problems like division by zero.
pub fn check_soft_crash() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "1/0"}"#.to_string()),
        "test".to_string(),
        None,
    );
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "ZeroDivisionError: division by zero\nTraceback (most recent call last):\n  File \"<string>\", line 1, in <module>".to_string(),
            "test".to_string()
        )]
    );
}

/// Simple helper function that checks whether the given string is a path to a directory we can read from.
pub fn check_directory(path: &str) -> bool {
    std::fs::read_dir(path).is_ok()
}

/// Checks that the code interpreter can catch syntax errors
/// AND highlight the line where the error occured.
fn check_syntax_error() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "dsa=na034ß94?ß"}"#.to_string()),
        "test".to_string(),
        None,
    );
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "(An error occured; no traceback available)\nSyntaxError: invalid syntax (<string>, line 1)\n\nHint: the error occured on line 1\n1: > dsa=na034ß94?ß <\n".to_string(),
            "test".to_string()
        )]
    );
}

/// Checks that the code interpreter can catch syntax error
/// and highlight the lines AROUND the line where the error occured.
fn check_syntax_error_surround() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "import np\ndsa=na034ß94?ß\nprint('Hello World!')"}"#.to_string()),
        "test".to_string(),
        None,
    );
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "(An error occured; no traceback available)\nSyntaxError: invalid syntax (<string>, line 2)\n\nHint: the error occured on line 2\n1: import np\n2: > dsa=na034ß94?ß <\n3: print('Hello World!')".to_string(),
            "test".to_string()
        )]
    );
}
