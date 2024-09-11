/// The default chatbot that will be used when the user doesn't specify one.
pub static DEFAULTCHATBOT: AvailableChatbots = AvailableChatbots::OpenAI(OpenAIModels::gpt_4o_mini);

#[derive(Debug, Clone, Copy)]
pub enum AvailableChatbots {
    OpenAI(OpenAIModels),
    // Here will be more chatbots, like LLAMA, etc.
}

// Implementing the conversion from the enum to a string
impl From<AvailableChatbots> for String {
    fn from(val: AvailableChatbots) -> Self {
        match val {
            AvailableChatbots::OpenAI(model) => match model {
                OpenAIModels::gpt_4o => "gpt-4o".to_string(),
                OpenAIModels::gpt_4o_mini => "gpt-4o-mini".to_string(),
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
