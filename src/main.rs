use cursive::reexports::enumset::enum_set;
use cursive::theme::{Color, ColorStyle, Effect, PaletteColor, Style, Theme};
use cursive::utils::markup::StyledString;
use cursive::view::{Nameable, Resizable, ScrollStrategy};
use cursive::views::{Dialog, EditView, LinearLayout, Panel, ScrollView, TextView};
use cursive::Cursive;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::channel;
use std::time::Duration;
use std::{env, thread};

#[derive(Serialize, Deserialize, Clone)]
enum Role {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "system")]
    System,
    #[serde(rename = "assistant")]
    Assistant,
}

#[derive(Serialize, Deserialize)]
enum FinishReason {
    #[serde(rename = "stop")]
    Stop,
}

#[derive(Serialize, Deserialize, Clone)]
struct Message {
    role: Role,
    content: String,
}

#[derive(Serialize, Deserialize)]
struct Choice {
    message: Message,
    finish_reason: FinishReason,
    index: i32,
}

#[derive(Serialize, Deserialize)]
struct ApiRequest {
    model: &'static str,
    messages: Vec<Message>,
}

#[derive(Serialize, Deserialize)]
struct ApiResponse {
    choices: Vec<Choice>,
}

#[derive(Serialize, Deserialize)]
enum User {
    You,
    ChatGPT,
}

enum SystemMessage {
    ResponsePending,
}

enum ProcessedMessage {
    SystemMessage(SystemMessage),
    ChatMessage(Result<Message, String>),
}

fn main() {
    let mut siv = cursive::default();

    // Create a channel and thread for 1 second ticks
    let (tick_send, tick_rcv) = channel::<i32>();

    let _tickhandler = thread::spawn(move || {
        let mut counter = 1;

        loop {
            thread::sleep(Duration::from_secs(1));

            if counter == 5 {
                counter = 1;
            }

            tick_send.send(counter).unwrap();

            counter += 1;
        }
    });

    // Get API key from environment variable
    let apikey = match env::var("OPENAI_API_KEY") {
        Ok(apikey) => apikey,
        _ => panic!("OPENAI_API_KEY is not set. Please set this environment variable to your OpenAI API Key.")
    };

    let client = reqwest::blocking::Client::new();
    let mut messages: Vec<Message> = vec![];

    let (user_msg_send, user_msg_recv) = channel::<Message>();
    let (processed_msg_send, processed_msg_recv) = channel::<ProcessedMessage>();

    let _reqhandler = thread::spawn(move || loop {
        for m in user_msg_recv.try_iter() {
            messages.push(m.to_owned());

            processed_msg_send
                .send(ProcessedMessage::ChatMessage(Ok(m)))
                .unwrap();

            // Tell the UI that we're waiting for a response from ChatGPT
            processed_msg_send
                .send(ProcessedMessage::SystemMessage(
                    SystemMessage::ResponsePending,
                ))
                .unwrap();

            let chatgpt_response = get_chatgpt_response(&client, &apikey, &messages).to_owned();

            match chatgpt_response.to_owned() {
                Ok(message) => {
                    messages.push(message);
                }
                Err(_) => {
                    messages.pop();
                }
            }

            processed_msg_send
                .send(ProcessedMessage::ChatMessage(chatgpt_response))
                .unwrap();
        }
    });

    // Use default terminal colors
    let theme = theme(&siv);
    siv.set_theme(theme);

    let mut runner = siv.try_into_runner().unwrap();

    // Render the layout
    runner.add_fullscreen_layer(
        LinearLayout::horizontal()
            // .child(
            //     Panel::new(
            //         LinearLayout::vertical()
            //             .child(TextView::new("Previous Sessions").h_align(HAlign::Center))
            //             .child(DummyView.fixed_height(1)),
            //     )
            //     .fixed_width(30),
            // )
            .child(
                Panel::new(
                    LinearLayout::vertical()
                        .child(
                            ScrollView::new(
                                LinearLayout::vertical().with_name("messages_container"),
                            )
                            .scroll_strategy(ScrollStrategy::StickToBottom)
                            .full_height(),
                        )
                        .child(Panel::new(
                            EditView::new()
                                .filler(" ")
                                .on_submit(move |s, m| {
                                    s.call_on_name("input_box", |view: &mut EditView| {
                                        view.disable();
                                        view.set_content("");
                                    });

                                    let message = Message {
                                        role: Role::User,
                                        content: m.trim().to_owned(),
                                    };

                                    user_msg_send.send(message).unwrap();
                                })
                                .with_name("input_box"),
                        )),
                )
                .full_width(),
            )
            .full_screen(),
    );

    runner.refresh();

    let mut pending = false;

    while runner.is_running() {
        runner.step();

        for m in processed_msg_recv.try_iter() {
            match m {
                ProcessedMessage::SystemMessage(m) => {
                    match m {
                        SystemMessage::ResponsePending => {
                            // Add a chat loading message
                            pending = true;

                            runner.call_on_name("messages_container", |view: &mut LinearLayout| {
                                view.add_child(TextView::new(format_message(&Message {
                                    role: Role::Assistant,
                                    content: "".to_string(),
                                })));
                            });
                        }
                    };
                }
                ProcessedMessage::ChatMessage(m) => {
                    match m {
                        Ok(m) => {
                            // Re-enable the input box when we receive a response from ChatGPT
                            if let Role::Assistant = m.role {
                                runner.call_on_name("input_box", |view: &mut EditView| {
                                    view.enable();
                                });

                                runner.call_on_name(
                                    "messages_container",
                                    |view: &mut LinearLayout| {
                                        view.remove_child(view.len() - 1);
                                    },
                                );

                                pending = false;
                            }

                            // Add the message to the message container
                            runner.call_on_name("messages_container", |view: &mut LinearLayout| {
                                view.add_child(TextView::new(format_message(&m)));
                            });
                        }
                        Err(error) => {
                            // Display error message in a dialog
                            runner.add_layer(Dialog::new().content(TextView::new(error)).button(
                                "Ok",
                                |runner| {
                                    runner.pop_layer();
                                },
                            ));

                            // Remove the last user message (avoids confusion later)
                            runner.call_on_name("messages_container", |view: &mut LinearLayout| {
                                for _ in 0..2 {
                                    view.remove_child(view.len() - 1);
                                }
                            });

                            // Re-enable the input box
                            runner.call_on_name("input_box", |view: &mut EditView| {
                                view.enable();
                            });

                            pending = false;
                        }
                    }
                }
            }
        }

        for m in tick_rcv.try_iter() {
            if pending {
                // Add the message to the message container
                runner.call_on_name("messages_container", |view: &mut LinearLayout| {
                    view.remove_child(view.len() - 1);
                    view.add_child(TextView::new(format_message(&Message {
                        role: Role::Assistant,
                        content: String::from_utf8(vec![b'.'; m as usize]).unwrap(),
                    })));
                });
            }
        }

        runner.refresh();
    }
}

