use once_cell::sync::Lazy;
use tracing::{debug, error, info, trace, warn};

/// The list of available chatbots that the user can choose from.
/// The first one is the default chatbot.
pub static AVAILABLE_CHATBOTS: Lazy<Vec<AvailableChatbots>> = Lazy::new(|| {
    let chatbots = get_available_chatbots_from_litellm_file();
    if chatbots.is_empty() {
        error!("No available chatbots found in the LiteLLM file. Please check the configuration.");
        eprintln!("Error: No available chatbots found in the LiteLLM file. Please check the configuration.");
        std::process::exit(1); // This is fatal because we can't run without any chatbots.
                               // But because it's in a lazy static, exiting is not a problem; it will never do it "in production", but before.
    }
    info!("Available chatbots: {:?}", chatbots);
    chatbots
});

/// Because we want a single source of truth for the available chatbots, we will read them from the file where LiteLLM stores them.
/// This is a yaml file, but I'll just read it as a string and parse it manually.
fn get_available_chatbots_from_litellm_file() -> Vec<AvailableChatbots> {
    // For now, we can just include the file, but later we might want to read it instead.
    let file_content = include_str!("../../litellm_config.yaml");

    // We are looking for lines that contain "model_name" and then want to extract the model name,
    // which is after that in quotes.
    // Example: \t - model_name: "qwen2_5_0b_instruct"
    let mut chatbots = Vec::new();
    for line in file_content.lines() {
        trace!("Processing line: {}", line);
        // In order to respsect commenting out, we will only look for lines that start with only spaces and maybe a dash.
        let line = line.trim_matches(|c: char| c == '-' || c.is_whitespace());
        if line.starts_with("model_name:") {
            // Expect the model name to be in quotes, and those quotes are the only thing on the line.
            if let Some(start) = line.find('"') {
                if let Some(end) = line.rfind('"') {
                    let model_name = line[start + 1..end].trim().to_string();
                    if !model_name.is_empty() {
                        chatbots.push(AvailableChatbots(model_name));
                    } else {
                        warn!("Found an empty model name in the LiteLLM file. Skipping it.");
                    }
                } else {
                    warn!("Found a line with model_name but no closing quote, skipping it.");
                }
            } else {
                warn!("Found a line with model_name but no opening quote, skipping it.");
            }
        } // Those lines that don't contain "model_name" are ignored.
    }
    if chatbots.is_empty() {
        error!("No valid chatbots found in the LiteLLM file.");
    }
    chatbots
}

/// The default chatbot that will be used when the user doesn't specify one.
/// It's always the first one in the list of available chatbots.
pub static DEFAULTCHATBOT: Lazy<AvailableChatbots> = Lazy::new(|| {
    let first = AVAILABLE_CHATBOTS.first();
    if let Some(chatbot) = first {
        chatbot.clone()
    } else {
        error!("No default chatbot found. Please check the configuration.");
        eprintln!("Error: No default chatbot found. Please check the configuration.");
        std::process::exit(1); // This technically should never happen, but just in case.
    }
});

#[derive(Debug, Clone)]
pub struct AvailableChatbots(pub String);

impl From<AvailableChatbots> for String {
    fn from(val: AvailableChatbots) -> Self {
        // We can just return the inner string, as it is already a String.
        val.0
    }
}

// Implementing the conversion from a string to the enum
// This one is fallible, because the string might not be a valid chatbot.
impl TryInto<AvailableChatbots> for String {
    type Error = (); // We have just one error (invalid string), so we can use a unit type
    fn try_into(self) -> Result<AvailableChatbots, Self::Error> {
        // To be forwards compatible, instead of matching on the input string, we'll try out all the possibilities.
        // If any available chatbot to String matches the input string, we'll return that chatbot.
        // If none of them match, we'll return an error.
        for chatbot in AVAILABLE_CHATBOTS.iter() {
            if String::from(chatbot.clone()) == self {
                return Ok(chatbot.clone());
            }
        }
        // No chatbot matched the input string, so we return an error.
        debug!("Invalid chatbot: {}", self);
        Err(())
    }
}

// Characteristics: Some models have different ways to interact with the API (because the API is not properly defined).
// These are just a few functions to properly record the differences between the models.

/// Some models, most of the qwen family, use a response with no choice in the choice field to denote that the stream should be ended, if used through the async-openai API.
/// If a model does this, it should return true, otherwise false.
///
/// Technically, LiteLLM should fix this, but just to be sure, we will keep this function here.
pub fn model_ends_on_no_choice(model: AvailableChatbots) -> bool {
    matches!(model, AvailableChatbots(model_name) if model_name.starts_with("qwen2_5"))
}

/// Some models are capable of recieving Images and encoding them for them to understand.
/// They can be given the gernerated image as a base64 string in the prompt.
pub fn model_supports_images(model: AvailableChatbots) -> bool {
    match model {
        // The new system only identifies the models by their name, so we will just check the name.
        AvailableChatbots(ref model_name)
            if model_name.starts_with("gpt-4o") || model_name.starts_with("gpt-4.1") =>
        {
            true
        }
        _ => false,
    }
}
