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

#[derive(Clone)]
pub struct StrOptions {
    pub background: Color,
    pub foreground: Color,
}

impl StrOptions {
    pub const ERROR: Self = Self {
        background: Color::Reset,
        foreground: Color::Red,
    };
}

pub trait Ui: Send {
    fn ask_styled(&mut self, options: AskOptions) -> io::Result<String>;
    fn start_working(&mut self);
    fn stop_working(&mut self);
    fn set_alert(&mut self, message: &str);
    fn clear_alert(&mut self);
    fn take_cancel(&mut self) -> bool {
        false
    }
    fn append_message_str(&mut self, note: &str);
    fn append_message_str_styled(&mut self, note: &str, options: StrOptions);
    fn append_message(&mut self, message: &Message);
    fn append_command(&mut self, command: &str);
    fn append_stream_delta(&mut self, delta: &str);
}