fn theme(siv: &Cursive) -> Theme {
    let mut theme = siv.current_theme().clone();
    theme.palette[PaletteColor::Background] = Color::TerminalDefault;
    theme.palette[PaletteColor::View] = Color::TerminalDefault;
    theme.palette[PaletteColor::Primary] = Color::TerminalDefault;
    theme.palette[PaletteColor::Secondary] = Color::TerminalDefault;
    theme.palette[PaletteColor::Tertiary] = Color::TerminalDefault;
    theme.palette[PaletteColor::TitlePrimary] = Color::TerminalDefault;
    theme.palette[PaletteColor::TitleSecondary] = Color::TerminalDefault;
    theme.palette[PaletteColor::Highlight] = Color::TerminalDefault;
    theme.palette[PaletteColor::HighlightInactive] = Color::TerminalDefault;
    theme.palette[PaletteColor::HighlightText] = Color::TerminalDefault;
    theme.shadow = false;
    theme
}

fn get_chatgpt_response(
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
        .header("user-agent", "curl/7.87.0")
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

fn format_message(m: &Message) -> StyledString {
    let user = match m.role {
        Role::User => "You",
        Role::Assistant => "ChatGPT",
        Role::System => "System",
    };

    let mut formatted_user = StyledString::styled(
        user,
        Style {
            effects: enum_set!(Effect::Bold | Effect::Underline),
            color: ColorStyle::primary(),
        },
    );

    let formatted_contents = StyledString::from(m.content.trim());

    formatted_user.append_plain(": ");
    formatted_user.append(formatted_contents);
    formatted_user.append("\n\n");

    formatted_user
}
