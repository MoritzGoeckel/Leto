use crate::core::{Model, ThinkingLevel};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, io, path::PathBuf};

#[derive(Clone)]
pub struct Config {
    path: PathBuf,
    data: Value,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    pub reasoning: Option<ThinkingLevel>,
}

impl Config {
    pub fn load() -> io::Result<Self> {
        let path = std::env::current_dir()?.join("atlas.json");
        let data = match fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => json!({"plugins": []}),
            Err(error) => return Err(error),
        };
        Ok(Self { path, data })
    }

    pub fn plugin_paths(&self) -> io::Result<Vec<PathBuf>> {
        self.data["plugins"]
            .as_array()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "`plugins` must be an array")
            })?
            .iter()
            .map(|value| {
                let path = value.as_str().map(PathBuf::from).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "plugin path must be a string")
                })?;
                Ok(if path.is_absolute() {
                    path
                } else {
                    self.path.parent().unwrap().join(path)
                })
            })
            .collect()
    }

    pub fn models(&self, provider: &str) -> io::Result<BTreeMap<String, Model>> {
        let path = self.path.parent().unwrap().join(
            self.data["models"]
                .as_str()
                .expect("atlas.json must contain a models path"),
        );
        let models: BTreeMap<String, BTreeMap<String, Model>> =
            serde_json::from_slice(&fs::read(path)?)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(models[provider]
            .iter()
            .map(|(id, model)| (id.clone(), model.clone()))
            .collect())
    }

    pub fn provider(&self, name: &str) -> Option<&Value> {
        self.data.get("provider")?.get(name)
    }

    pub fn set_provider(&mut self, name: &str, value: Value) {
        let data = self
            .data
            .as_object_mut()
            .expect("atlas.json must contain a JSON object");
        data.entry("provider").or_insert_with(|| json!({}))[name] = value;
    }

    pub fn model(&self) -> Option<ModelConfig> {
        self.data
            .get("model")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .expect("model config must be valid")
    }

    pub fn set_model(&mut self, model: ModelConfig) {
        self.data["model"] = serde_json::to_value(model).expect("model config must serialize");
    }

    pub fn save(&self) -> io::Result<()> {
        fs::write(&self.path, serde_json::to_vec_pretty(&self.data)?)
    }
}
