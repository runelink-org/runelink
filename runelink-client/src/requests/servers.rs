use log::info;
use reqwest::Client;
use runelink_types::{NewServer, Server, ServerWithChannels, UserRef};
use uuid::Uuid;

use crate::{error::Result, requests};

use super::{delete_authed, fetch_json, fetch_json_authed, post_json_authed};

pub async fn create(
    client: &Client,
    api_url: &str,
    access_token: &str,
    new_server: &NewServer,
    target_host: Option<&str>,
) -> Result<Server> {
    let mut url = format!("{api_url}/servers");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("creating server: {url}");
    post_json_authed::<_, Server>(client, &url, access_token, new_server).await
}

pub async fn fetch_all(
    client: &Client,
    api_url: &str,
    target_host: Option<&str>,
) -> Result<Vec<Server>> {
    let mut url = format!("{api_url}/servers");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("fetching all servers: {url}");
    fetch_json::<Vec<Server>>(client, &url).await
}

pub async fn fetch_by_id(
    client: &Client,
    api_url: &str,
    server_id: Uuid,
    target_host: Option<&str>,
) -> Result<Server> {
    let mut url = format!("{api_url}/servers/{server_id}");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("fetching server: {url}");
    fetch_json::<Server>(client, &url).await
}

pub async fn fetch_by_user(
    client: &Client,
    api_url: &str,
    user_ref: UserRef,
) -> Result<Vec<Server>> {
    let servers =
        requests::memberships::fetch_by_user(client, api_url, user_ref)
            .await?
            .into_iter()
            .map(|m| m.server)
            .collect();
    info!("converted memberships to servers");
    Ok(servers)
}

pub async fn fetch_with_channels(
    client: &Client,
    api_url: &str,
    access_token: &str,
    server_id: Uuid,
    target_host: Option<&str>,
) -> Result<ServerWithChannels> {
    let mut url = format!("{api_url}/servers/{server_id}/with_channels");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("fetching server with channels (federation): {url}");
    fetch_json_authed::<ServerWithChannels>(client, &url, access_token).await
}

pub async fn delete(
    client: &Client,
    api_url: &str,
    access_token: &str,
    server_id: Uuid,
    target_host: Option<&str>,
) -> Result<()> {
    let mut url = format!("{api_url}/servers/{server_id}");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("deleting server: {url}");
    delete_authed(client, &url, access_token).await
}
