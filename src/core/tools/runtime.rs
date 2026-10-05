use crate::core::tools::{ExecutableTool, ToolContext};
use crate::core::{
    AssistantContent, AssistantMessage, TextContent, ToolResultContent, ToolResultMessage, now_ms,
};
use crate::{config::Config, plugins::PluginManager, ui::tui::Tui};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct ToolRuntime {
    pub ui: Arc<Mutex<Tui>>,
    pub config: Arc<Config>,
    pub plugins: Arc<Mutex<PluginManager>>,
    pub tools: HashMap<String, ExecutableTool>,
}

impl ToolRuntime {
    pub fn new(
        ui: Arc<Mutex<Tui>>,
        config: Arc<Config>,
        plugins: Arc<Mutex<PluginManager>>,
    ) -> Self {
        Self {
            ui,
            config,
            plugins,
            tools: HashMap::new(),
        }
    }

    pub fn add_tool(&mut self, tool: ExecutableTool) {
        self.tools.insert(tool.definition.name.clone(), tool);
    }

    pub fn add_tools(&mut self, tools: Vec<ExecutableTool>) {
        for tool in tools {
            self.add_tool(tool);
        }
    }

    pub fn run_tool_calls(&mut self, message: AssistantMessage) -> Vec<ToolResultMessage> {
        message
            .content
            .into_iter()
            .filter_map(|content| match content {
                AssistantContent::ToolCall(call) => {
                    let call_hook_result = self.plugins.lock().unwrap().notify_tool_call(&call);
                    let arguments = call.arguments.clone();
                    let result = match call_hook_result {
                        Err(error) => Err(format!("Tool call hook failed: {error}")),
                        Ok(()) => {
                            if let Some(tool) = self.tools.get_mut(&call.name) {
                                let mut ui = self.ui.lock().unwrap();
                                let mut plugins = self.plugins.lock().unwrap();
                                let mut context = ToolContext {
                                    ui: &mut *ui,
                                    config: &self.config,
                                    plugins: &mut plugins,
                                };
                                (tool.handler)(&mut context, Value::Object(arguments))
                            } else {
                                Err(format!("Unknown tool: {}", call.name))
                            }
                        }
                    };
                    let (text, is_error) = match result {
                        Ok(value) => (value.to_string(), false),
                        Err(error) => (error, true),
                    };
                    let result = ToolResultMessage {
                        tool_call_id: call.id,
                        tool_name: call.name,
                        content: vec![ToolResultContent::Text(TextContent {
                            text,
                            text_signature: None,
                        })],
                        details: None,
                        usage: None,
                        nested_calls: None,
                        is_error,
                        timestamp: now_ms(),
                    };
                    let fallback = ToolResultMessage {
                        tool_call_id: result.tool_call_id.clone(),
                        tool_name: result.tool_name.clone(),
                        content: vec![ToolResultContent::Text(TextContent {
                            text: String::new(),
                            text_signature: None,
                        })],
                        details: result.details.clone(),
                        usage: result.usage.clone(),
                        nested_calls: result.nested_calls.clone(),
                        is_error: true,
                        timestamp: result.timestamp,
                    };
                    Some(
                        self.plugins
                            .lock()
                            .unwrap()
                            .transform_tool_result(result)
                            .unwrap_or_else(|error| ToolResultMessage {
                                content: vec![ToolResultContent::Text(TextContent {
                                    text: format!("Tool result hook failed: {error}"),
                                    text_signature: None,
                                })],
                                ..fallback
                            }),
                    )
                }
                _ => None,
            })
            .collect()
    }
}
