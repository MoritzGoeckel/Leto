use crate::{
    config::Config,
    core::{
        AssistantContent, AssistantMessage, TextContent, Tool, ToolResultContent,
        ToolResultMessage, now_ms,
    },
    plugins::PluginManager,
    ui::tui::Tui,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub fn make_default_tools() -> Vec<Tool> {
    Vec::new()
}

pub struct ToolRuntime {
    pub ui: Arc<Mutex<Tui>>,
    pub config: Arc<Config>,
    pub plugins: Arc<Mutex<PluginManager>>,
    pub tools: HashMap<String, Tool>,
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

    pub fn add_tool(&mut self, tool: Tool) {
        self.tools.insert(tool.name.clone(), tool);
    }

    pub fn add_tools(&mut self, tools: Vec<Tool>) {
        for tool in tools {
            self.add_tool(tool);
        }
    }

    pub fn run_tool_calls(&mut self, message: AssistantMessage) -> Vec<ToolResultMessage> {
        // TODO: Notify tool call
        // TODO: Transform tool call 
        message
            .content
            .into_iter()
            .filter_map(|content| match content {
                AssistantContent::ToolCall(call) => Some(ToolResultMessage {
                    tool_call_id: call.id,
                    tool_name: call.name.clone(),
                    content: vec![ToolResultContent::Text(TextContent {
                        text: format!("Unknown tool: {}", call.name),
                        text_signature: None,
                    })],
                    details: None,
                    usage: None,
                    nested_calls: None,
                    is_error: true,
                    timestamp: now_ms(),
                }),
                _ => None,
            })
            .collect()
    }
}
