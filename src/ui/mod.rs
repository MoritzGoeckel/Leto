use std::io;

use crate::core::Message;

pub mod tui;

pub trait Ui: Send {
    fn wait_for_next_prompt(&mut self) -> io::Result<String>;
    fn note(&mut self, note: &str);
    fn inform(&mut self, title: &str, message: &str);
    fn on_message(&mut self, message: &Message);
    fn on_command(&mut self, command: &str);
}