use actix_web::{HttpRequest, HttpResponse, Responder};
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
    CreateChatCompletionRequestArgs,
};
use futures::{stream, StreamExt};
use once_cell::sync::Lazy;
use tracing::{debug, error, info, trace, warn};

use crate::chatbot::{
    available_chatbots::DEFAULTCHATBOT,
    handle_active_conversations::{
        add_to_conversation, conversation_state, end_conversation, new_conversation_id,
        remove_conversation,
    },
    prompting::{STARTING_PROMPT, STARTING_PROMPT_JSON},
    thread_storage::read_thread,
    types::{ConversationState, StreamVariant},
    CLIENT,
};

/// Takes in a thread_id and input from the query parameters and returns a stream of responses from the chatbot.
/// These are wrapped in a StreamVariant and sent to the client.
pub async fn stream_response(req: HttpRequest) -> impl Responder {
    // Try to get the thread ID and input from the request's query parameters.
    let qstring = qstring::QString::from(req.query_string());
    let (thread_id, create_new) = match qstring.get("thread_id") {
        None => {
            // If the thread ID is not found, we'll return a 400
            warn!("The User requested a stream without a thread ID.");
            return HttpResponse::BadRequest()
                .body("Thread ID not found. Please provide a thread_id in the query parameters. Leave it empty to create a new thread.");
        }
        Some("") => {
            // If the thread ID is empty, we'll create a new thread.
            debug!("Creating a new thread.");
            (new_conversation_id(), true)
        }
        Some(thread_id) => (thread_id.to_string(), false),
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

    let messages = if create_new {
        // If the thread is new, we'll start with the base messages and the user's input.
        let mut base_message: Vec<ChatCompletionRequestMessage> = STARTING_PROMPT.clone();

        trace!("Adding base message to stream.");

        let variant_vec = StreamVariant::Prompt((*STARTING_PROMPT_JSON).clone()); // This is a bit hacky, but it works. (We just dump the base messages into a string.)
        add_to_conversation(&thread_id, vec![variant_vec]);

        let user_message = ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
            name: Some("user".to_string()),
            content: async_openai::types::ChatCompletionRequestUserMessageContent::Text(
                input.clone(),
            ),
        });
        base_message.push(user_message);
        base_message
    } else {
        debug!("Expecting there to be a file for thread_id {}", thread_id);
        let content = match read_thread(thread_id.as_str()) {
            Ok(content) => content,
            Err(e) => {
                // If we can't read the thread, we'll return a generic error.
                warn!("Error reading thread: {:?}", e);
                return HttpResponse::InternalServerError().body("Error reading thread.");
            }
        };

        // We have a Vec of StreamVariant, but we want a Vec of ChatCompletionRequestMessage.
        content
            .iter()
            .map(|e| TryInto::<Vec<ChatCompletionRequestMessage>>::try_into(e.clone())) // A single Stream variant, like prompt, might turn into multiple messages.
            .filter_map(std::result::Result::ok)
            .flatten()
            .chain(std::iter::once(ChatCompletionRequestMessage::User(
                // Add the user's input to the stream so that the chatbot can respond to it.
                ChatCompletionRequestUserMessage {
                    name: Some("user".to_string()),
                    content: async_openai::types::ChatCompletionRequestUserMessageContent::Text(
                        input.clone(),
                    ),
                },
            )))

            .collect::<Vec<_>>()
    };

    // Also don't forget to add the user's input to the thread file.
    add_to_conversation(&thread_id, vec![StreamVariant::User(input.clone())]);
    

    let request: CreateChatCompletionRequest = match CreateChatCompletionRequestArgs::default()
        .model(String::from(DEFAULTCHATBOT))
        .n(1)
        // .prompt(input) // This isn't used for the chat API
        .messages(messages)
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

// The last event in the event. Should be sent if the stream is stopped by the client sending a stop request.
static STREAM_STOP_CONTENT: Lazy<actix_web::web::Bytes> = Lazy::new(|| {
    actix_web::web::Bytes::copy_from_slice(
        serde_json::to_string(&StreamVariant::StreamEnd(
            "Conversation aborted".to_string(),
        ))
        .expect("const Stream Variant unable to be converted to actix bytes!")
        .as_bytes(),
    )
});

