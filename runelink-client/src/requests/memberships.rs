use log::info;
use reqwest::Client;
use runelink_types::{
    FullServerMembership, NewServerMembership, ServerMembership, UserRef,
};
use uuid::Uuid;

use crate::error::Result;

use super::{delete_authed, fetch_json, post_json_authed};

pub async fn fetch_by_user(
    client: &Client,
    api_url: &str,
    user: UserRef,
) -> Result<Vec<ServerMembership>> {
    let url = format!(
        "{api_url}/users/{host}/{name}/servers",
        host = user.host,
        name = user.name
    );
    info!("fetching memberships by user: {url}");
    fetch_json::<Vec<ServerMembership>>(client, &url).await
}

pub async fn fetch_members_by_server(
    client: &Client,
    api_url: &str,
    server_id: Uuid,
    target_host: Option<&str>,
) -> Result<Vec<runelink_types::ServerMember>> {
    let mut url = format!("{api_url}/servers/{server_id}/users");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("fetching members by server: {url}");
    fetch_json::<Vec<runelink_types::ServerMember>>(client, &url).await
}

pub async fn fetch_member_by_user_and_server(
    client: &Client,
    api_url: &str,
    server_id: Uuid,
    user: UserRef,
    target_host: Option<&str>,
) -> Result<runelink_types::ServerMember> {
    let mut url = format!(
        "{api_url}/servers/{server_id}/users/{host}/{name}",
        host = user.host,
        name = user.name
    );
    if let Some(d) = target_host {
        url = format!("{url}?target_host={d}");
    }
    info!("fetching member by user and server: {url}");
    fetch_json::<runelink_types::ServerMember>(client, &url).await
}

pub async fn create(
    client: &Client,
    api_url: &str,
    access_token: &str,
    new_membership: &NewServerMembership,
) -> Result<FullServerMembership> {
    let url = format!(
        "{api_url}/servers/{server_id}/users",
        server_id = new_membership.server_id
    );
    info!("creating membership: {url}");
    post_json_authed::<NewServerMembership, FullServerMembership>(
        client,
        &url,
        access_token,
        new_membership,
    )
    .await
}

pub async fn delete(
    client: &Client,
    api_url: &str,
    access_token: &str,
    server_id: Uuid,
    user: UserRef,
    target_host: Option<&str>,
) -> Result<()> {
    let mut url = format!(
        "{api_url}/servers/{server_id}/users/{host}/{name}",
        host = user.host,
        name = user.name
    );
    if let Some(d) = target_host {
        url = format!("{url}?target_host={d}");
    }
    info!("deleting server membership: {url}");
    delete_authed(client, &url, access_token).await
}
