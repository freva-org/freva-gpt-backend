use actix_web::{HttpRequest, HttpResponse, Responder};
use async_openai::types::{ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs};
use futures::StreamExt;
use tracing::{trace, warn};

use crate::chatbot::{available_chatbots::DEFAULTCHATBOT, CLIENT};

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
        Some(thread_id) => thread_id,
    };

    let input = match qstring.get("input") {
        None | Some("") => {
            // If the input is not found, we'll return a 400
            warn!("The User requested a stream without an input.");
            return HttpResponse::BadRequest().body(
                "Input not found. Please provide a non-empty input in the query parameters.",
            );
        }
        Some(input) => input,
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
    let request = match CreateChatCompletionRequestArgs::default()
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
    // For now, it'll be a simple string stream.

    let string_stream = stream.map(|response| match response {
        Ok(response) => match response.choices.first() {
            // The reponse contains a list of choices for the next word, we only care about the first one.
            Some(choice) => {
                // let delta = choice.delta;
                // trace!("Delta: {}", delta);
                // Ok(actix_web::web::Bytes::copy_from_slice(delta.as_bytes())) // Actix wants the stream in this exact format.
                match (&choice.delta.content, choice.finish_reason) {
                    (Some(string_delta), _) => {
                        trace!("Delta: {}", string_delta);
                        Ok(actix_web::web::Bytes::copy_from_slice(
                            string_delta.as_bytes(),
                        )) // Actix wants the stream in this exact format.
                    }
                    (None, Some(reason)) => {
                        trace!("Got stop event from OpenAI: {:?}", reason);
                        match reason {
                            async_openai::types::FinishReason::Stop => {
                                trace!("Stopping stream.");
                                Err("Stop event received.".to_string())
                            }
                            _ => {
                                warn!("Unknown finish reason: {:?}", reason);
                                Err("Unknown finish reason.".to_string())
                            }
                        }
                    }
                    (None, None) => {
                        warn!("No content found in response and no reason to stop given: {:?}", response);
                        Err("No content found in response and no reason to stop given.".to_string())
                    }
                }
            }
            None => {
                trace!("No response found, ending stream.");
                Err("No response found.".to_string())
            }
        },
        Err(e) => {
            // If we can't get the response, we'll return a generic error.
            warn!("Error getting response: {:?}", e);
            Err("Error getting response.".to_string())
        }
    });

    HttpResponse::Ok().streaming(string_stream)
}