/// First creates a stream from the `OpenAI` client.
/// Then transforms the Stream from the `OpenAI` client into a Stream for Actix.
/// Note that there will also be added events that don't come from the `OpenAI::Client`, like `ClientHint` events.
/// This is only possible due to using `Stream::unfold`.
async fn create_and_stream(
    request: CreateChatCompletionRequest,
    thread_id: String,
) -> actix_web::HttpResponse {
    let open_ai_stream = match CLIENT.chat().create_stream(request).await {
        Ok(stream) => stream,
        Err(e) => {
            // If we can't create the stream, we'll return a generic error.
            warn!("Error creating stream: {:?}", e);
            return HttpResponse::InternalServerError().body("Error creating stream.");
        }
    };

    trace!("Stream created!");
    let out_stream = stream::unfold(
        (open_ai_stream, thread_id, false),
        |(mut open_ai_stream, thread_id, should_stop)| async move {
            if should_stop {
                // If the stream should stop, we'll simply return None.
                // We do it in this order to be able to send one last event to the client signaling the end of the stream.
                trace!("Stream is stopping, sent one last event, removing the conversation from the pool and then aborting stream.");
                remove_conversation(&thread_id);
                None
            } else {
                // If the stream should not stop, we'll continue.

                // First checks whether it should stop the stream. (This happens if the client sent a stop request.)
                if matches!(
                    conversation_state(&thread_id),
                    Some(ConversationState::Stopping)
                ) {
                    debug!("Conversation with thread_id {} has been stopped, sending one last event and then aborting stream.", thread_id);
                    // We need to signal the end of the stream, so we'll have to tell actix to send one last StreamEnd event.
                    end_conversation(&thread_id);
                    Some((
                        Ok::<actix_web::web::Bytes, std::convert::Infallible>(
                            STREAM_STOP_CONTENT.clone(),
                        ),
                        (open_ai_stream, thread_id, true),
                    )) // The type annotation is necessary because the actix web HttpResponse streaming wants a Result.
                } else {
                    // If the client didn't send a stop request, we'll continue.

                    // gets the response from the OpenAI Stream
                    let response = open_ai_stream.next().await;

                    let variant = match response {
                        Some(Ok(response)) => {
                            if let Some(choice) = response.choices.first() {
                                match (&choice.delta.content, choice.finish_reason) {
                                    (Some(string_delta), _) => {
                                        trace!("Delta: {}", string_delta);
                                        StreamVariant::Assistant(string_delta.clone())
                                    }
                                    (None, Some(reason)) => {
                                        trace!("Got stop event from OpenAI: {:?}", reason);
                                        match reason {
                                            async_openai::types::FinishReason::Stop => {
                                                trace!("Stopping stream due to successfull end of generation.");
                                                StreamVariant::StreamEnd(
                                                    "Generation complete".to_string(),
                                                )
                                            }
                                            async_openai::types::FinishReason::Length => {
                                                info!(
                                                    "Stopping stream due to reaching max tokens."
                                                );
                                                StreamVariant::StreamEnd(
                                                    "Reached max tokens".to_string(),
                                                )
                                            }
                                            async_openai::types::FinishReason::ContentFilter => {
                                                info!("Stopping stream due to content filter.");
                                                StreamVariant::StreamEnd(
                                                    "Content filter triggered".to_string(),
                                                )
                                            }
                                            async_openai::types::FinishReason::FunctionCall
                                            | async_openai::types::FinishReason::ToolCalls => {
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
                            }
                        }
                        Some(Err(e)) => {
                            // If we can't get the response, we'll return a generic error.
                            warn!("Error getting response: {:?}", e);
                            StreamVariant::OpenAIError("Error getting response.".to_string())
                        }
                        None => {
                            warn!("Stream ended abruptly and without error. This should not happen; returning StreamEnd.");
                            StreamVariant::StreamEnd("Stream ended abruptly".to_string())

                            // This does not mean that the stream ended abruptly, I misunderstood.
                            // It happends when the stream is done on the OpenAI side.
                            // We need to detect when a StreamEnd was sent by OpenAI and then stop the stream.
                        }
                    };

                    // Also add the variant into the active conversation
                    add_to_conversation(&thread_id, vec![variant.clone()]);

                    // Check whether the stream should end by checking the variant.
                    let should_end = matches!(variant, StreamVariant::StreamEnd(_));

                    // Transform to string and then to actix_web::Bytes

                    let string_variant = match serde_json::to_string(&variant) {
                        Ok(string) => string,
                        Err(e) => {
                            warn!("Error converting StreamVariant to string with serde_json; falling back to debug representation: {:?}", e);
                            format!(
                                "{:?}",
                                StreamVariant::ServerError(format!(
                                    "Error converting StreamVariant to string: {variant:?}"
                                ))
                            )
                        }
                    };

                    let bytes = actix_web::web::Bytes::copy_from_slice(string_variant.as_bytes());

                    // Everything worked, so we'll return the bytes and the new state.
                    Some((Ok(bytes), (open_ai_stream, thread_id, should_end))) // Ends if the variant is a StreamEnd
                }
            }
        },
    );

    HttpResponse::Ok().streaming(out_stream)
}
