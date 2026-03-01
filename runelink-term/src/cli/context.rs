use reqwest::Client;
use runelink_client::requests;
use runelink_types::UserRef;
use time::OffsetDateTime;

use crate::error::CliError;
use crate::storage::{AccountConfig, AppConfig, TryGetHost};
use crate::storage_auth::AuthCache;

pub struct CliContext<'a> {
    pub client: &'a Client,
    pub config: &'a mut AppConfig,
    pub auth_cache: &'a mut AuthCache,
    pub account: Option<&'a AccountConfig>,
}

impl<'a> CliContext<'a> {
    pub fn home_api_url(&self) -> Result<String, CliError> {
        self.account.try_get_api_url()
    }

    pub fn home_host(&self) -> Result<&str, CliError> {
        self.account.try_get_host()
    }

    pub async fn get_access_token(&mut self) -> Result<String, CliError> {
        let account = self.account.ok_or(CliError::MissingAccount)?;
        let api_url = self.home_api_url()?;
        self.get_access_token_for(&account.user_ref, &api_url).await
    }

    pub async fn get_access_token_for(
        &mut self,
        user_ref: &UserRef,
        api_url: &str,
    ) -> Result<String, CliError> {
        let needs_refresh = {
            let auth = self.auth_cache.get(user_ref).ok_or_else(|| {
                CliError::InvalidArgument(
                    "Not logged in. Use 'rune account login' to authenticate."
                        .into(),
                )
            })?;

            let now = OffsetDateTime::now_utc().unix_timestamp();
            if let Some(access_token) = &auth.access_token {
                if let Some(expires_at) = auth.expires_at {
                    if expires_at > now + 60 {
                        return Ok(access_token.clone());
                    }
                } else {
                    return Ok(access_token.clone());
                }
            }
            true
        };

        if !needs_refresh {
            let auth = self.auth_cache.get(user_ref).unwrap();
            return Ok(auth.access_token.as_ref().unwrap().clone());
        }

        let auth = self.auth_cache.get_mut(user_ref).unwrap();
        let token_response = requests::auth::token_refresh(
            self.client,
            api_url,
            &auth.refresh_token,
            auth.scope.as_deref(),
            auth.client_id.as_deref(),
        )
        .await?;

        let now = OffsetDateTime::now_utc().unix_timestamp();
        auth.access_token = Some(token_response.access_token.clone());
        auth.expires_at = Some(now + token_response.expires_in);
        if !token_response.refresh_token.is_empty() {
            auth.refresh_token = token_response.refresh_token;
        }

        self.auth_cache.save()?;

        Ok(token_response.access_token)
    }
}
