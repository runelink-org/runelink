use runelink_types::{
    server::{NewServer, Server, ServerId, ServerWithChannels},
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
    target_host: Option<&str>,
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
        Locality::Remote(host) => {
            let user_ref = session.user_ref.clone().ok_or_else(|| {
                ApiError::Internal(
                    "User reference required for federated server creation"
                        .to_string(),
                )
            })?;
            let reply = federation::request(
                state,
                host,
                Some(user_ref.clone()),
                FederationWsRequest::ServersCreate(new_server.clone()),
            )
            .await?;
            let FederationWsReply::ServersCreate(server) = reply else {
                return Err(ApiError::Internal(format!(
                    "Unexpected federation reply from {host} for servers.create"
                )));
            };
            // The peer's updates for the server and its creator's membership
            // cache them here.
            Ok(server)
        }
    }
}

/// List all servers (public).
pub async fn get_all(
    state: &AppState,
    target_host: Option<&str>,
) -> ApiResult<Vec<Server>> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            // TODO: add visibility specification for servers
            let servers =
                queries::servers::get_all(&state.db_pool, &state.config)
                    .await?;
            Ok(servers)
        }
        Locality::Remote(host) => {
            // Fetch from remote host
            let reply = federation::request(
                state,
                host,
                None,
                FederationWsRequest::ServersGetAll,
            )
            .await?;
            let FederationWsReply::ServersGetAll(servers) = reply else {
                return Err(ApiError::Internal(format!(
                    "Unexpected federation reply from {host} for servers.get_all"
                )));
            };
            Ok(servers)
        }
    }
}

/// Get a server by ID (public).
pub async fn get_by_id(
    state: &AppState,
    server_id: ServerId,
    target_host: Option<&str>,
) -> ApiResult<Server> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            // TODO: separate public and private server objects?
            let server = queries::servers::get_by_id(
                &*state.db_pool,
                &state.config,
                server_id,
            )
            .await?;
            Ok(server)
        }
        Locality::Remote(host) => {
            // Fetch from remote host
            let reply = federation::request(
                state,
                host,
                None,
                FederationWsRequest::ServersGetById { server_id },
            )
            .await?;
            let FederationWsReply::ServersGetById(server) = reply else {
                return Err(ApiError::Internal(format!(
                    "Unexpected federation reply from {host} for servers.get_by_id"
                )));
            };
            Ok(server)
        }
    }
}

/// Get a server with its channels.
pub async fn get_with_channels(
    state: &AppState,
    session: &Session,
    server_id: ServerId,
    target_host: Option<&str>,
) -> ApiResult<ServerWithChannels> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            // One snapshot, so the channels always match the server.
            let mut tx = db::begin_snapshot(&state.db_pool).await?;
            let server =
                queries::servers::get_by_id(&mut *tx, &state.config, server_id)
                    .await?;
            let channels =
                queries::channels::list(&mut *tx, Some(server_id)).await?;
            Ok(ServerWithChannels { server, channels })
        }
        Locality::Remote(host) => {
            let user_ref = session.user_ref.clone().ok_or_else(|| {
                ApiError::Internal(
                    "User reference required for federated server fetching"
                        .to_string(),
                )
            })?;
            let reply = federation::request(
                state,
                host,
                Some(user_ref),
                FederationWsRequest::ServersGetWithChannels { server_id },
            )
            .await?;
            let FederationWsReply::ServersGetWithChannels(server_with_channels) =
                reply
            else {
                return Err(ApiError::Internal(format!(
                    "Unexpected federation reply from {host} for servers.get_with_channels"
                )));
            };
            Ok(server_with_channels)
        }
    }
}

/// Delete a server by ID.
pub async fn delete(
    state: &AppState,
    session: &Session,
    server_id: ServerId,
    target_host: Option<&str>,
) -> ApiResult<()> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            state
                .actors
                .delete_server(server_id, session.authorization())
                .await
        }
        Locality::Remote(host) => {
            let user_ref = session.user_ref.clone().ok_or_else(|| {
                ApiError::Internal(
                    "User reference required for federated server deletion"
                        .to_string(),
                )
            })?;
            let reply = federation::request(
                state,
                host,
                Some(user_ref),
                FederationWsRequest::ServersDelete { server_id },
            )
            .await?;
            let FederationWsReply::ServersDelete = reply else {
                return Err(ApiError::Internal(format!(
                    "Unexpected federation reply from {host} for servers.delete"
                )));
            };
            Ok(())
        }
    }
}

/// Auth requirements for server operations.
pub mod auth {
    use super::*;
    use crate::auth::Requirement as Req;

    pub async fn update(
        state: &AppState,
        server_id: ServerId,
        target_host: Option<&str>,
    ) -> ApiResult<Req> {
        if state.config.target_locality(target_host).is_remote() {
            return Ok(Req::Client);
        }
        Ok(Req::ServerAdmin(server_id).or_admin().client_only())
    }
    pub fn create() -> Req {
        // TODO: add rate limiting or something
        Req::Always.or_admin().client_only()
    }

    pub fn get_with_channels(server_id: ServerId) -> Req {
        Req::ServerMember(server_id).or_admin().client_only()
    }

    pub fn delete(server_id: ServerId) -> Req {
        Req::ServerAdmin(server_id).or_admin().client_only()
    }

    pub mod federated {
        use super::*;

        pub async fn update(
            _state: &AppState,
            server_id: ServerId,
        ) -> ApiResult<Req> {
            Ok(Req::ServerAdmin(server_id).federated_only())
        }
        pub fn create() -> Req {
            // TODO: see above
            Req::Always.federated_only()
        }

        pub fn get_with_channels(server_id: ServerId) -> Req {
            Req::ServerMember(server_id).federated_only()
        }

        pub fn delete(server_id: ServerId) -> Req {
            Req::ServerAdmin(server_id).federated_only()
        }
    }
}

pub async fn update(
    state: &AppState,
    session: &Session,
    server_id: ServerId,
    server_update: &runelink_types::server::ServerUpdate,
    target_host: Option<&str>,
) -> ApiResult<Server> {
    if let Locality::Remote(host) = state.config.target_locality(target_host) {
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::AuthError("User identity required".into())
        })?;
        let reply = federation::request(
            state,
            host,
            Some(user_ref),
            FederationWsRequest::ServersUpdate {
                server_id,
                server_update: server_update.clone(),
            },
        )
        .await?;
        let FederationWsReply::ServersUpdate(resource) = reply else {
            return Err(ApiError::Internal("Unexpected edit reply".into()));
        };
        return Ok(resource);
    }
    state
        .actors
        .update_server(
            server_id,
            server_update.clone(),
            session.authorization(),
        )
        .await
}
