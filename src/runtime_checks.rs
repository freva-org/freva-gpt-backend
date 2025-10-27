use std::io::Write;

use tracing::{debug, error, info, trace};

use crate::{
    auth::{ALLOW_GUESTS, AUTH_KEY},
    chatbot::{
        self, is_lite_llm_running, stream_response::STREAM_STOP_CONTENT, types::StreamVariant,
        LITE_LLM_ADDRESS,
    },
    static_serve,
    tool_calls::{mcp::execute::try_execute_mcp_tool_call, route_call::print_and_clear_tool_logs},
};

/// Helper function to flush stdout and stderr.
fn flush_stdout_stderr() {
    if let Err(e) = std::io::stdout().flush() {
        error!("Error flushing stdout: {e:?}",);
        eprintln!("Error flushing stdout: {e:?}",);
    }
    if let Err(e) = std::io::stderr().flush() {
        error!("Error flushing stderr: {e:?}",);
        eprintln!("Error flushing stderr: {e:?}",);
    }
}

/// Check that the setup is correct for the runtime to run:
/// - Initializes lazy variables to make sure they don't fail later.
/// - Checks Auth setup.
/// - Runs a few basic tests agains the code interpreter.
pub async fn run_runtime_checks() {
    // The function can fail if the prompt or messages cannot be converted to a string.
    // To make sure that this is caught early, we'll just test it here.
    let entire_prompt_json = chatbot::prompting::get_entire_prompt_json("testing", "testing");
    trace!("Starting messages JSON: {:?}", entire_prompt_json);
    let entire_prompt_json_gpt_5 =
        chatbot::prompting::get_entire_prompt_json_gpt_5("testing", "testing");
    trace!(
        "Starting messages JSON for GPT-5: {:?}",
        entire_prompt_json_gpt_5
    );

    trace!("Ping Response: {:?}", static_serve::RESPONSE_STRING);

    // The lazy static STREAM_STOP_CONTENT can also fail, so we need to test it here.
    let _ = STREAM_STOP_CONTENT.clone();

    // The heartbeat module also has a lazy static variable that we should initialize here.
    {
        let guard = chatbot::heartbeat::SYSINFO.read().await;
        debug!("System information: {:?}", guard.0);
    }

    // We can also check whether all expected environment variables are actually set.
    // Dotenvy set them in the main function already, so we check the .env.example file against std::env::var
    // We can just include the file as a string and parse it line by line.
    check_env_variables();

    // We'll also initialize the authentication here so it's available for the entire server, from the very start.
    print!("Checking the authentication string... ");
    flush_stdout_stderr();
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

    // Also part of the authentication check is whether or not to allow guests.
    let allow_guests = match std::env::var("ALLOW_GUESTS") {
        Ok(allow_guests) => allow_guests,
        Err(e) => {
            error!("Error reading the ALLOW_GUESTS environment variable: {e:?}",);
            eprintln!("Error reading the ALLOW_GUESTS environment variable: {e:?}");
            std::process::exit(1);
        }
    };

    ALLOW_GUESTS
        .set(allow_guests == "true")
        .unwrap_or_else(|_| {
            error!("Error setting the ALLOW_GUESTS variable. Exiting...");
            eprintln!("Error setting the ALLOW_GUESTS variable. Exiting...");
            std::process::exit(1);
        });

    info!("Authentication string set successfully.");
    println!("Success!");

    // Run the basic checks for the code interpreter.
    // Note that those checks need to be runtime, not compiletime, as the code interpreter calles the binary itself.
    print!("Running runtime checks including library checks for the code interpreter... ");
    flush_stdout_stderr();
    info!("Running runtime checks including library checks for the code interpreter.");
    check_assignments().await;
    check_two_plus_two().await;
    check_print().await;
    check_print_noflush().await;
    check_print_two().await;
    check_imports().await;
    println!("Success!");
    flush_stdout_stderr();
    info!("Runtime checks for the code interpreter were successful and all required libraries are available.");

    // Also check that the code interpreter can handle hard and soft crashes.
    print!("Checking whether the code interpreter can handle crashes... ");
    flush_stdout_stderr();
    info!("Checking whether the code interpreter can handle crashes.");
    check_hard_crash().await;
    check_soft_crash().await;
    println!("Success!");
    flush_stdout_stderr();
    info!("The code interpreter can handle crashes.");

    // Also check that required directories exist.
    if check_directory("/app/logs")
        // & check_directory("/app/threads") // Threads are typically not used, in favor of MongoDB.
        & check_directory("/app/python_pickles")
        & check_directory("/app/rw_dir")
        & check_directory("/app/target")
    // The code interpreter calls itself currently, so the target directory needs to be readable.
    {
        println!("All required directories exist and are readable.");
        info!("All required directories exist and are readable.");
    } else {
        println!("Some required directories are missing or not readable");
        error!("Some required directories are missing or not readable");
    }

    print!("Checking robustness and jupyter like behavior of the code interpreter... ");
    flush_stdout_stderr();
    info!("Checking robustness and jupyter like behavior of the code interpreter.");
    // Check that the syntax error catching works.
    check_syntax_error().await;
    check_syntax_error_surround().await;
    check_traceback_error_surround().await;
    check_eval_exec().await;
    check_plot_extraction().await;
    check_plot_extraction_no_import().await;
    check_plot_extraction_second_to_last_line().await;
    check_plot_extraction_false_negative().await;
    check_plot_extraction_false_positive().await;
    check_plot_extraction_close().await;
    check_indentation().await;
    println!("Success!");
    info!(
        "The code interpreter is robust enough and behaves like a Jupyter notebook in all tests."
    );

    check_available_chatbots();

    // Finally, check whether the LiteLLM Proxy is running.
    if is_lite_llm_running().await {
        info!("LiteLLM is running and available.");
        println!("LiteLLM is running and available.");
    } else {
        info!("LiteLLM is either not running or not available, some LLMs might not work. Address: {} (Defaults to http://litellm:4000)", *LITE_LLM_ADDRESS);
        println!("LiteLLM is either not running or not available, some LLMs might not work. Address: {} (Defaults to http://litellm:4000)", *LITE_LLM_ADDRESS);
    }

    // Check whether MCP tools can be called.
    // TODO: replace with the actual MCP tool call.
    print!("Checking whether MCP tools can be called... ");
    flush_stdout_stderr();
    info!("Checking whether MCP tools can be called.");

    let mcp_result = try_execute_mcp_tool_call(
        "hostname".to_string(),
        // Some("Europe/Berlin".to_string()),
        None,
    )
    .await;
    if let Err(s) = mcp_result {
        error!("Failed to call the MCP tool 'hostname': {s}");
        eprintln!("Failed to call the MCP tool 'hostname': {s}");
    } else {
        println!("Success, hostname is: {mcp_result:?}");
        flush_stdout_stderr();
    }

    // Also test the new rag system
    let mut rag_payload = serde_json::Map::new();
    rag_payload.insert(
        "question".to_string(),
        "How can I create artificial climate data?".into(),
    );
    rag_payload.insert(
        "resources_to_retrieve_from".to_string(),
        "stableclimgen".into(),
    );
    rag_payload.insert(
        "collection".to_string(), // This should be a mongoDB collection, but just to show that, we set it to a strin, like an LLM would.
        "stableclimgen".into(),
    );
    let mcp_result =
        try_execute_mcp_tool_call("get_context_from_resources".to_string(), Some(rag_payload))
            .await;
    if let Err(s) = mcp_result {
        error!("Failed to call the MCP tool 'get_context_from_resources': {s}");
        eprintln!("Failed to call the MCP tool 'get_context_from_resources': {s}");
    } else {
        println!("Success, context is: {mcp_result:?}");
        flush_stdout_stderr();
    }

    // Because the model for genAI takes some time to load, we'll start the loading here.
    // To load the mode, we simply have to execute:
    //     from stableclimgen.src.decode import init_model
    //     init_model()
    // in the code interpreter.
    // I'm not sure whether we can defer it, so we'll just do it live here.
    print!("Loading the genAI model... ");
    flush_stdout_stderr();
    info!("Loading the genAI model...");
    let _output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(
            r#"{"code": "from stableclimgen.src.decode import init_model\ninit_model()"}"#
                .to_string(),
        ),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    // No asserts necessary, as the model loading is done in the background and we don't care about the output.
    println!("Success!");
    info!("The genAI model has been loaded.");
    flush_stdout_stderr();

    // To make sure not to confuse the backend, clear the tool logger.
    // Due to debugging, this now needs two arguments.
    print_and_clear_tool_logs(std::time::SystemTime::now(), std::time::SystemTime::now());
}

