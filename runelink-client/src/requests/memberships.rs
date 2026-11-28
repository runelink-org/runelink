use log::info;
use reqwest::Client;
use runelink_types::{
    capability::Capability,
    server::{
        FullServerMembership, NewServerMembership, ServerMembership, ServerRef,
    },
    user::UserRef,
};

use crate::error::Result;

use super::{delete_authed, fetch_json, post_json_authed};

pub async fn fetch_by_user(
    client: &Client,
    api_url: &str,
    user_ref: UserRef,
) -> Result<Vec<ServerMembership>> {
    let url = format!(
        "{api_url}/users/{host}/{name}/servers",
        host = user_ref.host,
        name = user_ref.name
    );
    info!("fetching memberships by user: {url}");
    fetch_json::<Vec<ServerMembership>>(
        client,
        &url,
        Capability::MembershipsRead,
    )
    .await
}

pub async fn fetch_members_by_server(
    client: &Client,
    api_url: &str,
    server_ref: &ServerRef,
) -> Result<Vec<runelink_types::ServerMember>> {
    let url = format!(
        "{api_url}/servers/{}/{}/users",
        server_ref.host, server_ref.id
    );
    info!("fetching members by server: {url}");
    fetch_json::<Vec<runelink_types::ServerMember>>(
        client,
        &url,
        Capability::MembershipsRead,
    )
    .await
}

pub async fn fetch_member_by_user_and_server(
    client: &Client,
    api_url: &str,
    server_ref: &ServerRef,
    user_ref: UserRef,
) -> Result<runelink_types::ServerMember> {
    let url = format!(
        "{api_url}/servers/{}/{}/users/{}/{}",
        server_ref.host, server_ref.id, user_ref.host, user_ref.name
    );
    info!("fetching member by user and server: {url}");
    fetch_json::<runelink_types::ServerMember>(
        client,
        &url,
        Capability::MembershipsRead,
    )
    .await
}

pub async fn create(
    client: &Client,
    api_url: &str,
    access_token: &str,
    new_membership: &NewServerMembership,
) -> Result<FullServerMembership> {
    let server_ref = &new_membership.server_ref;
    let url = format!(
        "{api_url}/servers/{}/{}/users",
        server_ref.host, server_ref.id
    );
    info!("creating membership: {url}");
    post_json_authed::<NewServerMembership, FullServerMembership>(
        client,
        &url,
        access_token,
        new_membership,
        Capability::MembershipsWrite,
    )
    .await
}

pub async fn delete(
    client: &Client,
    api_url: &str,
    access_token: &str,
    server_ref: &ServerRef,
    user_ref: UserRef,
) -> Result<()> {
    let url = format!(
        "{api_url}/servers/{}/{}/users/{}/{}",
        server_ref.host, server_ref.id, user_ref.host, user_ref.name
    );
    info!("deleting server membership: {url}");
    delete_authed(client, &url, access_token, Capability::MembershipsWrite)
        .await
}
