use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum EventValue {
    Pwd(String),
    SystemPrompt(String),
    UserMessage(serde_json::Value),
    AssistantMessage(serde_json::Value),
    ToolResult(serde_json::Value),
    Command(String),
}

#[derive(Serialize)]
pub struct Event {
    #[serde(flatten)]
    pub event: EventValue,
    pub timestamp: u64,
}

pub struct EventLog {
    file: BufWriter<File>,
}

impl EventLog {
    pub fn create(path: impl AsRef<Path>, working_directory: String) -> std::io::Result<Self> {
        let mut log = Self {
            file: BufWriter::new(OpenOptions::new().create(true).append(true).open(path)?),
        };
        log.append(EventValue::Pwd(working_directory))?;
        Ok(log)
    }

    pub fn append(&mut self, event: EventValue) -> std::io::Result<()> {
        serde_json::to_writer(
            &mut self.file,
            &Event {
                event,
                timestamp: crate::core::now_ms(),
            },
        )?;
        self.file.write_all(b"\n")?;
        self.file.flush()
    }
}

impl From<File> for EventLog {
    fn from(file: File) -> Self {
        Self {
            file: BufWriter::new(file),
        }
    }
}
