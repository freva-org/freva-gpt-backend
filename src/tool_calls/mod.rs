// For all tool calls that the LLM is able to make.

use std::sync::Arc;

use async_openai::types::{ChatCompletionTool, FunctionObject};
use rmcp::{model::Tool, service::RunningService, RoleClient};
use tracing::{debug, error, trace, warn};

/// Routes the tool call to the appropriate function.
pub mod route_call;

/// The code interpreter that recieves python code and returns the result
pub mod code_interpreter;

/// The module that handles all MCP (Model Context Protocol) servers and their connections.
pub mod mcp;

/// All tools that the LLM can call that don't come from the MCP servers.
pub static ALL_TOOLS_NO_MCP: once_cell::sync::Lazy<Vec<async_openai::types::ChatCompletionTool>> =
    once_cell::sync::Lazy::new(|| vec![code_interpreter::CODE_INTERPRETER_TOOL_TYPE.clone()]);

/// Returns all tools that the LLM can call, including those from the MCP servers.
pub async fn all_tools() -> Vec<async_openai::types::ChatCompletionTool> {
    let mut tools = ALL_TOOLS_NO_MCP.clone();

    let mcp_tools_raw = mcp::ALL_MCP_CLIENTS
        .force()
        .await
        .iter()
        // .flat_map(async |client| client.list_tools(None).await.unwrap_or_default().tools),
        .map(async |client| mcp_client_to_tools(client.clone()).await)
        .collect::<Vec<_>>();

    // The MCP tools all connect simultaneously, so we can use `join_all` to wait for all of them.
    let mcp_tools = futures::future::join_all(mcp_tools_raw)
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    debug!("Found {} MCP tools", mcp_tools.len());
    trace!("MCP tools: {:?}", mcp_tools);

    tools.extend(mcp_tools);
    trace!("All tools: {:?}", tools);
    tools
}

/// Returns properly formatted tools from the MCP servers.
/// Doesn't surface errors and instead just returns an empty vector if something goes wrong.
async fn mcp_client_to_tools(
    client: Arc<RunningService<RoleClient, ()>>,
) -> Vec<ChatCompletionTool> {
    let raw_tool_result = match client.list_tools(None).await {
        Ok(tools) => tools,
        Err(e) => {
            error!("Failed to list tools for MCP client: {}", e);
            return vec![]; // Return an empty vector if the tool listing fails.
        }
    };

    // Again, despite the existance of a cursor, we assume all tools are returned in one go.
    if let Some(cursor) = raw_tool_result.next_cursor {
        warn!("The MCP client returned a cursor for the tool list implying there are more tools than we can see, which is not yet supported. The cursor is: {}", cursor);
    }

    let raw_tool_list = raw_tool_result.tools;

    raw_tool_list
        .into_iter()
        .filter_map(convert_tool)
        .collect::<Vec<_>>()
}

/// Converts a MCP tool into an async_openai tool.
/// Returns `None` if the conversion fails.
fn convert_tool(tool: Tool) -> Option<ChatCompletionTool> {
    let input_schema = tool.input_schema;

    // OpenAI does use the exact same schema as MCP, but the libraries handle it differently;
    // async_openai just requires you to provide a `serde_json::Value` for the parameters.
    // While MCP uses a proper Rust type.
    // That means that we can manually create the parameters for the function object.

    let parameters = match serde_json::to_value(input_schema) {
        Ok(value) => value,
        Err(e) => {
            error!("Failed to convert MCP tool input schema to JSON: {}", e);
            return None; // Return None if the conversion fails.
        }
    };

    let function = FunctionObject {
        name: tool.name.to_string(),
        description: tool.description.map(|d| d.to_string()),
        parameters: Some(parameters),
        strict: None, // Again, we can't use strict mode
    };

    Some(ChatCompletionTool {
        r#type: async_openai::types::ChatCompletionToolType::Function,
        function,
    })
}
