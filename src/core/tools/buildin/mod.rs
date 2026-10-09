mod bash;

use super::ExecutableTool;

pub fn make_default_tools() -> Vec<ExecutableTool> {
    vec![bash::make_bash_tool()]
}
