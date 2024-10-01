/// The default chatbot that will be used when the user doesn't specify one.
pub static DEFAULTCHATBOT: AvailableChatbots = AvailableChatbots::Ollama(OllamaModels::llama3_1_70B);

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
            },
            AvailableChatbots::Ollama(model) => match model {
                OllamaModels::llama3_2_3B => "llama3.2".to_string(),
                OllamaModels::llama3_1_70B => "llama3.1:70b".to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum OpenAIModels {
    #[allow(non_camel_case_types)] // Easier to read
    gpt_4o,
    #[allow(non_camel_case_types)]
    gpt_4o_mini,
}

#[derive(Debug, Clone, Copy)]
pub enum OllamaModels {
    #[allow(non_camel_case_types)]
    llama3_2_3B,
    #[allow(non_camel_case_types)]
    llama3_1_70B,
}
