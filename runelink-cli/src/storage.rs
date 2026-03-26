use directories::ProjectDirs;
use reqwest::Client;
use runelink_client::{Error as ClientError, requests, util::get_api_url};
use runelink_types::UserRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::{fmt, fs};
use uuid::Uuid;

use crate::error::CliError;

const CONFIG_FILENAME: &str = "config.json";

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct AppConfig {
    pub default_account: Option<UserRef>,
    pub default_server: Option<Uuid>,
    pub accounts: Vec<AccountConfig>,
    #[serde(default)]
    pub hosts: HashMap<String, HostConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AccountConfig {
    #[serde(flatten)]
    pub user_ref: UserRef,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct HostConfig {
    #[serde(default = "default_secure")]
    pub secure: bool,
}

impl fmt::Display for AccountConfig {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.user_ref)
    }
}

#[allow(dead_code)]
impl AppConfig {
    pub fn load() -> Result<Self, CliError> {
        let config: AppConfig = load_data(CONFIG_FILENAME)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<(), CliError> {
        save_data(self, CONFIG_FILENAME)
    }

    pub fn get_default_account(&self) -> Option<&AccountConfig> {
        self.default_account.as_ref().and_then(|user_ref| {
            self.accounts.iter().find(|ac| ac.user_ref == *user_ref)
        })
    }

    pub fn get_default_account_mut(&mut self) -> Option<&mut AccountConfig> {
        let user_ref = self.default_account.clone()?;
        self.accounts.iter_mut().find(|ac| ac.user_ref == user_ref)
    }

    pub fn get_account_config(
        &self,
        user_ref: UserRef,
    ) -> Option<&AccountConfig> {
        self.accounts.iter().find(|ac| ac.user_ref == user_ref)
    }

    pub fn get_account_config_mut(
        &mut self,
        user_ref: UserRef,
    ) -> Option<&mut AccountConfig> {
        self.accounts.iter_mut().find(|ac| ac.user_ref == user_ref)
    }

    pub fn get_or_create_account_config(
        &mut self,
        user_ref: UserRef,
    ) -> &mut AccountConfig {
        if let Some(idx) =
            self.accounts.iter().position(|ac| ac.user_ref == user_ref)
        {
            self.accounts[idx].user_ref = user_ref;
            &mut self.accounts[idx]
        } else {
            if self.accounts.is_empty() {
                self.default_account = Some(user_ref.clone());
            }
            self.accounts.push(AccountConfig { user_ref });
            self.accounts.last_mut().unwrap()
        }
    }

    pub fn host_config(&self, host: &str) -> Option<HostConfig> {
        self.hosts.get(host).copied()
    }

    pub fn set_host_secure(&mut self, host: &str, secure: bool) {
        self.hosts.insert(host.to_string(), HostConfig { secure });
    }
}

pub trait TryGetHost {
    fn try_get_host(&self) -> Result<&str, CliError>;
}

impl TryGetHost for Option<&AccountConfig> {
    fn try_get_host(&self) -> Result<&str, CliError> {
        self.map(|ac| ac.user_ref.host.as_str())
            .ok_or(CliError::MissingAccount)
    }
}

pub async fn resolve_api_url(
    client: &Client,
    config: &mut AppConfig,
    host: &str,
) -> Result<String, CliError> {
    let mut attempts = Vec::with_capacity(2);
    if let Some(host_config) = config.host_config(host) {
        attempts.push(host_config.secure);
        attempts.push(!host_config.secure);
    } else {
        attempts.push(true);
        attempts.push(false);
    }

    let mut last_transport_error = None;
    for secure in attempts {
        let api_url = get_api_url(host, secure);
        match requests::ping(client, &api_url).await {
            Ok(_) => {
                let changed = config
                    .host_config(host)
                    .map(|host_config| host_config.secure != secure)
                    .unwrap_or(true);
                config.set_host_secure(host, secure);
                if changed {
                    config.save()?;
                }
                return Ok(api_url);
            }
            Err(error) if is_transport_error(&error) => {
                last_transport_error = Some(error);
            }
            Err(error) => return Err(error.into()),
        }
    }

    Err(match last_transport_error {
        Some(error) => error.into(),
        None => CliError::ConfigError(format!(
            "Unable to resolve API URL for host `{host}`"
        )),
    })
}

fn default_secure() -> bool {
    true
}

fn is_transport_error(error: &ClientError) -> bool {
    match error {
        ClientError::Reqwest(error) => {
            error.is_connect() || error.is_timeout() || error.is_request()
        }
        ClientError::Status(_, _) | ClientError::Json(_) => false,
    }
}

pub fn get_data_dir() -> Result<PathBuf, CliError> {
    if let Some(proj_dirs) = ProjectDirs::from("com", "RuneLink", "RuneLink") {
        let data_dir = proj_dirs.data_dir();
        if !data_dir.exists() {
            fs::create_dir_all(data_dir)?;
        }
        Ok(data_dir.to_path_buf())
    } else {
        Err(CliError::ConfigError(
            "Could not determine home directory or project directories.".into(),
        ))
    }
}

pub fn get_data_file_path(filename: &str) -> Result<PathBuf, CliError> {
    Ok(get_data_dir()?.join(filename))
}

pub fn load_data<T>(filename: &str) -> Result<T, CliError>
where
    T: for<'de> Deserialize<'de> + Default,
{
    let file_path = get_data_file_path(filename)?;
    if file_path.exists() {
        let data_str = fs::read_to_string(&file_path)?;
        if data_str.trim().is_empty() {
            Ok(T::default())
        } else {
            serde_json::from_str(&data_str).map_err(CliError::from)
        }
    } else {
        Ok(T::default())
    }
}

pub fn save_data<T>(data: &T, filename: &str) -> Result<(), CliError>
where
    T: Serialize,
{
    let file_path = get_data_file_path(filename)?;
    let data_str = serde_json::to_string_pretty(data)?;
    fs::write(file_path, data_str)?;
    Ok(())
}
