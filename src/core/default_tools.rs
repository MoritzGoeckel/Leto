use crate::core::Tool;
use crate::core::tools::ExecutableTool;
use serde_json::{Value, json};

pub fn make_default_tools() -> Vec<ExecutableTool> {
    vec![ExecutableTool {
        definition: Tool {
            name: "read".to_string(),
            description: "Read a file, optionally selecting a range of character positions."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path to the file to read." },
                    "from": { "type": "integer", "description": "First character position to include." },
                    "to": { "type": "integer", "description": "Character position to stop before." }
                },
                "required": ["file"],
                "additionalProperties": false
            }),
            constrained_sampling: None,
        },
        handler: Box::new(|_, parameters: Value| {
            let file = parameters["file"].as_str().unwrap();
            let content = std::fs::read_to_string(file).map_err(|error| error.to_string())?;
            let from = parameters["from"].as_u64().unwrap_or(0) as usize;
            let to = parameters["to"].as_u64().map(|position| position as usize);
            let content: String = content
                .chars()
                .skip(from)
                .take(to.map(|end| end.saturating_sub(from)).unwrap_or(usize::MAX))
                .collect();
            Ok(json!({ "content": content }))
        }),
    }]
}
