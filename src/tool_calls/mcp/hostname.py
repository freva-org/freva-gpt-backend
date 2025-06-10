
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("hostname")

# A single tool that just returns the hostname of the machine
@mcp.tool()
def get_hostname() -> str:
    """
    Returns the hostname of the machine.
    """
    import socket
    return socket.gethostname()

print("Python: Hostname MCP server is ready to use.")