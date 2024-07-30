use std::future;

use actix_web::{HttpRequest, HttpResponse, Responder};
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, CreateChatCompletionRequest, CreateChatCompletionRequestArgs
};
use futures::StreamExt;
use tracing::{debug, info, trace, warn};

use crate::chatbot::{available_chatbots::DEFAULTCHATBOT, handle_active_conversations::{add_to_conversation, conversation_state, end_conversation, new_conversation_id, remove_conversation}, prompting::STARTING_MESSAGES, thread_storage::read_thread, types::{ConversationState, StreamVariant}, CLIENT};

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
        let mut base_message: Vec<ChatCompletionRequestMessage> = STARTING_MESSAGES.clone();
        let user_message = ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage{
            name: Some("user".to_string()),
            content: async_openai::types::ChatCompletionRequestUserMessageContent::Text(input.clone()),
        });
        base_message.push(user_message);
        base_message
    } else {
        debug!("Expecting there to be a file for thread_id {}", thread_id);
        let content = match read_thread(thread_id.as_str()){
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
            .map(|e| StreamVariant::try_into(e.clone()))
            .filter_map(std::result::Result::ok)
            .collect::<Vec<_>>()
    };

    // For testing, a basic request
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

        
        (variant, thread_id.clone())
    });

    // Stopping logic:
    // The stop REST API point will set a conversation to stopping.
    // The stopped_guard will check if the conversation is Ended and stop the stream if it is.
    // The stream_end_guard will check if the conversation is stopping and if it is, set it to Ended and replace the current variant with a StreamEnd.
    // This ensures that as soon as the conversation is stopped, the next variant will be a StreamEnd and there will be no more variants.
    
    // Checks whether the conversation has been stopped and ends the stream if it has.
    let stopped_guard = variant_stream.take_while(|(_, thread_id)| {
        // future::ready(!crate::chatbot::handle_active_conversations::conversation_stopped(thread_id.as_str()))

        // the thread_id is gotten from the outer scope, where I copied it before moving the original into the closure.
        let thread_stopped = matches!(conversation_state(thread_id.as_str()), Some(ConversationState::Ended(_))); // If the conversation state can be gotten and is Ended, the thread is stopped.
        if thread_stopped {
            debug!("Conversation with thread_id {} has been stopped, aborting stream.", thread_id);

            // Also remove the conversation from the active conversations, writing it to disk.
            remove_conversation(thread_id.as_str());
        }

        future::ready(!thread_stopped)
    });

    // Now we set the stream item to a StreamEnd if the conversation has been stopped.
    let stream_end_guard = stopped_guard.map(|(v, thread_id)| {
        if matches!(conversation_state(thread_id.as_str()), Some(ConversationState::Stopping)) { // If the conversation state can be gotten and is Stopping, the thread is stopping.
            debug!("Conversation with thread_id {} has been stopped, setting the next variant to StreamEnd.", thread_id);
            end_conversation(thread_id.as_str());

            trace!("Stopping stream, overwriting variant {:?} with StreamEnd.", v);
            StreamVariant::StreamEnd("Conversation stopped".to_string())
        } else {
            v
        }
    });

    // Now we can transform the stream to a string stream that Actix can use.
    let string_stream = stream_end_guard
        .map(|v| match serde_json::to_string(&v) {
            Ok(string) => string,
            Err(e) => {
                warn!("Error converting StreamVariant to string with serde_json; falling back to debug representation: {:?}", e);
                format!("{:?}",StreamVariant::ServerError(format!("Error converting StreamVariant to string: {v:?}")))
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