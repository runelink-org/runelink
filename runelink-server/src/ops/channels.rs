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
        let mut tx = state.db_pool.begin().await?;
        queries::channels::lock_tree(&mut tx, server_id).await?;
        if let Some(parent_id) = new_channel.parent_channel_id {
            let parent = queries::channels::get_by_id(&mut *tx, parent_id)
                .await
                .map_err(|error| match error {
                    ApiError::NotFound => ApiError::BadRequest(
                        "Parent channel not found in specified server".into(),
                    ),
                    error => error,
                })?;
            channel_tree::check_parent(server_id, &parent)?;
        }
        let channel =
            queries::channels::insert(&mut tx, server_id, new_channel).await?;
        tx.commit().await?;
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
        let channels = queries::channels::list(&*state.db_pool, None).await?;
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
        queries::channels::list(&*state.db_pool, Some(server_id)).await
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
    channel_id: ServerChannelId,
    target_host: Option<&str>,
) -> ApiResult<ServerChannel> {
    if !state.config.is_remote_host(target_host) {
        // Handle local case
        queries::channels::get_by_id(&*state.db_pool, channel_id).await
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
            FederationWsRequest::ChannelsGetById { channel_id },
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
    channel_id: ServerChannelId,
    target_host: Option<&str>,
) -> ApiResult<()> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let mut tx = state.db_pool.begin().await?;
        let server_id =
            queries::channels::get_server_id(&mut *tx, channel_id).await?;
        queries::channels::lock_tree(&mut tx, server_id).await?;
        if queries::channels::has_children(&mut *tx, server_id, channel_id)
            .await?
        {
            return Err(ApiError::BadRequest(
                "Only empty categories can be deleted".into(),
            ));
        }
        queries::channels::delete(&mut *tx, channel_id).await?;
        tx.commit().await?;
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
            FederationWsRequest::ChannelsDelete { channel_id },
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
    channel_id: ServerChannelId,
    channel_move: &ServerChannelMove,
    target_host: Option<&str>,
) -> ApiResult<Vec<ServerChannel>> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let mut tx = state.db_pool.begin().await?;
        let server_id =
            queries::channels::get_server_id(&mut *tx, channel_id).await?;
        queries::channels::lock_tree(&mut tx, server_id).await?;
        let channels =
            queries::channels::list(&mut *tx, Some(server_id)).await?;
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

    pub async fn update(
        state: &AppState,
        channel_id: ServerChannelId,
        target_host: Option<&str>,
    ) -> ApiResult<Req> {
        delete(state, channel_id, target_host).await
    }
    pub fn create(server_id: ServerId) -> Req {
        Req::ServerAdmin(server_id).or_admin().client_only()
    }

    pub fn get_all() -> Req {
        Req::HostAdmin.client_only()
    }

    pub fn get_by_server(server_id: ServerId) -> Req {
        Req::ServerMember(server_id).or_admin().client_only()
    }

    pub async fn get_by_id(
        state: &AppState,
        channel_id: ServerChannelId,
        target_host: Option<&str>,
    ) -> ApiResult<Req> {
        if state.config.is_remote_host(target_host) {
            return Ok(Req::Client);
        }
        let server_id =
            queries::channels::get_server_id(&*state.db_pool, channel_id)
                .await?;
        Ok(Req::ServerMember(server_id).or_admin().client_only())
    }

    pub async fn delete(
        state: &AppState,
        channel_id: ServerChannelId,
        target_host: Option<&str>,
    ) -> ApiResult<Req> {
        if state.config.is_remote_host(target_host) {
            return Ok(Req::Client);
        }
        let server_id =
            queries::channels::get_server_id(&*state.db_pool, channel_id)
                .await?;
        Ok(Req::ServerAdmin(server_id).or_admin().client_only())
    }

    pub async fn move_channel(
        state: &AppState,
        channel_id: ServerChannelId,
        target_host: Option<&str>,
    ) -> ApiResult<Req> {
        delete(state, channel_id, target_host).await
    }

    pub mod federated {
        use super::*;

        pub async fn update(
            state: &AppState,
            channel_id: ServerChannelId,
        ) -> ApiResult<Req> {
            delete(state, channel_id).await
        }
        pub fn create(server_id: ServerId) -> Req {
            Req::ServerAdmin(server_id).federated_only()
        }

        pub fn get_all() -> Req {
            Req::Never.federated_only()
        }

        pub fn get_by_server(server_id: ServerId) -> Req {
            Req::ServerMember(server_id).federated_only()
        }

        pub async fn get_by_id(
            state: &AppState,
            channel_id: ServerChannelId,
        ) -> ApiResult<Req> {
            let server_id =
                queries::channels::get_server_id(&*state.db_pool, channel_id)
                    .await?;
            Ok(Req::ServerMember(server_id).federated_only())
        }

        pub async fn delete(
            state: &AppState,
            channel_id: ServerChannelId,
        ) -> ApiResult<Req> {
            let server_id =
                queries::channels::get_server_id(&*state.db_pool, channel_id)
                    .await?;
            Ok(Req::ServerAdmin(server_id).federated_only())
        }

        pub async fn move_channel(
            state: &AppState,
            channel_id: ServerChannelId,
        ) -> ApiResult<Req> {
            delete(state, channel_id).await
        }
    }
}

pub async fn update(
    state: &AppState,
    session: &Session,
    channel_id: ServerChannelId,
    channel_update: &runelink_types::channel::ServerChannelUpdate,
    target_host: Option<&str>,
) -> ApiResult<ServerChannel> {
    if state.config.is_remote_host(target_host) {
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::AuthError("User identity required".into())
        })?;
        let reply = federation::request(
            state,
            target_host.unwrap(),
            Some(user_ref),
            FederationWsRequest::ChannelsUpdate {
                channel_id,
                channel_update: channel_update.clone(),
            },
        )
        .await?;
        let FederationWsReply::ChannelsUpdate(resource) = reply else {
            return Err(ApiError::Internal("Unexpected edit reply".into()));
        };
        return Ok(resource);
    }
    let resource =
        queries::channels::update(&state.db_pool, channel_id, channel_update)
            .await?;
    let server_id = resource.server_id;
    fanout::fanout_update(
        state,
        fanout::resolve_server_targets(state, server_id).await?,
        ClientWsUpdate::ChannelUpserted(resource.clone()),
        FederationWsUpdate::ChannelUpserted(resource.clone()),
    )
    .await;
    Ok(resource)
}
