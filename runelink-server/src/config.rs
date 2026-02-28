use std::{collections::HashMap, path::PathBuf};

use runelink_client::util::{get_api_url, pad_host};
use serde::Deserialize;

pub type ConfigResult<T> = std::result::Result<T, ConfigError>;

#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file `{0}`: {1}")]
    ReadConfigFile(PathBuf, #[source] std::io::Error),

    #[error("Failed to parse config file `{0}`: {1}")]
    ParseConfigFile(PathBuf, #[source] toml::de::Error),

    #[error("Config file `{0}` must contain at least one [[servers]] entry")]
    NoServers(PathBuf),

    #[error("Invalid server config at index {index}: {reason}")]
    InvalidServerEntry { index: usize, reason: String },

    #[error(
        "Invalid cluster config: duplicate port `{port}` used by servers at indices {first_index} and {second_index}"
    )]
    DuplicatePort {
        port: u16,
        first_index: usize,
        second_index: usize,
    },

    #[error(
        "Invalid cluster config: duplicate database_url `{database_url}` used by servers at indices {first_index} and {second_index}"
    )]
    DuplicateDatabaseUrl {
        database_url: String,
        first_index: usize,
        second_index: usize,
    },
}

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub local_host_raw: String,
    pub database_url: String,
    pub port: u16,
    pub key_dir: PathBuf,
}

impl ServerConfig {
    pub fn from_toml_file(path: &PathBuf) -> ConfigResult<Vec<Self>> {
        let file_contents = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::ReadConfigFile(path.clone(), e))?;
        let parsed: RootConfig = toml::from_str(&file_contents)
            .map_err(|e| ConfigError::ParseConfigFile(path.clone(), e))?;

        if parsed.servers.is_empty() {
            return Err(ConfigError::NoServers(path.clone()));
        }

        let configs: Vec<Self> = parsed
            .servers
            .into_iter()
            .enumerate()
            .map(|(index, raw)| raw.resolve(index))
            .collect::<ConfigResult<Vec<_>>>()?;

        validate_unique_resources(&configs)?;
        Ok(configs)
    }

    /// Includes port if it's not the default port (7000)
    pub fn local_host(&self) -> String {
        if self.port == 7000 {
            self.local_host_raw.clone()
        } else {
            format!("{}:{}", &self.local_host_raw, self.port)
        }
    }

    /// Always includes port for machine-to-machine communication
    pub fn local_host_with_explicit_port(&self) -> String {
        format!("{}:{}", &self.local_host_raw, self.port)
    }

    pub fn api_url(&self) -> String {
        get_api_url(self.local_host_with_explicit_port().as_str())
    }

    pub fn is_remote_host(&self, host: Option<&str>) -> bool {
        let Some(host) = host else {
            return false;
        };
        pad_host(host) != pad_host(self.local_host().as_str())
    }
}

#[derive(Deserialize, Debug)]
struct RootConfig {
    #[serde(default)]
    servers: Vec<RawServerConfig>,
}

#[derive(Deserialize, Debug)]
struct RawServerConfig {
    local_host: String,
    database_url: String,
    #[serde(default = "default_port")]
    port: u16,
    key_dir: Option<PathBuf>,
}

impl RawServerConfig {
    fn resolve(self, index: usize) -> ConfigResult<ServerConfig> {
        let local_host = self.local_host.trim().to_string();
        if local_host.is_empty() {
            return Err(ConfigError::InvalidServerEntry {
                index,
                reason: "local_host cannot be empty".to_string(),
            });
        }
        let database_url = self.database_url.trim().to_string();
        if database_url.is_empty() {
            return Err(ConfigError::InvalidServerEntry {
                index,
                reason: "database_url cannot be empty".to_string(),
            });
        }
        let key_dir =
            self.key_dir.unwrap_or_else(|| default_key_dir(self.port));
        Ok(ServerConfig {
            local_host_raw: local_host,
            database_url,
            port: self.port,
            key_dir,
        })
    }
}

fn default_port() -> u16 {
    7000
}

fn default_key_dir(port: u16) -> PathBuf {
    let mut path = dirs_next::home_dir().expect("failed to get home directory");
    path.extend([".local", "share", "runelink", "keys", &port.to_string()]);
    path
}

fn validate_unique_resources(configs: &[ServerConfig]) -> ConfigResult<()> {
    let mut first_index_by_port: HashMap<u16, usize> = HashMap::new();
    let mut first_index_by_database_url: HashMap<String, usize> =
        HashMap::new();
    for (index, config) in configs.iter().enumerate() {
        if let Some(first_index) =
            first_index_by_port.insert(config.port, index)
        {
            return Err(ConfigError::DuplicatePort {
                port: config.port,
                first_index,
                second_index: index,
            });
        }
        if let Some(first_index) = first_index_by_database_url
            .insert(config.database_url.clone(), index)
        {
            return Err(ConfigError::DuplicateDatabaseUrl {
                database_url: config.database_url.clone(),
                first_index,
                second_index: index,
            });
        }
    }
    Ok(())
}
