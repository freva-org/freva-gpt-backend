use std::{cell::Cell, collections::VecDeque};

use actix_web::{web::Bytes, HttpRequest, HttpResponse, Responder};
use async_openai::types::{
    ChatChoiceStream, ChatCompletionMessageToolCallChunk, ChatCompletionRequestMessage,
    ChatCompletionRequestUserMessage, ChatCompletionResponseStream, ChatCompletionToolChoiceOption,
    ChatCompletionToolType, CreateChatCompletionRequest, CreateChatCompletionRequestArgs,
    CreateChatCompletionStreamResponse, FinishReason, FunctionCallStream,
};
use documented::docs_const;
use futures::{
    stream::{self, Fuse},
    StreamExt,
};
use once_cell::sync::Lazy;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, error, info, trace, warn};

use crate::{
    chatbot::{
        available_chatbots::DEFAULTCHATBOT,
        handle_active_conversations::{
            add_to_conversation, conversation_state, end_conversation, get_conversation,
            new_conversation_id, save_and_remove_conversation,
        },
        heartbeat::heartbeat_content,
        prompting::{STARTING_PROMPT, STARTING_PROMPT_JSON},
        select_client,
        thread_storage::read_thread,
        types::{help_convert_sv_ccrm, ConversationState, StreamVariant},
    },
    logging::{silence_logger, undo_silence_logger},
    tool_calls::{code_interpreter::verify_can_access, route_call::route_call, ALL_TOOLS},
};

use super::{available_chatbots::AvailableChatbots, handle_active_conversations::generate_id};

/// # Stream Response
/// Takes in a thread_id, an input, a path to the freva config file, an auth_key and a chatbot and returns a stream of StreamVariants and their content.
///
/// The thread_id is the unique identifier for the thread, given to the client when the stream started in a ServerHint variant.
/// If it's empty or not given, a new thread is created.
///
/// The freva config file should be always set, as it's needed for the freva library to work.
///
/// The chatbot parameter can be one of the possibilities as described in the /availablechatbots endpoint.
/// If it's not set, the default chatbot is used, which is the first one in the list.
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
/// If the thread_id is already being streamed, a Conflict response is returned.
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

    // Because the call to conversation_state writes a warning if the thread is not found, we'll temporarily silence the logging.
    silence_logger();
    let state = conversation_state(&thread_id);
    undo_silence_logger();

    // To avoid one thread being streamed more than once at the same time, we'll check if the thread is already being streamed.
    if let Some(state) = state {
        warn!("The User requested a stream for a thread that is already being streamed. Thread ID: {}", thread_id);
        info!("Conversation state: {:?}", state);
        // Just send an error to the client. A 409 Conflict is the most appropriate status code.
        return HttpResponse::Conflict().body(format!(
            "Thread {} is already being streamed. Please wait until it's done.",
            thread_id
        ));
    }

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

    // Set chatbot to the one the user requested or the default one.
    let chatbot = match qstring.get("chatbot") {
        None | Some("") => {
            debug!("Using default chatbot as user didn't supply one.");
            DEFAULTCHATBOT
        }
        Some(chatbot) => match String::try_into(chatbot.to_owned()) {
            Ok(chatbot) => chatbot,
            Err(e) => {
                warn!("Error converting chatbot to string, user requested chatbot that is not available: {:?}", e);
                return HttpResponse::BadRequest().body("Chatbot not found. Consult the /availablechatbots endpoint for available chatbots.");
            }
        },
    };

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
        let content = match read_thread(thread_id.as_str()) {
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

    let request: CreateChatCompletionRequest = match build_request(messages, chatbot) {
        Ok(request) => request,
        Err(e) => {
            // If we can't build the request, we'll return a generic error.
            warn!("Error building request: {:?}", e);
            return HttpResponse::InternalServerError().body("Error building request.");
        }
    };
    trace!("Request built!");

    create_and_stream(request, thread_id, freva_config_path, chatbot).await
}

