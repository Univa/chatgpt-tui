use comrak::nodes::AstNode;
use comrak::nodes::NodeValue::{
    BlockQuote, Code, CodeBlock, DescriptionDetails, DescriptionItem, DescriptionList,
    DescriptionTerm, Document, Emph, FootnoteDefinition, FootnoteReference, FrontMatter, Heading,
    HtmlBlock, HtmlInline, Image, Item, LineBreak, Link, List, Paragraph, SoftBreak, Strikethrough,
    Strong, Superscript, Table, TableCell, TableRow, TaskItem, Text, ThematicBreak,
};
use comrak::{parse_document, Arena, ComrakOptions};
use cursive::reexports::enumset::enum_set;
use cursive::theme::{BaseColor, Color, ColorStyle, ColorType, Effect, PaletteColor, Style, Theme};
use cursive::utils::markup::{StyledIndexedSpan, StyledString};
use cursive::utils::span::IndexedCow;
use cursive::view::{Nameable, Resizable, ScrollStrategy};
use cursive::views::{Dialog, EditView, LinearLayout, Panel, ScrollView, TextView};
use cursive::Cursive;
use cursive_syntect::translate_effects;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::channel;
use std::time::Duration;
use std::{env, str, thread};
use syntect::dumps::from_binary;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as HighlightingStyle, Theme as HighlightingTheme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::Error;

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

fn format_message(
    syntax_set: &SyntaxSet,
    theme: &syntect::highlighting::Theme,
    m: &Message,
) -> StyledString {
    let mut formatted_user = match m.role {
        Role::User => StyledString::styled(
            "You",
            Style {
                effects: enum_set!(Effect::Bold | Effect::Underline),
                color: ColorStyle::new(BaseColor::Cyan, ColorType::InheritParent),
            },
        ),
        Role::Assistant => StyledString::styled(
            "ChatGPT",
            Style {
                effects: enum_set!(Effect::Bold | Effect::Underline),
                color: ColorStyle::new(BaseColor::Magenta, ColorType::InheritParent),
            },
        ),
        Role::System => StyledString::styled(
            "System",
            Style {
                effects: enum_set!(Effect::Bold | Effect::Underline),
                color: ColorStyle::new(BaseColor::Green, ColorType::InheritParent),
            },
        ),
    };

    let arena = Arena::new();

    let formatted_contents = match m.role {
        Role::User => StyledString::from(m.content.trim()),
        Role::Assistant => format_markdown(
            syntax_set,
            theme,
            parse_document(&arena, m.content.trim(), &ComrakOptions::default()),
        ),
        Role::System => StyledString::from(m.content.trim()),
    };

    formatted_user.append_plain(": ");
    formatted_user.append(formatted_contents);
    formatted_user.append("\n\n");

    formatted_user
}

fn format_markdown<'a>(
    syntax_set: &SyntaxSet,
    theme: &syntect::highlighting::Theme,
    r: &'a AstNode<'a>,
) -> StyledString {
    let mut stack: Vec<(&AstNode, bool)> = vec![(r, true)];
    let mut string = StyledString::new();

    while let Some((node, entering)) = stack.pop() {
        if entering {
            stack.push((node, false));
        }

        match node.data.borrow().value {
            Document => {}
            FrontMatter(..) => {}
            List(..) => {}
            Item(..) => {}
            BlockQuote => {}
            DescriptionItem(..) => {}
            DescriptionList => {}
            Code(ref code_node) => {
                if entering {
                    string.append_plain('`');
                    string.append(StyledString::styled(
                        str::from_utf8(&code_node.literal).unwrap(),
                        Style {
                            effects: enum_set!(Effect::Bold),
                            color: ColorStyle::terminal_default(),
                        },
                    ));
                    string.append_plain('`');
                }
            }
            CodeBlock(ref code_node) => {
                if entering {
                    // We assume that the first tag in the info string is the language
                    let mut first_space_idx = 0;
                    while first_space_idx < code_node.info.len()
                        && !char::is_ascii_whitespace(&(code_node.info[first_space_idx] as char))
                    {
                        first_space_idx += 1;
                    }

                    let language = syntax_set
                        .find_syntax_by_token(
                            str::from_utf8(&code_node.info[..first_space_idx]).unwrap(),
                        )
                        .unwrap_or_else(|| {
                            syntax_set
                                .find_syntax_by_first_line(
                                    str::from_utf8(&code_node.literal).unwrap().trim_start(),
                                )
                                .unwrap_or_else(|| syntax_set.find_syntax_plain_text())
                        });

                    let parsed_code = {
                        let mut highlighter = HighlightLines::new(language, theme);
                        let parsed_code = parse_code(
                            str::from_utf8(&code_node.literal).unwrap(),
                            &mut highlighter,
                            syntax_set,
                        );

                        match parsed_code {
                            Ok(code) => code,
                            Err(_) => {
                                StyledString::from(str::from_utf8(&code_node.literal).unwrap())
                            }
                        }
                    };

                    // Append another newline because the end of code blocks usually have a newline already
                    string.append(parsed_code);
                    string.append_plain('\n');
                };
            }
            HtmlBlock(ref html_node) => {
                // If ChatGPT for some reason tries to send HTML tags in their response without any code fences, we'll just render the literal string.
                if entering {
                    string.append_plain(str::from_utf8(&html_node.literal).unwrap());
                    string.append_plain('\n');
                }
            }
            HtmlInline(ref inline_html) => {
                // See comment above
                if entering {
                    string.append_plain(str::from_utf8(inline_html).unwrap());
                }
            }
            Paragraph => {
                if !entering {
                    // Append new lines only if there is nothing but the document node in the stack (i.e. we're at the end of the markdown text)
                    if stack.len() > 1 {
                        string.append_plain("\n\n");
                    }
                };
            }
            Text(ref text) => {
                if entering {
                    string.append_styled(
                        String::from_utf8(text.to_owned()).unwrap(),
                        Style::terminal_default(),
                    );
                }
            }
            DescriptionDetails => {}
            ThematicBreak => {}
            FootnoteDefinition(..) => {}
            FootnoteReference(..) => {}
            Heading(..) => {}
            Table(..) => {}
            TableRow(..) => {}
            TableCell => {}
            TaskItem { checked, symbol } => {}
            DescriptionTerm => {}
            SoftBreak => {}
            LineBreak => {}
            Emph => {}
            Strong => {}
            Strikethrough => {}
            Superscript => {}
            Link(..) => {}
            Image(..) => {}
        };

        if entering {
            for child in node.reverse_children() {
                stack.push((child, true));
            }
        }
    }

    string
}

fn parse_code(
    input: &str,
    highlighter: &mut HighlightLines,
    syntax_set: &SyntaxSet,
) -> Result<StyledString, Error> {
    let mut spans: Vec<StyledIndexedSpan> = vec![];

    for line in input.split_inclusive('\n') {
        for (style, text) in highlighter.highlight_line(line, syntax_set)? {
            spans.push(StyledIndexedSpan {
                content: IndexedCow::from_str(text, input),
                attr: translate_style(style),
                width: text.len(),
            });
        }
    }

    Ok(StyledString::with_spans(input, spans))
}

fn translate_style(style: HighlightingStyle) -> Style {
    let foreground_color: Color = if style.foreground.a == 0 {
        Color::Dark(BaseColor::from(style.foreground.r))
    } else {
        Color::TerminalDefault
    };

    let background_color = Color::TerminalDefault;

    Style {
        effects: translate_effects(style.font_style),
        color: (foreground_color, background_color).into(),
    }
}
