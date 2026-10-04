use super::hooks::Hook;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout};
use std::sync::mpsc::{self, Receiver};
use std::thread;

pub(crate) struct Plugin {
    child: Child,
    stdin: ChildStdin,
    incoming: Receiver<io::Result<Value>>,
    pending: VecDeque<Value>,
    next_id: u64,
    pub(crate) hooks: Vec<Hook>,
}

impl Plugin {
    pub(crate) fn new(child: Child, stdin: ChildStdin, stdout: BufReader<ChildStdout>) -> Self {
        let (sender, incoming) = mpsc::channel();
        thread::spawn(move || Self::listen(stdout, sender));
        Self {
            child,
            stdin,
            incoming,
            pending: VecDeque::new(),
            next_id: 1,
            hooks: Vec::new(),
        }
    }

    fn listen(stdout: BufReader<ChildStdout>, sender: mpsc::Sender<io::Result<Value>>) {
        for line in stdout.lines() {
            let message = line
                .map_err(io::Error::other)
                .and_then(|line| serde_json::from_str(&line).map_err(io::Error::other));
            if sender.send(message).is_err() {
                return;
            }
        }
        let _ = sender.send(Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "plugin stdout closed",
        )));
    }

    pub(crate) fn invoke(&mut self, name: &str, params: Value) -> io::Result<()> {
        self.send_invocation(name, params).map(|_| ())
    }

    pub(crate) fn invoke_and_wait(&mut self, name: &str, params: Value) -> io::Result<Value> {
        let id = self.send_invocation(name, params)?;
        let response = self.wait_for_id(id)?;
        assert_eq!(response["type"], "return");
        assert_eq!(response["name"], name);
        Ok(response["value"].clone())
    }

    fn send_invocation(&mut self, name: &str, params: Value) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        serde_json::to_writer(
            &mut self.stdin,
            &json!({"type":"hook", "id":id, "name":name, "params":params}),
        )
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(id)
    }

    fn wait_for_id(&mut self, id: u64) -> io::Result<Value> {
        loop {
            if let Some(index) = self.pending.iter().position(|message| message["id"] == id) {
                return Ok(self.pending.remove(index).unwrap());
            }
            self.pending
                .push_back(self.incoming.recv().map_err(io::Error::other)??);
        }
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
