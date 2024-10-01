use std::{collections::VecDeque, pin::Pin};

use actix_web::{HttpRequest, HttpResponse, Responder};
use async_openai::{
    error::OpenAIError,
    types::{
        ChatChoiceStream, ChatCompletionRequestMessage, ChatCompletionRequestUserMessage,
        CreateChatCompletionRequest, CreateChatCompletionRequestArgs,
        CreateChatCompletionStreamResponse, ChatCompletionToolChoiceOption,
    },
};
use documented::docs_const;
use futures::{stream, Stream, StreamExt};
use once_cell::sync::Lazy;
use tracing::{debug, error, info, trace, warn};

use crate::{
    chatbot::{
        available_chatbots::DEFAULTCHATBOT,
        handle_active_conversations::{
            add_to_conversation, conversation_state, end_conversation, get_conversation,
            new_conversation_id, save_and_remove_conversation,
        },
        prompting::{STARTING_PROMPT, STARTING_PROMPT_JSON},
        select_client,
        thread_storage::read_thread,
        types::{help_convert_sv_ccrm, ConversationState, StreamVariant},
    },
    tool_calls::{code_interpreter::verify_can_access, route_call::route_call, ALL_TOOLS},
};

/// # Stream Response
/// Takes in a thread_id, an input, a path to the freva config file and an auth_key and returns a stream of StreamVariants and their content.
///
/// The thread_id is the unique identifier for the thread, given to the client when the stream started in a ServerHint variant.
/// If it's empty or not given, a new thread is created.
///
/// The freva config file should be always set, as it's needed for the freva library to work.
///
/// The stream consists of StreamVariants and their content. See the different Stream Variants above.
/// If the stream creates a new thread, the new thread_id will be sent as a ServerHint.
/// The stream always ends with a StreamEnd event, unless a server error occurs.
///
/// A usual stream consists mostly of Assistant messages many times a second. This is to give the impression of a real-time conversation.
///
/// If the input is not given, a BadRequest response is returned.
///
/// If the auth_key is not given or does not match the one on the backend, an Unauthorized response is returned.
///
/// If the thread_id is blank but does not point to an existing thread, an InternalServerError response is returned.
///
/// If the stream fails due to something else on the backend, an InternalServerError response is returned.
#[docs_const]
pub async fn stream_response(req: HttpRequest) -> impl Responder {
    let qstring = qstring::QString::from(req.query_string());

    trace!("Query string: {:?}", qstring);

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

    debug!("Thread ID: {}, Input: {}", thread_id, input);

    // We also require the freva_config_path to be set. From the frontend, it's called "freva_config".
    let freva_config_path = match qstring
        .get("freva_config")
        .or_else(|| qstring.get("freva-config"))
    {
        // allow both freva_config and freva-config
        None | Some("") => {
            warn!("The User requested a stream without a freva_config path being set.");
            // // If the freva_config is not found, we'll return a 400
            // return HttpResponse::BadRequest().body(
            //     "Freva config not found. Please provide a freva_config in the query parameters.",
            // );

            // FIXME: remove this temporary fix
            "/work/ch1187/clint/freva-dev/freva/evaluation_system.conf".to_string()
        }
        Some(freva_config_path) => freva_config_path.to_string(),
    };

    if !verify_can_access(freva_config_path.clone()) {
        warn!("The User requested a stream with a freva_config path that cannot be accessed. Path: {}", freva_config_path);
        warn!("Because it is not set, any usage of the freva library will fail.");
    }

    info!(
        "Starting stream for thread {} with input: {}",
        thread_id, input
    );

    let messages = if create_new {
        // If the thread is new, we'll start with the base messages and the user's input.
        let mut base_message: Vec<ChatCompletionRequestMessage> = STARTING_PROMPT.clone();

        trace!("Adding base message to stream.");

        let starting_prompt = StreamVariant::Prompt((*STARTING_PROMPT_JSON).clone());
        add_to_conversation(&thread_id, vec![starting_prompt], freva_config_path.clone());

        let user_message = ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
            name: Some("user".to_string()),
            content: async_openai::types::ChatCompletionRequestUserMessageContent::Text(
                input.clone(),
            ),
        });
        base_message.push(user_message);
        base_message
    } else {
        // Don't create a new thread, but continue the existing one.
        debug!("Expecting there to be a file for thread_id {}", thread_id);
        let content = match read_thread(thread_id.as_str(), false) {
            Ok(content) => content,
            Err(e) => {
                // If we can't read the thread, we'll return a generic error.
                warn!("Error reading thread: {:?}", e);
                return HttpResponse::InternalServerError().body("Error reading thread.");
            }
        };

        // We have a Vec of StreamVariant, but we want a Vec of ChatCompletionRequestMessage.
        let mut past_messages = help_convert_sv_ccrm(content);
        let user_message = ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
            name: Some("user".to_string()),
            content: async_openai::types::ChatCompletionRequestUserMessageContent::Text(
                input.clone(),
            ),
        });

        // We also add the user's input to the past messages.
        past_messages.push(user_message);
        past_messages
    };

    // We'll also add a ServerHint about the thread_id to the messages.
    let server_hint = StreamVariant::ServerHint(format!("{{\"thread_id\": \"{thread_id}\"}}")); // resolves to {"thread_id": "<thread_id>"}

    // Also don't forget to add the user's input to the thread file.
    add_to_conversation(
        &thread_id,
        vec![server_hint, StreamVariant::User(input.clone())],
        freva_config_path.clone(),
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

    create_and_stream(request, thread_id, freva_config_path).await
}

