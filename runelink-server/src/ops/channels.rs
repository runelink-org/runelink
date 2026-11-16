use runelink_types::{
    channel::{
        NewServerChannel, ServerChannel, ServerChannelId, ServerChannelMove,
    },
    server::ServerId,
    ws::{FederationWsReply, FederationWsRequest},
};

use super::federation;

use crate::{
    auth::Session,
    config::Locality,
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
    match state.config.target_locality(target_host) {
        Locality::Local => {
            state
                .actors
                .create_channel(
                    server_id,
                    new_channel.clone(),
                    session.authorization(),
                )
                .await
        }
        Locality::Remote(host) => {
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
}

/// Get all channels.
pub async fn get_all(
    state: &AppState,
    session: &Session,
    target_host: Option<&str>,
) -> ApiResult<Vec<ServerChannel>> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            let channels =
                queries::channels::list(&*state.db_pool, None).await?;
            Ok(channels)
        }
        Locality::Remote(host) => {
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
}

/// Get channels in a server.
pub async fn get_by_server(
    state: &AppState,
    session: &Session,
    server_id: ServerId,
    target_host: Option<&str>,
) -> ApiResult<Vec<ServerChannel>> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            queries::channels::list(&*state.db_pool, Some(server_id)).await
        }
        Locality::Remote(host) => {
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
}

/// Get a channel by its ID.
pub async fn get_by_id(
    state: &AppState,
    session: &Session,
    channel_id: ServerChannelId,
    target_host: Option<&str>,
) -> ApiResult<ServerChannel> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            queries::channels::get_by_id(&*state.db_pool, channel_id).await
        }
        Locality::Remote(host) => {
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
}

/// Delete a channel by ID.
pub async fn delete(
    state: &AppState,
    session: &Session,
    channel_id: ServerChannelId,
    target_host: Option<&str>,
) -> ApiResult<()> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            let server_id =
                queries::channels::get_server_id(&*state.db_pool, channel_id)
                    .await?;
            state
                .actors
                .delete_channel(server_id, channel_id, session.authorization())
                .await
        }
        Locality::Remote(host) => {
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
    match state.config.target_locality(target_host) {
        Locality::Local => {
            let server_id =
                queries::channels::get_server_id(&*state.db_pool, channel_id)
                    .await?;
            state
                .actors
                .move_channel(
                    server_id,
                    channel_id,
                    channel_move.clone(),
                    session.authorization(),
                )
                .await
        }
        Locality::Remote(host) => {
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
        if state.config.target_locality(target_host).is_remote() {
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
        if state.config.target_locality(target_host).is_remote() {
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
    if let Locality::Remote(host) = state.config.target_locality(target_host) {
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::AuthError("User identity required".into())
        })?;
        let reply = federation::request(
            state,
            host,
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
    let server_id =
        queries::channels::get_server_id(&*state.db_pool, channel_id).await?;
    state
        .actors
        .update_channel(
            server_id,
            channel_id,
            channel_update.clone(),
            session.authorization(),
        )
        .await
}
