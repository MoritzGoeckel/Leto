use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

#[derive(Deserialize)]
struct LoadedEvent {
    #[serde(rename = "type")]
    event_type: String,
    value: Option<serde_json::Value>,
}

pub fn load_context(path: impl AsRef<Path>) -> std::io::Result<crate::core::Context> {
    let mut context = crate::core::Context::default();
    for line in BufReader::new(File::open(path)?).lines() {
        let event: LoadedEvent = serde_json::from_str(&line?).map_err(std::io::Error::other)?;
        match event.event_type.as_str() {
            "system_prompt" => {
                context.system_prompt = Some(
                    serde_json::from_value(event.value.unwrap()).map_err(std::io::Error::other)?,
                )
            }
            "user_message" => context.messages.push(crate::core::Message::User(
                serde_json::from_value(event.value.unwrap()).map_err(std::io::Error::other)?,
            )),
            "assistant_message" => context.messages.push(crate::core::Message::Assistant(
                serde_json::from_value(event.value.unwrap()).map_err(std::io::Error::other)?,
            )),
            "tool_result" => context.messages.push(crate::core::Message::ToolResult(
                serde_json::from_value(event.value.unwrap()).map_err(std::io::Error::other)?,
            )),
            "pwd" | "command" => {}
            event_type => panic!("unexpected event type: {event_type}"),
        }
    }
    Ok(context)
}

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
