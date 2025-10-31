use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha1::Sha1;
use crate::config::WebConfig;

type HmacSha1 = Hmac<Sha1>;

#[derive(Serialize)]
pub struct TurnCredentials {
    pub username: String,
    pub credential: String,
    pub urls: Vec<String>,
}

pub fn generate_turn_credentials(config: WebConfig) -> TurnCredentials {
    let expiry = Utc::now().timestamp() + config.turn_ttl;

    let rest_username = format!("{}:{}", expiry, config.turn_user_name);
    let mut mac = HmacSha1::new_from_slice(config.turn_secret_key.as_bytes()).unwrap();
    mac.update(rest_username.as_bytes());
    let result = mac.finalize().into_bytes();
    let credential = general_purpose::STANDARD.encode(result);
    let realm = config.turn_realm;
    let port = config.turn_port;
    TurnCredentials {
        username: rest_username,
        credential,
        urls: vec![
            format!("turn:{realm}:{port}?transport=udp"),
            format!("turn:{realm}:{port}?transport=tcp"),
        ],
    }
}