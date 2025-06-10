// For the Tool Calls, this module is used to define all MCP (Model Context Protocol) servers and their connections.

// This module is responsible for executing MCP tool calls.
pub mod execute;

use std::sync::Arc;

use once_cell::sync::Lazy;
use rust_mcp_sdk::{
    mcp_client::{client_runtime, ClientHandler, ClientRuntime},
    schema::{
        ClientCapabilities, Implementation, InitializeRequestParams, LATEST_PROTOCOL_VERSION,
    },
    StdioTransport, TransportOptions,
};

use crate::static_serve::VERSION;

/// The MCP library requires a type for the client handling.
struct MCPClient;
#[async_trait::async_trait]
impl ClientHandler for MCPClient {}

/// The global MCP Client that has connections to all supported MCP servers.
static MCP_TEST_CLIENT: Lazy<Arc<ClientRuntime>> = Lazy::new(|| {
    // First, we need the details of the Client we want to build.
    let client_details = InitializeRequestParams {
        capabilities: ClientCapabilities::default(), // No capabilities for now.
        client_info: Implementation {
            name: "Freva-GPT MCP Client".to_string(),
            version: VERSION.to_string(),
        },
        protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
    };

    // We'll use stdio transport for the MCP Client for now.
    let transport = match StdioTransport::create_with_server_launch(
        "uv",
        vec![
            "run".to_string(),
            "src/tool_calls/mcp/hostname.py".to_string(),
        ], // Just a dummy MCP server for testing purposes.
        None,
        TransportOptions::default(),
    ) {
        Ok(transport) => transport,
        Err(e) => {
            panic!("Failed to create MCP Client transport: {}", e);
        }
    };

    // Dummy Handler for the MCP Client.
    let handler = MCPClient;

    client_runtime::create_client(client_details, transport, handler)
});

/// The `rust_mcp_sdk` library assigns a client to each MCP server, so we'll collect them here.
pub static ALL_MCP_CLIENTS: Lazy<Vec<Arc<ClientRuntime>>> = Lazy::new(|| {
    vec![MCP_TEST_CLIENT.clone()] // Add more clients as needed.
});
