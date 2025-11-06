from typing import Callable, Awaitable, Dict, Any
from contextvars import ContextVar
import logging

MONGODB_URI_HDR = "mongodb-uri"

def make_header_gate(
    inner_app,
    *,
    mongo_ctx: ContextVar[str | None],
    logger: logging.Logger | None = None,
    mcp_path: str = "/mcp",
):
    """
    Wrap the FastMCP ASGI app so every request to `mcp_path`:
      - enforces a valid mongodb URI in mongodb-uri,
      - sets ContextVars for downstream code.
    """
    log = logger or logging.getLogger("rag.header_gate")

    class HeaderCaptureASGI:
        def __init__(self, app):
            self.app = app

        async def __call__(
            self,
            scope: Dict[str, Any],
            receive: Callable[..., Awaitable],
            send: Callable[..., Awaitable],
        ):
            print(scope) #DEBUG
            if scope.get("type") != "http":
                return await self.app(scope, receive, send)

            path = scope.get("path", "")
            if path != mcp_path:
                return await self.app(scope, receive, send)

            # Normalize headers to a case-insensitive dict
            hdrs = {
                k.decode("latin-1").lower(): v.decode("latin-1")
                for k, v in scope.get("headers", [])
            }
            v = hdrs.get(MONGODB_URI_HDR)
            # Fallback: also try the bearer token header
            if not v:
                raw_auth = hdrs.get("authorization")
                if raw_auth and raw_auth.lower().startswith("bearer "):
                    v = raw_auth[7:].strip()
                    # Also remove the token from headers to avoid confusion downstream
                    # del scope["headers"]  # Be careful with case sensitivity
                    # Scope is a dict, where "headers" is a list of tuples, so we need to reconstruct it
                    new_headers = [
                        (k, val) for k, val in scope.get("headers", [])
                        if k.decode("latin-1").lower() != "authorization"
                    ]
                    scope["headers"] = new_headers

            try:
                log.info("RAG headers (ASGI wrap): vault=%r all=%r", v, hdrs)
            except Exception:
                pass  # never fail on logging

            # Enforce required vault header
            if not v or not (v.startswith("mongodb://") or v.startswith("mongodb+srv://")):
                body = (
                    b'event: message\r\n'
                    b'data: {"jsonrpc":"2.0","error":{"code":-32600,'
                    b'"message":"Missing or invalid header \'' + MONGODB_URI_HDR.encode("utf-8") + b'\' '
                    b'(expected mongodb:// or mongodb+srv://)"}}\r\n\r\n'
                )
                await send({
                    "type": "http.response.start",
                    "status": 400,
                    "headers": [
                        (b"content-type", b"text/event-stream"),
                        (b"cache-control", b"no-cache, no-transform"),
                        (b"connection", b"keep-alive"),
                    ],
                })
                await send({"type": "http.response.body", "body": body, "more_body": False})
                return

            # Set ContextVars for downstream code
            tok_v = mongo_ctx.set(v)
            try:
                return await self.app(scope, receive, send)
            finally:
                mongo_ctx.reset(tok_v)

    return HeaderCaptureASGI(inner_app)
