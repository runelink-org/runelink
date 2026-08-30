use reqwest::Client;
use runelink_types::capability::CapabilityDiscovery;

use crate::error::Result;

pub mod auth;
pub mod channels;
pub mod generic;
pub mod memberships;
pub mod messages;
pub mod servers;
pub mod users;

pub use generic::*;

pub async fn ping(client: &Client, api_url: &str) -> Result<String> {
    let url = format!("{api_url}/ping");
    generic::fetch_text(client, &url).await
}

pub async fn capabilities(
    client: &Client,
    api_url: &str,
) -> Result<CapabilityDiscovery> {
    let url = format!("{api_url}/.well-known/runelink");
    let response = client.get(url).send().await?;
    let status = response.status();
    if !status.is_success() {
        let message = response.text().await.unwrap_or_else(|error| {
            format!("Failed to get error message body: {error}")
        });
        return Err(crate::Error::Status(status, message));
    }
    Ok(response.json().await?)
}
