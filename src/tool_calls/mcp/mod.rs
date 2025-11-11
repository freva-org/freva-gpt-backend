// For the Tool Calls, this module is used to define all MCP (Model Context Protocol) servers and their connections.

// This module is responsible for executing MCP tool calls.
pub mod execute;

// Our implemenetation of the MCP Client.
pub mod client;

use std::sync::Arc;

use async_lazy::Lazy;
use clap::crate_version;
use rmcp::model::{ClientCapabilities, ClientInfo, Implementation};

use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::ServiceExt;
use tokio::process::Command;
use tracing::{debug, error};

use crate::tool_calls::mcp::client::{MCPRAGClient, ServiceType};

/// The global MCP Client that has connections to all supported MCP servers.
static MCP_TEST_CLIENT: Lazy<Option<Arc<ServiceType>>> = Lazy::new(|| {
    Box::pin(async {
        // For testing purposes, use Tokio to spawn a child process for the MCP server.
        let client = ()
            .into_dyn()
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

// The MCP Client that connects to the RAG server.
static MCP_RAG_CLIENT: Lazy<Option<Arc<ServiceType>>> = Lazy::new(|| {
    Box::pin(async {
        // We assume that the server has already started. We know its adress and currently hard code it.

        let mongodb_uri = "mongodb://testing:testing@host.docker.internal:27017".to_string();
        // First construct the inner client.
        let client = MCPRAGClient {
            inner: reqwest::Client::new(),
            mongodb_uri: mongodb_uri.clone(),
        };
        let transport = StreamableHttpClientTransport::with_client(
            client,
            StreamableHttpClientTransportConfig {
                uri: "http://localhost:8050/mcp".into(),
                ..Default::default()
            },
        );
        // let test = StreamableHttpClientTransport::from_uri("http://localhost:8050");

        let client_info = ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "freva-gpt2-backend-rag-client".to_string(),
                version: crate_version!().to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
        };

        let client = client_info.into_dyn().serve(transport).await;

        let client = match client {
            Ok(client) => client,
            Err(e) => {
                error!("Failed to create MCP RAG client: {}", e);
                return None;
            }
        };

        let server_info = client.peer_info();
        debug!("Connected to MCP RAG server: {:?}", server_info);

        let tools = client.list_all_tools().await;
        debug!("MCP RAG server tools: {:?}", tools);

        Some(Arc::new(client))
    })
});

/// The `rust_mcp_sdk` library assigns a client to each MCP server, so we'll collect them here.
pub static ALL_MCP_CLIENTS: Lazy<Vec<Arc<ServiceType>>> = Lazy::new(|| {
    Box::pin(async {
        // We need to collect all the MCP clients here.
        let mut clients = Vec::new();
        // if let Some(client) = (*MCP_TEST_CLIENT.force().await).clone() {
        //     clients.push(client);
        // }

        // Create a new MCPRagClient and add it to the clients list.
        if let Some(client) = (*MCP_RAG_CLIENT.force().await).clone() {
            clients.push(client);
        }
        clients
    })
});
