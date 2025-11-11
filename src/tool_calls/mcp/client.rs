// Because the MCP Client that comes with the MCP crate does not allow us to modify the
// request headers, which we need for our own authentication, this file copies and adapts
// the MCP Client implementation from the MCP crate.

use std::borrow::Cow;
use std::sync::Arc;

use futures::stream::BoxStream;
use futures::StreamExt;
use reqwest::header::{ACCEPT, WWW_AUTHENTICATE};
use rmcp::model::{ClientJsonRpcMessage, ServerJsonRpcMessage};
use rmcp::service::{DynService, RunningService};
use rmcp::transport::common::http_header::{
    EVENT_STREAM_MIME_TYPE, HEADER_LAST_EVENT_ID, HEADER_SESSION_ID, JSON_MIME_TYPE,
};
use rmcp::transport::streamable_http_client::{
    AuthRequiredError, StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
};
use rmcp::RoleClient;
use sse_stream::{Sse, SseStream};
use tracing::{debug, error};

use clap::crate_version;
use rmcp::model::{ClientCapabilities, ClientInfo, Implementation};

use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;

pub type ServiceType = RunningService<RoleClient, Box<dyn DynService<RoleClient> + 'static>>;

// We need our own implementation of the Client class for the MCP Client.
#[derive(Clone)]
pub struct MCPRAGClient {
    pub inner: reqwest::Client,
    pub mongodb_uri: String,
}

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
            .header("mongodb-uri", self.mongodb_uri.as_str())
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
            .header(ACCEPT, [EVENT_STREAM_MIME_TYPE, JSON_MIME_TYPE].join(", "))
            .header("mongodb-uri", self.mongodb_uri.as_str());
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

async fn construct_mcp_rag_client(mongodb_uri: String) -> Option<Arc<ServiceType>> {
    // First construct the inner client.
    let client = MCPRAGClient {
        inner: reqwest::Client::new(),
        mongodb_uri: mongodb_uri.clone(),
    };
    let transport = StreamableHttpClientTransport::with_client(
        client,
        StreamableHttpClientTransportConfig {
            uri: "http://localhost:8050/mcp".into(), // TODO: make it properly configurable
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
}

// Because the client is dependant on the mongodb URI, we may not be able to reuse a single one for all requests.
// For now, we construct one every time it's needed, but in the future we may want to cache them based on the URI.
pub async fn get_mcp_rag_client(mongodb_uri: String) -> Option<Arc<ServiceType>> {
    construct_mcp_rag_client(mongodb_uri).await
}
