use actix_web::{HttpRequest, HttpResponse, Responder};
use async_openai::types::{
    ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequest,
    CreateChatCompletionRequestArgs,
};
use futures::StreamExt;
use tracing::{debug, info, trace, warn};

use crate::chatbot::{available_chatbots::DEFAULTCHATBOT, handle_active_conversations::add_to_conversation, types::StreamVariant, CLIENT};

pub(crate) async fn stream_response(req: HttpRequest) -> impl Responder {
    // Try to get the thread ID and input from the request's query parameters.
    let qstring = qstring::QString::from(req.query_string());
    let thread_id = match qstring.get("thread_id") {
        None | Some("") => {
            // If the thread ID is not found, we'll return a 400
            warn!("The User requested a stream without a thread ID.");
            return HttpResponse::BadRequest()
                .body("Thread ID not found. Please provide a thread_id in the query parameters.");
        }
        Some(thread_id) => thread_id.to_string(),
    };

    let input = match qstring.get("input") {
        None | Some("") => {
            // If the input is not found, we'll return a 400
            warn!("The User requested a stream without an input.");
            return HttpResponse::BadRequest().body(
                "Input not found. Please provide a non-empty input in the query parameters.",
            );
        }
        Some(input) => input.to_string(),
    };
    trace!(
        "Starting stream for thread {} with input: {}",
        thread_id,
        input
    );

    let messages = match ChatCompletionRequestUserMessageArgs::default()
        .content(input)
        .build()
    {
        Ok(messages) => messages,
        Err(e) => {
            // If we can't build the messages, we'll return a generic error.
            warn!("Error building messages: {:?}", e);
            return HttpResponse::InternalServerError().body("Error building messages.");
        }
    };

    // For testing, a basic request
    let request: CreateChatCompletionRequest = match CreateChatCompletionRequestArgs::default()
        .model(String::from(DEFAULTCHATBOT))
        .n(1)
        // .prompt(input) // This isn't used for the chat API
        .messages(vec![messages.into()])
        .stream(true)
        .max_tokens(1000u32)
        .build()
    {
        Ok(request) => request,
        Err(e) => {
            // If we can't build the request, we'll return a generic error.
            warn!("Error building request: {:?}", e);
            return HttpResponse::InternalServerError().body("Error building request.");
        }
    };
    trace!("Request built!");

    create_and_stream(request, thread_id).await
}

async fn create_and_stream(request: CreateChatCompletionRequest, thread_id: String) -> actix_web::HttpResponse {
    let stream = match CLIENT.chat().create_stream(request).await {
        Ok(stream) => stream,
        Err(e) => {
            // If we can't create the stream, we'll return a generic error.
            warn!("Error creating stream: {:?}", e);
            return HttpResponse::InternalServerError().body("Error creating stream.");
        }
    };

    trace!("Stream created!");

    // First we need to transform the stream from the OpenAI client into a stream that can be used by Actix.
    // Before we can do that, we'll first transform the stream of responses into a stream of Stream Variants.

    let variant_stream = stream.map(move |response| { // moves the thread_id into the closure
        let variant = match response {
            Ok(response) => if let Some(choice) = response.choices.first() {
                // let delta = choice.delta;
                // trace!("Delta: {}", delta);
                // Ok(actix_web::web::Bytes::copy_from_slice(delta.as_bytes())) // Actix wants the stream in this exact format.
                match (&choice.delta.content, choice.finish_reason) {
                    (Some(string_delta), _) => {
                        trace!("Delta: {}", string_delta);
                        StreamVariant::Assistant(string_delta.clone())
                    }
                    (None, Some(reason)) => {
                        trace!("Got stop event from OpenAI: {:?}", reason);
                        match reason {
                            async_openai::types::FinishReason::Stop => {
                                trace!("Stopping stream.");
                                StreamVariant::StreamEnd("Generation complete".to_string())
                            }
                            async_openai::types::FinishReason::Length => {
                                info!("Stopping stream due to reaching max tokens.");
                                StreamVariant::StreamEnd("Reached max tokens".to_string())
                            }
                            async_openai::types::FinishReason::ContentFilter => {
                                info!("Stopping stream due to content filter.");
                                StreamVariant::StreamEnd("Content filter triggered".to_string())
                            }
                            async_openai::types::FinishReason::FunctionCall | async_openai::types::FinishReason::ToolCalls => {
                                warn!("Stopping stream due to function call or tool calls. This should not happen, as it isn't implemented yet.");
                                StreamVariant::StreamEnd("Function call or tool calls not yet implemented".to_string())
                            }
                        }
                    }
                    (None, None) => {
                        warn!("No content found in response and no reason to stop given: {:?}", response);
                        StreamVariant::StreamEnd("No content found in response and no reason to stop given.".to_string())
                    }
                }
            } else {
                debug!("No response found, ending stream.");
                StreamVariant::OpenAIError("No response found.".to_string())
            },
            Err(e) => {
                // If we can't get the response, we'll return a generic error.
                warn!("Error getting response: {:?}", e);
                StreamVariant::OpenAIError("Error getting response.".to_string())
            }
        };
        
        // Also add the variant into the active conversation
        add_to_conversation(&thread_id, variant.clone());

        variant
    });

    // Now we can transform the stream to a string stream that Actix can use.
    let string_stream = variant_stream
        .map(|v| match serde_json::to_string(&v) {
            Ok(string) => string,
            Err(e) => {
                warn!("Error converting StreamVariant to string with serde_json; falling back to debug representation: {:?}", e);
                format!("{:?}",StreamVariant::ServerError(format!("Error converting StreamVariant to string: {v:?}").to_string()))
            }
        })
        .map(|string| {
            Ok::<actix_web::web::Bytes, std::convert::Infallible>(
                // It requires a Result, so we'll wrap it in an Ok where the Error cannot happen.
                actix_web::web::Bytes::copy_from_slice(string.as_bytes()),
            )
        });

    HttpResponse::Ok().streaming(string_stream)
}
