// For the Tool Calls, this module is used to define all MCP (Model Context Protocol) servers and their connections.

// This module is responsible for executing MCP tool calls.
pub mod execute;

use std::borrow::Cow;
use std::sync::Arc;

use async_lazy::Lazy;
use clap::crate_version;
use futures::stream::BoxStream;
use futures::StreamExt;
use reqwest::header::{ACCEPT, WWW_AUTHENTICATE};
use rmcp::model::{
    ClientCapabilities, ClientInfo, ClientJsonRpcMessage, Implementation, ServerJsonRpcMessage,
};
use rmcp::service::{DynService, RunningService};
use rmcp::transport::common::http_header::{
    EVENT_STREAM_MIME_TYPE, HEADER_LAST_EVENT_ID, HEADER_SESSION_ID, JSON_MIME_TYPE,
};
use rmcp::transport::streamable_http_client::{
    AuthRequiredError, StreamableHttpClient, StreamableHttpClientTransportConfig,
    StreamableHttpError, StreamableHttpPostResponse,
};
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use sse_stream::{Sse, SseStream};
use tokio::process::Command;
use tracing::{debug, error};

pub type ServiceType = RunningService<RoleClient, Box<dyn DynService<RoleClient> + 'static>>;

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

// We need our own implementation of the Client class for the MCP Client.
#[derive(Clone)]
struct MCPRAGClient {
    inner: reqwest::Client,
    mongodb_uri: String,
}

// #[derive(Debug, thiserror::Error)]
// enum RAGError {
//     Reqwest(#[from] reqwest::Error),
//     Generic(String), // TODO: More specific error types.
// }

// impl std::fmt::Display for RAGError {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         match self {
//             RAGError::Generic(msg) => write!(f, "RAGError: {}", msg),
//             RAGError::Reqwest(error) => write!(f, "RAGError: {}", error),
//         }
//     }
// }

impl StreamableHttpClient for MCPRAGClient {
    type Error = reqwest::Error;

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        last_event_id: Option<String>,
        auth_token: Option<String>,
    ) -> Result<BoxStream<'static, Result<Sse, sse_stream::Error>>, StreamableHttpError<Self::Error>>
    {
        let mut request_builder = self
            .inner
            .get(uri.as_ref())
            .header(ACCEPT, EVENT_STREAM_MIME_TYPE)
            .header(HEADER_SESSION_ID, session_id.as_ref())
            .header("mongodb-uri", self.mongodb_uri.as_str());
        if let Some(last_event_id) = last_event_id {
            request_builder = request_builder.header(HEADER_LAST_EVENT_ID, last_event_id);
        }
        if let Some(auth_header) = auth_token {
            request_builder = request_builder.bearer_auth(auth_header);
        }
        let response = match request_builder.send().await {
            Ok(response) => response,
            Err(e) => {
                error!("Failed to send request to MCP server: {}", e);
                return Err(e.into());
            }
        };
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return Err(StreamableHttpError::ServerDoesNotSupportSse);
        }
        let response = response.error_for_status()?;
        match response.headers().get(reqwest::header::CONTENT_TYPE) {
            Some(ct) => {
                if !ct.as_bytes().starts_with(EVENT_STREAM_MIME_TYPE.as_bytes()) {
                    return Err(StreamableHttpError::UnexpectedContentType(Some(
                        String::from_utf8_lossy(ct.as_bytes()).to_string(),
                    )));
                }
            }
            None => {
                return Err(StreamableHttpError::UnexpectedContentType(None));
            }
        }
        let event_stream = SseStream::from_byte_stream(response.bytes_stream()).boxed();
        Ok(event_stream)
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session: Arc<str>,
        auth_token: Option<String>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        let mut request_builder = self.inner.delete(uri.as_ref());
        if let Some(auth_header) = auth_token {
            request_builder = request_builder.bearer_auth(auth_header);
        }
        let response = request_builder
            .header(HEADER_SESSION_ID, session.as_ref())
            .send()
            .await?;

        // if method no allowed
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            tracing::debug!("this server doesn't support deleting session");
            return Ok(());
        }
        let _response = response.error_for_status()?;
        Ok(())
    }

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_token: Option<String>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        let mut request = self
            .inner
            .post(uri.as_ref())
            .header(ACCEPT, [EVENT_STREAM_MIME_TYPE, JSON_MIME_TYPE].join(", "));
        if let Some(auth_header) = auth_token {
            request = request.bearer_auth(auth_header);
        }
        if let Some(session_id) = session_id {
            request = request.header(HEADER_SESSION_ID, session_id.as_ref());
        }
        let response = request.json(&message).send().await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            if let Some(header) = response.headers().get(WWW_AUTHENTICATE) {
                let header = header
                    .to_str()
                    .map_err(|_| {
                        StreamableHttpError::UnexpectedServerResponse(Cow::from(
                            "invalid www-authenticate header value",
                        ))
                    })?
                    .to_string();
                return Err(StreamableHttpError::AuthRequired(AuthRequiredError {
                    www_authenticate_header: header,
                }));
            }
        }
        let status = response.status();
        let response = response.error_for_status()?;
        if matches!(
            status,
            reqwest::StatusCode::ACCEPTED | reqwest::StatusCode::NO_CONTENT
        ) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        let content_type = response.headers().get(reqwest::header::CONTENT_TYPE);
        let session_id = response.headers().get(HEADER_SESSION_ID);
        let session_id = session_id
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        match content_type {
            Some(ct) if ct.as_bytes().starts_with(EVENT_STREAM_MIME_TYPE.as_bytes()) => {
                let event_stream = SseStream::from_byte_stream(response.bytes_stream()).boxed();
                Ok(StreamableHttpPostResponse::Sse(event_stream, session_id))
            }
            Some(ct) if ct.as_bytes().starts_with(JSON_MIME_TYPE.as_bytes()) => {
                let message: ServerJsonRpcMessage = response.json().await?;
                Ok(StreamableHttpPostResponse::Json(message, session_id))
            }
            _ => {
                // unexpected content type
                tracing::error!("unexpected content type: {:?}", content_type);
                Err(StreamableHttpError::UnexpectedContentType(
                    content_type.map(|ct| String::from_utf8_lossy(ct.as_bytes()).to_string()),
                ))
            }
        }
    }
}

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
                auth_header: Some(mongodb_uri),
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
