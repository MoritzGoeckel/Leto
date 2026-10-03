pub mod core;
pub mod openai_chatgpt;
mod plugins;

use core::{
    AssistantContent, Context, InputModality, Message, Model, ModelCost, StreamOptions,
    UserContent, UserMessage,
};
use std::{
    fs,
    io::{self, Read, Write},
    net::TcpListener,
    thread,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut credential = match load_credential()? {
        Some(credential) if credential.expires > now_ms() => {
            println!("Using saved ChatGPT sign-in.");
            credential
        }
        Some(credential) => match openai_chatgpt::refresh(&credential) {
            Ok(credential) => {
                save_credential(&credential)?;
                credential
            }
            Err(error) => {
                eprintln!("Saved ChatGPT sign-in could not be refreshed: {error}");
                login_and_save()?
            }
        },
        None => login_and_save()?,
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
            save_credential(&credential)?;
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

fn login_and_save() -> Result<openai_chatgpt::Credential, Box<dyn std::error::Error>> {
    let login = openai_chatgpt::begin_login("00000000-0000-4000-8000-000000000001");
    let use_listener = read_line("Use SSH port forwarding to receive the callback? [y/N] ")? == "y";
    let callback_receiver = if use_listener {
        Some(start_callback_listener()?)
    } else {
        None
    };
    println!(
        "Open this URL to sign in with ChatGPT:\n{}",
        login.authorization_url
    );
    let callback_url = if let Some(receiver) = callback_receiver {
        println!(
            "Forward port 1455 from your local computer with: ssh -L 1455:127.0.0.1:1455 <remote-host>"
        );
        println!("Waiting for the browser callback on remote port 1455...");
        receiver.recv()?
    } else {
        read_line("After approving, paste the full callback URL from your browser address bar: ")?
    };
    let credential = openai_chatgpt::exchange_callback(&login, &callback_url)?;
    save_credential(&credential)?;
    Ok(credential)
}

fn load_credential() -> Result<Option<openai_chatgpt::Credential>, Box<dyn std::error::Error>> {
    let path = std::env::current_dir()?.join("atlas.json");
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let config: serde_json::Value = serde_json::from_str(&contents)?;
    config
        .get("openaiChatGpt")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(Into::into)
}

fn save_credential(
    credential: &openai_chatgpt::Credential,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::current_dir()?.join("atlas.json");
    let mut config = match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str::<serde_json::Value>(&contents)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => serde_json::json!({"plugins": []}),
        Err(error) => return Err(error.into()),
    };
    let object = config
        .as_object_mut()
        .expect("atlas.json must contain a JSON object");
    object
        .entry("plugins")
        .or_insert_with(|| serde_json::json!([]));
    object.insert("openaiChatGpt".into(), serde_json::to_value(credential)?);
    fs::write(path, serde_json::to_vec_pretty(&config)?)?;
    Ok(())
}

fn start_callback_listener() -> io::Result<std::sync::mpsc::Receiver<String>> {
    let listener = TcpListener::bind("127.0.0.1:1455")?;
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let (mut connection, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        let bytes_read = connection.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..bytes_read]);
        let callback_path = request.split_whitespace().nth(1).unwrap();
        let callback_url = format!("http://127.0.0.1:1455{callback_path}");
        let body = "ChatGPT sign-in complete. You can close this window.";
        write!(connection, "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        sender.send(callback_url).unwrap();
    });
    Ok(receiver)
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
