use super::hooks::Hook;
use super::plugin::Plugin;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct PluginManager {
    plugins: HashMap<usize, Plugin>,
    hooks: HashMap<Hook, Vec<usize>>,
    next_plugin_id: usize,
}

impl PluginManager {
    pub fn start() -> io::Result<Self> {
        let config_path = std::env::current_dir()?.join("atlas.json");
        let config: Value = serde_json::from_reader(File::open(config_path)?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let mut manager = Self {
            plugins: HashMap::new(),
            hooks: HashMap::new(),
            next_plugin_id: 0,
        };

        let plugin_paths = config["plugins"].as_array().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "`plugins` must be an array")
        })?;

        for path in plugin_paths {
            let path = path.as_str().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "plugin path must be a string")
            })?;
            let path = PathBuf::from(path);
            let mut child = Command::new(&path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            let stdin = child.stdin.take().unwrap();
            let stdout = BufReader::new(child.stdout.take().unwrap());
            let plugin = Plugin::new(child, stdin, stdout);
            manager.plugins.insert(manager.next_plugin_id, plugin);
            manager.next_plugin_id += 1;
        }

        Ok(manager)
    }

    pub fn init_plugins(&mut self) -> io::Result<()> {
        let mut plugin_ids: Vec<usize> = self.plugins.keys().copied().collect();
        plugin_ids.sort_unstable();
        for plugin_id in plugin_ids {
            self.init_plugin(plugin_id)?;
        }
        Ok(())
    }

    pub fn init_plugin(&mut self, plugin_id: usize) -> io::Result<()> {
        let plugin = self.plugins.get_mut(&plugin_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("plugin {plugin_id} not found"),
            )
        })?;
        let result = plugin.request("init", json!({}))?;
        let hooks = result["hooks"].as_array().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "plugin init `hooks` must be an array",
            )
        })?;
        plugin.hooks = hooks
            .iter()
            .map(|value| {
                let name = value.as_str().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "plugin hook must be a string")
                })?;
                Hook::from_str(name).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("unknown plugin hook: {name}"),
                    )
                })
            })
            .collect::<io::Result<Vec<_>>>()?;

        for hook in &plugin.hooks {
            self.hooks.entry(*hook).or_default().push(plugin_id);
        }
        Ok(())
    }

    pub fn hooks(&self) -> &HashMap<Hook, Vec<usize>> {
        &self.hooks
    }

    pub fn call_hook(&mut self, hook: Hook, params: Value) -> io::Result<Vec<Value>> {
        let mut results = Vec::new();
        for plugin_id in self.hooks.get(&hook).cloned().unwrap_or_default() {
            results.push(
                self.plugins
                    .get_mut(&plugin_id)
                    .unwrap()
                    .request(hook.as_str(), params.clone())?,
            );
        }
        Ok(results)
    }
}
