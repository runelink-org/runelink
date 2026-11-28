use std::{collections::HashMap, path::PathBuf};

use runelink_types::{
    host::{DEFAULT_PORT, Host},
    ids::ResourceRef,
};
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
    host: Host,
    pub database_url: String,
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

    /// This host, as resources and peers name it: with its port, unless it
    /// is the default port.
    pub fn public_host(&self) -> &Host {
        &self.host
    }

    /// A host at `localhost` on the default port, for tests.
    #[cfg(test)]
    pub fn for_test(database_url: String) -> Self {
        Self {
            host: Host::parse("localhost").unwrap(),
            database_url,
            bind_host: "127.0.0.1".into(),
            bind_port: 7000,
            secure: false,
            key_dir: PathBuf::new(),
        }
    }

    /// A reference to a resource on this host.
    pub fn local_ref<Id>(&self, id: Id) -> ResourceRef<Id> {
        ResourceRef::new(self.host.clone(), id)
    }

    pub fn api_url(&self) -> String {
        self.host.api_url(self.secure)
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.bind_host, self.bind_port)
    }

    /// Whether a host is this host or a remote one.
    pub fn locality<'a>(&self, host: &'a Host) -> Locality<'a> {
        if *host == self.host {
            Locality::Local
        } else {
            Locality::Remote(host)
        }
    }

    /// Like [`Self::locality`], but no target host means this host.
    pub fn target_locality<'a>(&self, host: Option<&'a Host>) -> Locality<'a> {
        host.map_or(Locality::Local, |host| self.locality(host))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locality<'a> {
    Local,
    Remote(&'a Host),
}

impl Locality<'_> {
    pub fn is_local(self) -> bool {
        self == Self::Local
    }

    pub fn is_remote(self) -> bool {
        !self.is_local()
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
        let invalid =
            |reason: String| ConfigError::InvalidServerEntry { index, reason };
        let public_host = self.public_host.trim();
        // An IPv6 address has colons inside its brackets.
        let (_, after_address) =
            public_host.rsplit_once(']').unwrap_or(("", public_host));
        if after_address.contains(':') {
            return Err(invalid(
                "public_host must not include a port; set public_port".into(),
            ));
        }
        let host = Host::parse(&format!("{public_host}:{}", self.public_port))
            .map_err(|error| invalid(error.to_string()))?;
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
            host,
            database_url,
            bind_host,
            bind_port,
            secure: self.secure,
            key_dir,
        })
    }
}

fn default_public_port() -> u16 {
    DEFAULT_PORT
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
    let mut index_by_public_host = HashMap::<&Host, usize>::new();
    let mut index_by_bind_addr = HashMap::<String, usize>::new();
    let mut index_by_database_url = HashMap::<String, usize>::new();
    for (index, config) in configs.iter().enumerate() {
        if let Some(first_index) =
            index_by_public_host.insert(config.public_host(), index)
        {
            return Err(ConfigError::DuplicatePublicAddress {
                public_address: config.public_host().to_string(),
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
