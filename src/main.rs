pub mod config;
pub mod core;
pub mod openai_chatgpt;
mod plugins;

use core::{
    AssistantContent, Context, InputModality, Message, Model, ModelCost, StreamOptions,
    UserContent, UserMessage,
};
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = config::Config::load()?;
    let mut credential = match openai_chatgpt::load_credential(&config)? {
        Some(credential) if credential.expires > now_ms() => {
            println!("Using saved ChatGPT sign-in.");
            credential
        }
        Some(credential) => match openai_chatgpt::refresh(&credential) {
            Ok(credential) => {
                openai_chatgpt::save_credential(&mut config, &credential)?;
                credential
            }
            Err(error) => {
                eprintln!("Saved ChatGPT sign-in could not be refreshed: {error}");
                openai_chatgpt::login_and_save(&mut config)?
            }
        },
        None => openai_chatgpt::login_and_save(&mut config)?,
    };
    let model = Model {
        id: "gpt-6-luna".into(),
        name: "GPT-6 Luna".into(),
        api: "openai-responses".into(),
        provider: "openai".into(),
        base_url: "https://api.openai.com/v1".into(),
        input: vec![InputModality::Text, InputModality::Image],
        input_limits: None,
        cost: ModelCost::default(),
        model_type: None,
        reasoning: true,
        thinking_level_map: None,
        prompt_cache: None,
        context_window: 272_000,
        max_tokens: 128_000,
        headers: None,
        sampling_params: None,
        compat: None,
    };
    let mut context = Context::default();
    println!("Signed in. Enter a message, or /exit to quit.");
    loop {
        let input = read_line("you> ")?;
        if input == "/exit" {
            break;
        }
        if credential.expires <= now_ms() {
            credential = openai_chatgpt::refresh(&credential)?;
            openai_chatgpt::save_credential(&mut config, &credential)?;
        }
        context.messages.push(Message::User(UserMessage {
            content: UserContent::Text(input),
            timestamp: now_ms(),
        }));
        let events = openai_chatgpt::stream(
            &model,
            &context,
            &StreamOptions {
                api_key: Some(credential.access.clone()),
                ..StreamOptions::default()
            },
        )?;
        let message = events
            .into_iter()
            .find_map(|event| match event {
                core::AssistantMessageEvent::Done { message, .. } => Some(message),
                core::AssistantMessageEvent::Error { error, .. } => Some(error),
                _ => None,
            })
            .expect("Responses API returned no assistant message");
        for block in &message.content {
            if let AssistantContent::Text(text) = block {
                println!("assistant> {}", text.text);
            }
        }
        context.messages.push(Message::Assistant(message));
    }
    Ok(())
}

fn read_line(prompt: &str) -> io::Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_owned())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
