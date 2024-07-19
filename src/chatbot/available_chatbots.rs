// all available chatbots the backend supports

// use async_openai::{config::OpenAIConfig, Client};
// /// Prints all available models to the console
// pub async fn debug_print_all_models() {
//     let config = OpenAIConfig::new(); // Because dotenvy loads into std::env::var, we don't need to pass the api key here. This only works if the api key is in the .env file and was already loaded.
//     let client = Client::with_config(config);
//     let models = client.models().list().await;
//     println!("Available models:{:?}", models);
// }

static DEFAULTCHATBOT: AvailableChatbots = AvailableChatbots::OpenAI(OpenAIModels::gpt_4o);

pub enum AvailableChatbots {
    OpenAI(OpenAIModels),
    // Here will be more chatbots, like LLAMA, etc.
}

impl From<AvailableChatbots> for String{
    fn from(val: AvailableChatbots) -> Self {
        match val {
            AvailableChatbots::OpenAI(model) => match model {
                OpenAIModels::gpt_4o => "gpt-4o".to_string(),
                OpenAIModels::gpt_4o_mini => "gpt-4o-mini".to_string(),
            },
        }
    }
}

pub enum OpenAIModels {
    #[allow(non_camel_case_types)] // Easier to read
    gpt_4o,
    #[allow(non_camel_case_types)]
    gpt_4o_mini,
}
