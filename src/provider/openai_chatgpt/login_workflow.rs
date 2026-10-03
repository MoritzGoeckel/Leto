use std::{
    io::{self, Read, Write},
    net::TcpListener,
    thread,
};

use crate::config::Config;
use crate::ui::Ui;

use super::{Credential, begin_login, exchange_callback};

pub fn load_credential(config: &Config) -> Result<Option<Credential>, Box<dyn std::error::Error>> {
    config
        .provider("openai")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(Into::into)
}

pub fn login_and_save(
    config: &mut Config,
    ui: &mut dyn Ui,
) -> Result<Credential, Box<dyn std::error::Error>> {
    let login = begin_login("00000000-0000-4000-8000-000000000001");
    let use_listener =
        ui.get_input("Use SSH port forwarding to receive the callback? [y/N] ")? == "y";
    let callback_receiver = if use_listener {
        Some(start_callback_listener()?)
    } else {
        None
    };
    let message = if use_listener {
        format!(
            "Open this URL to sign in with ChatGPT:\n{}\nForward port 1455 from your local computer with: ssh -L 1455:127.0.0.1:1455 <remote-host>\nWaiting for the browser callback on remote port 1455...",
            login.authorization_url
        )
    } else {
        format!(
            "Open this URL to sign in with ChatGPT:\n{}",
            login.authorization_url
        )
    };
    let blocking_id = ui.inform_blocking(&message);
    let callback_url = if let Some(receiver) = callback_receiver {
        receiver.recv()?
    } else {
        ui.get_input(
            "After approving, paste the full callback URL from your browser address bar: ",
        )?
    };
    ui.close(blocking_id);
    let credential = exchange_callback(&login, &callback_url)?;
    config.set_provider("openai", serde_json::to_value(&credential)?);
    config.save()?;
    Ok(credential)
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