/// A simple helper function to build the stream.
fn build_request(
    messages: Vec<ChatCompletionRequestMessage>,
    chatbot: AvailableChatbots,
) -> Result<CreateChatCompletionRequest, async_openai::error::OpenAIError> {
    // Because some errors occured around here, we'll log the messages.
    trace!("Messages sending to OpenAI: {:?}", messages);
    CreateChatCompletionRequestArgs::default()
        .model(String::from(chatbot))
        .n(1)
        .messages(messages)
        .stream(true)
        .max_tokens(16000u32)
        .tools(ALL_TOOLS.clone())
        .parallel_tool_calls(false) // No parallel tool calls!
        .frequency_penalty(0.1) // The chatbot sometimes repeats the empty string endlessly, so we'll try to prevent that.
        .tool_choice(ChatCompletionToolChoiceOption::Auto) // Explicitly set to auto, because the LLM should be free to choose the tool.
        .temperature(0.4) // The model shouldn't be too creative, but also not too boring.
        .stream_options(async_openai::types::ChatCompletionStreamOptions {
            include_usage: true,
        })
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
    chatbot: AvailableChatbots,
) -> actix_web::HttpResponse {
    let open_ai_stream = match select_client(chatbot)
        .await
        .chat()
        .create_stream(request)
        .await
    {
        Ok(stream) => stream.fuse(), // Fuse the stream so calling next() will return None after the stream ends instead of blocking.
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
            Cell::new(None), // the content of a llama tool call (See https://github.com/ollama/ollama/issues/5796 for why this needs to be done manually)
            None::<(mpsc::Receiver<Vec<StreamVariant>>, JoinHandle<()>)>, // the reciever for the tool call and the join handle for the tool call
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
            mut llama_tool_call_content,
            mut reciever,
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
                                    .unwrap_or_else(|e| {warn!("Error converting ServerHint to string: {:?}; falling back to byte ServerHint.", e);
                                    format!(r#"{{"variant":"ServerHint", "content":"{{\"thread_id\": \"{thread_id}\"}}"}}"#).to_owned()
                                })
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
                            llama_tool_call_content,
                            reciever,
                        ),
                    ));
                }

                // After potentially sending a thread_id hint, but before stopping, check whether the variants queue contains something; if so, send it.
                if let Some(content) = variant_queue.pop_front() {
                    let bytes = variant_to_bytes(content);

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
                            llama_tool_call_content,
                            reciever,
                        ),
                    ))
                } else if should_stop {
                    // If the stream should stop, we'll simply return None.

                    // However, the usage stats are contained after the stop event, so we'll poll the stream until it's completely stopped.
                    while let Some(content) = open_ai_stream.next().await {
                        if let Ok(response) = content {
                            if let Some(usage) = response.usage {
                                info!("Tokens used: {:?}; with chatbot: {:?}", usage, chatbot);
                            }
                        }
                    }

                    // In order to not do unnecessary work, we'll abort the tool call task if it's still running.
                    if let Some((_, handle)) = reciever {
                        debug!("Aborting tool call task.");
                        handle.abort();
                    }

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
                        add_to_conversation(
                            &thread_id,
                            vec![StreamVariant::StreamEnd("Conversation aborted".to_string())],
                            freva_config_path_clone,
                        );
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
                                llama_tool_call_content,
                                reciever,
                            ),
                        ))
                    } else {
                        // If the client didn't send a stop request, we'll continue.

                        // We have to check whether we have an active tool call.If so, the reviecer is not None.
                        // In that case, we shouldn't poll the stream, but instead wait for the tool call to finish.
                        // In the waiting, we'll return a heartbeat to the client.
                        if let Some((mut inner_reciever, handle)) = reciever {
                            // tokio::select! didn't seem to work when called on the reciever and sleep,
                            // So we'll sacrifice some efficiency and only check the reciever every 3 seconds.

                            //DEBUG
                            println!("Starting tool call reciever loop.");

                            // let state = inner_reciever.try_recv();
                            let state = tokio::time::timeout(
                                std::time::Duration::from_secs(5),
                                inner_reciever.recv(),
                            )
                            .await;
                            match state {
                                Err(_) => {
                                    trace!("Reciever has no data yet, sending timeout.");
                                    //DEBUG
                                    println!("Reciever has no data yet, sending timeout.");
                                    // Also add the heartbeat to the conversation.
                                    let heartbeat = heartbeat_content().await;
                                    trace!("Sending heartbeat: {:?}", heartbeat);
                                    add_to_conversation(
                                        &thread_id,
                                        vec![heartbeat.clone()],
                                        freva_config_path_clone.clone(),
                                    );
                                    // Actually sleep three seconds
                                    // std::thread::sleep(std::time::Duration::from_secs(3)); // Works
                                    // tokio::time::sleep(std::time::Duration::from_secs(3)).await; // Doesn't
                                    // tokio::time::delay_for(std::time::Duration::from_secs(3)).await; // Doesn't exist anymore
                                    // If the timeout expires, we'll send a heartbeat to the client.

                                    //DEBUG
                                    println!("Sent heartbeat: {:?}", heartbeat);

                                    return Some((
                                        Ok(variant_to_bytes(heartbeat)),
                                        (
                                            open_ai_stream,
                                            thread_id,
                                            should_stop,
                                            false,
                                            variant_queue,
                                            tool_name,
                                            tool_arguments,
                                            tool_id,
                                            llama_tool_call_content,
                                            Some((inner_reciever, handle)),
                                        ),
                                    ));
                                }
                                Ok(output) => {
                                    trace!("Reciever sent result!");

                                    // The output might fail if the tool call was not successful.
                                    let mut output = match output {
                                        Some(output) => output,
                                        None => {
                                            error!("Error recieving tool call output, the reciever was closed.");
                                            vec![StreamVariant::CodeError(
                                                "Error recieving tool call output.".to_string(),
                                            )]
                                        }
                                    };

                                    // Before returning the bytes, we need to restart the stream.
                                    restart_stream(
                                        &thread_id,
                                        output.clone(),
                                        chatbot,
                                        &mut open_ai_stream,
                                    )
                                    .await;

                                    // It also needs to be added to the conversation.
                                    add_to_conversation(
                                        &thread_id,
                                        output.clone(),
                                        freva_config_path_clone.clone(),
                                    );

                                    // The output can contain more than one variant, so we'll add them to the queue.
                                    let first = output.pop().unwrap_or_else(|| {
                                        StreamVariant::ServerError(
                                            "No variants found in tool call output.".to_string(),
                                        )
                                    });
                                    variant_queue.extend(output.into_iter());

                                    let bytes = variant_to_bytes(first);

                                    return Some((
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
                                            llama_tool_call_content,
                                            None,
                                        ),
                                    ));
                                }
                            }
                        }

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
                            chatbot,
                            &mut llama_tool_call_content,
                            &mut reciever,
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

                        let bytes = variant_to_bytes(first_variant);

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
                                llama_tool_call_content,
                                reciever,
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