/// Checks that the code interpreter can calculate 2+2.
/// It's a very basic check to make sure that the code interpreter is working.
async fn check_two_plus_two() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "2+2"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
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
async fn check_print() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "print('Hello World!', flush=True)"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
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
async fn check_print_noflush() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "print('Hello World!')"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
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
async fn check_print_two() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "print('Hello')\nprint('World!')"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
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
async fn check_assignments() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "a = 2"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    // The output should be empty, as we're not printing anything.
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(String::new(), "test".to_string())]
    );
}

/// Checks that all wanted libraries can be imported.
async fn check_imports() {
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
        check_single_import(library).await;
    }
}

/// Checks that the code interpreter can import one specific library.
async fn check_single_import(library: &str) {
    let formatted_import_code =
        format!(r#"{{"code": "import {library};print(\"success!\", flush=True)"}}"#);
    debug!(formatted_import_code);
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(formatted_import_code),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert!(output.len() == 1);
    assert_eq!(
        output[0],
        StreamVariant::CodeOutput("success!".to_string(), "test".to_string())
    );
}

/// Checks that the code interpreter can run code that crashes python hard with crashing itself.
pub async fn check_hard_crash() {
    let _ = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "exit()"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    // If we reach this point, the code interpreter did not crash.
}

/// Checks that the code interpreter can handle simple problems like division by zero.
pub async fn check_soft_crash() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "1/0"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "ZeroDivisionError: division by zero\nTraceback (most recent call last):\n  File \"<string>\", line 1, in <module>\n\nHint: the error occured on line 1\n1: > 1/0 <\n".to_string(),
            "test".to_string()
        )]
    );
}

