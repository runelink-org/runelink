use runelink_types::UserRef;
use serde::{Deserialize, Serialize};
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
    pub accounts: HashMap<String, AccountAuth>,
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

    /// Get mutable auth data for a user.
    pub fn get_mut(&mut self, user_ref: &UserRef) -> Option<&mut AccountAuth> {
        self.accounts.get_mut(&user_ref.as_subject())
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
