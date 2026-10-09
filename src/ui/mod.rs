use std::io;

use crate::core::Message;

pub mod tui;

pub trait Ui: Send {
    fn wait_for_next_prompt(&mut self) -> io::Result<String>;
    fn start_working(&mut self);
    fn stop_working(&mut self);
    fn note(&mut self, note: &str);
    fn on_message(&mut self, message: &Message);
    fn on_command(&mut self, command: &str);
}
