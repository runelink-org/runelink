use log::info;
use reqwest::Client;
use runelink_types::{
    capability::Capability,
    channel::{
        NewServerChannel, ServerChannel, ServerChannelId, ServerChannelMove,
    },
    server::ServerId,
};

use crate::error::Result;

use super::{delete_authed, fetch_json_authed, post_json_authed};

pub async fn create(
    client: &Client,
    api_url: &str,
    access_token: &str,
    server_id: ServerId,
    new_channel: &NewServerChannel,
    target_host: Option<&str>,
) -> Result<ServerChannel> {
    let mut url = format!("{api_url}/servers/{server_id}/channels");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("creating channel: {url}");
    post_json_authed::<NewServerChannel, ServerChannel>(
        client,
        &url,
        access_token,
        new_channel,
        Capability::ChannelsCreate,
    )
    .await
}

pub async fn fetch_all(
    client: &Client,
    api_url: &str,
    access_token: &str,
    target_host: Option<&str>,
) -> Result<Vec<ServerChannel>> {
    let mut url = format!("{api_url}/channels");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("fetching all channels: {url}");
    fetch_json_authed::<Vec<ServerChannel>>(
        client,
        &url,
        access_token,
        Capability::ChannelsRead,
    )
    .await
}

pub async fn fetch_by_server(
    client: &Client,
    api_url: &str,
    access_token: &str,
    server_id: ServerId,
    target_host: Option<&str>,
) -> Result<Vec<ServerChannel>> {
    let mut url = format!("{api_url}/servers/{server_id}/channels");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("fetching channels by server: {url}");
    fetch_json_authed::<Vec<ServerChannel>>(
        client,
        &url,
        access_token,
        Capability::ChannelsRead,
    )
    .await
}

pub async fn fetch_by_id(
    client: &Client,
    api_url: &str,
    access_token: &str,
    channel_id: ServerChannelId,
    target_host: Option<&str>,
) -> Result<ServerChannel> {
    let mut url = format!("{api_url}/channels/{channel_id}");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("fetching channel: {url}");
    fetch_json_authed::<ServerChannel>(
        client,
        &url,
        access_token,
        Capability::ChannelsRead,
    )
    .await
}

pub async fn delete(
    client: &Client,
    api_url: &str,
    access_token: &str,
    channel_id: ServerChannelId,
    target_host: Option<&str>,
) -> Result<()> {
    let mut url = format!("{api_url}/channels/{channel_id}");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("deleting channel: {url}");
    delete_authed(client, &url, access_token, Capability::ChannelsDelete).await
}

pub async fn move_channel(
    client: &Client,
    api_url: &str,
    access_token: &str,
    channel_id: ServerChannelId,
    channel_move: &ServerChannelMove,
    target_host: Option<&str>,
) -> Result<Vec<ServerChannel>> {
    let mut url = format!("{api_url}/channels/{channel_id}/move");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("moving channel: {url}");
    post_json_authed::<ServerChannelMove, Vec<ServerChannel>>(
        client,
        &url,
        access_token,
        channel_move,
        Capability::ChannelsMove,
    )
    .await
}

pub async fn update(
    client: &Client,
    api_url: &str,
    access_token: &str,
    channel_id: ServerChannelId,
    update: &runelink_types::channel::ServerChannelUpdate,
    target_host: Option<&str>,
) -> Result<ServerChannel> {
    let mut url = format!("{api_url}/channels/{channel_id}");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    super::patch_json_authed(
        client,
        &url,
        access_token,
        update,
        Capability::ChannelsUpdate,
    )
    .await
}
