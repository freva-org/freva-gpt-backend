use tracing::debug;

/// The list of available chatbots that the user can choose from.
/// The first one is the default chatbot.
pub static AVAILABLE_CHATBOTS: &[AvailableChatbots] = &[
    AvailableChatbots::OpenAI(OpenAIModels::gpt_4o),
    AvailableChatbots::OpenAI(OpenAIModels::gpt_4o_mini),
    // AvailableChatbots::OpenAI(OpenAIModels::o1_mini), // In Beta, doesn't do streaming yet.
    AvailableChatbots::OpenAI(OpenAIModels::gpt_3_5_turbo),
    AvailableChatbots::OpenAI(OpenAIModels::o3_mini),
    AvailableChatbots::OpenAI(OpenAIModels::gpt_4_1),
    AvailableChatbots::OpenAI(OpenAIModels::gpt_4_1_mini),
    AvailableChatbots::OpenAI(OpenAIModels::gpt_4_1_nano),
    // AvailableChatbots::Ollama(OllamaModels::llama3_2_3B),
    // AvailableChatbots::Ollama(OllamaModels::llama3_1_70B),
    // AvailableChatbots::Ollama(OllamaModels::llama3_1_8B),
    // AvailableChatbots::Ollama(OllamaModels::llama3_groq_8B),
    // AvailableChatbots::Ollama(OllamaModels::gemma2),
    AvailableChatbots::Ollama(OllamaModels::qwen2_5_3B), // Only one active for development purposes. Will be expanded back after the 12th.
    // AvailableChatbots::Ollama(OllamaModels::qwen2_5_7B),
    // AvailableChatbots::Ollama(OllamaModels::qwen2_5_7B_tool),
    // AvailableChatbots::Ollama(OllamaModels::qwen2_5_32B),
    // AvailableChatbots::Google(GoogleModels::gemini_1_5_flash), // Not yet available in the EU.
    // AvailableChatbots::Ollama(OllamaModels::deepseek_r1_70b), // Doesn't support tool calls!.
    AvailableChatbots::Ollama(OllamaModels::deepseek_r1_32b_tools), // the community model, doesn't support tool calls yet, the community needs to work on it
    AvailableChatbots::Ollama(OllamaModels::qwq),
];

/// The default chatbot that will be used when the user doesn't specify one.
/// It's always the first one in the list of available chatbots.
pub static DEFAULTCHATBOT: AvailableChatbots = AVAILABLE_CHATBOTS[0];

#[derive(Debug, Clone, Copy)]
pub enum AvailableChatbots {
    OpenAI(OpenAIModels),
    Ollama(OllamaModels),
    Google(GoogleModels),
}

