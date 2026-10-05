use crate::core::Tool;
use crate::core::tools::ExecutableTool;
use serde_json::{Value, json};

pub(super) fn make_write_tool() -> ExecutableTool {
    ExecutableTool {
        definition: Tool {
            name: "write".to_string(),
            description: "Create or overwrite a file with the provided content.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path to the file to write." },
                    "content": { "type": "string", "description": "Content to write to the file." }
                },
                "required": ["file", "content"],
                "additionalProperties": false
            }),
            constrained_sampling: None,
        },
        handler: Box::new(|_, parameters: Value| {
            let file = parameters["file"].as_str().unwrap();
            let content = parameters["content"].as_str().unwrap();
            std::fs::write(file, content).map_err(|error| error.to_string())?;
            Ok(json!(format!("Wrote {file}")))
        }),
    }
}
