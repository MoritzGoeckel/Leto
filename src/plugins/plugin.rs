use super::plugin_manager::Event;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

pub(crate) type HostMethod = Box<dyn FnMut(Value) -> io::Result<Value> + Send>;
pub(crate) type HostMethods = Arc<Mutex<HashMap<String, Arc<Mutex<HostMethod>>>>>;

pub(crate) struct Plugin {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    incoming: Receiver<io::Result<Value>>,
    pending: VecDeque<Value>,
    next_id: u64,
    pub(crate) hooks: Vec<Event>,
}

impl Plugin {
    pub(crate) fn new(
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
        host_methods: HostMethods,
    ) -> Self {
        let (sender, incoming) = mpsc::channel();
        let stdin = Arc::new(Mutex::new(stdin));
        thread::spawn({
            let stdin = Arc::clone(&stdin);
            move || Self::listen(stdout, sender, stdin, host_methods)
        });
        Self {
            child,
            stdin,
            incoming,
            pending: VecDeque::new(),
            next_id: 1,
            hooks: Vec::new(),
        }
    }

    fn listen(
        stdout: BufReader<ChildStdout>,
        sender: mpsc::Sender<io::Result<Value>>,
        stdin: Arc<Mutex<ChildStdin>>,
        host_methods: HostMethods,
    ) {
        for line in stdout.lines() {
            let message = line
                .map_err(io::Error::other)
                .and_then(|line| serde_json::from_str(&line).map_err(io::Error::other));
            let message: Value = match message {
                Ok(message) => message,
                Err(error) => {
                    let _ = sender.send(Err(error));
                    return;
                }
            };
            match message["type"].as_str().unwrap() {
                "invoke" => match Self::handle_invoke(&message, &stdin, &host_methods) {
                    Ok(()) => {}
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        return;
                    }
                },
                "return" => {
                    if sender.send(Ok(message)).is_err() {
                        return;
                    }
                }
                message_type => {
                    let _ = sender.send(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("unexpected plugin message type: {message_type}"),
                    )));
                    return;
                }
            }
        }
        let _ = sender.send(Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "plugin stdout closed",
        )));
    }

    fn handle_invoke(
        message: &Value,
        stdin: &Arc<Mutex<ChildStdin>>,
        host_methods: &HostMethods,
    ) -> io::Result<()> {
        let name = message["name"].as_str().unwrap();
        let id = message["id"].clone();
        let method = host_methods.lock().unwrap().get(name).cloned();
        let value = method.map(|method| (method.lock().unwrap())(message["params"].clone()));
        let response = match value {
            Some(Ok(value)) => json!({"type":"return", "id":id, "name":name, "value":value}),
            Some(Err(error)) => {
                json!({"type":"return", "id":id, "name":name, "error":{"message":error.to_string()}})
            }
            None => {
                json!({"type":"return", "id":id, "name":name, "error":{"message":format!("unknown host method: {name}")}})
            }
        };
        Self::send(stdin, response)
    }

    pub(crate) fn invoke(&mut self, name: &str, params: Value) -> io::Result<()> {
        self.send_invocation(name, params).map(|_| ())
    }

    pub(crate) fn invoke_and_wait(&mut self, name: &str, params: Value) -> io::Result<Value> {
        let id = self.send_invocation(name, params)?;
        let response = self.wait_for_id(id)?;
        assert_eq!(response["type"], "return");
        assert_eq!(response["name"], name);
        if let Some(error) = response.get("error") {
            return Err(io::Error::other(error["message"].as_str().unwrap()));
        }
        Ok(response["value"].clone())
    }

    fn send_invocation(&mut self, name: &str, params: Value) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        Self::send(
            &self.stdin,
            json!({"type":"invoke", "id":id, "name":name, "params":params}),
        )?;
        Ok(id)
    }

    fn send(stdin: &Arc<Mutex<ChildStdin>>, message: Value) -> io::Result<()> {
        let mut stdin = stdin.lock().unwrap();
        serde_json::to_writer(&mut *stdin, &message)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        stdin.write_all(b"\n")?;
        stdin.flush()
    }

    fn wait_for_id(&mut self, id: u64) -> io::Result<Value> {
        loop {
            if let Some(index) = self
                .pending
                .iter()
                .position(|message| message["type"] == "return" && message["id"] == id)
            {
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
