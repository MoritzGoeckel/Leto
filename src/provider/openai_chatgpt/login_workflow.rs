use std::{
    io::{self, BufRead, BufReader, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

use crate::config::Config;
use crate::ui::{AskOptions, Ui};

use super::{Credential, begin_login, exchange_callback};

pub fn login_and_save(
    config: &mut Config,
    ui: &mut dyn Ui,
) -> Result<Credential, Box<dyn std::error::Error>> {
    let login = begin_login("00000000-0000-4000-8000-000000000001");
    let use_listener = ui.ask(AskOptions::default())? == "y";
    let callback_receiver = if use_listener {
        Some(start_callback_listener()?)
    } else {
        None
    };
    let message = if use_listener {
        format!(
            "Open this URL to sign in with ChatGPT:\n{}\nForward port 1455 from your local computer with: ssh -N -o ExitOnForwardFailure=yes -L 127.0.0.1:1455:127.0.0.1:1455 <remote-host>\nWaiting for the browser callback on remote port 1455...",
            login.authorization_url
        )
    } else {
        format!(
            "Open this URL to sign in with ChatGPT:\n{}",
            login.authorization_url
        )
    };
    ui.append_message_str(&message);
    let callback_url = if let Some(receiver) = callback_receiver {
        receiver.recv()?
    } else {
        ui.ask(AskOptions::default())?
    };
    let credential = exchange_callback(&login, &callback_url)?;
    config.set_provider("openai", serde_json::to_value(&credential)?);
    config.save()?;
    Ok(credential)
}

fn start_callback_listener() -> io::Result<std::sync::mpsc::Receiver<String>> {
    let listener = TcpListener::bind("127.0.0.1:1455")?;
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        loop {
            let (mut connection, _) = listener.accept().unwrap();
            connection
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = String::new();
            let mut reader = BufReader::new(&mut connection);
            match reader.read_line(&mut request) {
                Ok(0) => continue,
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(error) => panic!("Failed to read OAuth callback: {error}"),
            }
            let callback_path = request.split_whitespace().nth(1).unwrap();
            let is_callback = callback_path.starts_with("/auth/callback?");
            let callback_url = format!("http://127.0.0.1:1455{callback_path}");
            let mut header = String::new();
            loop {
                header.clear();
                let bytes_read = reader.read_line(&mut header).unwrap();
                if bytes_read == 0 || header == "\r\n" {
                    break;
                }
            }
            let body = if is_callback {
                "ChatGPT callback received. You can close this window."
            } else {
                "Atlas callback listener is reachable. Complete sign-in using the authorization URL in your terminal."
            };
            write!(connection, "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            if is_callback {
                sender.send(callback_url).unwrap();
                break;
            }
        }
    });
    Ok(receiver)
}
