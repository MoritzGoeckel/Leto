use std::io;

use crate::core::Message;

pub mod tui;

pub type BlockingId = usize;

pub trait Ui {
    fn get_input(&mut self, message: &str) -> io::Result<String>;
    fn inform_blocking(&mut self, message: &str) -> BlockingId;
    fn close(&mut self, id: BlockingId);
    fn add_message(&mut self, message: &Message);
}
