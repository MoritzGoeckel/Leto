pub mod tools;
pub mod types;
pub use types::*;

mod r#loop;
pub use r#loop::Loop;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
