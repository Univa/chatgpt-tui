use std::str;
use std::sync::mpsc::Sender;

use futures::AsyncBufReadExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use surf::Client;

use crate::ProcessedMessage;

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
    stream: bool,
}

#[derive(Serialize, Deserialize)]
pub struct ApiResponse {
    choices: Vec<Choice>,
}

pub async fn stream_chatgpt_response(
    client: &Client,
    apikey: &String,
    messages: &Vec<Message>,
    processed_msg_send: &Sender<ProcessedMessage>,
) -> Result<Message, String> {
    let body = ApiRequest {
        model: "gpt-3.5-turbo",
        messages: messages.clone(),
        stream: true,
    };

    // Fetch the ChatGPT response
    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {apikey}"))
        .body_json(&body)
        .unwrap()
        .send()
        .await;

    let mut response = match res {
        Ok(response) => response,
        Err(e) => return Err(format!("Could not get a response from ChatGPT: {e:?}")),
    };

    let mut message = Message {
        role: Role::Assistant,
        content: String::new(),
    };

    loop {
        let buf = response.fill_buf().await.unwrap();
        let buffer_length = buf.len();

        let data = str::from_utf8(buf).unwrap();
        let deltas: Vec<Value> = data
            .lines()
            .filter_map(|d| serde_json::from_str(&d.chars().skip(6).collect::<String>()).ok())
            .collect();

        // very cool
        for data in deltas {
            let delta_value = data
                .as_object()
                .unwrap()
                .get("choices")
                .unwrap()
                .as_array()
                .unwrap()
                .last()
                .unwrap()
                .as_object()
                .unwrap()
                .get("delta")
                .unwrap()
                .as_object()
                .unwrap();

            if delta_value.contains_key("content") {
                message
                    .content
                    .push_str(delta_value.get("content").unwrap().as_str().unwrap());
            }
        }

        processed_msg_send
            .send(ProcessedMessage::ChatMessage(Ok(message.to_owned())))
            .unwrap();

        response.consume_unpin(buffer_length);

        if buffer_length == 0 {
            break;
        }
    }

    Ok(message)
}
