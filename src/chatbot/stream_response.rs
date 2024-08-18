use std::collections::VecDeque;

use actix_web::{HttpRequest, HttpResponse, Responder};
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
    CreateChatCompletionRequestArgs,
};
use futures::{stream, StreamExt};
use once_cell::sync::Lazy;
use tracing::{debug, error, info, trace, warn};

use crate::{
    chatbot::{
        available_chatbots::DEFAULTCHATBOT,
        handle_active_conversations::{
            add_to_conversation, conversation_state, end_conversation, get_conversation,
            new_conversation_id, remove_conversation,
        },
        prompting::{STARTING_PROMPT, STARTING_PROMPT_JSON},
        thread_storage::read_thread,
        types::{help_convert_sv_ccrm, ConversationState, StreamVariant},
        CLIENT,
    },
    tool_calls::{route_call::route_call, ALL_TOOLS},
};

/// Takes in a thread_id, an input and an auth_key and returns a stream of StreamVariants and their content.
///
/// The thread_id is the unique identifier for the thread, given to the client when the stream started in a ServerHint variant.
/// If it's empty or not given, a new thread is created.
///
/// The stream consists of StreamVariants and their content. See the different Stream Variants above.
/// If the stream creates a new thread, the new thread_id will be sent as a ServerHint.
/// The stream always ends with a StreamEnd event, unless a server error occurs.
///
/// A usual stream cosists mostly of Assistant messages many times a second. This is to give the impression of a real-time conversation.
///
/// If the input is not given, a BadRequest response is returned.
///
/// If the auth_key is not given or does not match the one on the backend, an Unauthorized response is returned.
///
/// If the thread_id does not point to an existing thread, an InternalServerError response is returned.
///
/// If the stream fails due to something else on the backend, an InternalServerError response is returned.
///
pub async fn stream_response(req: HttpRequest) -> impl Responder {
    let qstring = qstring::QString::from(req.query_string());

    // First try to authorize the user.
    crate::auth::authorize_or_fail!(qstring);

    // Try to get the thread ID and input from the request's query parameters.
    let (thread_id, create_new) = match qstring.get("thread_id") {
        None | Some("") => {
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

    // We'll also add a ServerHint about the thread_id to the messages.
    let server_hint = StreamVariant::ServerHint(format!("{{\"thread_id\": \"{}\"}}", thread_id)); // resolves to {"thread_id": "<thread_id>"}
    // Also don't forget to add the user's input to the thread file.
    add_to_conversation(
        &thread_id,
        vec![server_hint, StreamVariant::User(input.clone())],
    );

    let request: CreateChatCompletionRequest = match build_request(messages) {
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

/// A simple helper function to build the stream.
fn build_request(
    messages: Vec<ChatCompletionRequestMessage>,
) -> Result<CreateChatCompletionRequest, async_openai::error::OpenAIError> {
    CreateChatCompletionRequestArgs::default()
        .model(String::from(DEFAULTCHATBOT))
        .n(1)
        .messages(messages)
        .stream(true)
        .max_tokens(500u32)
        .tools(ALL_TOOLS.clone())
        .build()
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
        (
            open_ai_stream, // the stream from the OpenAI client
            thread_id,
            false,           // whether the stream should stop
            true,            // whether the stream should hint the thread_id
            VecDeque::new(), // the queue of variants to send
            None,            // The tool name, if it was called
            String::new(),   // the tool arguments,
            String::new(),   // the tool id
        ),
        |(
            mut open_ai_stream,
            thread_id,
            should_stop,
            should_hint_thread_id,
            mut variant_queue,
            mut tool_name,
            mut tool_arguments,
            mut tool_id,
        )| async move {
            // Even higher priority than stopping the stream is sending the thread_id hint.
            if should_hint_thread_id {
                // If we should hint the thread_id, we'll send a ServerHint event.
                let hint = StreamVariant::ServerHint(format!("{{\"thread_id\": \"{}\"}}", thread_id)); // resolves to {"thread_id":"<thread_id>"}
                return Some((
                    Ok::<actix_web::web::Bytes, std::convert::Infallible>(
                        actix_web::web::Bytes::copy_from_slice(
                            serde_json::to_string(&hint)
                                .expect("ServerHint unable to be converted to actix bytes!")
                                .as_bytes(),
                        ),
                    ),
                    (
                        open_ai_stream,
                        thread_id,
                        should_stop,
                        false,
                        variant_queue,
                        tool_name,
                        tool_arguments,
                        tool_id,
                    ),
                ));
            }

            // After potentially sending a thread_id hint, but before stopping, check whether the variants queue contains something; if so, send it.
            if let Some(content) = variant_queue.pop_front() {
                let string_variant = match serde_json::to_string(&content) {
                    Ok(string) => string,
                    Err(e) => {
                        warn!("Error converting StreamVariant to string with serde_json; falling back to debug representation: {:?}", e);
                        format!(
                            "{:?}",
                            StreamVariant::ServerError(format!(
                                "Error converting StreamVariant to string: {content:?}"
                            ))
                        )
                    }
                };

                let bytes = actix_web::web::Bytes::copy_from_slice(string_variant.as_bytes());

                // Everything worked, so we'll return the bytes and the new state.
                Some((
                    Ok(bytes),
                    (
                        open_ai_stream,
                        thread_id,
                        should_stop,
                        false,
                        variant_queue,
                        tool_name,
                        tool_arguments,
                        tool_id,
                    ),
                ))
            } else if should_stop {
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
                        (
                            open_ai_stream,
                            thread_id,
                            true,
                            false,
                            variant_queue,
                            tool_name,
                            tool_arguments,
                            tool_id,
                        ),
                    )) // The type annotation is necessary because the actix web HttpResponse streaming wants a Result.
                } else {
                    // If the client didn't send a stop request, we'll continue.

                    // gets the response from the OpenAI Stream
                    let response = open_ai_stream.next().await;

                    trace!("Polled Stream, got response: {:?}", response);

                    let variants: Vec<StreamVariant> = match response {
                        Some(Ok(response)) => {
                            if let Some(choice) = response.choices.first() {
                                match (
                                    &choice.delta.tool_calls,
                                    &choice.delta.content,
                                    choice.finish_reason,
                                ) {
                                    (None, Some(string_delta), _) => {
                                        trace!("Delta: {}", string_delta);
                                        vec![StreamVariant::Assistant(string_delta.clone())]
                                    }
                                    (_, None, Some(reason)) => {
                                        trace!("Got stop event from OpenAI: {:?}", reason);
                                        match reason {
                                            async_openai::types::FinishReason::Stop => {
                                                trace!("Stopping stream due to successfull end of generation.");
                                                vec![StreamVariant::StreamEnd(
                                                    "Generation complete".to_string(),
                                                )]
                                            }
                                            async_openai::types::FinishReason::Length => {
                                                info!(
                                                    "Stopping stream due to reaching max tokens."
                                                );
                                                vec![StreamVariant::StreamEnd(
                                                    "Reached max tokens".to_string(),
                                                )]
                                            }
                                            async_openai::types::FinishReason::ContentFilter => {
                                                info!("Stopping stream due to content filter.");
                                                vec![StreamVariant::StreamEnd(
                                                    "Content filter triggered".to_string(),
                                                )]
                                            }
                                            async_openai::types::FinishReason::FunctionCall => {
                                                warn!("Stopping stream due to function call. This should not happen, as it it's deprecated and the LLM was instructed not to use them.");
                                                vec![StreamVariant::StreamEnd("Function call is deprecated, LLM should use Tool call instead.".to_string())]
                                            }
                                            async_openai::types::FinishReason::ToolCalls => {
                                                // We expect there to now be a tool call in the response.
                                                let temp = choice.delta.tool_calls.clone();

                                                if let Some(content) = temp {
                                                    // Handle the tool call
                                                    trace!("Tool call: {:?}", content);
                                                }

                                                let mut all_generated_variants = vec![];

                                                // There is NOT a tool call there, because that was accumulated in the previous iterations.
                                                // The stream ending is just OpenAI's way of telling us that the tool call is done and can now be executed.
                                                if let Some(name) = tool_name {
                                                    let mut temp = route_call(
                                                        name,
                                                        Some(tool_arguments),
                                                        tool_id,
                                                    ); // call the tool with the arguments
                                                    all_generated_variants.append(&mut temp);
                                                    // Reset the tool_name and tool_arguments
                                                    tool_name = None;
                                                    tool_arguments = String::new();
                                                    tool_id = String::new();
                                                } else {
                                                    warn!("Tool call expected, but not found in response: {:?}", response);
                                                    all_generated_variants.push(StreamVariant::CodeError("Tool call expected, but not found in response.".to_string()));
                                                }

                                                // Before we can return the generated variants, we need to start a new steam because the old one is done.
                                                // We need a list of all messages, which we can get from the active conversation global variable.
                                                match get_conversation(&thread_id) {
                                                    None => {
                                                        error!("Tried to restart conversation after tool call, but failed! No active conversation found with thread_id: {}", thread_id);
                                                        vec![StreamVariant::ServerError("Tried to restart conversation after tool call, but failed! No active conversation found.".to_string())]
                                                    }
                                                    Some(messages) => {
                                                        trace!("Restarting conversation after tool call with messages: {:?}", messages);
                                                        // the actual messages we need to put there are those plus the generated ones, because the generated one were not added to the conversation yet.
                                                        let mut all_messages = messages.clone();
                                                        all_messages.append(
                                                            &mut all_generated_variants.clone(),
                                                        );

                                                        // The stream wants a vector of ChatCompletionRequestMessage, so we need to convert the StreamVariants to that.
                                                        let all_oai_messages = help_convert_sv_ccrm(all_messages);

                                                        trace!(
                                                            "All messages: {:?}",
                                                            all_oai_messages
                                                        );

                                                        // Now we construct a new stream and substitute the old one with it.
                                                        match build_request(all_oai_messages) {
                                                            Err(e) => {
                                                                // If we can't build the request, we'll return a generic error.
                                                                warn!(
                                                                    "Error building request: {:?}",
                                                                    e
                                                                );
                                                                vec![StreamVariant::ServerError(
                                                                    "Error building request."
                                                                        .to_string(),
                                                                )]
                                                            }
                                                            Ok(request) => {
                                                                trace!("Request built successfully: {:?}", request);
                                                                match CLIENT
                                                                    .chat()
                                                                    .create_stream(request)
                                                                    .await
                                                                {
                                                                    Err(e) => {
                                                                        // If we can't create the stream, we'll return a generic error.
                                                                        warn!("Error creating stream: {:?}", e);
                                                                        vec![StreamVariant::ServerError("Error creating new stream.".to_string())]
                                                                    }
                                                                    Ok(stream) => {
                                                                        // Everything worked, so we'll return the new stream and the new state.
                                                                        open_ai_stream = stream;
                                                                        all_generated_variants
                                                                        // we need to return the generated variants, because the stream will be restarted with the tool call.
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    (Some(tool_calls), None, None) => {
                                        debug!("A tool was called, converting the delta to a Code variant: {:?}", tool_calls);
                                        if tool_calls.len() > 1 {
                                            warn!("Multiple tool calls found, but only one is supported. All are ignored except the first: {:?}", tool_calls);
                                        }
                                        match tool_calls.first() {
                                            // TODO: This doesn't support multiple tool calls at once yet.
                                            Some(tool_call) => {
                                                // We now know that we are sending the delta of a tool call.
                                                // For the user to see a stream of i.e. the code interpreter's code being written by the LLM, we need to send the code interpreter's code as a stream.
                                                match &tool_call.function {
                                                    Some(function) => {
                                                        // Now we need to check what function was called. For now, we only have the code interpreter.
                                                        let arguments = function
                                                            .arguments
                                                            .clone()
                                                            .unwrap_or("".to_string());

                                                        // Because of the genius way OpenAI constructed this very good API, the name of the tool call is only sent in the very first delta.
                                                        // So if the name is not None, we store it in the tool_name variable that is passed to the next iteration of the stream.
                                                        // If the name is None, we try to read the tool_name from the tool_name variable.
                                                        if let Some(name) = function.name.clone() {
                                                            debug!(
                                                                "New tool call started: {:?}",
                                                                name
                                                            );
                                                            tool_name = Some(name.clone());
                                                        }

                                                        // Another things is that the arguments for the tool calls, even though they are strings, are not repeated when the actual tool call is made.
                                                        // that means that I need to add another state to the closure to keep track of the tool arguments.
                                                        tool_arguments.push_str(&arguments);

                                                        // The same thing goes for the tool call id, which is neccessary to be machted later on in the response.
                                                        match tool_call.id.clone() {
                                                            Some(id) => {
                                                                // We need to store the id in the tool_name variable, because the id is not repeated in the response.
                                                                tool_id = id;
                                                            }
                                                            None => {
                                                                if tool_id.is_empty() {
                                                                    warn!("Tool call expected id, but not found in response: {:?}", response);
                                                                }
                                                            }
                                                        }

                                                        let name_copy = tool_name.clone(); // because tool_name will be used at the end to pass the tool name to the next iteration of the stream, we need to clone it here.
                                                        if name_copy
                                                            != Some("code_interpreter".to_string())
                                                        {
                                                            warn!("Tool call expected code_interpreter, but found: {:?}", name_copy);
                                                            // Instead of ending the stream, we'll just ignore the tool call, but send the user a ServerHint.
                                                            vec![StreamVariant::ServerHint(format!("{{\"warning\": \"Tool call expected code_interpreter, but found ->{}<-; content: ->{}<-\"}}", name_copy.unwrap_or(String::new()), arguments).to_string())]
                                                        } else {
                                                            // We know it's the code interpreter and can send it as a delta.
                                                            trace!("Tool call: {:?} with arguments: {:?} and id: {}", name_copy, arguments, tool_id);
                                                            if tool_id.is_empty() {
                                                                warn!("Tool call expected id, but not set yet: {:?}", response);
                                                            }
                                                            vec![StreamVariant::Code(
                                                                arguments,
                                                                tool_id.clone(),
                                                            )]
                                                        }
                                                    }
                                                    None => {
                                                        warn!("Tool call expected function, but not found in response: {:?}", response);
                                                        vec![StreamVariant::CodeError("Tool call expected function, but not found in response.".to_string())]
                                                    }
                                                }
                                            }
                                            None => {
                                                warn!("Tool call expected, but not found in response: {:?}", response);
                                                vec![StreamVariant::CodeError("Tool call expected, but not found in response.".to_string())]
                                            }
                                        }
                                    }
                                    (None, None, None) => {
                                        warn!("No content found in response and no reason to stop given; treating this as an empty Assistant response: {:?}", response);
                                        // vec![StreamVariant::StreamEnd("No content found in response and no reason to stop given.".to_string())]
                                        vec![StreamVariant::Assistant("".to_string())]
                                    }
                                    (Some(tool_calls), Some(string_delta), _) => {
                                        warn!("Tool call AND content found in response, the API specified that this couldn't happen: {:?} and {:?}", tool_calls, string_delta);
                                        vec![StreamVariant::StreamEnd("Tool call AND content found in response, the API specified that this couldn't happen.".to_string())]
                                    }
                                }
                            } else {
                                debug!("No response found, ending stream.");
                                vec![StreamVariant::OpenAIError("No response found.".to_string())]
                            }
                        }
                        Some(Err(e)) => {
                            // If we can't get the response, we'll return a generic error.
                            warn!("Error getting response: {:?}", e);
                            vec![StreamVariant::OpenAIError(
                                "Error getting response.".to_string(),
                            )]
                        }
                        None => {
                            warn!("Stream ended abruptly and without error. This should not happen; returning StreamEnd.");
                            vec![StreamVariant::StreamEnd(
                                "Stream ended abruptly".to_string(),
                            )]

                            // This does not mean that the stream ended abruptly, I misunderstood.
                            // It happends when the stream is done on the OpenAI side.
                            // We need to detect when a StreamEnd was sent by OpenAI and then stop the stream.
                        }
                    };

                    // Also add the variants into the active conversation
                    add_to_conversation(&thread_id, variants.clone());

                    // Check whether the stream should end by checking the variants.
                    // let should_end = matches!(variants, StreamVariant::StreamEnd(_));
                    let should_end = variants
                        .iter()
                        .any(|v| matches!(v, StreamVariant::StreamEnd(_)));

                    // Transform to string and then to actix_web::Bytes

                    // The variant to return if there are no variants in the response.
                    let error_variant =
                        StreamVariant::ServerError("No variants found in response.".to_string());

                    // Split the variants into the first variant and the rest of the variants.
                    let mut variants: VecDeque<StreamVariant> = variants.into();
                    let first_variant = variants.pop_front().unwrap_or(error_variant);

                    let string_variant = match serde_json::to_string(&first_variant) {
                        Ok(string) => string,
                        Err(e) => {
                            warn!("Error converting StreamVariant to string with serde_json; falling back to debug representation: {:?}", e);
                            format!(
                                "{:?}",
                                StreamVariant::ServerError(format!(
                                    "Error converting StreamVariant to string: {first_variant:?}"
                                ))
                            )
                        }
                    };

                    let bytes = actix_web::web::Bytes::copy_from_slice(string_variant.as_bytes());

                    // Everything worked, so we'll return the bytes and the new state.
                    Some((
                        Ok(bytes),
                        (
                            open_ai_stream,
                            thread_id,
                            should_end,
                            false,
                            variants,
                            tool_name,
                            tool_arguments,
                            tool_id,
                        ),
                    ))
                    // Ends if the variant is a StreamEnd
                }
            }
        },
    );

    HttpResponse::Ok().streaming(out_stream)
}
