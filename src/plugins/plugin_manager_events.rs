use super::plugin_manager::{Event, PluginManager};
use crate::core::{AssistantMessage, ToolCall, ToolResultMessage, UserMessage};
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

    pub fn notify_tool_call(&mut self, call: &ToolCall) -> io::Result<()> {
        self.invoke(
            Event::OnToolCall,
            json!({
                "type": "tool_call",
                "toolCallId": call.id,
                "toolName": call.name,
                "input": call.arguments,
            }),
        )
    }

    pub fn transform_tool_result(
        &mut self,
        result: ToolResultMessage,
    ) -> io::Result<ToolResultMessage> {
        let params = json!({
            "type": "tool_result",
            "toolCallId": result.tool_call_id,
            "toolName": result.tool_name,
            "input": {},
            "content": result.content,
            "details": result.details,
            "isError": result.is_error,
            "usage": result.usage,
        });
        let transformed = self.invoke_wait(Event::OnToolResult, params)?;
        let mut result = result;
        if let Some(content) = transformed.get("content") {
            result.content = serde_json::from_value(content.clone()).map_err(io::Error::other)?;
        }
        if let Some(details) = transformed.get("details") {
            result.details = if details.is_null() {
                None
            } else {
                Some(details.clone())
            };
        }
        if let Some(is_error) = transformed
            .get("isError")
            .and_then(serde_json::Value::as_bool)
        {
            result.is_error = is_error;
        }
        if let Some(usage) = transformed.get("usage") {
            result.usage = if usage.is_null() {
                None
            } else {
                Some(serde_json::from_value(usage.clone()).map_err(io::Error::other)?)
            };
        }
        Ok(result)
    }

    pub fn notify_exit(&mut self) -> io::Result<()> {
        self.invoke_without_params(Event::OnExit)
    }
}
