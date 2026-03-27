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
        "Invalid cluster config: duplicate public address `{public_address}` used by servers at indices {first_index} and {second_index}"
    )]
    DuplicatePublicAddress {
        public_address: String,
        first_index: usize,
        second_index: usize,
    },

    #[error(
        "Invalid cluster config: duplicate bind address `{bind_host}:{bind_port}` used by servers at indices {first_index} and {second_index}"
    )]
    DuplicateBindAddress {
        bind_host: String,
        bind_port: u16,
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
    pub public_host_raw: String,
    pub database_url: String,
    pub public_port: u16,
    pub bind_host: String,
    pub bind_port: u16,
    pub secure: bool,
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
        let configs = parsed
            .servers
            .into_iter()
            .enumerate()
            .map(|(index, raw)| raw.resolve(index))
            .collect::<ConfigResult<Vec<Self>>>()?;
        validate_unique_resources(&configs)?;
        Ok(configs)
    }

    /// Includes port if it's not the default port (7000)
    pub fn public_host(&self) -> String {
        if self.public_port == 7000 {
            self.public_host_raw.clone()
        } else {
            self.public_host_with_explicit_port()
        }
    }

    /// Always includes port for machine-to-machine communication
    pub fn public_host_with_explicit_port(&self) -> String {
        format!("{}:{}", &self.public_host_raw, self.public_port)
    }

    pub fn api_url(&self) -> String {
        get_api_url(self.public_host_with_explicit_port().as_str(), self.secure)
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.bind_host, self.bind_port)
    }

    #[allow(dead_code)]
    pub fn client_ws_url(&self) -> String {
        runelink_client::util::get_client_ws_url(
            self.public_host_with_explicit_port().as_str(),
            self.secure,
        )
    }

    #[allow(dead_code)]
    pub fn federation_ws_url(&self) -> String {
        runelink_client::util::get_federation_ws_url(
            self.public_host_with_explicit_port().as_str(),
            self.secure,
        )
    }

    pub fn is_remote_host(&self, host: Option<&str>) -> bool {
        let Some(host) = host else {
            return false;
        };
        pad_host(host) != pad_host(self.public_host().as_str())
    }
}

#[derive(Deserialize, Debug)]
struct RootConfig {
    #[serde(default)]
    servers: Vec<RawServerConfig>,
}

#[derive(Deserialize, Debug)]
struct RawServerConfig {
    public_host: String,
    database_url: String,
    #[serde(default = "default_public_port")]
    public_port: u16,
    #[serde(default = "default_bind_host")]
    bind_host: String,
    bind_port: Option<u16>,
    #[serde(default = "default_secure")]
    secure: bool,
    key_dir: Option<PathBuf>,
}

impl RawServerConfig {
    fn resolve(self, index: usize) -> ConfigResult<ServerConfig> {
        let public_host = self.public_host.trim().to_string();
        if public_host.is_empty() {
            return Err(ConfigError::InvalidServerEntry {
                index,
                reason: "public_host cannot be empty".to_string(),
            });
        }
        let database_url = self.database_url.trim().to_string();
        if database_url.is_empty() {
            return Err(ConfigError::InvalidServerEntry {
                index,
                reason: "database_url cannot be empty".to_string(),
            });
        }
        let bind_host = self.bind_host.trim().to_string();
        if bind_host.is_empty() {
            return Err(ConfigError::InvalidServerEntry {
                index,
                reason: "bind_host cannot be empty".to_string(),
            });
        }
        let bind_port = self.bind_port.unwrap_or(self.public_port);
        let key_dir = self
            .key_dir
            .unwrap_or_else(|| default_key_dir(self.public_port));
        Ok(ServerConfig {
            public_host_raw: public_host,
            database_url,
            public_port: self.public_port,
            bind_host,
            bind_port,
            secure: self.secure,
            key_dir,
        })
    }
}

fn default_public_port() -> u16 {
    7000
}

fn default_secure() -> bool {
    true
}

fn default_bind_host() -> String {
    "0.0.0.0".to_string()
}

fn default_key_dir(port: u16) -> PathBuf {
    let mut path = dirs_next::home_dir().expect("failed to get home directory");
    path.extend([".local", "share", "runelink", "keys", &port.to_string()]);
    path
}

fn validate_unique_resources(configs: &[ServerConfig]) -> ConfigResult<()> {
    let mut index_by_public_addr = HashMap::<String, usize>::new();
    let mut index_by_bind_addr = HashMap::<String, usize>::new();
    let mut index_by_database_url = HashMap::<String, usize>::new();
    for (index, config) in configs.iter().enumerate() {
        let public_address = config.public_host_with_explicit_port();
        if let Some(first_index) =
            index_by_public_addr.insert(public_address.clone(), index)
        {
            return Err(ConfigError::DuplicatePublicAddress {
                public_address,
                first_index,
                second_index: index,
            });
        }
        let bind_addr = config.bind_addr();
        if let Some(first_index) =
            index_by_bind_addr.insert(bind_addr.clone(), index)
        {
            return Err(ConfigError::DuplicateBindAddress {
                bind_host: config.bind_host.clone(),
                bind_port: config.bind_port,
                first_index,
                second_index: index,
            });
        }
        if let Some(first_index) =
            index_by_database_url.insert(config.database_url.clone(), index)
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