/// A simple helper function to build the stream.
fn build_request(
    messages: Vec<ChatCompletionRequestMessage>,
) -> Result<CreateChatCompletionRequest, async_openai::error::OpenAIError> {
    // Because some errors occured around here, we'll log the messages.
    trace!("Messages sending to OpenAI: {:?}", messages);
    CreateChatCompletionRequestArgs::default()
        .model(String::from(DEFAULTCHATBOT))
        .n(1)
        .messages(messages)
        .stream(true)
        .max_tokens(16000u32)
        .tools(ALL_TOOLS.clone())
        .parallel_tool_calls(false) // No parallel tool calls!
        .frequency_penalty(0.1) // The chatbot sometimes repeats the empty string endlessly, so we'll try to prevent that.
        .tool_choice(ChatCompletionToolChoiceOption::Auto) // Explicitly set to auto, because the LLM should be free to choose the tool.
        .temperature(0.4) // The model shouldn't be too creative, but also not too boring.
        .build()
}

// The last event in the event. Should be sent if the stream is stopped by the client sending a stop request.
pub static STREAM_STOP_CONTENT: Lazy<actix_web::web::Bytes> = Lazy::new(|| {
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
/// Note that there will also be added events that don't come from the `OpenAI::Client`, like `ServerHint` events.
/// This is only possible due to using `Stream::unfold`, which allows the manual construction of the stream.
async fn create_and_stream(
    request: CreateChatCompletionRequest,
    thread_id: String,
    freva_config_path: String,
) -> actix_web::HttpResponse {
    let open_ai_stream = match select_client(DEFAULTCHATBOT)
        .await
        .chat()
        .create_stream(request)
        .await
    {
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
        move |(
            mut open_ai_stream,
            thread_id,
            should_stop,
            should_hint_thread_id,
            mut variant_queue,
            mut tool_name,
            mut tool_arguments,
            mut tool_id,
        )| {
            // It is required to clone the freva_config_path, because it is moved into the closure.
            let freva_config_path_clone = freva_config_path.clone();
            async move {
                // Even higher priority than stopping the stream is sending the thread_id hint.
                if should_hint_thread_id {
                    // If we should hint the thread_id, we'll send a ServerHint event.
                    let hint =
                        StreamVariant::ServerHint(format!("{{\"thread_id\": \"{thread_id}\"}}")); // resolves to {"thread_id":"<thread_id>"}
                                                                                                  // return the hint and the new state
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
                    save_and_remove_conversation(&thread_id);
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
                            Ok(STREAM_STOP_CONTENT.clone()),
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
                        ))
                    } else {
                        // If the client didn't send a stop request, we'll continue.

                        // gets the response from the OpenAI Stream
                        let response = open_ai_stream.next().await;

                        trace!("Polled Stream, got response: {:?}", response);

                        let variants: Vec<StreamVariant> = oai_stream_to_variants(
                            response,
                            &mut tool_name,
                            &mut tool_arguments,
                            &mut tool_id,
                            &thread_id,
                            &mut open_ai_stream,
                        )
                        .await;

                        // Also add the variants into the active conversation
                        add_to_conversation(
                            &thread_id,
                            variants.clone(),
                            freva_config_path_clone.clone(),
                        );

                        // Check whether the stream should end by checking the variants.
                        let should_end = variants
                            .iter()
                            .any(|v| matches!(v, StreamVariant::StreamEnd(_)));

                        // The variant to return if there are no variants in the response.
                        let error_variant = StreamVariant::ServerError(
                            "No variants found in response.".to_string(),
                        );

                        // Split the variants into the first variant and the rest of the variants.
                        // This is so we can send the first variant immediately and write the rest to the queue.
                        let mut variants: VecDeque<StreamVariant> = variants.into();
                        let first_variant = variants.pop_front().unwrap_or(error_variant);

                        let string_variant = match serde_json::to_string(&first_variant) {
                            Ok(string) => string,
                            Err(e) => {
                                error!("Error converting StreamVariant to string with serde_json; falling back to debug representation: {:?}", e);
                                format!(
                                    "{:?}",
                                    StreamVariant::ServerError(format!(
                                    "Error converting StreamVariant to string: {first_variant:?}"
                                ))
                                )
                            }
                        };

                        let bytes =
                            actix_web::web::Bytes::copy_from_slice(string_variant.as_bytes());

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
            }
        },
    );

    HttpResponse::Ok().streaming(out_stream)
}

/// Converts the response from the OpenAI stream into a vector of StreamVariants.
async fn oai_stream_to_variants(
    response: Option<
        Result<
            async_openai::types::CreateChatCompletionStreamResponse,
            async_openai::error::OpenAIError,
        >,
    >,
    tool_name: &mut Option<String>,
    tool_arguments: &mut String,
    tool_id: &mut String,
    thread_id: &String,
    open_ai_stream: &mut Pin<
        Box<dyn Stream<Item = Result<CreateChatCompletionStreamResponse, OpenAIError>> + Send>,
    >,
) -> Vec<StreamVariant> {
    match response {
        Some(Ok(response)) => {
            if let Some(choice) = response.choices.first() {
                // The choices represent the multiple completions that the LLM can make. We always set n=1, so there is exactly one choice.
                match (
                    &choice.delta.tool_calls,
                    &choice.delta.content,
                    choice.finish_reason,
                ) {
                    (None, Some(string_delta), _) => {
                        // Basic case: the Assistant sends a text delta.
                        trace!("Delta: {}", string_delta);
                        vec![StreamVariant::Assistant(string_delta.clone())]
                    }
                    (_, None, Some(reason)) => {
                        // The Assistant sends a stop event.
                        debug!("Got stop event from OpenAI: {:?}", reason);
                        handle_stop_event(
                            reason,
                            choice,
                            tool_arguments,
                            tool_name,
                            tool_id,
                            thread_id,
                            open_ai_stream,
                            &response,
                        )
                        .await
                    }
                    (Some(tool_calls), None, None) => {
                        // A tool was called. This can include partial completions of the tool call, "tool call deltas", like code fragments.
                        debug!(
                            "A tool was called, converting the delta to a Code variant: {:?}",
                            tool_calls
                        );
                        if tool_calls.len() > 1 {
                            warn!("Multiple tool calls found, but only one is supported. All are ignored except the first: {:?}", tool_calls);
                        }
                        match tool_calls.first() {
                            // This doesn't support multiple tool calls at once, but they are disabled in the request.
                            Some(tool_call) => {
                                // We now know that we are sending the delta of a tool call.
                                // For the user to see a stream of i.e. the code interpreter's code being written by the LLM, we need to send the code interpreter's code as a stream.
                                match &tool_call.function {
                                    Some(function) => {
                                        // Now we need to check what function was called. For now, we only have the code interpreter.
                                        let mut arguments =
                                            function.arguments.clone().unwrap_or(String::new());

                                        // Instead of just storing the arguments as-is, if the arguments contain no code yet, we'll ignore whitespace and newlines.
                                        // This will effectively trim the arguments.
                                        if arguments.trim().is_empty() {
                                            // Only set the arguments to the empty String, if no code was written yet.
                                            if tool_arguments.is_empty() {
                                                arguments = String::new();
                                            }
                                        }

                                        // Because of the genius way OpenAI constructed this very good API, the name of the tool call is only sent in the very first delta.
                                        // So if the name is not None, we store it in the tool_name variable that is passed to the next iteration of the stream.
                                        // If the name is None, we try to read the tool_name from the tool_name variable.
                                        if let Some(name) = function.name.clone() {
                                            debug!("New tool call started: {:?}", name);
                                            *tool_name = Some(name);
                                        }

                                        // Another things is that the arguments for the tool calls, even though they are strings, are not repeated when the actual tool call is made.
                                        // that means that I need to add another state to the closure to keep track of the tool arguments.
                                        tool_arguments.push_str(&arguments);

                                        // The same thing goes for the tool call id, which is neccessary to be matched later on in the response.
                                        match tool_call.id.clone() {
                                            Some(id) => {
                                                // We need to store the id in the tool_name variable, because the id is not repeated in the response.
                                                *tool_id = id;
                                            }
                                            None => {
                                                if tool_id.is_empty() {
                                                    warn!("Tool call expected id, but not found in response: {:?}", response);
                                                }
                                            }
                                        }

                                        let name_copy = tool_name.clone(); // because tool_name will be used at the end to pass the tool name to the next iteration of the stream, we need to clone it here.
                                        if name_copy != Some("code_interpreter".to_string()) {
                                            warn!("Tool call expected code_interpreter, but found: {:?}", name_copy);
                                            // Instead of ending the stream, we'll just ignore the tool call, but send the user a ServerHint.
                                            // Depending on the implementation of the OpenAI API, this might result in a unspecified Server Error on the LLM side.
                                            vec![StreamVariant::ServerHint(format!("{{\"warning\": \"Tool call expected code_interpreter, but found ->{}<-; content: ->{}<-\"}}", name_copy.unwrap_or_default(), arguments))]
                                        } else {
                                            // We know it's the code interpreter and can send it as a delta.
                                            trace!(
                                                "Tool call: {:?} with arguments: {:?} and id: {}",
                                                name_copy,
                                                arguments,
                                                tool_id
                                            );
                                            if tool_id.is_empty() {
                                                warn!(
                                                    "Tool call expected id, but not set yet: {:?}",
                                                    response
                                                );
                                            }
                                            vec![StreamVariant::Code(arguments, tool_id.clone())]
                                        }
                                    }
                                    None => {
                                        warn!("Tool call expected function, but not found in response: {:?}", response);
                                        vec![StreamVariant::CodeError("Tool call expected function, but not found in response.".to_string())]
                                    }
                                }
                            }
                            None => {
                                warn!(
                                    "Tool call expected, but not found in response: {:?}",
                                    response
                                );
                                vec![StreamVariant::CodeError(
                                    "Tool call expected, but not found in response.".to_string(),
                                )]
                            }
                        }
                    }
                    (None, None, None) => {
                        warn!("No content found in response and no reason to stop given; treating this as an empty Assistant response: {:?}", response);
                        vec![StreamVariant::Assistant(String::new())]
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
        }
    }
}

async fn handle_stop_event(
    reason: async_openai::types::FinishReason,
    choice: &ChatChoiceStream,
    tool_arguments: &mut String,
    tool_name: &mut Option<String>,
    tool_id: &mut String,
    thread_id: &String,
    open_ai_stream: &mut Pin<
        Box<dyn Stream<Item = Result<CreateChatCompletionStreamResponse, OpenAIError>> + Send>,
    >,
    response: &CreateChatCompletionStreamResponse,
) -> Vec<StreamVariant> {
    match reason {
        async_openai::types::FinishReason::Stop => {
            debug!("Stopping stream due to successfull end of generation.");
            vec![StreamVariant::StreamEnd("Generation complete".to_string())]
        }
        async_openai::types::FinishReason::Length => {
            info!("Stopping stream due to reaching max tokens.");
            vec![StreamVariant::StreamEnd("Reached max tokens".to_string())]
        }
        async_openai::types::FinishReason::ContentFilter => {
            info!("Stopping stream due to content filter.");
            vec![StreamVariant::StreamEnd(
                "Content filter triggered".to_string(),
            )]
        }
        async_openai::types::FinishReason::FunctionCall => {
            warn!("Stopping stream due to function call. This should not happen, as it it's deprecated and the LLM was instructed not to use them.");
            vec![StreamVariant::StreamEnd(
                "Function call is deprecated, LLM should use Tool call instead.".to_string(),
            )]
        }
        async_openai::types::FinishReason::ToolCalls => {
            // We expect there to now be a tool call in the response.

            if let Some(content) = choice.delta.tool_calls.clone() {
                // Handle the tool call
                trace!("Tool call: {:?}", content);
            }

            let mut all_generated_variants = vec![];

            // There is NOT a tool call there, because that was accumulated in the previous iterations.
            // The stream ending is just OpenAI's way of telling us that the tool call is done and can now be executed.
            if let Some(name) = tool_name {
                let mut temp = route_call(
                    name.to_string(),
                    Some(tool_arguments.to_string()),
                    tool_id.to_string(),
                    thread_id.to_string(),
                ); // call the tool with the arguments
                all_generated_variants.append(&mut temp);
                // Reset the tool_name and tool_arguments
                *tool_name = None;
                *tool_arguments = String::new();
                *tool_id = String::new();
            } else {
                warn!(
                    "Tool call expected, but not found in response: {:?}",
                    response
                );
                all_generated_variants.push(StreamVariant::CodeError(
                    "Tool call expected, but not found in response.".to_string(),
                ));
            }

            // Before we can return the generated variants, we need to start a new steam because the old one is done.
            // We need a list of all messages, which we can get from the active conversation global variable.
            match get_conversation(thread_id) {
                None => {
                    error!("Tried to restart conversation after tool call, but failed! No active conversation found with thread_id: {}", thread_id);
                    vec![StreamVariant::ServerError("Tried to restart conversation after tool call, but failed! No active conversation found.".to_string())]
                }
                Some(messages) => {
                    // the actual messages we need to put there are those plus the generated ones, because the generated one were not added to the conversation yet.
                    let mut all_messages = messages.clone();
                    all_messages.append(&mut all_generated_variants.clone());

                    trace!(
                        "Restarting conversation after tool call with messages: {:?}",
                        all_messages
                    );

                    // The stream wants a vector of ChatCompletionRequestMessage, so we need to convert the StreamVariants to that.
                    let all_oai_messages = help_convert_sv_ccrm(all_messages);

                    trace!("All messages: {:?}", all_oai_messages);

                    // Now we construct a new stream and substitute the old one with it.
                    match build_request(all_oai_messages) {
                        Err(e) => {
                            // If we can't build the request, we'll return a generic error.
                            warn!("Error building request: {:?}", e);
                            vec![StreamVariant::ServerError(
                                "Error building request.".to_string(),
                            )]
                        }
                        Ok(request) => {
                            trace!("Request built successfully: {:?}", request);
                            match select_client(DEFAULTCHATBOT)
                                .await
                                .chat()
                                .create_stream(request)
                                .await
                            {
                                Err(e) => {
                                    // If we can't create the stream, we'll return a generic error.
                                    warn!("Error creating stream: {:?}", e);
                                    vec![StreamVariant::ServerError(
                                        "Error creating new stream.".to_string(),
                                    )]
                                }
                                Ok(stream) => {
                                    // Everything worked, so we'll return the new stream and the new state.
                                    *open_ai_stream = stream;
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
