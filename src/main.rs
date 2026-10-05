pub mod config;
pub mod core;
mod plugins;
pub mod provider;
pub mod ui;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    core::Loop::new()?.start()
}
