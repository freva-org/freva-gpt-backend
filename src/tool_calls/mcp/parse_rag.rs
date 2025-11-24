use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace, warn};

use crate::chatbot::types::StreamVariant;

/// The MCP RAG Server returns the response in JSON format.
/// It contains, for each found document, a description of the content
/// and examples to help the LLM answer the question.
pub fn parse_rag_response(response: &str) -> Result<(Vec<String>, Vec<Vec<StreamVariant>>), ()> {
    // TODO: maybe, because the parsed StreamVariants Code and Codeoutput IDs are not coordinated,
    // they could potentially collide. Maybe auto-mangle the IDs here to make them unique?

    let rag_responses: Result<Vec<Vec<RagResponse>>, _> = serde_json::from_str(response);
    let rag_responses = match rag_responses {
        Ok(responses) => {
            debug!("Successfully parsed RAG response into RagResponse enum.");
            responses
        }
        Err(err) => {
            warn!(
                "Failed to parse RAG response into RagResponse enum: {:?}",
                err
            );
            return Err(());
        }
    };
    let mut explanations = Vec::new();
    let mut examples = Vec::new();

    // Iterate over the RagResponses
    for rag_response in rag_responses.iter().flatten() {
        match rag_response {
            RagResponse::Document(docs) => {
                explanations.extend(docs.clone());
            }
            RagResponse::Examples(exs) => {
                for ex in exs {
                    // There are multiple different examples here, so we
                    // store them separately.
                    if let Ok(variants) = split_concatted_json_variants(ex) {
                        examples.push(variants);
                    } else {
                        warn!("Failed to parse example into StreamVariants: {:?}", ex);
                    }
                }
            }
        }
    }

    Ok((explanations, examples))
}

/// The RAG response can be either a list of explanations (documents)
/// or a list of examples (each example is a list of StreamVariants).
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind", content = "content")]
pub enum RagResponse {
    #[serde(rename = "document")]
    Document(Vec<String>),
    #[serde(rename = "example")]
    Examples(Vec<String>),
}

/// Because the RAG response, at the lowest level, returns the list of StreamVariants
/// just concatinated without delimiters, we need to split them into separate examples.
fn split_concatted_json_variants(concatted: &str) -> Result<Vec<StreamVariant>, ()> {
    // We use the same algorithm that I already talked about Bianca on the frontend.
    // Because each JSON object starts with "{" and ends with "}", we can iterate through the string until we find
    // a closing "}". Then we try to parse the substring from the last opening "{" to the current closing "}".
    // If the parsing is successful, we add the parsed object to the result list and continue.
    // Else, we continue until we find the next closing "}".

    // Helper function that takes in a single string slice and tries to return a StreamVariant together with the rest of the string slice.
    fn split_next_variant(s: &str) -> Result<(StreamVariant, &str), ()> {
        for potential in s.chars().enumerate().positions(|(_, c)| c == '}') {
            let (candidate, rest) = s.split_at(potential + 1);
            // Try to parse the candidate
            if let Ok(variant) = serde_json::from_str::<StreamVariant>(candidate) {
                trace!("Parsed StreamVariant: {:?}", variant);
                return Ok((variant, rest));
            }
        }
        // This string was not composed of valid StreamVariants
        debug!("Could not parse any StreamVariant from the given string slice.");
        trace!("Given string slice: {}", s);
        Err(())
    }

    let mut rest = concatted;
    let mut variants = Vec::new();
    while !rest.is_empty() {
        match split_next_variant(rest) {
            Ok((variant, new_rest)) => {
                variants.push(variant);
                rest = new_rest;
            }
            Err(()) => {
                warn!("Could not parse the remaining string slice into StreamVariants.");
                return Err(());
            }
        }
    }
    Ok(variants)
}
