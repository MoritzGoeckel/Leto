use std::io::{self, Write};

use crate::core::{
    AssistantContent, Message, SystemContent, ToolResultContent, UserContent, UserContentBlock,
};

use super::{BlockingId, Ui};

pub struct Tui;

impl Ui for Tui {
    fn get_input(&mut self, message: &str) -> io::Result<String> {
        print!("{message}");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input.trim().to_owned())
    }

    fn inform_blocking(&mut self, message: &str) -> BlockingId {
        println!("{message}");
        0
    }

    fn close(&mut self, _id: BlockingId) {}

    fn add_message(&mut self, message: &Message) {
        match message {
            Message::System(message) => match &message.content {
                SystemContent::Text(text) => println!("system> {text}"),
                SystemContent::Blocks(blocks) => {
                    for block in blocks {
                        println!("system> {}", block.text);
                    }
                }
            },
            Message::User(message) => match &message.content {
                UserContent::Text(text) => println!("you> {text}"),
                UserContent::Blocks(blocks) => {
                    for block in blocks {
                        match block {
                            UserContentBlock::Text(text) => println!("you> {}", text.text),
                            UserContentBlock::Image(image) => {
                                println!("you> [image: {}]", image.mime_type)
                            }
                        }
                    }
                }
            },
            Message::Assistant(message) => {
                for content in &message.content {
                    match content {
                        AssistantContent::Text(text) => println!("assistant> {}", text.text),
                        AssistantContent::Thinking(thinking) => {
                            println!("thinking> {}", thinking.thinking)
                        }
                        AssistantContent::ToolCall(call) => {
                            println!("tool call> {} {:?}", call.name, call.arguments)
                        }
                    }
                }
            }
            Message::ToolResult(message) => {
                for content in &message.content {
                    match content {
                        ToolResultContent::Text(text) => {
                            println!("{}> {}", message.tool_name, text.text)
                        }
                        ToolResultContent::Image(image) => {
                            println!("{}> [image: {}]", message.tool_name, image.mime_type)
                        }
                    }
                }
            }
        }
    }
}
