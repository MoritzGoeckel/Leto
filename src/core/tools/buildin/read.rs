use crate::core::Tool;
use crate::core::tools::ExecutableTool;
use serde_json::{Value, json};

pub(super) fn make_read_tool() -> ExecutableTool {
    ExecutableTool {
        definition: Tool {
            name: "read".to_string(),
            description: "Read a file, optionally selecting a range of characters.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path to the file to read." },
                    "offset": { "type": "integer", "description": "Number of characters to skip." },
                    "limit": { "type": "integer", "description": "Maximum number of characters to read." }
                },
                "required": ["file"],
                "additionalProperties": false
            }),
            constrained_sampling: None,
        },
        handler: Box::new(|_, parameters: Value| {
            let file = parameters["file"].as_str().unwrap();
            let content = std::fs::read_to_string(file).map_err(|error| error.to_string())?;
            let offset = parameters["offset"].as_u64().unwrap_or(0) as usize;
            let limit = parameters["limit"].as_u64().map(|value| value as usize);
            let content: String = content
                .chars()
                .skip(offset)
                .take(limit.unwrap_or(usize::MAX))
                .collect();
            Ok(json!(content))
        }),
    }
}
