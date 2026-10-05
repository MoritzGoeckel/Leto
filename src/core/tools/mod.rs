use crate::{config::Config, plugins::PluginManager, ui::Ui};
use serde_json::Value;

pub mod buildin;
mod runtime;

pub use runtime::ToolRuntime;

pub type ToolHandler = Box<dyn FnMut(&mut ToolContext<'_>, Value) -> Result<Value, String> + Send>;

pub struct ExecutableTool {
    pub definition: crate::core::Tool,
    pub handler: ToolHandler,
}

pub struct ToolContext<'a> {
    pub ui: &'a mut dyn Ui,
    pub config: &'a Config,
    pub plugins: &'a mut PluginManager,
}
