use runelink_types::UserRef;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

use crate::error::CliError;
use crate::storage::{load_data, save_data};

const AUTH_CACHE_FILENAME: &str = "auth.json";

/// Cached authentication data for a single account.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AccountAuth {
    /// Long-lived refresh token (required)
    pub refresh_token: String,
    /// Optional cached access token
    pub access_token: Option<String>,
    /// Optional expiration timestamp (Unix timestamp)
    pub expires_at: Option<i64>,
    /// Optional client ID used for token requests
    pub client_id: Option<String>,
    /// Optional scope used for token requests
    pub scope: Option<String>,
}

/// Auth cache storing authentication data for multiple accounts.
/// Keys are "name@host" identity strings (JSON requires string keys).
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct AuthCache {
    #[serde(deserialize_with = "deserialize_accounts")]
    pub accounts: HashMap<String, AccountAuth>,
}

/// Reads keys saved before hosts were canonical, which may name the default
/// port. A key that was already canonical wins over other spellings of it, and
/// keys that aren't `name@host` are dropped.
fn deserialize_accounts<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<HashMap<String, AccountAuth>, D::Error> {
    let mut accounts = HashMap::new();
    for (key, auth) in
        HashMap::<String, AccountAuth>::deserialize(deserializer)?
    {
        let Some(user_ref) = UserRef::parse_subject(&key) else {
            continue;
        };
        let canonical = user_ref.as_subject();
        if canonical == key {
            accounts.insert(canonical, auth);
        } else {
            accounts.entry(canonical).or_insert(auth);
        }
    }
    Ok(accounts)
}

impl AuthCache {
    /// Load auth cache from disk.
    pub fn load() -> Result<Self, CliError> {
        load_data(AUTH_CACHE_FILENAME)
    }

    /// Save auth cache to disk.
    pub fn save(&self) -> Result<(), CliError> {
        save_data(self, AUTH_CACHE_FILENAME)
    }

    /// Get auth data for a user.
    pub fn get(&self, user_ref: &UserRef) -> Option<&AccountAuth> {
        self.accounts.get(&user_ref.as_subject())
    }

    /// Set auth data for a user.
    pub fn set(&mut self, user_ref: &UserRef, auth: AccountAuth) {
        self.accounts.insert(user_ref.as_subject(), auth);
    }

    /// Remove auth data for a user.
    pub fn remove(&mut self, user_ref: &UserRef) -> Option<AccountAuth> {
        self.accounts.remove(&user_ref.as_subject())
    }

    /// Check if a user has auth data.
    #[allow(dead_code)]
    pub fn has_auth(&self, user_ref: &UserRef) -> bool {
        self.accounts.contains_key(&user_ref.as_subject())
    }
}