/// Helper Enum to describe the different Stream Events that can be recieved from OpenAI/OLLama.
enum StreamEvents {
    Delta(String),           // The Assistant wrote a simple delta.
    StopEvent(FinishReason), // The API gave a reason to stop the conversation.
    ToolCall(Vec<ChatCompletionMessageToolCallChunk>), // A tool delta was recieved.
    Empty,        // An event was recieved that contained no useful content, but was unexpected.
    LiveToolCall, // The LLama tool call is running; nothing can be streamed.
    Error(ChatChoiceStream), // An error occured, contains the raw event.
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
    open_ai_stream: &mut Fuse<ChatCompletionResponseStream>,
    chatbot: AvailableChatbots,
    llama_tool_call_content: &mut Cell<Option<Cell<String>>>,
    reciever: &mut Option<(mpsc::Receiver<Vec<StreamVariant>>, JoinHandle<()>)>,
) -> Vec<StreamVariant> {
    match response {
        Some(Ok(response)) => {
            // Debug info: how many tokens were used?
            if let Some(usage) = response.clone().usage {
                debug!("Tokens used: {:?}", usage);
            }
            // The choices represent the multiple completions that the LLM can make. We always set n=1, so there is exactly one choice.
            if let Some(choice) = response.choices.first() {
                // First create the Stream Event so we can match on that later.
                // This simplyfies the modification of how it's decided what event happened.
                let event = match (
                    &choice.delta.tool_calls,
                    &choice.delta.content,
                    choice.finish_reason,
                ) {
                    (None, Some(string_delta), _) => {
                        // Because the ollama implementation of the openAI-compliant API is not yet implemented for streaming,
                        // We need to manually detect the tokens for the start of a tool call: "<tool_call>" and end: "</tool_call>".
                        // Depending on them, we need to either emit a Delta or a ToolCall event.

                        let tool_call_started = match string_delta.as_str() {
                            "<tool_call>" => Some(true), // Because that's how the tokens are represented in ASCII, they're sent inside one delta, not split and with no other content.
                            "</tool_call>" => Some(false),
                            _ => None,
                        };

                        match (tool_call_started, llama_tool_call_content.take()) {
                            (None, None) => {
                                // We are in the normal case, where the Assistant sends a delta.
                                StreamEvents::Delta(string_delta.clone())
                            }
                            (Some(true), inner_llama_tool_call_content) => {
                                // If the tool call started and we are not in a tool call, this is the start of a tool call.
                                // The standard OpenAI API now emits an empty Tool Call event, but it's not neccessary; an empty event will do the same.
                                // However, the problem is now that the tool call is in the JSON strucuture where the name and arguments are stored, which can't really be streamed.
                                // So we need to store the content of the tool call in a state variable to be able to pass it to the next iteration of the stream.

                                if let Some(content) = inner_llama_tool_call_content {
                                    warn!(
                                        "Tool call started, but content was not empty: {:?}",
                                        content.take()
                                    );
                                    // Clear the content just to be sure the next call is not affected.
                                    llama_tool_call_content.set(None);
                                }

                                // We store the content inside the llama_tool_call_content variable and emit a ToolCall event once it's JSON parseable.
                                llama_tool_call_content.set(Some(Cell::new(String::new())));
                                debug!("LLama tool call started: {:?}", string_delta);

                                StreamEvents::LiveToolCall
                            }
                            (None, Some(content)) => {
                                // Add the delta to the content of the tool call.
                                let inner_content = content.take() + string_delta;

                                trace!("Tool call content: {:?}", inner_content);

                                // If the content can now be parsed by JSON, we construct a ToolCall event.
                                let extracted = try_extract_tool_call(inner_content.trim());

                                content.set(inner_content);

                                // If it's none, the tool call is probably not finished yet.
                                match extracted {
                                    None => {
                                        // Re-set the content of the cell so it doesn't get lost.
                                        llama_tool_call_content.set(Some(content));
                                        // The tool call is not finished yet, so we emit an empty event.
                                        StreamEvents::LiveToolCall
                                    }
                                    Some((name, arguments)) => {
                                        // The tool call is finished, so we emit a ToolCall event.
                                        debug!(
                                            "LLama tool call finished: {:?} with arguments: {:?}",
                                            name, arguments
                                        );

                                        // Reset the llama_tool_call_content variable so new tool calls can be detected.
                                        llama_tool_call_content.set(None);

                                        StreamEvents::ToolCall(vec![
                                            ChatCompletionMessageToolCallChunk {
                                                id: Some(generate_id()),
                                                function: Some(FunctionCallStream {
                                                    name: Some(name),
                                                    arguments: Some(arguments),
                                                }),
                                                index: 0,
                                                r#type: Some(ChatCompletionToolType::Function),
                                            },
                                        ])
                                    }
                                }
                            }
                            (Some(false), inner_llama_tool_call_content) => {
                                // The end of the tool calls was reached; just emit a streamend event due to the tool call.

                                if let Some(content) = inner_llama_tool_call_content {
                                    warn!(
                                        "Tool call ended, but content was not empty: {:?}",
                                        content.take()
                                    );
                                    // Clear the content just to be sure the next call is not affected.
                                    llama_tool_call_content.set(None);
                                }

                                StreamEvents::StopEvent(FinishReason::ToolCalls)
                            }
                        }
                    }
                    (_, None, Some(reason)) => StreamEvents::StopEvent(reason),
                    (Some(tool_calls), None, None) => StreamEvents::ToolCall(tool_calls.clone()),
                    (None, None, None) => StreamEvents::Empty,
                    (Some(tool_calls), Some(string_delta), _) => {
                        warn!("Tool call AND content found in response, the API specified that this couldn't happen: {:?} and {:?}", tool_calls, string_delta);
                        StreamEvents::Error(choice.clone())
                    }
                };

                // Now that we determined the event, we can match on it to act accordingly.

                match event {
                    StreamEvents::Delta(string_delta) => {
                        // Basic case: the Assistant sends a text delta.
                        trace!("Delta: {}", string_delta);
                        vec![StreamVariant::Assistant(string_delta.clone())]
                    }
                    StreamEvents::StopEvent(reason) => {
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
                            chatbot,
                            reciever,
                        )
                        .await
                    }
                    StreamEvents::ToolCall(tool_calls) => {
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
                    StreamEvents::Empty => {
                        warn!("No content found in response and no reason to stop given; treating this as an empty Assistant response: {:?}", response);
                        vec![StreamVariant::Assistant(String::new())]
                    }
                    StreamEvents::Error(choice) => {
                        // Depending on what happened, we'll return a different error message.

                        // This is only called when a tool call and content was found in the response, which is not supposed to happen.
                        // If also a stop event was found, the message should be different.
                        if choice.finish_reason.is_some() {
                            vec![StreamVariant::StreamEnd("Tool call AND content AND stop event found in response, the API specified that this couldn't happen.".to_string())]
                        } else {
                            vec![StreamVariant::StreamEnd("Tool call AND content found in response, the API specified that this couldn't happen.".to_string())]
                        }
                    }
                    StreamEvents::LiveToolCall => {
                        // The tool call is still running, so we'll just send an empty event.
                        vec![StreamVariant::Code(String::new(), String::new())] // Just empty ID??? TODO: is this important?
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
            // The llama chatbot sometimes forgets to write </tool_call> at the end of the tool call.
            // If it's not an openAI chatbot, check whether we can get some tool call from the running tool.
            if matches!(chatbot, AvailableChatbots::Ollama(_)) {
                // Try to get the tool call from the running tool.
                let tool_call = try_extract_tool_call(
                    &llama_tool_call_content
                        .take()
                        .map(|c| c.take())
                        .unwrap_or_default(),
                );
                match tool_call {
                    None => {
                        info!("Stream ended abruptly and without error. Ollama just does this, returning streamend.");
                        vec![StreamVariant::StreamEnd("Ollama Stream ended".to_string())]
                    }
                    Some((name, arguments)) => {
                        // We know it's the code interpreter and can send it as a delta.
                        trace!("Tool call: {:?} with arguments: {:?}", name, arguments);
                        vec![
                            StreamVariant::Code(arguments, generate_id()),
                            StreamVariant::StreamEnd("Ollama Stream ended".to_string()), // We still need to end the stream, because the tool call is done.
                        ]
                    }
                }
            } else {
                // Else, it's just an abrupt end of the stream.
                warn!("Stream ended abruptly and without error. This should not happen; returning StreamEnd.");
                vec![StreamVariant::StreamEnd(
                    "Stream ended abruptly".to_string(),
                )]
            }
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
    open_ai_stream: &mut Fuse<ChatCompletionResponseStream>,
    response: &CreateChatCompletionStreamResponse,
    chatbot: AvailableChatbots,
    reciever: &mut Option<(mpsc::Receiver<Vec<StreamVariant>>, JoinHandle<()>)>,
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

            // In order to allow for a heartbeat, we need to create a mspc channel for the tool call to communicate with the main thread.
            let (tx, rx) = mpsc::channel::<Vec<StreamVariant>>(1);

            // There is NOT a tool call there, because that was accumulated in the previous iterations.
            // The stream ending is just OpenAI's way of telling us that the tool call is done and can now be executed.
            if let Some(name) = tool_name {
                let handle = tokio::spawn(route_call(
                    name.to_string(),
                    Some(tool_arguments.to_string()),
                    tool_id.to_string(),
                    thread_id.to_string(),
                    tx,
                ));
                // Reset the tool_name and tool_arguments
                *tool_name = None;
                *tool_arguments = String::new();
                *tool_id = String::new();

                // At this point, we need to inform the main thread that that the tool call is running.
                // Specifically, we need to return the info that a tool call was started and the reciever of the mpsc channel.
                reciever.replace((rx, handle));
                vec![heartbeat_content().await]
            } else {
                warn!(
                    "Tool call expected, but not found in response: {:?}",
                    response
                );
                all_generated_variants.push(StreamVariant::CodeError(
                    "Tool call expected, but not found in response.".to_string(),
                ));

                restart_stream(thread_id, all_generated_variants, chatbot, open_ai_stream).await
            }
        }
    }
}

/// Helper function to restart the stream.
async fn restart_stream(
    thread_id: &String,
    all_generated_variants: Vec<StreamVariant>,
    chatbot: AvailableChatbots,
    open_ai_stream: &mut Fuse<ChatCompletionResponseStream>,
) -> Vec<StreamVariant> {
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
            match build_request(all_oai_messages, chatbot) {
                Err(e) => {
                    // If we can't build the request, we'll return a generic error.
                    warn!("Error building request: {:?}", e);
                    vec![StreamVariant::ServerError(
                        "Error building request.".to_string(),
                    )]
                }
                Ok(request) => {
                    trace!("Request built successfully: {:?}", request);
                    match select_client(chatbot)
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
                            *open_ai_stream = stream.fuse();
                            all_generated_variants
                            // we need to return the generated variants, because the stream will be restarted with the tool call.
                        }
                    }
                }
            }
        }
    }
}

