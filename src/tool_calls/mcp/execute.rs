// This file is for executing the MCP tool call.

use rust_mcp_sdk::{
    schema::{CallToolRequestParams, CallToolResultContentItem},
    McpClient,
};
use tracing::{debug, trace, warn};

use crate::tool_calls::mcp::ALL_MCP_CLIENTS;

/// Tries to execute a tool call on the MCP servers.
/// If it fails, it returns an error.
pub async fn try_execute_mcp_tool_call(
    func_name: String,
    arguments: Option<String>,
) -> Result<String, String> {
    // We first need to instantiate all MCP clients to find the one that has the function.

    // If the arguments are empty, we'll keep them as None.
    let arguments = match arguments {
        None => None,
        Some(args) => {
            // We need to parse the arguments to a JSON object.
            let parsed = serde_json::from_str(&args);
            match parsed {
                Ok(json) => Some(json),
                Err(e) => {
                    warn!("Failed to parse arguments for function '{func_name}': {e}");
                    return Err(format!(
                        "Failed to parse arguments for function '{func_name}': {e}",
                    ));
                }
            }
        }
    };

    let mut result = None;
    for client in ALL_MCP_CLIENTS.iter() {
        // each client first needs to be initialized.
        match client.clone().start().await {
            Err(e) => {
                tracing::error!("Failed to start MCP client: {}", e);
                continue; // Skip to the next client if this one fails.
            }
            Ok(()) => {
                // Now we can try to call the function on the client.
                // For that we first need to check if the client has the function.
                let tool_list = match client.list_tools(None).await {
                    Ok(tools) => tools,
                    Err(e) => {
                        tracing::error!("Failed to list tools for MCP client: {}", e);
                        continue; // Skip to the next client if this one fails.
                    }
                };
                trace!("MCP client listed tools: {:?}", tool_list);

                // TODO: The MCP specifies that the return type of the tool listing is pagenated, so we might need to handle that for larger servers.
                // For now, we'll just assume that all tools are returned in one go.
                if let Some(cursor) = tool_list.next_cursor {
                    warn!("The MCP client returned a cursor for the tool list implying there are more tools than we can see, which is not yet supported. The cursor is: {}", cursor);
                }

                let tools = tool_list.tools;

                // Now we can check if the function is in the list of tools.
                if !tools.iter().any(|tool| tool.name == func_name) {
                    debug!("MCP client does not have the function '{}'.", func_name);
                    continue; // Skip to the next client if this one doesn't have the function.
                }

                // Now that we know that the client has the function, we can call it.

                let request = CallToolRequestParams {
                    name: func_name.clone(),
                    arguments: arguments.clone(),
                };

                match client.call_tool(request).await {
                    Ok(call_result) => {
                        // The MCP client returns a result that we can use.
                        if !matches!(call_result.is_error, Some(false)) {
                            warn!(
                                "MCP client returned an error for function '{func_name}': {:?}",
                                call_result
                            );
                        }

                        // The content of the call result is the output of the function.
                        // It has a few different variants, but we currently only support the string variant.
                        let content = call_result.content;
                        debug!(
                            "MCP client returned content for function '{func_name}': {:?}",
                            content
                        );

                        let mut output = String::new();
                        for item in content {
                            if let CallToolResultContentItem::TextContent(s) = item {
                                output.push_str(&s.text);
                                output.push('\n'); // Add a newline for each text item.
                            } else {
                                warn!(
                                    "MCP client returned unsupported content type for function '{func_name}': {:?}",
                                    item
                                );
                            }
                        }
                        // That's it, just return the output.
                        result = Some(output);
                        break;
                    }
                    Err(e) => {
                        warn!("Failed to call tool '{func_name}' on MCP client: {e}");
                        continue; // Skip to the next client if this one fails.
                    }
                };
            }
        };
    }

    match result {
        None => {
            warn!(
                "No MCP client was able to execute the function '{}'.",
                func_name
            );
            Err(format!(
                "No MCP client was able to execute the function '{}'.",
                func_name
            ))
        }
        Some(output) => {
            debug!(
                "MCP client successfully executed the function '{}'.",
                func_name
            );
            Ok(output)
        }
    }
}
