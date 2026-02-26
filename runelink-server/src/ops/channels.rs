use runelink_types::{
    channel::{Channel, NewChannel},
    ws::{
        ClientWsUpdate, FederationWsReply, FederationWsRequest,
        FederationWsUpdate,
    },
};
use uuid::Uuid;

use super::{fanout, federation};

use crate::{
    auth::Session,
    error::{ApiError, ApiResult},
    queries,
    state::AppState,
};

/// Create a new channel in a server.
pub async fn create(
    state: &AppState,
    session: &Session,
    server_id: Uuid,
    new_channel: &NewChannel,
    target_host: Option<&str>,
) -> ApiResult<Channel> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let channel =
            queries::channels::insert(&state.db_pool, server_id, new_channel)
                .await?;
        fanout::fanout_update(
            state,
            fanout::resolve_server_targets(state, server_id).await?,
            ClientWsUpdate::ChannelUpserted(channel.clone()),
            FederationWsUpdate::ChannelUpserted(channel.clone()),
        )
        .await;
        Ok(channel)
    } else {
        // Create on remote host using federation
        let host = target_host.unwrap();
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated channel creation"
                    .to_string(),
            )
        })?;
        let reply = federation::request(
            state,
            host,
            Some(user_ref),
            FederationWsRequest::ChannelsCreate {
                server_id,
                new_channel: new_channel.clone(),
            },
        )
        .await?;
        let FederationWsReply::ChannelsCreate(channel) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for channels.create"
            )));
        };
        Ok(channel)
    }
}

/// Get all channels.
pub async fn get_all(
    state: &AppState,
    session: &Session,
    target_host: Option<&str>,
) -> ApiResult<Vec<Channel>> {
    if !state.config.is_remote_host(target_host) {
        // Handle local case
        let channels = queries::channels::get_all(&state.db_pool).await?;
        Ok(channels)
    } else {
        // Fetch from remote host using federation
        let host = target_host.unwrap();
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated channel fetching"
                    .to_string(),
            )
        })?;
        let reply = federation::request(
            state,
            host,
            Some(user_ref),
            FederationWsRequest::ChannelsGetAll,
        )
        .await?;
        let FederationWsReply::ChannelsGetAll(channels) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for channels.get_all"
            )));
        };
        Ok(channels)
    }
}

/// Get channels in a server.
pub async fn get_by_server(
    state: &AppState,
    session: &Session,
    server_id: Uuid,
    target_host: Option<&str>,
) -> ApiResult<Vec<Channel>> {
    if !state.config.is_remote_host(target_host) {
        // Handle local case
        queries::channels::get_by_server(&state.db_pool, server_id).await
    } else {
        // Fetch from remote host using federation
        let host = target_host.unwrap();
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated channel fetching"
                    .to_string(),
            )
        })?;
        let reply = federation::request(
            state,
            host,
            Some(user_ref),
            FederationWsRequest::ChannelsGetByServer { server_id },
        )
        .await?;
        let FederationWsReply::ChannelsGetByServer(channels) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for channels.get_by_server"
            )));
        };
        Ok(channels)
    }
}

/// Get a channel by its ID.
pub async fn get_by_id(
    state: &AppState,
    session: &Session,
    server_id: Uuid,
    channel_id: Uuid,
    target_host: Option<&str>,
) -> ApiResult<Channel> {
    if !state.config.is_remote_host(target_host) {
        // Handle local case
        let channel =
            queries::channels::get_by_id(&state.db_pool, channel_id).await?;
        Ok(channel)
    } else {
        // Fetch from remote host using federation
        let host = target_host.unwrap();
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated channel fetching"
                    .to_string(),
            )
        })?;
        let reply = federation::request(
            state,
            host,
            Some(user_ref),
            FederationWsRequest::ChannelsGetById {
                server_id,
                channel_id,
            },
        )
        .await?;
        let FederationWsReply::ChannelsGetById(channel) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for channels.get_by_id"
            )));
        };
        Ok(channel)
    }
}

/// Delete a channel by ID.
pub async fn delete(
    state: &AppState,
    session: &Session,
    server_id: Uuid,
    channel_id: Uuid,
    target_host: Option<&str>,
) -> ApiResult<()> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        // Verify the channel belongs to the server
        let channel =
            queries::channels::get_by_id(&state.db_pool, channel_id).await?;
        if channel.server_id != server_id {
            return Err(ApiError::AuthError(
                "Channel not found in specified server".into(),
            ));
        }
        queries::channels::delete(&state.db_pool, channel_id).await?;
        fanout::fanout_update(
            state,
            fanout::resolve_server_targets(state, server_id).await?,
            ClientWsUpdate::ChannelDeleted {
                server_id,
                channel_id,
            },
            FederationWsUpdate::ChannelDeleted {
                server_id,
                channel_id,
            },
        )
        .await;
        Ok(())
    } else {
        // Delete on remote host using federation
        let host = target_host.unwrap();
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated channel deletion"
                    .to_string(),
            )
        })?;
        let reply = federation::request(
            state,
            host,
            Some(user_ref),
            FederationWsRequest::ChannelsDelete {
                server_id,
                channel_id,
            },
        )
        .await?;
        let FederationWsReply::ChannelsDelete = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for channels.delete"
            )));
        };
        Ok(())
    }
}

/// Auth requirements for channel operations.
pub mod auth {
    use super::*;
    use crate::auth::Requirement as Req;

    pub fn create(server_id: Uuid) -> Req {
        Req::ServerAdmin(server_id).or_admin().client_only()
    }

    pub fn get_all() -> Req {
        Req::HostAdmin.client_only()
    }

    pub fn get_by_server(server_id: Uuid) -> Req {
        Req::ServerMember(server_id).or_admin().client_only()
    }

    pub fn get_by_id(server_id: Uuid) -> Req {
        Req::ServerMember(server_id).or_admin().client_only()
    }

    pub fn delete(server_id: Uuid) -> Req {
        Req::ServerAdmin(server_id).or_admin().client_only()
    }

    pub mod federated {
        use super::*;

        pub fn create(server_id: Uuid) -> Req {
            Req::ServerAdmin(server_id).federated_only()
        }

        pub fn get_all() -> Req {
            Req::Never.federated_only()
        }

        pub fn get_by_server(server_id: Uuid) -> Req {
            Req::ServerMember(server_id).federated_only()
        }

        pub fn get_by_id(server_id: Uuid) -> Req {
            Req::ServerMember(server_id).federated_only()
        }

        pub fn delete(server_id: Uuid) -> Req {
            Req::ServerAdmin(server_id).federated_only()
        }
    }
}
