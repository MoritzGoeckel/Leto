use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn list_conversations() -> std::io::Result<Vec<PathBuf>> {
    let directory = Path::new(".orpheus/conversations");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    Ok(fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?)
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
    directory: PathBuf,
    filename: String,
    file: Option<BufWriter<File>>,
    pending: Vec<Event>,
}

impl EventLog {
    pub fn create() -> std::io::Result<Self> {
        let working_directory = std::env::current_dir()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let seconds = now.as_secs() as libc::time_t;
        let mut local_time = unsafe { std::mem::zeroed::<libc::tm>() };
        if unsafe { libc::localtime_r(&seconds, &mut local_time) }.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let mut log = Self {
            directory: working_directory.join(".orpheus/conversations"),
            filename: format!(
                "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}.jsonl",
                local_time.tm_year + 1900,
                local_time.tm_mon + 1,
                local_time.tm_mday,
                local_time.tm_hour,
                local_time.tm_min,
                local_time.tm_sec,
            ),
            file: None,
            pending: Vec::new(),
        };
        log.append(
            EventValue::Pwd(working_directory.display().to_string()),
            false,
        )?;
        Ok(log)
    }

    pub fn append(&mut self, event: EventValue, flush: bool) -> std::io::Result<()> {
        self.pending.push(Event {
            event,
            timestamp: crate::core::now_ms(),
        });
        if !flush {
            return Ok(());
        }

        if self.file.is_none() {
            fs::create_dir_all(&self.directory)?;
            let gitignore = self.directory.parent().unwrap().join(".gitignore");
            if !gitignore.exists() {
                fs::write(gitignore, "*\n")?;
            }
            let path = self.directory.join(&self.filename);
            self.file = Some(BufWriter::new(
                OpenOptions::new().create(true).append(true).open(path)?,
            ));
        }
        let file = self.file.as_mut().unwrap();
        for event in &self.pending {
            serde_json::to_writer(&mut *file, event)?;
            file.write_all(b"\n")?;
        }
        self.pending.clear();
        file.flush()?;
        Ok(())
    }
}

impl From<File> for EventLog {
    fn from(file: File) -> Self {
        Self {
            directory: PathBuf::new(),
            filename: String::new(),
            file: Some(BufWriter::new(file)),
            pending: Vec::new(),
        }
    }
}
