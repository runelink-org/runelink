use directories::ProjectDirs;
use reqwest::Client;
use runelink_client::{Error as ClientError, requests};
use runelink_types::{Host, UserRef};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::{fmt, fs};
use uuid::Uuid;

use crate::cli::input::{can_prompt, read_input};
use crate::error::CliError;

const CONFIG_FILENAME: &str = "config.json";

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct AppConfig {
    /// Read from `default_account` in configs saved before the rename
    #[serde(alias = "default_account")]
    pub active_account: Option<UserRef>,
    pub default_server: Option<Uuid>,
    pub accounts: Vec<AccountConfig>,
    #[serde(default, deserialize_with = "deserialize_hosts")]
    pub hosts: HashMap<Host, HostConfig>,
    /// Hosts approved for HTTP by `--dangerously-allow-http` for this run
    #[serde(skip)]
    pub http_approved_hosts: Vec<Host>,
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
    /// Explicit user approval, separate from automatically discovered transport.
    #[serde(default)]
    pub allow_insecure: bool,
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

    pub fn get_active_account(&self) -> Option<&AccountConfig> {
        self.active_account.as_ref().and_then(|user_ref| {
            self.accounts.iter().find(|ac| ac.user_ref == *user_ref)
        })
    }

    pub fn is_active(&self, user_ref: &UserRef) -> bool {
        self.active_account.as_ref() == Some(user_ref)
    }

    pub fn get_active_account_mut(&mut self) -> Option<&mut AccountConfig> {
        let user_ref = self.active_account.clone()?;
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
                self.active_account = Some(user_ref.clone());
            }
            self.accounts.push(AccountConfig { user_ref });
            self.accounts.last_mut().unwrap()
        }
    }

    pub fn host_config(&self, host: &Host) -> Option<HostConfig> {
        self.hosts.get(host).copied()
    }

    pub fn set_host_secure(&mut self, host: &Host, secure: bool) {
        let allow_insecure = self
            .host_config(host)
            .is_some_and(|config| config.allow_insecure);
        self.hosts.insert(
            host.clone(),
            HostConfig {
                secure,
                allow_insecure,
            },
        );
    }
}

pub trait TryGetHost {
    fn try_get_host(&self) -> Result<&Host, CliError>;
}

impl TryGetHost for Option<&AccountConfig> {
    fn try_get_host(&self) -> Result<&Host, CliError> {
        self.map(|ac| &ac.user_ref.host)
            .ok_or(CliError::MissingAccount)
    }
}

pub async fn resolve_api_url(
    client: &Client,
    config: &mut AppConfig,
    host: &Host,
) -> Result<String, CliError> {
    let mut transport_failed = false;
    for secure in [true, false] {
        let api_url = host.api_url(secure);
        match requests::ping(client, &api_url).await {
            Ok(_) => {
                if !secure
                    && !config
                        .host_config(host)
                        .is_some_and(|config| config.allow_insecure)
                {
                    if !config.http_approved_hosts.contains(host) {
                        confirm_insecure_host(host)?;
                    }
                    config.hosts.insert(
                        host.clone(),
                        HostConfig {
                            secure: false,
                            allow_insecure: true,
                        },
                    );
                    config.save()?;
                    return Ok(api_url);
                }
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
                log::debug!("Failed to reach `{api_url}`: {error:?}");
                transport_failed = true;
            }
            Err(error) => return Err(error.into()),
        }
    }

    Err(if transport_failed {
        CliError::HostUnreachable(host.clone())
    } else {
        CliError::ConfigError(format!(
            "Unable to resolve API URL for host `{host}`"
        ))
    })
}

fn confirm_insecure_host(host: &Host) -> Result<(), CliError> {
    if !can_prompt() {
        return Err(CliError::ConfigError(format!(
            "HTTP for `{host}` requires approval. Pass `--dangerously-allow-http {host}`, or run in a terminal without `--non-interactive` to confirm."
        )));
    }
    eprintln!(
        "A secure connection to `{host}` failed. HTTP is available, but passwords, tokens, and messages will be sent unencrypted."
    );
    let answer = read_input(&format!(
        "Allow HTTP for `{host}` and remember this choice? [y/N]: "
    ))?;
    if !answer.is_some_and(|answer| {
        answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")
    }) {
        return Err(CliError::Cancellation);
    }
    Ok(())
}

/// Reads hosts saved before they were canonical, which may name the default
/// port. Spellings of one host keep HTTP approval only if all of them had it,
/// and keys that aren't hosts are dropped rather than failing the config.
fn deserialize_hosts<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<HashMap<Host, HostConfig>, D::Error> {
    let mut hosts = HashMap::<Host, HostConfig>::new();
    for (key, config) in
        HashMap::<String, HostConfig>::deserialize(deserializer)?
    {
        let Ok(host) = Host::parse(&key) else {
            continue;
        };
        hosts
            .entry(host)
            .and_modify(|merged| {
                merged.secure |= config.secure;
                merged.allow_insecure &= config.allow_insecure;
            })
            .or_insert(config);
    }
    Ok(hosts)
}

fn default_secure() -> bool {
    true
}

fn is_transport_error(error: &ClientError) -> bool {
    match error {
        ClientError::Reqwest(error) => {
            error.is_connect() || error.is_timeout() || error.is_request()
        }
        ClientError::Status(_, _)
        | ClientError::Json(_)
        | ClientError::UnsupportedCapability(_) => false,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovered_http_is_not_approval() {
        let config: AppConfig = serde_json::from_str(
            r#"{"accounts":[],"hosts":{"localhost":{"secure":false}}}"#,
        )
        .unwrap();
        let host = Host::parse("localhost").unwrap();
        assert!(!config.host_config(&host).unwrap().allow_insecure);
    }

    #[test]
    fn approval_is_per_host_and_survives_secure_discovery() {
        let mut config: AppConfig = serde_json::from_str(
            r#"{"accounts":[],"hosts":{"localhost:7000":{"secure":false,"allow_insecure":true}}}"#,
        )
        .unwrap();
        let localhost = Host::parse("localhost").unwrap();
        assert!(config.host_config(&localhost).unwrap().allow_insecure);
        let other_port = Host::parse("localhost:7001").unwrap();
        assert!(config.host_config(&other_port).is_none());
        config.set_host_secure(&localhost, true);
        let restored: AppConfig =
            serde_json::from_str(&serde_json::to_string(&config).unwrap())
                .unwrap();
        let host = restored.host_config(&localhost).unwrap();
        assert!(host.secure && host.allow_insecure);
    }
}
