// For the Tool Calls, this module is used to define all MCP (Model Context Protocol) servers and their connections.

// This module is responsible for executing MCP tool calls.
pub mod execute;

use std::sync::Arc;

use async_lazy::Lazy;
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use tokio::process::Command;
use tracing::{debug, error};

/// The global MCP Client that has connections to all supported MCP servers.
static MCP_TEST_CLIENT: Lazy<Option<Arc<RunningService<RoleClient, ()>>>> = Lazy::new(|| {
    Box::pin(async {
        // For testing purposes, use Tokio to spawn a child process for the MCP server.
        let client = ()
            .serve({
                let spawned = TokioChildProcess::new(Command::new("uv").configure(|cmd| {
                    cmd.arg("run").arg("src/tool_calls/mcp/hostname.py");
                }));
                let Ok(process) = spawned else {
                    // Failed to spawn the process. This is bad, but we shouldn't crash. Throw an Error and return None
                    error!("Failed to spawn MCP server process");
                    return None;
                };
                process
            })
            .await;

        let client = match client {
            Ok(client) => client,
            Err(e) => {
                error!("Failed to create MCP client: {}", e);
                return None;
            }
        };

        let server_info = client.peer_info();
        debug!("Connected to MCP server: {:?}", server_info);

        let tools = client.list_all_tools().await;
        debug!("MCP server tools: {:?}", tools);

        // // Dummy Handler for the MCP Client.
        // let handler = MCPClient;

        // client_runtime::create_client(client_details, transport, handler)

        Some(Arc::new(client))
    })
});

/// The `rust_mcp_sdk` library assigns a client to each MCP server, so we'll collect them here.
pub static ALL_MCP_CLIENTS: Lazy<Vec<Arc<RunningService<RoleClient, ()>>>> = Lazy::new(|| {
    Box::pin(async {
        // We need to collect all the MCP clients here.
        let mut clients = Vec::new();
        if let Some(client) = (*MCP_TEST_CLIENT.force().await).clone() {
            clients.push(client);
        }
        clients
    })
});
