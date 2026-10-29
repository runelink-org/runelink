use log::info;
use reqwest::Client;
use runelink_types::{
    capability::Capability,
    channel::TextChannelId,
    message::{Message, MessageId, NewMessage},
    server::ServerId,
};

use crate::error::Result;

use super::{delete_authed, fetch_json_authed, post_json_authed};

pub async fn create(
    client: &Client,
    api_url: &str,
    access_token: &str,
    text_channel_id: TextChannelId,
    new_message: &NewMessage,
    target_host: Option<&str>,
) -> Result<Message> {
    let mut url = format!("{api_url}/text_channels/{text_channel_id}/messages");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("creating message: {url}");
    post_json_authed::<NewMessage, Message>(
        client,
        &url,
        access_token,
        new_message,
        Capability::MessagesCreate,
    )
    .await
}

#[allow(dead_code)]
pub async fn fetch_all(
    client: &Client,
    api_url: &str,
    access_token: &str,
    target_host: Option<&str>,
) -> Result<Vec<Message>> {
    let mut url = format!("{api_url}/messages");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("fetching all messages: {url}");
    fetch_json_authed::<Vec<Message>>(
        client,
        &url,
        access_token,
        Capability::MessagesRead,
    )
    .await
}

#[allow(dead_code)]
pub async fn fetch_by_server(
    client: &Client,
    api_url: &str,
    access_token: &str,
    server_id: ServerId,
    target_host: Option<&str>,
) -> Result<Vec<Message>> {
    let mut url = format!("{api_url}/servers/{server_id}/messages");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("fetching messages by server: {url}");
    fetch_json_authed::<Vec<Message>>(
        client,
        &url,
        access_token,
        Capability::MessagesRead,
    )
    .await
}

pub async fn fetch_by_text_channel(
    client: &Client,
    api_url: &str,
    access_token: &str,
    text_channel_id: TextChannelId,
    target_host: Option<&str>,
) -> Result<Vec<Message>> {
    let mut url = format!("{api_url}/text_channels/{text_channel_id}/messages");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("fetching messages by channel: {url}");
    fetch_json_authed::<Vec<Message>>(
        client,
        &url,
        access_token,
        Capability::MessagesRead,
    )
    .await
}

pub async fn fetch_by_id(
    client: &Client,
    api_url: &str,
    access_token: &str,
    message_id: MessageId,
    target_host: Option<&str>,
) -> Result<Message> {
    let mut url = format!("{api_url}/messages/{message_id}");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("fetching message: {url}");
    fetch_json_authed::<Message>(
        client,
        &url,
        access_token,
        Capability::MessagesRead,
    )
    .await
}

pub async fn delete(
    client: &Client,
    api_url: &str,
    access_token: &str,
    message_id: MessageId,
    target_host: Option<&str>,
) -> Result<()> {
    let mut url = format!("{api_url}/messages/{message_id}");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("deleting message: {url}");
    delete_authed(client, &url, access_token, Capability::MessagesDelete).await
}

pub async fn update(
    client: &Client,
    api_url: &str,
    access_token: &str,
    message_id: MessageId,
    update: &runelink_types::message::MessageUpdate,
    target_host: Option<&str>,
) -> Result<Message> {
    let mut url = format!("{api_url}/messages/{message_id}");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    super::patch_json_authed(
        client,
        &url,
        access_token,
        update,
        Capability::MessagesUpdate,
    )
    .await
}
