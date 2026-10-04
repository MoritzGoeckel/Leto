use super::plugin_manager::{Event, PluginManager};
use crate::core::{AssistantMessage, UserMessage};
use serde_json::json;
use std::io;

impl PluginManager {
    pub fn notify_init(&mut self) -> io::Result<()> {
        self.invoke_without_params(Event::OnInit)
    }

    pub fn notify_new_conversation(&mut self) -> io::Result<()> {
        self.invoke_without_params(Event::OnNewConversation)
    }

    pub fn transform_user_message(&mut self, message: UserMessage) -> io::Result<UserMessage> {
        let params = serde_json::to_value(message).map_err(io::Error::other)?;
        serde_json::from_value(self.invoke_wait(Event::OnUserMessage, params)?)
            .map_err(io::Error::other)
    }

    pub fn notify_assistant_message(&mut self, message: &AssistantMessage) -> io::Result<()> {
        self.invoke(Event::OnAssistantMessage, json!({"message": message}))
    }

    pub fn notify_exit(&mut self) -> io::Result<()> {
        self.invoke_without_params(Event::OnExit)
    }
}
