mod bash;
mod edit;
mod read;
mod write;

use super::ExecutableTool;

pub fn make_default_tools() -> Vec<ExecutableTool> {
    vec![
        bash::make_bash_tool(),
        read::make_read_tool(),
        write::make_write_tool(),
        edit::make_edit_tool(),
    ]
}
