use actix_web::{HttpRequest, HttpResponse, Responder};
use async_openai::types::CreateCompletionRequestArgs;
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

    // For testing, a basic request
    let request = match CreateCompletionRequestArgs::default()
        // .model(String::from(DEFAULTCHATBOT))
        // .model("gpt-4o")
        .model("gpt-3.5-turbo-instruct")// TODO: change this to the default chatbot
        .n(1)
        .prompt(input)
        .stream(true)
        .max_tokens(100u32)
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

    let stream = match CLIENT.completions().create_stream(request).await {
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
                let delta = choice.text.clone();
                trace!("Delta: {}", delta);
                Ok(actix_web::web::Bytes::copy_from_slice(delta.as_bytes())) // Actix wants the stream in this exact format.
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
