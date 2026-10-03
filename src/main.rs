pub mod config;
pub mod core;
mod plugins;
pub mod provider;
pub mod ui;

use core::{Context, Message, StreamOptions, UserContent, UserMessage};
use provider::{AuthError, Provider, openai_chatgpt::OpenAiChatGpt};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut provider = OpenAiChatGpt::load()?;
    let mut ui = ui::tui::Tui;
    if let Err(error) = provider.auth_refresh() {
        match error {
            AuthError::NotLoggedIn => provider.auth_login(&mut ui)?,
            error => return Err(error.into()),
        }
    }
    run(&mut provider, &mut ui)
}

fn run(
    provider: &mut impl Provider,
    ui: &mut dyn ui::Ui,
) -> Result<(), Box<dyn std::error::Error>> {
    let model = provider
        .get_models()
        .into_values()
        .next()
        .expect("provider has no models");
    let mut context = Context::default();
    let notice_id = ui.inform_blocking("Signed in. Enter a message, or /exit to quit.");
    ui.close(notice_id);
    loop {
        let input = ui.get_input("you> ")?;
        if input == "/exit" {
            break;
        }
        if let Err(error) = provider.auth_refresh() {
            match error {
                AuthError::NotLoggedIn => provider.auth_login(ui)?,
                error => return Err(error.into()),
            }
        }
        let user_message = Message::User(UserMessage {
            content: UserContent::Text(input),
            timestamp: core::now_ms(),
        });
        ui.add_message(&user_message);
        context.messages.push(user_message);
        let events = provider.stream(&model, &context, &StreamOptions::default())?;
        let message = events
            .into_iter()
            .find_map(|event| match event {
                core::AssistantMessageEvent::Done { message, .. } => Some(message),
                core::AssistantMessageEvent::Error { error, .. } => Some(error),
                _ => None,
            })
            .expect("Responses API returned no assistant message");
        let assistant_message = Message::Assistant(message);
        ui.add_message(&assistant_message);
        context.messages.push(assistant_message);
    }
    Ok(())
}
