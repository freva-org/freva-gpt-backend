import os
import ssl

from mcp.server.fastmcp import FastMCP
import litellm

from src.tool_calls.rag.helpers import *
from src.tool_calls.rag.document_loaders import CustomDirectoryLoader
from src.tool_calls.rag.text_splitters import CustomDocumentSplitter

EMBEDDING_MODEL = os.getenv("EMBEDDING_MODEL", "ollama/mxbai-embed-large:latest")
OLLAMA_BASE_URL = os.getenv("OLLAMA_ADDRESS", "http://localhost:11434")
RESOURCE_DIR = os.getenv("RESOURCE_DIRECTORY", "resources")
EMBEDDING_LENGTH = 1024
AVAILABLE_RESOURCES = [f for f in os.listdir(RESOURCE_DIR) if os.path.isdir(os.path.join(RESOURCE_DIR, f))]
CLEAR_EMBEDDINGS = False
 
logger = configure_logger()

# Create an MCP server
mcp = FastMCP("rag_server")

def get_embedding(text):
    """Get embedding for a given text"""
    response = litellm.embedding(
        input=text,
        model=EMBEDDING_MODEL, # os.getenv("EMBEDDING_MODEL"),  # Model to use for embeddings
        temperature=0.2,  # Temperature for the model
        api_base= OLLAMA_BASE_URL,  # Base URL for the API
    )

    if response.data and len(response.data) != 0:
        embedding = response.data[0]['embedding']
        return embedding
    elif not response.data:
        raise ValueError("No embedding data returned from the model.")
    if not isinstance(response.data[0], dict) or 'embedding' not in response.data[0]:
        raise ValueError("Embedding data is not in the expected format.")


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


def store_documents_in_mongodb(documents, db_collection):
    """Create and store embeddings for the provided documents."""
    new_documents = get_new_or_changed_documents(documents, db_collection)
    new_entries = []

    for d in new_documents:
        entry = create_db_entry_for_document(d)
        new_entries.append(entry)

    # Insert new embeddings
    if new_entries:
        logger.info(f"Inserting {len(new_entries)} new embeddings into MongoDB")
        db_collection.insert_many(new_entries)


def get_query_results(query: str, resource_name, db_collection):
    """Gets results from a vector search query."""
    add_vector_search_index_to_db(db_collection, EMBEDDING_LENGTH)

    logger.info(f"Searching for query: {query}")
    query_embedding = get_embedding(query)
    query_results = []

    src_types = db_collection.distinct("resource_type")
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

        query_results.append(list(db_collection.aggregate(pipeline)))

    if query_results:
        return postprocessing_query_result(query_results)
    else:
        logger.info("No results found for the query.")
        return "No content found."


@mcp.tool()
def get_context_from_resources(question: str, resources_to_retrieve_from: str, collection) -> str:
    """
    Search Python package/library documentation and examples to find relevant context.
    Args:
        question (str): The user's question.
        resources_to_retrieve_from (str): The name of the library to search the documentation for. It should be one of the folder names in RESOURCE_DIR.
    Returns:
        str: Relevant context extracted from the library documentation.
    """
    logger.info(f"Searching for context in {resources_to_retrieve_from} documentation for question: {question}")
    if resources_to_retrieve_from not in AVAILABLE_RESOURCES:
        logger.error(f"Library '{resources_to_retrieve_from}' is not supported.")
        return f"Library '{resources_to_retrieve_from}' is not supported."

    if CLEAR_EMBEDDINGS:
        clear_embeddings_collection(collection)

    dir_loader = CustomDirectoryLoader(os.path.join(RESOURCE_DIR, resources_to_retrieve_from))
    documents = dir_loader.load()
    doc_splitter = CustomDocumentSplitter(documents, chunk_size=500, chunk_overlap=50, separators="\n\n")
    chunked_documents = doc_splitter.split()

    store_documents_in_mongodb(chunked_documents, collection)

    context = get_query_results(question, resources_to_retrieve_from)

    return context

def debug():
    resources_to_retrieve_from = "stableclimgen"
    question = "Get global temperature data from February 2nd 1940"

    dir_loader = CustomDirectoryLoader(os.path.join(RESOURCE_DIR, resources_to_retrieve_from))
    documents = dir_loader.load()
    doc_splitter = CustomDocumentSplitter(documents, chunk_size=500, chunk_overlap=50, separators="\n\n")
    chunked_documents = doc_splitter.split()

    store_documents_in_mongodb(chunked_documents)

    context = get_query_results(question, resources_to_retrieve_from)
    print(context)

    
if __name__ == "__main__":
    mcp.run(transport='stdio')
    # debug()
