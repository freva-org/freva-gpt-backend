use tracing::{warn, debug};



/// Checks whether the given code passes the basic safety checks.
/// The code is should actually be in JSON format, but our checks should be able to handle that. 
pub fn code_is_likely_safe(code: &String) -> bool {
    // For now, we'll implement a simple check: test whether a "dangerous pattern" is present.

    // Patterns considered "dangerous" for now.
    // Note that we allow the opening of files, as we'll need that for the code interpreter.
    const DANGEROUS_PATTERNS: [&str; 11] = [
        "import os",
        "import sys",
        "exec(",
        "eval(",
        "subprocess",
        "socket",
        "os.system",
        "shutil",
        "ctypes",
        "pickle",
        "__import__",
    ];

    for pattern in DANGEROUS_PATTERNS.iter() {
        if code.contains(pattern) {
            warn!("The code contains a dangerous pattern: {}", pattern);
            debug!("The code is: {}", code);
            return false;
        }
    }

    // Later, we'll expand this to include more sophisticated checks.
    true
}


/// Sanitizes the code for problems that we want to avoid.
/// This isn't something like rm rf, but instead things like using the wrong matplotlib backend.
pub fn sanitize_code(code: String) -> String {
    let mut code = code;
    // Matplotlib backend selection: we are on a linux server and don't do interactive plotting, 
    // so we enforce the Agg backend.

    // If either matplotlib or `plt` is found in the code, we'll add the backend selection.
    if code.contains("matplotlib") || code.contains("plt") {
        code = format!("import matplotlib\nmatplotlib.use('agg')\n{}", code);
    }

    code
}