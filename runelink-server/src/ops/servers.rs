use runelink_types::{
    host::Host,
    server::{NewServer, Server, ServerRef, ServerWithChannels},
    ws::{FederationWsReply, FederationWsRequest},
};

use super::federation;
use crate::{
    auth::Session,
    config::Locality,
    db,
    error::{ApiError, ApiResult},
    queries,
    state::AppState,
};

/// Create a new server and add the creator as admin.
pub async fn create(
    state: &AppState,
    session: &Session,
    new_server: &NewServer,
    target_host: Option<&Host>,
) -> ApiResult<Server> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            // Get the creator's user identity
            // Since this requires HostAdmin (which requires client auth), these fields are always present
            let user_ref = session.user_ref.clone().ok_or_else(|| {
                ApiError::Internal(
                    "Session missing user identity for server creation".into(),
                )
            })?;
            state
                .actors
                .create_server(
                    new_server.clone(),
                    user_ref,
                    session.authorization(),
                )
                .await
        }
        // The peer's updates for the server and its creator's membership
        // cache them here.
        Locality::Remote(host) => federation::request!(
            state,
            host,
            Some(session),
            FederationWsRequest::ServersCreate(new_server.clone()),
            FederationWsReply::ServersCreate(server) => server
        ),
    }
}

/// List all servers (public).
pub async fn get_all(
    state: &AppState,
    target_host: Option<&Host>,
) -> ApiResult<Vec<Server>> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            // TODO: add visibility specification for servers
            let servers =
                queries::servers::get_all(&state.db_pool, &state.config)
                    .await?;
            Ok(servers)
        }
        Locality::Remote(host) => federation::request!(
            state,
            host,
            None,
            FederationWsRequest::ServersGetAll,
            FederationWsReply::ServersGetAll(servers) => servers
        ),
    }
}

/// Get a server by ID (public).
pub async fn get_by_id(
    state: &AppState,
    server_ref: &ServerRef,
) -> ApiResult<Server> {
    match state.config.locality(&server_ref.host) {
        Locality::Local => {
            // TODO: separate public and private server objects?
            let server = queries::servers::get_by_id(
                &*state.db_pool,
                &state.config,
                server_ref.id,
            )
            .await?;
            Ok(server)
        }
        Locality::Remote(host) => federation::request!(
            state,
            host,
            None,
            FederationWsRequest::ServersGetById {
                server_id: server_ref.id,
            },
            FederationWsReply::ServersGetById(server) => server
        ),
    }
}

/// Get a server with its channels.
pub async fn get_with_channels(
    state: &AppState,
    session: &Session,
    server_ref: &ServerRef,
) -> ApiResult<ServerWithChannels> {
    match state.config.locality(&server_ref.host) {
        Locality::Local => {
            // One snapshot, so the channels always match the server.
            let mut tx = db::begin_snapshot(&state.db_pool).await?;
            let channels = queries::channels::list(
                &mut *tx,
                state.config.public_host(),
                Some(server_ref.id),
            )
            .await?;
            let server = queries::servers::get_by_id(
                &mut *tx,
                &state.config,
                server_ref.id,
            )
            .await?;
            Ok(ServerWithChannels { server, channels })
        }
        Locality::Remote(host) => federation::request!(
            state,
            host,
            Some(session),
            FederationWsRequest::ServersGetWithChannels {
                server_id: server_ref.id,
            },
            FederationWsReply::ServersGetWithChannels(server_with_channels) =>
                server_with_channels
        ),
    }
}

/// Delete a server by ID.
pub async fn delete(
    state: &AppState,
    session: &Session,
    server_ref: &ServerRef,
) -> ApiResult<()> {
    match state.config.locality(&server_ref.host) {
        Locality::Local => {
            state
                .actors
                .delete_server(server_ref.id, session.authorization())
                .await
        }
        Locality::Remote(host) => federation::request!(
            state,
            host,
            Some(session),
            FederationWsRequest::ServersDelete {
                server_id: server_ref.id,
            },
            FederationWsReply::ServersDelete => ()
        ),
    }
}

/// Auth requirements for server operations.
pub mod auth {
    use super::*;
    use crate::auth::Requirement as Req;

    pub fn update(server_ref: &ServerRef) -> Req {
        Req::ServerAdmin(server_ref.clone())
            .or_admin()
            .client_only()
    }
    pub fn create() -> Req {
        // TODO: add rate limiting or something
        Req::Always.or_admin().client_only()
    }

    pub fn get_with_channels(server_ref: &ServerRef) -> Req {
        Req::ServerMember(server_ref.clone())
            .or_admin()
            .client_only()
    }

    pub fn delete(server_ref: &ServerRef) -> Req {
        Req::ServerAdmin(server_ref.clone())
            .or_admin()
            .client_only()
    }

    pub mod federated {
        use super::*;

        pub fn update(server_ref: ServerRef) -> Req {
            Req::ServerAdmin(server_ref).federated_only()
        }
        pub fn create() -> Req {
            // TODO: see above
            Req::Always.federated_only()
        }

        pub fn get_with_channels(server_ref: ServerRef) -> Req {
            Req::ServerMember(server_ref).federated_only()
        }

        pub fn delete(server_ref: ServerRef) -> Req {
            Req::ServerAdmin(server_ref).federated_only()
        }
    }
}

pub async fn update(
    state: &AppState,
    session: &Session,
    server_ref: &ServerRef,
    server_update: &runelink_types::server::ServerUpdate,
) -> ApiResult<Server> {
    if let Locality::Remote(host) = state.config.locality(&server_ref.host) {
        return federation::request!(
            state,
            host,
            Some(session),
            FederationWsRequest::ServersUpdate {
                server_id: server_ref.id,
                server_update: server_update.clone(),
            },
            FederationWsReply::ServersUpdate(server) => server
        );
    }
    state
        .actors
        .update_server(
            server_ref.id,
            server_update.clone(),
            session.authorization(),
        )
        .await
}
