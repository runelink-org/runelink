use crate::{state::AppState, ws};
use axum::{
    Router, middleware,
    routing::{get, post},
};
use tower_http::cors;

mod auth;
mod capabilities;
mod channels;
mod memberships;
mod messages;
mod servers;
mod users;

/// Creates a router for all API endpoints.
pub fn router() -> Router<AppState> {
    let cors = cors::CorsLayer::new()
        .allow_origin(cors::Any)
        .allow_methods(cors::Any)
        .allow_headers(cors::Any);

    Router::new()
        // Mount auth router (includes OIDC discovery and auth endpoints)
        .merge(auth::router())
        .route("/.well-known/runelink", get(capabilities::discovery))
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
            "/messages/{host}/{id}",
            get(messages::get_by_id)
                .delete(messages::delete)
                .patch(messages::update),
        )
        .route(
            "/text_channels/{host}/{id}/messages",
            get(messages::get_by_text_channel).post(messages::create),
        )
        .route("/channels", get(channels::get_all))
        .route(
            "/channels/{host}/{id}",
            get(channels::get_by_id)
                .delete(channels::delete)
                .patch(channels::update),
        )
        .route("/channels/{host}/{id}/move", post(channels::move_channel))
        .route("/servers", get(servers::get_all).post(servers::create))
        .route(
            "/servers/{host}/{id}",
            get(servers::get_by_id)
                .delete(servers::delete)
                .patch(servers::update),
        )
        .route(
            "/servers/{host}/{id}/channels",
            get(channels::get_by_server).post(channels::create),
        )
        .route(
            "/servers/{host}/{id}/messages",
            get(messages::get_by_server),
        )
        .route(
            "/servers/{host}/{id}/with_channels",
            get(servers::get_with_channels),
        )
        .route(
            "/servers/{host}/{id}/users",
            get(memberships::get_members_by_server).post(memberships::create),
        )
        .route(
            "/servers/{host}/{id}/users/{user_host}/{name}",
            get(memberships::get_by_user_and_server)
                .delete(memberships::delete),
        )
        .route_layer(middleware::from_fn(capabilities::require_capability))
        .layer(cors)
}

pub async fn ping() -> &'static str {
    "pong"
}