#[cfg(target_os = "linux")]
/// Simple helper function that checks whether the given string is a path to a directory we can read from.
pub fn check_directory(path: &str) -> bool {
    std::fs::read_dir(path).is_ok()
}

#[cfg(not(target_os = "linux"))]
/// Simple helper function that checks whether the given string is a path to a directory we can read from.
pub fn check_directory(_path: &str) -> bool {
    println!("Directory checks are only implemented for Linux (Docker), skipping.");
    true
}

/// Checks that the code interpreter can catch syntax errors
/// AND highlight the line where the error occured.
async fn check_syntax_error() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "dsa=na034ß94?ß"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
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
async fn check_syntax_error_surround() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "import np\ndsa=na034ß94?ß\nprint('Hello World!')"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "(An error occured; no traceback available)\nSyntaxError: invalid syntax (<string>, line 2)\n\nHint: the error occured on line 2\n1: import np\n2: > dsa=na034ß94?ß <\n3: print('Hello World!')".to_string(),
            "test".to_string()
        )]
    );
}

/// Checks that the code interpreter can catch tracebacks
/// and highlight the line around the error.
/// The base error is already tested in check_soft_crash.
async fn check_traceback_error_surround() {
    // Code to check: 1/0
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "a=2\n1/0\nb=3"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "ZeroDivisionError: division by zero\nTraceback (most recent call last):\n  File \"<string>\", line 2, in <module>\n\nHint: the error occured on line 2\n1: a=2\n2: > 1/0 <\n3: b=3".to_string(),
            "test".to_string()
        )]
    );
}

/// Checks that the code interpreter can properly decide which lines to execute and which to evaluate.
/// The logic should be the same as in a Jupyter notebook.
async fn check_eval_exec() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "a = 2\nb = 3\na+b"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "5".to_string(),
            "test".to_string()
        )]
    );
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "a = 2\nb = 3\na,b"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "(2, 3)".to_string(),
            "test".to_string()
        )]
    );
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "a = 2\nb = 3\nfloat(a+b)"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "5.0".to_string(),
            "test".to_string()
        )]
    );
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "a = 2\nb = 3\n(a, b if not a==b else a)"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "(2, 3)".to_string(),
            "test".to_string()
        )]
    );
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "test=[1,2,3]\nlen(test)"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 1);
    assert_eq!(
        output,
        vec![StreamVariant::CodeOutput(
            "3".to_string(),
            "test".to_string()
        )]
    );
}

/// Checks that the list of AvailableChatbots is correctly initialized
fn check_available_chatbots() {
    // This is a simple check to see if the list of available chatbots is not empty.
    // If it is empty, the server should not start.
    if chatbot::available_chatbots::AVAILABLE_CHATBOTS.is_empty() {
        error!("No available chatbots found. Please check the configuration.");
        eprintln!("Error: No available chatbots found. Please check the configuration.");
        std::process::exit(1);
    } else {
        info!(
            "Available chatbots: {:?}",
            chatbot::available_chatbots::AVAILABLE_CHATBOTS
        );
    }
}

/// Tests whether or not a plot is correctly extracted from the code interpreter.
async fn check_plot_extraction() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "import matplotlib.pyplot as plt\nplt.plot([1, 2, 3], [4, 5, 6])\nplt.show()"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 2);
    // The plot should be extracted and returned as a string.
    // assert!(matches!(output[0], StreamVariant::CodeOutput(_, _)));
    assert!(matches!(output[0], StreamVariant::CodeOutput(ref inner, _) if inner.is_empty()));
    assert!(matches!(output[1], StreamVariant::Image(_)));
}

