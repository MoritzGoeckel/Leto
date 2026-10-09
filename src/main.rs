pub mod config;
pub mod core;
mod plugins;
pub mod provider;
pub mod ui;

use std::sync::{Arc, Mutex};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Arc::new(config::Config::load()?);
    let tui = ui::tui::Tui::new();
    let ui: Arc<Mutex<dyn ui::Ui>> = Arc::new(Mutex::new(tui.clone()));
    let mut plugins = plugins::PluginManager::new();
    let ask_ui = Arc::clone(&ui);
    plugins.register_method("ask_user", move |_params: serde_json::Value| {
        Ok(serde_json::json!(
            ask_ui.lock().unwrap().wait_for_next_prompt()?
        ))
    });
    let notify_ui = Arc::clone(&ui);
    plugins.register_method("notify_user", move |params: serde_json::Value| {
        let message = params["message"].as_str().unwrap_or_default();
        notify_ui.lock().unwrap().inform("Notice", message);
        Ok(serde_json::Value::Null)
    });
    plugins.start(&config)?;
    let resume = std::env::args().any(|argument| argument == "--resume");
    let context = resume
        .then(|| core::events::load_context("conversation.jsonl"))
        .transpose()?;
    let app = core::Loop::new(ui, config, Arc::new(Mutex::new(plugins)), context)?;
    std::thread::scope(|scope| {
        let mut run_tui = tui.clone();
        scope.spawn(move || {
            if let Err(error) = run_tui.run() {
                eprintln!("TUI error: {error}");
            }
        });
        let result = app.start();
        tui.stop();
        result
    })
}
