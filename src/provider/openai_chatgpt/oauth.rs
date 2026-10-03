use crate::core::now_ms;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const REDIRECT_URI: &str = "http://127.0.0.1:1455/auth/callback";
const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPE: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const MARGIN_MS: u64 = 180_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub access: String,
    pub refresh: String,
    pub expires: u64,
    pub client_id: String,
    pub scopes: Vec<String>,
}

pub struct ChatGptLogin {
    pub authorization_url: String,
    pub verifier: String,
    pub state: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    #[serde(default)]
    id_token: Option<String>,
    scope: String,
}

fn random_value() -> String {
    let mut bytes = [0; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn begin_login(device_id: &str) -> ChatGptLogin {
    let verifier = random_value();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_value();
    let nonce = random_value();
    let query = [
        ("client_id", "dynamic_agent_client"),
        ("agent_name_hint", "Atlas"),
        (
            "ext_agent_host_id",
            &format!("urn:uuid:{}", device_id.to_lowercase()),
        ),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT_URI),
        ("resource", RESOURCE),
        ("scope", SCOPE),
        ("state", &state),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("nonce", &nonce),
    ]
    .iter()
    .map(|(key, value)| format!("{key}={}", urlencoding(value)))
    .collect::<Vec<_>>()
    .join("&");
    ChatGptLogin {
        authorization_url: format!("https://auth.openai.com/api/accounts/authorize?{query}"),
        verifier,
        state,
    }
}

fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub fn exchange_code(
    login: &ChatGptLogin,
    code: &str,
    client_id: &str,
) -> Result<Credential, reqwest::Error> {
    let body = [
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", &login.verifier),
        ("redirect_uri", REDIRECT_URI),
        ("resource", RESOURCE),
    ];
    let token: TokenResponse = Client::new()
        .post("https://auth.openai.com/api/accounts/oauth/token")
        .header("accept", "application/json")
        .form(&body)
        .send()?
        .error_for_status()?
        .json()?;
    assert!(
        token
            .id_token
            .as_ref()
            .is_some_and(|value| !value.is_empty()),
        "OpenAI token response omitted ID token"
    );
    credential(token, client_id)
}

pub fn exchange_callback(
    login: &ChatGptLogin,
    callback_url: &str,
) -> Result<Credential, Box<dyn std::error::Error>> {
    let callback = reqwest::Url::parse(callback_url)?;
    assert_eq!(
        callback.origin().ascii_serialization(),
        "http://127.0.0.1:1455"
    );
    assert_eq!(callback.path(), "/auth/callback");
    let params: std::collections::HashMap<_, _> = callback.query_pairs().into_owned().collect();
    assert_eq!(
        params.get("state").map(String::as_str),
        Some(login.state.as_str()),
        "OAuth state mismatch"
    );
    if let Some(error) = params.get("error") {
        return Err(format!("ChatGPT authorization failed: {error}").into());
    }
    let code = params.get("code").expect("OAuth callback omitted code");
    let client_id = params
        .get("client_id")
        .expect("OAuth callback omitted issued client ID");
    Ok(exchange_code(login, code, client_id)?)
}

pub fn refresh(stored: &Credential) -> Result<Credential, reqwest::Error> {
    let body = [
        ("grant_type", "refresh_token"),
        ("client_id", stored.client_id.as_str()),
        ("refresh_token", stored.refresh.as_str()),
        ("resource", RESOURCE),
    ];
    let token: TokenResponse = Client::new()
        .post("https://auth.openai.com/api/accounts/oauth/token")
        .header("accept", "application/json")
        .form(&body)
        .send()?
        .error_for_status()?
        .json()?;
    credential(token, &stored.client_id)
}

fn credential(token: TokenResponse, client_id: &str) -> Result<Credential, reqwest::Error> {
    let scopes: Vec<_> = token.scope.split_whitespace().map(str::to_owned).collect();
    assert!(
        scopes
            .iter()
            .any(|scope| scope == "chatgpt.tokens.use.direct"),
        "OAuth grant omitted direct token scope"
    );
    Ok(Credential {
        access: token.access_token,
        refresh: token.refresh_token,
        expires: now_ms() + token.expires_in * 1000 - MARGIN_MS,
        client_id: client_id.to_owned(),
        scopes,
    })
}
