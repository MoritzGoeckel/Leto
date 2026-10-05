macro_rules! events {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub(super) enum Event {
            $($variant),+
        }

        impl Event {
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),+
                }
            }

            pub fn from_str(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

events! {
    OnInit => "on_init",
    OnNewConversation => "on_new_conversation",
    OnUserMessage => "on_user_message",
    OnAssistantMessage => "on_assistant_message",
    OnToolCall => "tool_call",
    OnToolResult => "tool_result",
    OnExit => "on_exit",
}

use super::plugin::{HostMethod, HostMethods, Plugin};
use crate::config::Config;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

pub struct PluginManager {
    plugins: HashMap<usize, Plugin>,
    subscriptions: HashMap<Event, Vec<usize>>,
    next_plugin_id: usize,
    host_methods: HostMethods,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            subscriptions: HashMap::new(),
            next_plugin_id: 0,
            host_methods: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn start(&mut self, config: &Config) -> io::Result<()> {
        for path in config.plugin_paths()? {
            let mut child = Command::new(&path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            let stdin = child.stdin.take().unwrap();
            let stdout = BufReader::new(child.stdout.take().unwrap());
            let plugin = Plugin::new(child, stdin, stdout, Arc::clone(&self.host_methods));
            self.plugins.insert(self.next_plugin_id, plugin);
            self.next_plugin_id += 1;
        }
        Ok(())
    }

    pub fn init_plugins(&mut self) -> io::Result<()> {
        let mut plugin_ids: Vec<usize> = self.plugins.keys().copied().collect();
        plugin_ids.sort_unstable();
        for plugin_id in plugin_ids {
            self.init_plugin(plugin_id)?;
        }
        Ok(())
    }

    pub fn register_method(
        &mut self,
        name: impl Into<String>,
        method: impl FnMut(Value) -> io::Result<Value> + Send + 'static,
    ) {
        let method: HostMethod = Box::new(method);
        self.host_methods
            .lock()
            .unwrap()
            .insert(name.into(), Arc::new(Mutex::new(method)));
    }

    fn init_plugin(&mut self, plugin_id: usize) -> io::Result<()> {
        let plugin = self.plugins.get_mut(&plugin_id).unwrap();
        let result = plugin.invoke_and_wait("init", json!({}))?;
        let events = result["hooks"].as_array().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "plugin init `hooks` must be an array",
            )
        })?;
        plugin.hooks = events
            .iter()
            .map(|value| {
                let name = value.as_str().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "plugin hook must be a string")
                })?;
                Event::from_str(name).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("unknown plugin hook: {name}"),
                    )
                })
            })
            .collect::<io::Result<Vec<_>>>()?;

        for event in &plugin.hooks {
            self.subscriptions
                .entry(*event)
                .or_default()
                .push(plugin_id);
        }
        Ok(())
    }

    pub(super) fn invoke_without_params(&mut self, event: Event) -> io::Result<()> {
        self.invoke(event, json!({}))
    }

    pub(super) fn invoke(&mut self, event: Event, params: Value) -> io::Result<()> {
        for plugin_id in self.subscriptions.get(&event).cloned().unwrap_or_default() {
            self.plugins
                .get_mut(&plugin_id)
                .unwrap()
                .invoke(event.as_str(), params.clone())?;
        }
        Ok(())
    }

    pub(super) fn invoke_wait(&mut self, event: Event, mut params: Value) -> io::Result<Value> {
        for plugin_id in self.subscriptions.get(&event).cloned().unwrap_or_default() {
            params = self
                .plugins
                .get_mut(&plugin_id)
                .unwrap()
                .invoke_and_wait(event.as_str(), params)?;
        }
        Ok(params)
    }
}
