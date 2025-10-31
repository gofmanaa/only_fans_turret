use config::{Config, ConfigError};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct WebConfig {
    pub turn_realm: String,
    pub turn_port: i64,
    pub turn_secret_key: String,
    pub turn_user_name: String,
    pub turn_ttl: i64,
    pub controller_ttl: i64,
}

impl WebConfig {
    pub fn new() -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(config::File::with_name("web_config"))
            .build()?;

        config.try_deserialize()
    }
}