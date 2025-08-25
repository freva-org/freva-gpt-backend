// For the Tool Calls, this module is used to define all MCP (Model Context Protocol) servers and their connections.

// This module is responsible for executing MCP tool calls.
pub mod execute;

use std::sync::Arc;

use once_cell::sync::Lazy;
use rust_mcp_sdk::{
    mcp_client::{client_runtime, ClientHandler, ClientRuntime},
    schema::{
        ClientCapabilities, Implementation, InitializeRequestParams, RpcError,
        LATEST_PROTOCOL_VERSION,
    },
    McpClient, StdioTransport, TransportOptions,
};
use tracing::debug;

use crate::static_serve::VERSION;

/// The MCP library requires a type for the client handling.
struct MCPClient;
#[async_trait::async_trait]
impl ClientHandler for MCPClient {
    async fn handle_process_error(
        &self,
        error_message: String,
        _runtime: &dyn McpClient,
    ) -> std::result::Result<(), RpcError> {
        debug!("MCP Client encountered an error: {}", error_message); // We silence the error handling for now.
        Ok(())
    }
}

/// Constructs a MCP client given the command and args to use.
fn construct_mcp_client(command: &str, args: Vec<String>) -> Arc<ClientRuntime> {
    // First, we need the details of the Client we want to build.
    let client_details = InitializeRequestParams {
        capabilities: ClientCapabilities::default(), // No capabilities for now.
        client_info: Implementation {
            name: "Freva-GPT MCP Custom Client".to_string(),
            version: VERSION.to_string(),
        },
        protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
    };

    // We'll use stdio transport for the MCP Client for now.
    let transport = match StdioTransport::create_with_server_launch(
        command,
        args,
        None,
        TransportOptions::default(),
    ) {
        Ok(transport) => transport,
        Err(e) => {
            panic!("Failed to create MCP Client transport: {e}");
        }
    };

    // Dummy Handler for the MCP Client.
    let handler = MCPClient;

    client_runtime::create_client(client_details, transport, handler)
}

/// The global MCP Client that connects to the hostname mcp server, for testing purposes.
static MCP_TEST_CLIENT: Lazy<Arc<ClientRuntime>> = Lazy::new(|| {
    construct_mcp_client(
        "uv",
        vec![
            "run".to_string(),
            "src/tool_calls/mcp/hostname.py".to_string(),
        ],
    )
});

/// MCP client that connects to the RAG MCP Server
static MCP_RAG_CLIENT: Lazy<Arc<ClientRuntime>> = Lazy::new(|| {
    construct_mcp_client(
        "uv",
        #[cfg(target_os = "macos")]
        vec![
            "run".to_string(),
            "-p".to_string(),
            "/opt/homebrew/anaconda3/bin/python3".to_string(), // The Python interpreter to use
            "src/tool_calls/rag/rag_server.py".to_string(),
        ],
        #[cfg(not(target_os = "macos"))]
        vec![
            "run".to_string(),
            "src/tool_calls/rag/rag_server.py".to_string(),
        ],
    )
});

/// The `rust_mcp_sdk` library assigns a client to each MCP server, so we'll collect them here.
pub static ALL_MCP_CLIENTS: Lazy<Vec<Arc<ClientRuntime>>> = Lazy::new(|| {
    vec![MCP_TEST_CLIENT.clone(), MCP_RAG_CLIENT.clone()] // Add more clients as needed.
});
