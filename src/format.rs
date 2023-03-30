use comrak::nodes::AstNode;
use comrak::nodes::NodeValue::{
    BlockQuote, Code, CodeBlock, DescriptionDetails, DescriptionItem, DescriptionList,
    DescriptionTerm, Document, Emph, FootnoteDefinition, FootnoteReference, FrontMatter, Heading,
    HtmlBlock, HtmlInline, Image, Item, LineBreak, Link, List, Paragraph, SoftBreak, Strikethrough,
    Strong, Superscript, Table, TableCell, TableRow, TaskItem, Text, ThematicBreak,
};
use comrak::nodes::{ListDelimType, ListType};
use comrak::{parse_document, Arena, ComrakOptions};
use cursive::reexports::enumset::enum_set;
use cursive::theme::{BaseColor, Color, ColorStyle, ColorType, Effect, Style};
use cursive::utils::markup::{StyledIndexedSpan, StyledString};
use cursive::utils::span::IndexedCow;
use cursive_syntect::translate_effects;
use std::str;
use syntect::easy::HighlightLines;
use syntect::highlighting::Style as HighlightingStyle;
use syntect::parsing::SyntaxSet;
use syntect::Error;

use crate::{Message, Role};

pub fn format_message(
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
    let mut options = ComrakOptions::default();
    options.extension.strikethrough = true;
    options.extension.tasklist = true;

    let formatted_contents = match m.role {
        Role::User => StyledString::from(m.content.trim()),
        Role::Assistant => format_markdown(
            syntax_set,
            theme,
            parse_document(&arena, m.content.trim(), &options),
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

    let mut style_stack: Vec<Style> = Vec::new();

    // the current index for ordered lists
    let mut list_index = 0;
    let mut list_indentation_level = 0;

    while let Some((node, entering)) = stack.pop() {
        if entering {
            // we will re-process the node after we have processed all of their children
            // this is useful for rendering newlines after lists, for example.
            stack.push((node, false));
        }

        match node.data.borrow().value {
            Document => {}
            FrontMatter(..) => {}
            List(ref list_node) => {
                if entering {
                    list_indentation_level += 1;
                    match list_node.list_type {
                        ListType::Bullet => {}
                        ListType::Ordered => {
                            list_index = list_node.start;
                        }
                    }
                } else {
                    list_indentation_level -= 1;
                    if list_indentation_level == 0 {
                        string.append_plain("\n")
                    }
                };
            }
            Item(ref list_node) => {
                if entering {
                    match list_node.list_type {
                        ListType::Bullet => {
                            string.append_plain(format!(
                                "{: <1$}",
                                "",
                                (list_indentation_level - 1) * 2
                            ));
                            string.append_plain(format!(
                                "{} ",
                                str::from_utf8(&[list_node.bullet_char]).unwrap()
                            ));
                        }
                        ListType::Ordered => {
                            let delimiter = match list_node.delimiter {
                                ListDelimType::Period => ".",
                                ListDelimType::Paren => ")",
                            };

                            string.append_plain(format!(
                                "{: <1$}",
                                "",
                                (list_indentation_level - 1) * 2
                            ));
                            string.append_plain(format!("{list_index}{delimiter} "));
                            list_index += 1;
                        }
                    }
                };
            }
            TaskItem { checked, symbol } => {
                if entering {
                    let sym = &[symbol];

                    string.append_plain("[");
                    string.append_plain(if checked {
                        str::from_utf8(sym).unwrap()
                    } else {
                        " "
                    });
                    string.append_plain("] ");
                }
            }
            BlockQuote => {
                if entering {
                    style_stack.push(Style {
                        effects: enum_set!(Effect::Dim),
                        color: ColorStyle::inherit_parent(),
                    });
                } else {
                    style_stack.pop();
                }
            }
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
                        if list_indentation_level > 0 {
                            string.append_plain("\n");
                        } else {
                            string.append_plain("\n\n");
                        }
                    }
                };
            }
            Text(ref text) => {
                if entering {
                    string.append_styled(
                        String::from_utf8(text.to_owned()).unwrap(),
                        Style::merge(&style_stack),
                    );
                }
            }
            DescriptionDetails => {}
            ThematicBreak => {}
            FootnoteDefinition(..) => {}
            FootnoteReference(..) => {}
            Heading(ref heading_node) => {
                if entering {
                    style_stack.push(Style {
                        effects: enum_set!(),
                        color: ColorStyle::new(BaseColor::Red, ColorType::InheritParent),
                    });
                    string.append_styled(
                        format!("{:#<1$} ", "", heading_node.level.into()),
                        Style::merge(&style_stack),
                    );
                    style_stack.pop();
                    style_stack.push(Style {
                        effects: enum_set!(),
                        color: ColorStyle::new(BaseColor::Green, ColorType::InheritParent),
                    });
                } else {
                    style_stack.pop();
                    string.append_plain("\n\n")
                }
            }
            Table(..) => {}
            TableRow(..) => {}
            TableCell => {}
            DescriptionTerm => {}
            SoftBreak => {
                if entering {
                    string.append_plain(" ")
                }
            }
            LineBreak => {
                if entering {
                    string.append_plain("kjasdfjklhasdfklhjasdlkfhj")
                }
            }
            Emph => {
                if entering {
                    style_stack.push(Style {
                        effects: enum_set!(Effect::Italic),
                        color: ColorStyle::inherit_parent(),
                    });
                } else {
                    style_stack.pop();
                }
            }
            Strong => {
                if entering {
                    style_stack.push(Style {
                        effects: enum_set!(Effect::Bold),
                        color: ColorStyle::inherit_parent(),
                    });
                } else {
                    style_stack.pop();
                }
            }
            Strikethrough => {
                if entering {
                    style_stack.push(Style {
                        effects: enum_set!(Effect::Strikethrough),
                        color: ColorStyle::inherit_parent(),
                    });
                } else {
                    style_stack.pop();
                }
            }
            Superscript => {}
            Link(ref link_node) => {
                if !entering {
                    string.append_styled(" (", Style::merge(&style_stack));

                    style_stack.push(Style {
                        effects: enum_set!(Effect::Underline),
                        color: ColorStyle::new(BaseColor::Blue, ColorType::InheritParent),
                    });

                    string.append_styled(
                        str::from_utf8(&link_node.url).unwrap(),
                        Style::merge(&style_stack),
                    );

                    style_stack.pop();

                    string.append_styled(")", Style::merge(&style_stack));
                }
            }
            Image(ref link_node) => {
                if !entering {
                    string.append_styled(" (", Style::merge(&style_stack));

                    style_stack.push(Style {
                        effects: enum_set!(Effect::Underline),
                        color: ColorStyle::new(BaseColor::Blue, ColorType::InheritParent),
                    });

                    string.append_styled(
                        str::from_utf8(&link_node.url).unwrap(),
                        Style::merge(&style_stack),
                    );

                    style_stack.pop();

                    string.append_styled(")", Style::merge(&style_stack));
                }
            }
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
