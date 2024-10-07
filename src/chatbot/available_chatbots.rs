use tracing::debug;

/// The list of available chatbots that the user can choose from.
/// The first one is the default chatbot.
pub static AVAILABLE_CHATBOTS: &[AvailableChatbots] = &[
    AvailableChatbots::OpenAI(OpenAIModels::gpt_4o_mini),
    AvailableChatbots::OpenAI(OpenAIModels::gpt_4o),
    AvailableChatbots::OpenAI(OpenAIModels::o1_mini),
    AvailableChatbots::Ollama(OllamaModels::llama3_2_3B),
    AvailableChatbots::Ollama(OllamaModels::llama3_1_70B),
    AvailableChatbots::Ollama(OllamaModels::llama3_1_8B),
    AvailableChatbots::Ollama(OllamaModels::gemma2),
    AvailableChatbots::Ollama(OllamaModels::qwen2_5_3B),
    AvailableChatbots::Ollama(OllamaModels::qwen2_5_7B),
    AvailableChatbots::Ollama(OllamaModels::qwen2_5_32B),
];

/// The default chatbot that will be used when the user doesn't specify one.
/// It's always the first one in the list of available chatbots.
pub static DEFAULTCHATBOT: AvailableChatbots = AVAILABLE_CHATBOTS[0];

#[derive(Debug, Clone, Copy)]
pub enum AvailableChatbots {
    OpenAI(OpenAIModels),
    Ollama(OllamaModels),
}

// Implementing the conversion from the enum to a string
impl From<AvailableChatbots> for String {
    fn from(val: AvailableChatbots) -> Self {
        match val {
            AvailableChatbots::OpenAI(model) => match model {
                OpenAIModels::gpt_4o => "gpt-4o".to_string(),
                OpenAIModels::gpt_4o_mini => "gpt-4o-mini".to_string(),
                OpenAIModels::o1_mini => "o1-mini".to_string(),
            },
            AvailableChatbots::Ollama(model) => match model {
                OllamaModels::llama3_2_3B => "llama3.2".to_string(),
                OllamaModels::llama3_1_70B => "llama3.1:70b".to_string(),
                OllamaModels::llama3_1_8B => "llama3.1:8b".to_string(),
                OllamaModels::gemma2 => "gemma2".to_string(),
                OllamaModels::qwen2_5_3B => "qwen2.5:3b".to_string(),
                OllamaModels::qwen2_5_7B => "qwen2.5".to_string(),
                OllamaModels::qwen2_5_32B => "qwen2.5:32b".to_string(), // 72 is just too large for us to handle efficiently.
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
    qwen2_5_32B,
}
