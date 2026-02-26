use log::info;
use reqwest::Client;
use runelink_types::{NewUser, User, UserRef};

use crate::error::Result;

use super::{delete_authed, fetch_json, post_json_authed};

pub async fn create(
    client: &Client,
    api_url: &str,
    access_token: &str,
    new_user: &NewUser,
) -> Result<User> {
    let url = format!("{api_url}/users");
    info!("creating user: {url}");
    post_json_authed::<NewUser, User>(client, &url, access_token, new_user)
        .await
}

pub async fn fetch_all(
    client: &Client,
    api_url: &str,
    target_host: Option<&str>,
) -> Result<Vec<User>> {
    let mut url = format!("{api_url}/users");
    if let Some(host) = target_host {
        url = format!("{url}?target_host={host}");
    }
    info!("fetching all users: {url}");
    fetch_json::<Vec<User>>(client, &url).await
}

pub async fn fetch_by_ref(
    client: &Client,
    api_url: &str,
    user: UserRef,
) -> Result<User> {
    let url = format!(
        "{api_url}/users/{host}/{name}",
        host = user.host,
        name = user.name
    );
    info!("fetching user: {url}");
    fetch_json::<User>(client, &url).await
}

pub async fn delete(
    client: &Client,
    api_url: &str,
    access_token: &str,
    user: UserRef,
) -> Result<()> {
    let url = format!(
        "{api_url}/users/{host}/{name}",
        host = user.host,
        name = user.name
    );
    info!("deleting user: {url}");
    delete_authed(client, &url, access_token).await
}

pub async fn fetch_associated_hosts(
    client: &Client,
    api_url: &str,
    user_ref: UserRef,
    target_host: Option<&str>,
) -> Result<Vec<String>> {
    let mut url = format!(
        "{api_url}/users/{host}/{name}/hosts",
        host = user_ref.host,
        name = user_ref.name
    );
    if let Some(d) = target_host {
        url = format!("{url}?target_host={d}");
    }
    info!("fetching user associated hosts: {url}");
    fetch_json::<Vec<String>>(client, &url).await
}
