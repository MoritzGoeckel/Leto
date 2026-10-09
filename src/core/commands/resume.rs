use super::super::Loop;
use crate::core::events::EventLog;
use crate::ui::AskOptions;
use ratatui::style::Color;
use std::io;

pub(super) fn run(loop_state: &mut Loop) -> Result<(), Box<dyn std::error::Error>> {
    let conversations = crate::core::events::list_conversations()?
        .into_iter()
        .rev()
        .take(10)
        .collect::<Vec<_>>();
    if conversations.is_empty() {
        loop_state
            .ui
            .lock()
            .unwrap()
            .append_message_str("No saved conversations");
        return Ok(());
    }
    let names = conversations
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy())
        .collect::<Vec<_>>();
    let (answer, mut context, events) = loop {
        let mut ui = loop_state.ui.lock().unwrap();
        ui.append_message_str(&format!("{}", names.join("\n")));
        let answer = ui.ask_styled(AskOptions {
            background: Color::Rgb(45, 39, 26),
            form_text: "Conversation filename...".to_owned(),
            ..AskOptions::default()
        })?;
        drop(ui);
        let Some(path) = conversations
            .iter()
            .find(|path| path.file_name().unwrap().to_str().unwrap() == answer)
        else {
            loop_state
                .ui
                .lock()
                .unwrap()
                .append_message_str("Conversation not found");
            continue;
        };
        let loaded = EventLog::read_context(path)
            .and_then(|context| EventLog::open(path).map(|events| (context, events)));
        match loaded {
            Ok((context, events)) => break (answer, context, events),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                loop_state
                    .ui
                    .lock()
                    .unwrap()
                    .append_message_str("Conversation not found");
            }
            Err(error) => return Err(error.into()),
        }
    };
    context.tools = loop_state.context.tools.clone();
    loop_state.context = context;
    loop_state.events = events;
    let mut ui = loop_state.ui.lock().unwrap();
    ui.append_message_str(&format!("Resumed {answer}"));
    let earlier_messages = loop_state.context.messages.len().saturating_sub(5);
    if earlier_messages > 0 {
        ui.append_message_str(&format!("{earlier_messages} earlier messages"));
    }
    for message in loop_state.context.messages.iter().skip(earlier_messages) {
        ui.append_message(message);
    }
    Ok(())
}
