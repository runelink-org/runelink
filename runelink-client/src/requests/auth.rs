use log::info;
use reqwest::Client;
use runelink_types::{
    SignupRequest, TokenResponse, User, capability::Capability,
};
use std::collections::HashMap;

use crate::{
    capabilities::http_header_value,
    error::{Error, Result},
};

use super::post_json;

/// Create a new user account.
///
/// POST /auth/signup
pub async fn signup(
    client: &Client,
    api_url: &str,
    signup_req: &SignupRequest,
) -> Result<User> {
    let url = format!("{api_url}/auth/signup");
    info!("signing up: {url}");
    post_json::<SignupRequest, User>(
        client,
        &url,
        signup_req,
        Capability::AuthSignup,
    )
    .await
}

/// Request an access token using password grant.
///
/// POST /auth/token with grant_type=password
pub async fn token_password(
    client: &Client,
    api_url: &str,
    username: &str,
    password: &str,
    scope: Option<&str>,
    client_id: Option<&str>,
) -> Result<TokenResponse> {
    let url = format!("{api_url}/auth/token");
    info!("requesting token (password grant): {url}");

    let mut form = HashMap::new();
    form.insert("grant_type", "password");
    form.insert("username", username);
    form.insert("password", password);
    if let Some(scope) = scope {
        form.insert("scope", scope);
    }
    if let Some(client_id) = client_id {
        form.insert("client_id", client_id);
    }

    let capability = Capability::AuthToken;
    let response = client
        .post(&url)
        .header(
            runelink_types::capability::HTTP_CAPABILITY_HEADER,
            http_header_value(&capability)?,
        )
        .form(&form)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        let message = response.text().await.unwrap_or_else(|e| {
            format!("Failed to get error message body: {e}")
        });
        return Err(Error::Status(status, message));
    }
    let data = response.json::<TokenResponse>().await?;
    Ok(data)
}

/// Request a new access token using refresh token grant.
///
/// POST /auth/token with grant_type=refresh_token
pub async fn token_refresh(
    client: &Client,
    api_url: &str,
    refresh_token: &str,
    scope: Option<&str>,
    client_id: Option<&str>,
) -> Result<TokenResponse> {
    let url = format!("{api_url}/auth/token");
    info!("requesting token (refresh_token grant): {url}");

    let mut form = HashMap::new();
    form.insert("grant_type", "refresh_token");
    form.insert("refresh_token", refresh_token);
    if let Some(scope) = scope {
        form.insert("scope", scope);
    }
    if let Some(client_id) = client_id {
        form.insert("client_id", client_id);
    }

    let capability = Capability::AuthToken;
    let response = client
        .post(&url)
        .header(
            runelink_types::capability::HTTP_CAPABILITY_HEADER,
            http_header_value(&capability)?,
        )
        .form(&form)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        let message = response.text().await.unwrap_or_else(|e| {
            format!("Failed to get error message body: {e}")
        });
        return Err(Error::Status(status, message));
    }
    let data = response.json::<TokenResponse>().await?;
    Ok(data)
}
