#!/usr/bin/env bash
# Runs the code interpreter directly; for debugging. 
# Does not auto-import or format errors nicely, that is done outside of the code interpreter. 
# Note that the lines should be properly separated with a newline, not a \n.

set -euo pipefail

if [ $# -lt 1 ]; then
    echo "Usage: $(basename "$0") \"<python code>\""
    exit 1
fi

# Collect all arguments as the Python code snippet
CODE="$*"

# Determine the directory of this script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"

# Path to the freva-gpt2-backend binary
BINARY="$SCRIPT_DIR/../target/release/freva-gpt2-backend"

if [ ! -x "$BINARY" ]; then
    echo "Error: Binary not found or not executable at $BINARY"
    exit 1
fi

# Write into the log file that a debug session has started
echo "Starting debug session for code: $CODE" >> "$SCRIPT_DIR/../logging_from_tools.log"

# Execute the backend with the provided Python code
exec "$BINARY" --code-interpreter "$CODE"