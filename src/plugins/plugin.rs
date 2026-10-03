use super::hooks::Hook;
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout};

pub(crate) struct Plugin {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    pub(crate) hooks: Vec<Hook>,
}

impl Plugin {
    pub(crate) fn new(child: Child, stdin: ChildStdin, stdout: BufReader<ChildStdout>) -> Self {
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
            hooks: Vec::new(),
        }
    }

    pub(crate) fn request(&mut self, method: &str, params: Value) -> io::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        serde_json::to_writer(
            &mut self.stdin,
            &json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}),
        )
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        let mut line = String::new();
        self.stdout.read_line(&mut line)?;
        let response: Value = serde_json::from_str(&line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        assert_eq!(response["id"], id);
        if !response.get("error").unwrap_or(&Value::Null).is_null() {
            return Err(io::Error::other(response["error"].to_string()));
        }
        Ok(response["result"].clone())
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
