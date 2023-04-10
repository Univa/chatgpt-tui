use cursive::theme::{Color, PaletteColor, Theme};
use cursive::view::{Nameable, Resizable, ScrollStrategy};
use cursive::views::{Dialog, EditView, LinearLayout, Panel, ScrollView, TextView};
use cursive::Cursive;
use futures::executor::block_on;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::channel;
use std::time::Duration;
use std::{env, str, thread};
use syntect::dumps::from_binary;
use syntect::highlighting::{Theme as HighlightingTheme, ThemeSet};
use syntect::parsing::SyntaxSet;

mod api;
use api::{stream_chatgpt_response, Message, Role};

mod format;
use format::format_message;

#[derive(Serialize, Deserialize)]
pub enum SystemMessage {
    ResponsePending,
}

pub enum ProcessedMessage {
    SystemMessage(SystemMessage),
    ChatMessage(Result<Message, String>),
}

fn main() {
    let mut siv = cursive::default();

    // Create a channel and thread for 1 second ticks
    let (tick_send, tick_rcv) = channel::<i32>();

    let _tickhandler = thread::spawn(move || {
        let mut counter = 0;

        loop {
            thread::sleep(Duration::from_secs(1));
            counter = (counter % 4) + 1;
            tick_send.send(counter).unwrap();
        }
    });

    // Get API key from environment variable
    let apikey = match env::var("OPENAI_API_KEY") {
        Ok(apikey) => apikey,
        _ => panic!("OPENAI_API_KEY is not set. Please set this environment variable to your OpenAI API Key.")
    };

    let client = surf::Client::new();
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

            let chatgpt_response = block_on(stream_chatgpt_response(
                &client,
                &apikey,
                &messages,
                &processed_msg_send,
            ));

            match chatgpt_response.to_owned() {
                Ok(message) => {
                    messages.push(message);
                }
                Err(error) => {
                    messages.pop();
                    processed_msg_send
                        .send(ProcessedMessage::ChatMessage(Err(error)))
                        .unwrap();
                }
            }
        }
    });

    // Use default terminal colors
    let (theme, syntax_set, code_theme) = theme(&mut siv);
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
                LinearLayout::vertical()
                    .child(Panel::new(
                        ScrollView::new(LinearLayout::vertical().with_name("messages_container"))
                            .scroll_strategy(ScrollStrategy::StickToBottom)
                            .full_height(),
                    ))
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
                    ))
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
                                view.add_child(TextView::new(format_message(
                                    &syntax_set,
                                    &code_theme,
                                    &Message {
                                        role: Role::Assistant,
                                        content: "".to_string(),
                                    },
                                )));
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
                                view.add_child(TextView::new(format_message(
                                    &syntax_set,
                                    &code_theme,
                                    &m,
                                )));
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
                    view.add_child(TextView::new(format_message(
                        &syntax_set,
                        &code_theme,
                        &Message {
                            role: Role::Assistant,
                            content: String::from_utf8(vec![b'.'; m as usize]).unwrap(),
                        },
                    )));
                });
            }
        }

        runner.refresh();
    }
}

fn theme(siv: &mut Cursive) -> (Theme, SyntaxSet, HighlightingTheme) {
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

    let syntax_set = SyntaxSet::load_defaults_newlines();
    let theme_set: ThemeSet = from_binary(include_bytes!("../assets/ansi.bin"));
    let code_theme = theme_set.themes["ansi"].to_owned();

    (theme, syntax_set, code_theme)
}
