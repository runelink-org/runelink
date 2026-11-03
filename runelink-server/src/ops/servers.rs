use runelink_types::{
    server::{
        FullServerMembership, NewServer, NewServerMembership, Server, ServerId,
        ServerMembership, ServerRole, ServerWithChannels,
    },
    ws::{
        ClientWsUpdate, FederationWsReply, FederationWsRequest,
        FederationWsUpdate,
    },
};

use super::federation;
use crate::{
    auth::Session,
    config::Locality,
    error::{ApiError, ApiResult},
    ops::fanout,
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
            if state.config.locality(&user_ref.host).is_local() {
                // Never recreate a home user deleted while this request ran
                queries::users::get_by_ref(&state.db_pool, user_ref.clone())
                    .await?;
            } else {
                // Creates the record for federated users from other hosts
                queries::users::ensure_exists(&state.db_pool, user_ref.clone())
                    .await?;
            }
            let server = queries::servers::insert(state, new_server).await?;
            let new_membership = NewServerMembership {
                user_ref,
                server_id: server.id,
                server_host: server.host.clone(),
                role: ServerRole::Admin,
            };
            let member = queries::memberships::upsert_local(
                &state.db_pool,
                &new_membership,
            )
            .await?;
            let targets =
                fanout::resolve_server_targets(state, server.id).await?;
            fanout::fanout_update(
                state,
                targets.clone(),
                ClientWsUpdate::ServerUpserted(server.clone()),
                FederationWsUpdate::ServerUpserted(server.clone()),
            )
            .await;
            let full_membership = FullServerMembership {
                server: server.clone(),
                user: member.user,
                role: ServerRole::Admin,
                joined_at: member.joined_at,
                updated_at: member.updated_at,
                synced_at: None,
            };
            fanout::fanout_update(
                state,
                targets,
                ClientWsUpdate::MembershipUpserted(full_membership.clone()),
                FederationWsUpdate::MembershipUpserted(full_membership),
            )
            .await;
            Ok(server)
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
            // Cache the remote server and creator's admin membership on the home server.
            queries::servers::upsert_remote(&state.db_pool, &server, host)
                .await?;
            let remote_membership = ServerMembership {
                server: server.clone(),
                user_ref,
                role: ServerRole::Admin,
                joined_at: server.created_at,
                updated_at: server.updated_at,
                synced_at: Some(server.created_at),
            };
            queries::memberships::insert_remote(
                &state.db_pool,
                &remote_membership,
            )
            .await?;
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
            let servers = queries::servers::get_all(state).await?;
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
            let server = queries::servers::get_by_id(state, server_id).await?;
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
            let (server, channels) = tokio::join!(
                queries::servers::get_by_id(state, server_id),
                queries::channels::list(&*state.db_pool, Some(server_id)),
            );
            Ok(ServerWithChannels {
                server: server?,
                channels: channels?,
            })
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
            let targets =
                fanout::resolve_server_targets(state, server_id).await?;
            queries::servers::delete(state, server_id).await?;
            fanout::fanout_update(
                state,
                targets,
                ClientWsUpdate::ServerDeleted { server_id },
                FederationWsUpdate::ServerDeleted { server_id },
            )
            .await;
            Ok(())
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
        queries::servers::upsert_remote(&state.db_pool, &resource, host)
            .await?;
        return Ok(resource);
    }
    let resource =
        queries::servers::update(state, server_id, server_update).await?;
    let server_id = resource.id;
    fanout::fanout_update(
        state,
        fanout::resolve_server_targets(state, server_id).await?,
        ClientWsUpdate::ServerUpserted(resource.clone()),
        FederationWsUpdate::ServerUpserted(resource.clone()),
    )
    .await;
    Ok(resource)
}
