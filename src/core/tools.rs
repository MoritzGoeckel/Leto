use crate::{
    config::Config,
    core::{
        AssistantContent, AssistantMessage, TextContent, Tool, ToolResultContent,
        ToolResultMessage, now_ms,
    },
    plugins::PluginManager,
    ui::{Ui, tui::Tui},
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub type ToolHandler = Box<dyn FnMut(&mut ToolContext<'_>, Value) -> Result<Value, String> + Send>;

pub struct ExecutableTool {
    pub definition: Tool,
    pub handler: ToolHandler,
}

pub struct ToolContext<'a> {
    pub ui: &'a mut dyn Ui,
    pub config: &'a Config,
    pub plugins: &'a mut PluginManager,
}

pub fn make_default_tools() -> Vec<ExecutableTool> {
    Vec::new()
}

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
                    let result = if let Some(tool) = self.tools.get_mut(&call.name) {
                        let mut ui = self.ui.lock().unwrap();
                        let mut plugins = self.plugins.lock().unwrap();
                        let mut context = ToolContext {
                            ui: &mut *ui,
                            config: &self.config,
                            plugins: &mut plugins,
                        };
                        (tool.handler)(&mut context, Value::Object(call.arguments))
                    } else {
                        Err(format!("Unknown tool: {}", call.name))
                    };
                    let (text, is_error) = match result {
                        Ok(value) => (value.to_string(), false),
                        Err(error) => (error, true),
                    };
                    Some(ToolResultMessage {
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
                    })
                }
                _ => None,
            })
            .collect()
    }
}
