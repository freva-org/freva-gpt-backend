import os
import requests
import logging
from functools import lru_cache
from contextvars import ContextVar

from pymongo import MongoClient

from fastmcp import FastMCP

from src.tool_calls.rag.helpers import *
from src.tool_calls.rag.document_loaders import CustomDirectoryLoader
from src.tool_calls.rag.text_splitters import CustomDocumentSplitter
from src.tool_calls.rag.header_gate import make_header_gate, MONGODB_URI_HDR
from src.tool_calls.server_auth import jwt_verifier

from src.tool_calls.rag.helpers import configure_logger

logger = configure_logger()

LITE_LLM_ADDRESS = os.getenv("LITE_LLM_ADDRESS", "http://litellm:4000")
CLEAR_MONGODB_EMBEDDINGS = os.getenv("CLEAR_MONGODB_EMBEDDINGS", "0").lower() in {"1","true","yes"}


_disable_auth = os.getenv("MCP_DISABLE_AUTH", "0").lower() in {"1","true","yes"}  # for local testing
mcp = FastMCP("rag_server", auth=None if _disable_auth else jwt_verifier)

# ── Config ───────────────────────────────────────────────────────────────────
EMBEDDING_MODEL="ollama/mxbai-embed-large:latest"
EMBEDDING_LENGTH = 1024

RESOURCE_DIRECTORY="resources"
AVAILABLE_LIBRARIES={"stableclimgen"}

# ── Mongo helpers ────────────────────────────────────────────────────────────
# Per-request header context
mongo_uri_ctx: ContextVar[str | None] = ContextVar("mongo_uri_ctx", default=None)

@lru_cache(maxsize=32)
def _client_for(uri: str) -> MongoClient:
    return MongoClient(uri, serverSelectionTimeoutMS=5000)

def _collection():
    uri = mongo_uri_ctx.get()
    if not uri:
        raise RuntimeError(f"Missing required header '{MONGODB_URI_HDR}'")
    db = _client_for(uri)["rag"]
    return db["embeddings"]

def get_embedding(text):
    """Get embedding for a given text"""
    payload = {
        "model": EMBEDDING_MODEL, 
        "input": text,
        "temperature": 0.2,
        }
    r = requests.post(
            f"{LITE_LLM_ADDRESS}/v1/embeddings",
            json=payload,
            timeout=60,
            )
    try:
        r.raise_for_status()
    except requests.HTTPError as e:
        raise RuntimeError(f"Embeddings proxy error {r.status_code}: {r.text[:300]}") from e

    response = r.json()
    data = response.get("data")
    if not data or not isinstance(data, list):
        raise ValueError(f"Bad embeddings payload: {response}")
    first = data[0]
    if not isinstance(first, dict) or "embedding" not in first:
        raise ValueError(f"Missing 'embedding' in item: {first}")
    return first["embedding"]


def create_db_entry_for_document(document):
    entry = {
        "resource_type": "example" if ".json" in document.metadata.get("source") else "document", 
        "resource_name": document.metadata.get("resource_name"), 
        "document": document.metadata.get("source"),
        "chunk_id": document.metadata.get("chunk_id"),
        "file_hash": document.metadata.get("file_hash"),
        "content": document.page_content,
        "embedded_content": document.metadata["embedded_content"],
        "embedding": get_embedding(document.metadata["embedded_content"]),
        }
    return entry


def store_documents_in_mongodb(documents):
    """Create and store embeddings for the provided documents."""
    col = _collection()
    new_documents = get_new_or_changes_documents(documents, col)
    new_entries = []

    for d in new_documents:
        entry = create_db_entry_for_document(d)
        new_entries.append(entry)

    # Insert new embeddings
    if new_entries:
        logger.info(f"Inserting {len(new_entries)} new embeddings into MongoDB")
        col.insert_many(new_entries)


def get_query_results(query: str, resource_name):
    """Gets results from a vector search query."""
    col = _collection()
    add_vector_search_index_to_db(col, EMBEDDING_LENGTH)

    logger.info(f"Searching for query: {query}")
    query_embedding = get_embedding(query)
    query_results = []

    src_types = col.distinct("resource_type")
    for src_t in src_types:
        pipeline = [
        {
                "$vectorSearch": {
                "index": "vector_index",
                "queryVector": query_embedding,
                "filter": {
                    "$and": [
                        { "resource_type": src_t },
                        { "resource_name": resource_name} 
                        ] 
                },
                "path": "embedding",
                "numCandidates": 15,
                "limit": 3
                }
        }, {
                "$project": {
                "content": 1,
                "resource_type": 1,
                "resource_name":1,
                "document":1,
                "chunk_id":1,
                'score': {
                    '$meta': 'vectorSearchScore'
                    }
            }
        }
        ]

        query_results.append(list(col.aggregate(pipeline)))

    if query_results:
        return postprocessing_query_result(query_results)
    else:
        logger.info("No results found for the query.")
        return "No content found."


@mcp.tool()
def get_context_from_resources(question: str, resources_to_retrieve_from: str) -> str:
    """
    Search Python package/library documentation and examples to find relevant context.
    Args:
        question (str): The user's question.
        resources_to_retrieve_from (str): The name of the library to search the documentation for. It should be one of the folder names in RESOURCE_DIR.
    Returns:
        str: Relevant context extracted from the library documentation.
    """
    logger.info(f"Searching for context in {resources_to_retrieve_from} documentation for question: {question}")
    if resources_to_retrieve_from not in AVAILABLE_LIBRARIES:
        logger.error(f"Library '{resources_to_retrieve_from}' is not supported.")
        return f"Library '{resources_to_retrieve_from}' is not supported."

    if CLEAR_MONGODB_EMBEDDINGS:
        clear_embeddings_collection(_collection())

    src_dir = os.path.join(RESOURCE_DIRECTORY, resources_to_retrieve_from)
    if not os.path.isdir(src_dir):
        return f"Resource directory not found: {src_dir}"
    
    dir_loader = CustomDirectoryLoader(src_dir)
    documents = dir_loader.load()
    doc_splitter = CustomDocumentSplitter(documents, chunk_size=500, chunk_overlap=50, separators="\n\n")
    chunked_documents = doc_splitter.split()

    store_documents_in_mongodb(chunked_documents)

    context = get_query_results(question, resources_to_retrieve_from)

    return context

def debug():
    resources_to_retrieve_from = "stableclimgen"
    question = "Get global temperature data from February 2nd 1940"

    dir_loader = CustomDirectoryLoader(os.path.join(RESOURCE_DIRECTORY, resources_to_retrieve_from))
    documents = dir_loader.load()
    doc_splitter = CustomDocumentSplitter(documents, chunk_size=500, chunk_overlap=50, separators="\n\n")
    chunked_documents = doc_splitter.split()

    if CLEAR_MONGODB_EMBEDDINGS:
        clear_embeddings_collection(_collection())

    store_documents_in_mongodb(chunked_documents)

    context = get_query_results(question, resources_to_retrieve_from)
    print(context)

    
if __name__ == "__main__":
    # Configure Streamable HTTP transport 
    host = os.getenv("MCP_HOST", "0.0.0.0")
    port = int(os.getenv("MCP_PORT", "8050"))

    # Start the MCP server using Streamable HTTP transport
    wrapped_app = make_header_gate(
        mcp.http_app(),
        mongo_ctx=mongo_uri_ctx,
        logger=logger,       
        mcp_path="/mcp",  
    )

    import uvicorn
    uvicorn.run(wrapped_app, host=host, port=port)

    # debug()
