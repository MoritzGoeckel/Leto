use ratatui::style::Color;
use std::io;

use crate::core::Message;

pub mod tui;

pub const INPUT_BACKGROUND: Color = Color::Rgb(30, 30, 30);

#[derive(Clone)]
pub struct AskOptions {
    pub background: Color,
    pub foreground: Color,
    pub form_text: String,
    pub form_text_color: Color,
}

impl Default for AskOptions {
    fn default() -> Self {
        Self {
            background: INPUT_BACKGROUND,
            foreground: Color::White,
            form_text: String::new(),
            form_text_color: Color::DarkGray,
        }
    }
}

pub trait Ui: Send {
    fn ask(&mut self, options: AskOptions) -> io::Result<String>;
    fn start_working(&mut self);
    fn stop_working(&mut self);
    fn append_message_str(&mut self, note: &str);
    fn append_message(&mut self, message: &Message);
    fn append_command(&mut self, command: &str);
}
