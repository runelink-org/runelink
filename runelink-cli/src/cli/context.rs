use std::io::{self, IsTerminal};

use reqwest::{Client, StatusCode};
use runelink_client::requests;
use runelink_types::UserRef;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::cli::input::read_password;
use crate::error::CliError;
use crate::storage::{AccountConfig, AppConfig, TryGetHost, resolve_api_url};
use crate::storage_auth::{AccountAuth, AuthCache};

const MAX_PASSWORD_ATTEMPTS: u32 = 3;

pub struct CliContext<'a> {
    pub client: &'a Client,
    pub config: &'a mut AppConfig,
    pub auth_cache: &'a mut AuthCache,
    pub account: Option<&'a AccountConfig>,
    pub strict_input: bool,
}

impl<'a> CliContext<'a> {
    pub async fn home_api_url(&mut self) -> Result<String, CliError> {
        let host = self.account.try_get_host()?.to_string();
        resolve_api_url(self.client, self.config, &host).await
    }

    pub fn home_host(&self) -> Result<&str, CliError> {
        self.account.try_get_host()
    }

    pub async fn get_access_token(&mut self) -> Result<String, CliError> {
        let account = self.account.ok_or(CliError::MissingAccount)?;
        let api_url = self.home_api_url().await?;
        self.get_access_token_for(&account.user_ref, &api_url).await
    }

    pub async fn get_access_token_for(
        &mut self,
        user_ref: &UserRef,
        api_url: &str,
    ) -> Result<String, CliError> {
        let Some(auth) = self.auth_cache.get(user_ref) else {
            eprintln!("Not logged in: {user_ref}");
            return self.login_with_password(user_ref).await;
        };

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

        let auth = self.auth_cache.get_mut(user_ref).unwrap();
        let token_response = match requests::auth::token_refresh(
            self.client,
            api_url,
            &auth.refresh_token,
            auth.scope.as_deref(),
            auth.client_id.as_deref(),
        )
        .await
        {
            Ok(response) => response,
            // Expired or revoked refresh tokens get 401; unknown ones get 404
            Err(runelink_client::Error::Status(
                StatusCode::UNAUTHORIZED | StatusCode::NOT_FOUND,
                _,
            )) => {
                eprintln!("Session expired: {user_ref}");
                return self.login_with_password(user_ref).await;
            }
            Err(e) => return Err(e.into()),
        };

        let now = OffsetDateTime::now_utc().unix_timestamp();
        auth.access_token = Some(token_response.access_token.clone());
        auth.expires_at = Some(now + token_response.expires_in);
        if !token_response.refresh_token.is_empty() {
            auth.refresh_token = token_response.refresh_token;
        }

        self.auth_cache.save()?;

        Ok(token_response.access_token)
    }

    /// Prompts for the password and stores new tokens, re-prompting on
    /// empty or rejected passwords like sudo. Returns the access token.
    pub async fn login_with_password(
        &mut self,
        user_ref: &UserRef,
    ) -> Result<String, CliError> {
        if !io::stdin().is_terminal() {
            return Err(CliError::AuthenticationFailed(format!(
                "Cannot prompt for the password of {user_ref} without a \
                 terminal."
            )));
        }

        let api_url =
            resolve_api_url(self.client, self.config, &user_ref.host).await?;
        // TODO: Don't generate a random client_id for each session
        let client_id = Uuid::new_v4().to_string();

        let mut token_response = None;
        for attempt in 0..MAX_PASSWORD_ATTEMPTS {
            if attempt > 0 {
                eprintln!("Sorry, try again.");
            }
            let Some(password) = read_password("Password: ")? else {
                continue;
            };
            match requests::auth::token_password(
                self.client,
                &api_url,
                &user_ref.name,
                &password,
                None,
                Some(&client_id),
            )
            .await
            {
                Ok(response) => {
                    token_response = Some(response);
                    break;
                }
                Err(runelink_client::Error::Status(
                    StatusCode::UNAUTHORIZED,
                    _,
                )) => {}
                Err(runelink_client::Error::Status(
                    StatusCode::NOT_FOUND,
                    _,
                )) => {
                    return Err(CliError::NotFound(format!(
                        "Account not found on host: {user_ref}"
                    )));
                }
                Err(e) => return Err(e.into()),
            }
        }
        let token_response = token_response.ok_or_else(|| {
            CliError::AuthenticationFailed(format!(
                "{MAX_PASSWORD_ATTEMPTS} incorrect password attempts."
            ))
        })?;

        let access_token = token_response.access_token;
        self.auth_cache.set(
            user_ref,
            AccountAuth {
                refresh_token: token_response.refresh_token,
                access_token: Some(access_token.clone()),
                expires_at: Some(
                    OffsetDateTime::now_utc().unix_timestamp()
                        + token_response.expires_in,
                ),
                client_id: Some(client_id),
                scope: None,
            },
        );
        self.auth_cache.save()?;

        Ok(access_token)
    }
}
