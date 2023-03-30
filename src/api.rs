use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub enum Role {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "system")]
    System,
    #[serde(rename = "assistant")]
    Assistant,
}

#[derive(Serialize, Deserialize)]
pub enum FinishReason {
    #[serde(rename = "stop")]
    Stop,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Serialize, Deserialize)]
pub struct Choice {
    message: Message,
    finish_reason: FinishReason,
    index: i32,
}

#[derive(Serialize, Deserialize)]
pub struct ApiRequest {
    model: &'static str,
    messages: Vec<Message>,
}

#[derive(Serialize, Deserialize)]
pub struct ApiResponse {
    choices: Vec<Choice>,
}

pub fn get_chatgpt_response(
    client: &Client,
    apikey: &String,
    messages: &Vec<Message>,
) -> Result<Message, String> {
    let body = ApiRequest {
        model: "gpt-3.5-turbo",
        messages: messages.clone(),
    };

    // Fetch the ChatGPT response
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(apikey)
        .json(&body)
        .send();

    let json_data = match response {
        Ok(response) => response.json::<serde_json::Value>(),
        Err(error) => {
            return Err(format!("Could not get a response from ChatGPT: {error}"));
        }
    };

    match json_data {
        Ok(json) => {
            let choices = match json.get("choices") {
                Some(choices) => choices,
                None => return Err(format!("Response from ChatGPT contained an error: {json}")),
            };

            // Add ChatGPT's response to the container
            match choices.as_array().unwrap().last() {
                Some(response) => Ok(Message {
                    role: Role::Assistant,
                    content: serde_json::from_str(
                        &response
                            .get("message")
                            .unwrap()
                            .get("content")
                            .unwrap()
                            .to_string(),
                    )
                    .unwrap(),
                }),
                None => Err("Response from ChatGPT was empty.".to_string()),
            }
        }
        Err(error) => Err(format!("Could not decode response from ChatGPT: {error}")),
    }
}
