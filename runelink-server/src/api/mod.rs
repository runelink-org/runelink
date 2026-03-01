use crate::{state::AppState, ws};
use axum::{Router, extract::Query, response::IntoResponse, routing::get};
use log::info;
use serde::Deserialize;

mod auth;
mod channels;
mod memberships;
mod messages;
mod servers;
mod users;

/// Creates a router for all API endpoints.
pub fn router() -> Router<AppState> {
    Router::new()
        // Mount auth router (includes OIDC discovery and auth endpoints)
        .merge(auth::router())
        // Mount websocket routers
        .route("/ws/client", get(ws::client_ws))
        .route("/ws/federation", get(ws::federation_ws))
        // API routes
        .route("/ping", get(ping))
        .route("/users", get(users::get_all).post(users::create))
        .route(
            "/users/{host}/{name}",
            get(users::get_by_ref).delete(users::delete),
        )
        .route(
            "/users/{host}/{name}/hosts",
            get(users::get_associated_hosts),
        )
        .route(
            "/users/{host}/{name}/servers",
            get(memberships::get_by_user),
        )
        .route("/messages", get(messages::get_all))
        .route(
            "/servers/{server_id}/channels/{channel_id}/messages/{message_id}",
            get(messages::get_by_id).delete(messages::delete),
        )
        .route("/channels", get(channels::get_all))
        .route(
            "/servers/{server_id}/channels/{channel_id}",
            get(channels::get_by_id).delete(channels::delete),
        )
        .route(
            "/servers/{server_id}/channels/{channel_id}/messages",
            get(messages::get_by_channel).post(messages::create),
        )
        .route("/servers", get(servers::get_all).post(servers::create))
        .route(
            "/servers/{server_id}",
            get(servers::get_by_id).delete(servers::delete),
        )
        .route(
            "/servers/{server_id}/channels",
            get(channels::get_by_server).post(channels::create),
        )
        .route(
            "/servers/{server_id}/messages",
            get(messages::get_by_server),
        )
        .route(
            "/servers/{server_id}/with_channels",
            get(servers::get_with_channels),
        )
        .route(
            "/servers/{server_id}/users",
            get(memberships::get_members_by_server).post(memberships::create),
        )
        .route(
            "/servers/{server_id}/users/{host}/{name}",
            get(memberships::get_by_user_and_server)
                .delete(memberships::delete),
        )
}

#[derive(Deserialize, Debug)]
pub struct PingParams {
    id: Option<i32>,
    msg: Option<String>,
}

pub async fn ping(Query(params): Query<PingParams>) -> impl IntoResponse {
    info!("GET /ping?id={:?}&msg={:?}", params.id, params.msg);
    let msg_part = match params.msg {
        Some(msg) => format!(": \"{msg}\""),
        None => "".to_string(),
    };
    let id_part = match params.id {
        Some(id) => format!(" ({id})"),
        None => "".to_string(),
    };
    let message = format!("pong{id_part}{msg_part}");
    println!("{message}");
    message
}
