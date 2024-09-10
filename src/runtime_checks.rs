use tracing::{debug, error, info, trace};

use crate::{
    auth::AUTH_KEY,
    chatbot::{self, types::StreamVariant},
    static_serve,
};

/// Check that the setup is correct for the runtime to run
/// Initializes lazy variables to make sure they don't fail later.
/// Checks Auth setup.
/// Runs a few basic tests agains the code interpreter.
pub fn run_runtime_checks() {
    // The lazy static STARTING_MESSAGE_JSON can fail if the prompt or messages cannot be converted to a string.
    // To make sure that this is caught early, we'll just test it here.
    let _ = chatbot::prompting::STARTING_PROMPT_JSON.clone();
    trace!(
        "Starting messages JSON: {:?}",
        chatbot::prompting::STARTING_PROMPT_JSON
    );

    trace!("Ping Response: {:?}", static_serve::RESPONSE_STRING);

    // We'll also initialize the authentication here so it's available for the entire server, from the very start.
    let auth_string = match std::env::var("AUTH_KEY") {
        Ok(auth_string) => auth_string,
        Err(e) => {
            error!(
                "Error reading the authentication string from the environment variables: {:?}",
                e
            );
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
    println!("Authentication string set successfully.");

    // Run the basic checks for the code interpreter.
    // Note that those checks need to be runtime, not compiletime, as the code interpreter calles the binary itself.
    check_two_plus_two();
    check_print();

    // Because some python versions are allergic to mac, I'll disable the import checks if the OS is mac.
    check_imports();
}

/// Checks that the code interpreter can calculate 2+2.
/// It's a very basic check to make sure that the code interpreter is working.
fn check_two_plus_two() {
    let output = crate::tool_calls::code_interpreter::parse_input::start_code_interpeter(
        Some(r#"{"code": "2+2"}"#.to_string()),
        "test".to_string(),
    );
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "4\n\n".to_string(),
            "test".to_string()
        )]
    ); // I still don't know why the code interpreter adds an extra newline.
}

/// Checks that the code interpreter can handle printing.
fn check_print() {
    let output = crate::tool_calls::code_interpreter::parse_input::start_code_interpeter(
        Some(r#"{"code": "print('Hello World!', flush=True)"}"#.to_string()),
        "test".to_string(),
    );
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "Hello World!\n\n".to_string(),
            "test".to_string()
        )]
    ); // I still don't know why the code interpreter adds an extra newline.
}

/// Checks that all wanted libraries can be imported.
fn check_imports() {
    println!("Checking whether all python imports are available.");
    info!("Checking whether all python imports are available.");
    // libraries if not on mac
    #[cfg(not(target_os = "macos"))]
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
    #[cfg(target_os = "macos")]
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
    for library in libraries.iter() {
        check_single_import(library);
    }
    println!("All imports are available.");
    info!("All imports are available.");
}

/// Checks that the code interpreter can import one specific library.
fn check_single_import(library: &str) {
    let formatted_import_code = format!(r#"{{"code": "import {}"}}"#, library);
    debug!(formatted_import_code);
    let output = crate::tool_calls::code_interpreter::parse_input::start_code_interpeter(
        Some(formatted_import_code),
        "test".to_string(),
    );
    assert!(output.len() == 1);
    assert_eq!(
        output[0],
        StreamVariant::CodeOutput("\n".to_string(), "test".to_string())
    );
}
