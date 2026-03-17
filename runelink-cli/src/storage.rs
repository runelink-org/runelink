use directories::ProjectDirs;
use runelink_client::util::get_api_url;
use runelink_types::UserRef;
use serde::{Deserialize, Serialize};
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
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AccountConfig {
    #[serde(flatten)]
    pub user_ref: UserRef,
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
}

pub trait TryGetHost {
    fn try_get_host(&self) -> Result<&str, CliError>;
    fn try_get_api_url(&self) -> Result<String, CliError>;
}

impl TryGetHost for Option<&AccountConfig> {
    fn try_get_host(&self) -> Result<&str, CliError> {
        self.map(|ac| ac.user_ref.host.as_str())
            .ok_or(CliError::MissingAccount)
    }

    fn try_get_api_url(&self) -> Result<String, CliError> {
        self.try_get_host().map(get_api_url)
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
