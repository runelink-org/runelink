use log::info;
use reqwest::Client;
use runelink_types::{
    capability::Capability,
    channel::TextChannelRef,
    host::Host,
    message::{Message, MessageRef, NewMessage},
    server::ServerRef,
};

use crate::error::Result;

use super::{delete_authed, fetch_json_authed, post_json_authed};

pub async fn create(
    client: &Client,
    api_url: &str,
    access_token: &str,
    text_channel_ref: &TextChannelRef,
    new_message: &NewMessage,
) -> Result<Message> {
    let url = format!(
        "{api_url}/text_channels/{}/{}/messages",
        text_channel_ref.host, text_channel_ref.id
    );
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
    target_host: Option<&Host>,
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
    server_ref: &ServerRef,
) -> Result<Vec<Message>> {
    let url = format!(
        "{api_url}/servers/{}/{}/messages",
        server_ref.host, server_ref.id
    );
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
    text_channel_ref: &TextChannelRef,
) -> Result<Vec<Message>> {
    let url = format!(
        "{api_url}/text_channels/{}/{}/messages",
        text_channel_ref.host, text_channel_ref.id
    );
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
    message_ref: &MessageRef,
) -> Result<Message> {
    let url =
        format!("{api_url}/messages/{}/{}", message_ref.host, message_ref.id);
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
    message_ref: &MessageRef,
) -> Result<()> {
    let url =
        format!("{api_url}/messages/{}/{}", message_ref.host, message_ref.id);
    info!("deleting message: {url}");
    delete_authed(client, &url, access_token, Capability::MessagesDelete).await
}

pub async fn update(
    client: &Client,
    api_url: &str,
    access_token: &str,
    message_ref: &MessageRef,
    update: &runelink_types::message::MessageUpdate,
) -> Result<Message> {
    let url =
        format!("{api_url}/messages/{}/{}", message_ref.host, message_ref.id);
    super::patch_json_authed(
        client,
        &url,
        access_token,
        update,
        Capability::MessagesUpdate,
    )
    .await
}
