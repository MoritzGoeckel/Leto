use crate::core::{AssistantMessageEvent, Context, Model, StreamOptions};
use crate::ui::Ui;
use std::collections::BTreeMap;
use std::fmt;

pub mod openai_chatgpt;

#[derive(Debug)]
pub enum AuthError {
    NotLoggedIn,
    Provider(Box<dyn std::error::Error>),
}

impl fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotLoggedIn => formatter.write_str("not logged in"),
            Self::Provider(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for AuthError {}

pub trait Provider {
    fn auth_refresh(&mut self) -> Result<(), AuthError>;
    fn auth_is_valid(&self) -> bool;
    fn auth_login(&mut self, ui: &mut dyn Ui) -> Result<(), Box<dyn std::error::Error>>;
    fn stream(
        &self,
        model: &Model,
        context: &Context,
        options: &StreamOptions,
    ) -> Result<Vec<AssistantMessageEvent>, Box<dyn std::error::Error>>;
    fn get_models(&self) -> BTreeMap<String, Model>;
}
