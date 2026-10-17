use runelink_types::{
    channel::{
        NewServerChannel, ServerChannel, ServerChannelId, ServerChannelMove,
    },
    server::ServerId,
    ws::{
        ClientWsUpdate, FederationWsReply, FederationWsRequest,
        FederationWsUpdate,
    },
};

use super::{fanout, federation};

use crate::{
    auth::Session,
    channel_tree,
    error::{ApiError, ApiResult},
    queries,
    state::AppState,
};

/// Create a new channel in a server.
pub async fn create(
    state: &AppState,
    session: &Session,
    server_id: ServerId,
    new_channel: &NewServerChannel,
    target_host: Option<&str>,
) -> ApiResult<ServerChannel> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        if let Some(parent_id) = new_channel.parent_channel_id {
            let parent =
                queries::channels::get_by_id(&state.db_pool, parent_id)
                    .await
                    .map_err(|error| match error {
                        ApiError::NotFound => ApiError::BadRequest(
                            "Parent channel not found in specified server"
                                .into(),
                        ),
                        error => error,
                    })?;
            channel_tree::check_parent(server_id, &parent)?;
        }
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
) -> ApiResult<Vec<ServerChannel>> {
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
    server_id: ServerId,
    target_host: Option<&str>,
) -> ApiResult<Vec<ServerChannel>> {
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
    server_id: ServerId,
    channel_id: ServerChannelId,
    target_host: Option<&str>,
) -> ApiResult<ServerChannel> {
    if !state.config.is_remote_host(target_host) {
        // Handle local case
        let channel =
            queries::channels::get_by_id(&state.db_pool, channel_id).await?;
        if channel.server_id != server_id {
            return Err(ApiError::NotFound);
        }
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
    server_id: ServerId,
    channel_id: ServerChannelId,
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
        if queries::channels::has_children(&state.db_pool, channel_id).await? {
            return Err(ApiError::BadRequest(
                "Only empty categories can be deleted".into(),
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

/// Move a channel, with its subtree, to an index within a parent.
///
/// Returns every channel whose parent or position changed.
pub async fn move_channel(
    state: &AppState,
    session: &Session,
    server_id: ServerId,
    channel_id: ServerChannelId,
    channel_move: &ServerChannelMove,
    target_host: Option<&str>,
) -> ApiResult<Vec<ServerChannel>> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let mut tx = state.db_pool.begin().await?;
        let channels =
            queries::channels::get_by_server_for_update(&mut tx, server_id)
                .await?;
        let planned =
            channel_tree::plan_move(&channels, channel_id, channel_move)?;
        let changed =
            queries::channels::update_placements(&mut tx, &planned).await?;
        tx.commit().await?;
        let targets = fanout::resolve_server_targets(state, server_id).await?;
        for channel in &changed {
            fanout::fanout_update(
                state,
                targets.clone(),
                ClientWsUpdate::ChannelUpserted(channel.clone()),
                FederationWsUpdate::ChannelUpserted(channel.clone()),
            )
            .await;
        }
        Ok(changed)
    } else {
        // Move on remote host using federation
        let host = target_host.unwrap();
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated channel moves"
                    .to_string(),
            )
        })?;
        let reply = federation::request(
            state,
            host,
            Some(user_ref),
            FederationWsRequest::ChannelsMove {
                server_id,
                channel_id,
                channel_move: channel_move.clone(),
            },
        )
        .await?;
        let FederationWsReply::ChannelsMove(channels) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for channels.move"
            )));
        };
        Ok(channels)
    }
}

/// Auth requirements for channel operations.
pub mod auth {
    use super::*;
    use crate::auth::Requirement as Req;

    pub fn create(server_id: ServerId) -> Req {
        Req::ServerAdmin(server_id).or_admin().client_only()
    }

    pub fn get_all() -> Req {
        Req::HostAdmin.client_only()
    }

    pub fn get_by_server(server_id: ServerId) -> Req {
        Req::ServerMember(server_id).or_admin().client_only()
    }

    pub fn get_by_id(server_id: ServerId) -> Req {
        Req::ServerMember(server_id).or_admin().client_only()
    }

    pub fn delete(server_id: ServerId) -> Req {
        Req::ServerAdmin(server_id).or_admin().client_only()
    }

    pub fn move_channel(server_id: ServerId) -> Req {
        Req::ServerAdmin(server_id).or_admin().client_only()
    }

    pub mod federated {
        use super::*;

        pub fn create(server_id: ServerId) -> Req {
            Req::ServerAdmin(server_id).federated_only()
        }

        pub fn get_all() -> Req {
            Req::Never.federated_only()
        }

        pub fn get_by_server(server_id: ServerId) -> Req {
            Req::ServerMember(server_id).federated_only()
        }

        pub fn get_by_id(server_id: ServerId) -> Req {
            Req::ServerMember(server_id).federated_only()
        }

        pub fn delete(server_id: ServerId) -> Req {
            Req::ServerAdmin(server_id).federated_only()
        }

        pub fn move_channel(server_id: ServerId) -> Req {
            Req::ServerAdmin(server_id).federated_only()
        }
    }
}