// Implementing the conversion from the enum to a string
impl From<AvailableChatbots> for String {
    fn from(val: AvailableChatbots) -> Self {
        match val {
            AvailableChatbots::OpenAI(model) => match model {
                OpenAIModels::gpt_4o => "gpt-4o".to_string(),
                OpenAIModels::gpt_4o_mini => "gpt-4o-mini".to_string(),
                OpenAIModels::o1_mini => "o1-mini".to_string(),
                OpenAIModels::gpt_4_turbo => "gpt-4-turbo".to_string(),
                OpenAIModels::gpt_3_5_turbo => "gpt-3.5-turbo".to_string(),
                OpenAIModels::o3_mini => "o3-mini".to_string(),
                OpenAIModels::gpt_4_1 => "gpt-4.1".to_string(),
                OpenAIModels::gpt_4_1_mini => "gpt-4.1-mini".to_string(),
                OpenAIModels::gpt_4_1_nano => "gpt-4.1-nano".to_string(),
            },
            AvailableChatbots::Ollama(model) => match model {
                OllamaModels::llama3_2_3B => "llama3.2".to_string(),
                OllamaModels::llama3_1_70B => "llama3.1:70b".to_string(),
                OllamaModels::llama3_1_8B => "llama3.1:8b".to_string(),
                OllamaModels::llama3_groq_8B => "llama3-groq-tool-use".to_string(), // community model
                OllamaModels::gemma2 => "gemma2".to_string(),
                OllamaModels::qwen2_5_3B => "qwen2.5:3b".to_string(),
                OllamaModels::qwen2_5_7B => "qwen2.5".to_string(),
                OllamaModels::qwen2_5_7B_tool => "majx13/test".to_string(), // community model
                OllamaModels::qwen2_5_32B => "qwen2.5:32b".to_string(), // 72 is just too large for us to handle efficiently.
                OllamaModels::deepseek_r1_70b => "deepseek-r1:70b".to_string(), // For testing purposes.
                OllamaModels::deepseek_r1_32b_tools => "deepseek-r1:32b".to_string(), // The Qwen distill; technically capable of tool calling.
                OllamaModels::qwq => "qwq".to_string(), // Qwen but reasoning
            },
            AvailableChatbots::Google(model) => match model {
                GoogleModels::gemini_1_5_flash => "gemini-1.5-flash".to_string(),
            },
        }
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
        for chatbot in AVAILABLE_CHATBOTS {
            if String::from(*chatbot) == self {
                return Ok(*chatbot);
            }
        }
        // No chatbot matched the input string, so we return an error.
        debug!("Invalid chatbot: {}", self);
        Err(())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum OpenAIModels {
    #[allow(non_camel_case_types)] // Easier to read
    gpt_4o,
    #[allow(non_camel_case_types)]
    gpt_4o_mini,
    #[allow(non_camel_case_types)]
    o1_mini,
    #[allow(non_camel_case_types)]
    gpt_4_turbo,
    #[allow(non_camel_case_types)]
    gpt_3_5_turbo,
    #[allow(non_camel_case_types)]
    o3_mini,
    #[allow(non_camel_case_types)]
    gpt_4_1,
    #[allow(non_camel_case_types)]
    gpt_4_1_mini,
    #[allow(non_camel_case_types)]
    gpt_4_1_nano,
}

#[derive(Debug, Clone, Copy)]
pub enum OllamaModels {
    #[allow(non_camel_case_types)]
    llama3_2_3B,
    #[allow(non_camel_case_types)]
    llama3_1_70B,
    #[allow(non_camel_case_types)]
    llama3_1_8B,
    #[allow(non_camel_case_types)]
    gemma2,
    #[allow(non_camel_case_types)]
    qwen2_5_3B,
    #[allow(non_camel_case_types)]
    qwen2_5_7B,
    #[allow(non_camel_case_types)]
    qwen2_5_7B_tool,
    #[allow(non_camel_case_types)]
    qwen2_5_32B,
    #[allow(non_camel_case_types)]
    llama3_groq_8B,
    #[allow(non_camel_case_types)]
    deepseek_r1_70b,
    #[allow(non_camel_case_types)]
    deepseek_r1_32b_tools,
    #[allow(non_camel_case_types)]
    qwq,
}

#[derive(Debug, Clone, Copy)]
pub enum GoogleModels {
    #[allow(non_camel_case_types)]
    gemini_1_5_flash,
}

// Characteristics: Some models have different ways to interact with the API (because the API is not properly defined).
// These are just a few functions to properly record the differences between the models.

/// Some models, most of the qwen family, use a response with no choice in the choice field to denote that the stream should be ended, if used through the async-openai API.
/// If a model does this, it should return true, otherwise false.
pub fn model_ends_on_no_choice(model: AvailableChatbots) -> bool {
    match model {
        AvailableChatbots::Ollama(OllamaModels::qwen2_5_3B)
        | AvailableChatbots::Ollama(OllamaModels::qwen2_5_7B)
        | AvailableChatbots::Ollama(OllamaModels::qwen2_5_7B_tool)
        | AvailableChatbots::Ollama(OllamaModels::qwen2_5_32B) => true,
        // | AvailableChatbots::Ollama(OllamaModels::qwq) => true, // Test this!
        _ => false,
    }
}