/// Tests whether or not a plot is correctly extracted from the code interpreter, even if matplotlib is not imported AND plt.show() is not called.
async fn check_plot_extraction_no_import() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "plt.plot([1, 2, 3], [4, 5, 6])"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 2);
    // The plot should be extracted and returned as a string.
    assert!(matches!(output[0], StreamVariant::CodeOutput(_, _))); // Inner is NOT empty because that is evaluated to a Lines2D object.
    assert!(matches!(output[1], StreamVariant::Image(_)));
}

/// Tests whether or not a plot on the second-to-last line is correctly extracted from the code interpreter.
async fn check_plot_extraction_second_to_last_line() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "import matplotlib.pyplot as plt\nplt.plot([1, 2, 3], [4, 5, 6])\nplt.show()\nprint('Done!')"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 2);
    // The plot should be extracted and returned as a string.
    assert!(matches!(output[0], StreamVariant::CodeOutput(ref inner, _) if inner == "Done!"));
    assert!(matches!(output[1], StreamVariant::Image(_)));
}

/// Tests whether or not the code interpreter can handle a true negative plot, where it's commented out.
async fn check_plot_extraction_false_negative() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "import matplotlib.pyplot as plt\n# plt.plot([1, 2, 3], [4, 5, 6])\n# plt.show()"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 1);
    // The output should be empty, as we're not printing anything.
    assert!(matches!(output[0], StreamVariant::CodeOutput(ref inner, _) if inner.is_empty()));
}

/// Tests whether or not the code interpreter detects that it shouldn't output the plot if it was only imported and not used.
async fn check_plot_extraction_false_positive() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "import matplotlib.pyplot as plt"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 1);
    // The output should be empty, as we're not printing anything.
    assert!(matches!(output[0], StreamVariant::CodeOutput(ref inner, _) if inner.is_empty()));
}

/// Tests whether or not the code interpreter can handle plt.close() calls.
/// This is important because some LLMs like to end their code with plt.close(),
/// which would prevent the backend from extracting the plot.
async fn check_plot_extraction_close() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(r#"{"code": "import matplotlib.pyplot as plt\nplt.plot([1, 2, 3], [4, 5, 6])\nplt.close()"}"#.to_string()),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 2);
    // The plt.close() call should not prevent the plot from being extracted.
    // The plot should be extracted and returned as a string.
    assert!(matches!(output[0], StreamVariant::CodeOutput(ref inner, _) if inner.is_empty()));
    assert!(matches!(output[1], StreamVariant::Image(_)));
}

/// Tests whether or not the code interpreter can handle indentation on the last line.
async fn check_indentation() {
    let output = crate::tool_calls::code_interpreter::prepare_execution::start_code_interpeter(
        Some(
            r#"{"code": "a=3\nif a < 2:\n\tprint('smaller')\nelse:\n\tprint('larger')"}"#
                .to_string(),
        ),
        "test".to_string(),
        None,
        "testing".to_string(),
    )
    .await;
    assert_eq!(output.len(), 1);
    // The output should contain the results of the second branch.
    // assert!(matches!(output[0], StreamVariant::CodeOutput(ref inner, _) if inner == "larger"));
    let inner = match &output[0] {
        StreamVariant::CodeOutput(inner, _) => inner,
        _ => panic!("Expected a CodeOutput variant, instead got {:?}", output[0]),
    };
    assert_eq!(inner, "larger");
}

fn check_env_variables() {
    // Include the .env.example file as a string.
    let env_example = include_str!("../.env.example");
    // Parse the file line by line.
    for line in env_example.lines() {
        // Ignore comments and empty lines.
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Split the line into key and value.
        let parts: Vec<&str> = line.splitn(2, '=').collect();
        if parts.len() != 2 {
            continue;
        }
        let key = parts[0].trim();
        // Check if the environment variable is set.
        match std::env::var(key) {
            Ok(value) => {
                debug!("Environment variable {key} is set to {value}");
            }
            Err(std::env::VarError::NotPresent) => {
                error!("Environment variable {key} is not set, but expected (Check .env.example). Please set it in the .env file or environment.");
                eprintln!("Error: Environment variable {key} is not set, but expected (Check .env.example). Please set it in the .env file or environment.");
            }
            Err(e) => {
                error!("Error reading environment variable {key}: {:?}", e);
                eprintln!("Error reading environment variable {key}: {e:?}");
            }
        }
    }
}
