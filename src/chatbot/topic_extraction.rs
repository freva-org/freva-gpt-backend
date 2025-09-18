use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
};
use tracing::warn;

use crate::chatbot::LITE_LLM_CLIENT;

/// Given a "topic", that is, the users' first actual request of the conversation, sum it up.
/// This will then be used as a summary for the history view on the frontend.
pub async fn summarize_topic(topic: &str) -> String {
    // We will use the GPT-4.1-mini chatbot for now.

    // Cut the topic short if it is too long
    let topic = if topic.len() > 5000 {
        format!("{}...", &topic[..5000])
    } else {
        topic.to_string()
    };

    if topic.is_empty() {
        warn!("Received an empty topic for summarization.");
        return "Empty request".to_string();
    }

    let request = CreateChatCompletionRequest {
        model: "gpt-4.1-mini".to_string(),
        messages: vec![ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
            content: "A user has written the following request. Summarize it in a few words so that it may be displayed as an overview. Do not write anything other than the summary.".to_string().into(),
            name: None,
        }),
        ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
            content: topic.to_string().into(),
            name: None,
        })],
        n: Some(1),
        max_completion_tokens: Some(50),
        ..Default::default()
    };

    let result = match LITE_LLM_CLIENT.chat().create(request).await {
        Ok(response) => response.choices.first().map_or_else(
            || {
                warn!("No summary available, list of choices was empty.");
                "No summary available".to_string()
            },
            |choice| {
                choice.message.content.clone().unwrap_or_else(|| {
                    warn!("No summary available, message content was empty.");
                    "No summary available".to_string()
                })
            },
        ),
        Err(err) => {
            eprintln!("Error occurred while summarizing topic: {err}");
            return "Error occurred while summarizing topic".to_string();
        }
    };

    if result.is_empty() {
        warn!("Summary is empty, returning default message.");
        "No summary available".to_string()
    } else {
        result
    }
}
