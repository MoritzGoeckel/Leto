pub mod config;
pub mod core;
mod plugins;
pub mod provider;
pub mod ui;

use core::{Context, Message, StreamOptions, UserContent, UserMessage};
use plugins::{Hook, PluginManager};
use provider::{AuthError, Provider, openai_chatgpt::OpenAiChatGpt};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::load()?;
    let mut plugins = PluginManager::start(&config)?;
    let mut provider = OpenAiChatGpt::init(config)?;
    let mut ui = ui::tui::Tui;
    if let Err(error) = provider.auth_refresh() {
        match error {
            AuthError::NotLoggedIn => provider.auth_login(&mut ui)?,
            error => return Err(error.into()),
        }
    }
    run(&mut provider, &mut ui, &mut plugins)
}

fn run(
    provider: &mut impl Provider,
    ui: &mut dyn ui::Ui,
    plugins: &mut PluginManager,
) -> Result<(), Box<dyn std::error::Error>> {
    plugins.init_plugins()?;
    plugins.call_hook_without_params(Hook::OnInit)?;
    let model = provider
        .get_models()
        .into_values()
        .next()
        .expect("provider has no models");
    let mut context = Context::default();
    plugins.call_hook_without_params(Hook::OnNewConversation)?;
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
        let user_message = UserMessage {
            content: UserContent::Text(input),
            timestamp: core::now_ms(),
        };
        let user_message = plugins.rewrite_user_message(user_message)?;
        let user_message = Message::User(user_message);
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
        plugins.call_hook(
            Hook::OnAssistantMessage,
            json!({"message": assistant_message}),
        )?;
        ui.add_message(&assistant_message);
        context.messages.push(assistant_message);
    }
    plugins.call_hook_without_params(Hook::OnExit)?;
    Ok(())
}
