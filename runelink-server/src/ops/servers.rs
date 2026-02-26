use runelink_client::{requests, util::get_api_url};
use runelink_types::{
    server::{
        NewServer, NewServerMembership, Server, ServerMembership, ServerRole,
        ServerWithChannels,
    },
    ws::{ClientWsUpdate, FederationWsUpdate},
};
use uuid::Uuid;

use crate::{
    auth::Session,
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
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let server = queries::servers::insert(state, new_server).await?;
        // Get the creator's user identity
        // Since this requires HostAdmin (which requires client auth), these fields are always present
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::Internal(
                "Session missing user identity for server creation".into(),
            )
        })?;
        // Ensure user exists (creates record for federated users from other hosts)
        queries::users::ensure_exists(&state.db_pool, user_ref.clone()).await?;
        let new_membership = NewServerMembership {
            user_ref,
            server_id: server.id,
            server_host: server.host.clone(),
            role: ServerRole::Admin,
        };
        queries::memberships::insert_local(&state.db_pool, &new_membership)
            .await?;
        fanout::fanout_update(
            state,
            fanout::resolve_server_targets(state, server.id).await?,
            ClientWsUpdate::ServerUpserted(server.clone()),
            FederationWsUpdate::ServerUpserted(server.clone()),
        )
        .await;
        Ok(server)
    } else {
        // Create on remote host using federation
        let host = target_host.unwrap();
        let api_url = get_api_url(host);
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated server creation"
                    .to_string(),
            )
        })?;
        let token = state.key_manager.issue_federation_jwt_delegated(
            state.config.api_url(),
            api_url.clone(),
            user_ref.clone(),
        )?;
        let server = requests::servers::federated::create(
            &state.http_client,
            &api_url,
            &token,
            new_server,
        )
        .await
        .map_err(|e| {
            ApiError::Internal(format!(
                "Failed to create server on {host}: {e}"
            ))
        })?;
        // Cache the remote server and creator's admin membership on the home server.
        queries::servers::upsert_remote(&state.db_pool, &server).await?;
        let remote_membership = ServerMembership {
            server: server.clone(),
            user_ref,
            role: ServerRole::Admin,
            joined_at: server.created_at,
            updated_at: server.updated_at,
            synced_at: Some(server.created_at),
        };
        queries::memberships::insert_remote(&state.db_pool, &remote_membership)
            .await?;
        Ok(server)
    }
}

/// List all servers (public).
pub async fn get_all(
    state: &AppState,
    target_host: Option<&str>,
) -> ApiResult<Vec<Server>> {
    if !state.config.is_remote_host(target_host) {
        // Handle local case
        // TODO: add visibility specification for servers
        // We could then have an admin endpoint for all servers
        // and a public endpoint for only public servers
        let servers = queries::servers::get_all(state).await?;
        Ok(servers)
    } else {
        // Fetch from remote host
        let host = target_host.unwrap();
        let api_url = get_api_url(host);
        let servers =
            requests::servers::fetch_all(&state.http_client, &api_url, None)
                .await
                .map_err(|e| {
                    ApiError::Internal(format!(
                        "Failed to fetch servers from {host}: {e}"
                    ))
                })?;
        Ok(servers)
    }
}

/// Get a server by ID (public).
pub async fn get_by_id(
    state: &AppState,
    server_id: Uuid,
    target_host: Option<&str>,
) -> ApiResult<Server> {
    if !state.config.is_remote_host(target_host) {
        // Handle local case
        // TODO: separate public and private server objects?
        let server = queries::servers::get_by_id(state, server_id).await?;
        Ok(server)
    } else {
        // Fetch from remote host
        let host = target_host.unwrap();
        let api_url = get_api_url(host);
        let server = requests::servers::fetch_by_id(
            &state.http_client,
            &api_url,
            server_id,
            None,
        )
        .await
        .map_err(|e| {
            ApiError::Internal(format!(
                "Failed to fetch server from {host}: {e}"
            ))
        })?;
        Ok(server)
    }
}

/// Get a server with its channels.
pub async fn get_with_channels(
    state: &AppState,
    session: &Session,
    server_id: Uuid,
    target_host: Option<&str>,
) -> ApiResult<ServerWithChannels> {
    if !state.config.is_remote_host(target_host) {
        // Handle local case
        let (server, channels) = tokio::join!(
            queries::servers::get_by_id(state, server_id),
            queries::channels::get_by_server(&state.db_pool, server_id),
        );
        Ok(ServerWithChannels {
            server: server?,
            channels: channels?,
        })
    } else {
        // Fetch from remote host using federation
        let host = target_host.unwrap();
        let api_url = get_api_url(host);
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated server fetching"
                    .to_string(),
            )
        })?;
        let token = state.key_manager.issue_federation_jwt_delegated(
            state.config.api_url(),
            api_url.clone(),
            user_ref,
        )?;
        let server_with_channels =
            requests::servers::federated::fetch_with_channels(
                &state.http_client,
                &api_url,
                &token,
                server_id,
            )
            .await
            .map_err(|e| {
                ApiError::Internal(format!(
                    "Failed to fetch server with channels from {host}: {e}"
                ))
            })?;
        Ok(server_with_channels)
    }
}

/// Delete a server by ID.
pub async fn delete(
    state: &AppState,
    session: &Session,
    server_id: Uuid,
    target_host: Option<&str>,
) -> ApiResult<()> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        queries::servers::delete(state, server_id).await?;
        fanout::fanout_update(
            state,
            fanout::resolve_server_targets(state, server_id).await?,
            ClientWsUpdate::ServerDeleted { server_id },
            FederationWsUpdate::ServerDeleted { server_id },
        )
        .await;
        Ok(())
    } else {
        // Delete on remote host using federation
        let host = target_host.unwrap();
        let api_url = get_api_url(host);
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated server deletion"
                    .to_string(),
            )
        })?;
        let token = state.key_manager.issue_federation_jwt_delegated(
            state.config.api_url(),
            api_url.clone(),
            user_ref,
        )?;
        requests::servers::federated::delete(
            &state.http_client,
            &api_url,
            &token,
            server_id,
        )
        .await
        .map_err(|e| {
            ApiError::Internal(format!(
                "Failed to delete server on {host}: {e}"
            ))
        })?;
        Ok(())
    }
}

/// Auth requirements for server operations.
pub mod auth {
    use super::*;
    use crate::auth::Requirement as Req;

    pub fn create() -> Req {
        // TODO: add rate limiting or something
        Req::Always.or_admin().client_only()
    }

    pub fn get_with_channels(server_id: Uuid) -> Req {
        Req::ServerMember(server_id).or_admin().client_only()
    }

    pub fn delete(server_id: Uuid) -> Req {
        Req::ServerAdmin(server_id).or_admin().client_only()
    }

    pub mod federated {
        use super::*;

        pub fn create() -> Req {
            // TODO: see above
            Req::Always.federated_only()
        }

        pub fn get_with_channels(server_id: Uuid) -> Req {
            Req::ServerMember(server_id).federated_only()
        }

        pub fn delete(server_id: Uuid) -> Req {
            Req::ServerAdmin(server_id).federated_only()
        }
    }
}