/// Helper function that tries to parse a llama tool call from a string
fn try_extract_tool_call(content: &str) -> Option<(String, String)> {
    // Because the LLM wrote it, it's escaped JSON, so we'll first unescape it.
    // let content = unescape_string(content);
    trace!("Tool call content: {:?}", content);

    // Because the LLMs are sometimes bad at creating JSON, we'll help them a bit.
    // We check at all closing curly braces, if if the text were to end there, if it would be valid JSON.
    let positions_curly = content.match_indices("}").map(|e| e.0).collect::<Vec<_>>();

    let mut dict = None;

    for pos in positions_curly {
        let new_content = &content[..=pos];
        match serde_json::from_str::<serde_json::Value>(new_content) {
            Ok(value) => {
                dict = Some(value);
                break; // we are guaranteed that the first valid JSON is the correct one.
            }
            Err(_) => {
                continue;
            }
        }
    }

    // If we couldn't find a valid JSON, we'll return None, as the tool call is likely not finished yet.
    let dict = match dict {
        Some(dict) => dict,
        None => {
            return None;
        }
    };
    debug!("Tool call content: {:?}", dict);

    // The type should be object, because it contains the name and arguments.
    match dict {
        serde_json::Value::Object(inner_object) => {
            // We have the object, so we can extract the name and arguments.
            if let Some(serde_json::Value::String(name)) = inner_object.get("name") {
                if let Some(serde_json::Value::Object(arguments)) = inner_object.get("arguments") {
                    // We have the name and arguments, so we can return them.
                    // The arguments need to pe parsed to a string from JSON.
                    let arguments = match serde_json::to_string(arguments) {
                        Ok(arguments) => arguments,
                        Err(e) => {
                            warn!("Error converting tool call arguments to string: {:?}", e);
                            return None;
                        }
                    };
                    debug!("Tool call name: {:?}, arguments: {:?}", name, arguments);
                    Some((name.clone(), arguments.to_string()))
                } else {
                    // The arguments are missing, so we can't return anything.
                    warn!(
                        "Tool call expected arguments, but not found: {:?}",
                        inner_object
                    );
                    None
                }
            } else {
                // The name is missing, so we can't return anything.
                warn!("Tool call expected name, but not found: {:?}", inner_object);
                None
            }
        }
        _ => {
            // Shouldn't happen! The API specifies that it's always an object.
            warn!("Tool call expected to be an object, but found: {:?}", dict);
            None
        }
    }
}

/// Helper function to convert a StreamVariant to bytes.
/// Doesn't panic, always returns a valid byte array.
fn variant_to_bytes(variant: StreamVariant) -> Bytes {
    let string_rep = match serde_json::to_string(&variant) {
        Ok(string) => string,
        Err(e) => {
            error!("Error converting StreamVariant to string with serde_json; falling back to debug representation: {:?}", e);
            format!(
                "{:?}",
                StreamVariant::ServerError(format!(
                    "Error converting StreamVariant to string: {variant:?}"
                ))
            )
        }
    };

    actix_web::web::Bytes::copy_from_slice(string_rep.as_bytes())
}
