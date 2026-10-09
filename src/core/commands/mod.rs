mod clear;
mod exit;
mod model;
mod reasoning;
mod resume;

use super::Loop;
use std::collections::HashMap;

type Command = fn(&mut Loop) -> Result<(), Box<dyn std::error::Error>>;

pub(super) fn make_commands() -> HashMap<String, Command> {
    HashMap::from([
        ("exit".to_owned(), exit::run as _),
        ("clear".to_owned(), clear::run as _),
        ("resume".to_owned(), resume::run as _),
        ("model".to_owned(), model::run as _),
        ("reasoning".to_owned(), reasoning::run as _),
    ])
}

pub(super) fn clear(loop_state: &mut Loop) -> Result<(), Box<dyn std::error::Error>> {
    clear::run(loop_state)
}
