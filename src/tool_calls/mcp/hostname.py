
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("hostname")

# A single tool that just returns the hostname of the machine
@mcp.tool()
def hostname() -> str:
    """
    Returns the hostname of the machine.
    """
    import socket
    return socket.gethostname()

if __name__ == "__main__":
    # Run the MCP server
    mcp.run()