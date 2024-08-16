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