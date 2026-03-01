use runelink_types::{
    message::{Message, NewMessage},
    ws::{
        ClientWsUpdate, FederationWsReply, FederationWsRequest,
        FederationWsUpdate,
    },
};
use uuid::Uuid;

use super::federation;
use crate::{
    auth::Session,
    error::{ApiError, ApiResult},
    ops::fanout,
    queries,
    state::AppState,
};

/// Create a new message in a channel.
pub async fn create(
    state: &AppState,
    session: &Session,
    server_id: Uuid,
    channel_id: Uuid,
    new_message: &NewMessage,
    target_host: Option<&str>,
) -> ApiResult<Message> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let channel =
            queries::channels::get_by_id(&state.db_pool, channel_id).await?;
        if channel.server_id != server_id {
            return Err(ApiError::AuthError(
                "Channel not found in specified server".into(),
            ));
        }
        let message =
            queries::messages::insert(&state.db_pool, channel_id, new_message)
                .await?;
        fanout::fanout_update(
            state,
            fanout::resolve_server_targets(state, server_id).await?,
            ClientWsUpdate::MessageUpserted(message.clone()),
            FederationWsUpdate::MessageUpserted {
                server_id,
                message: message.clone(),
            },
        )
        .await;
        Ok(message)
    } else {
        // Create on remote host using federation
        let host = target_host.unwrap();
        let user_ref = session.user_ref.as_ref().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated message creation"
                    .to_string(),
            )
        })?;
        let reply = federation::request(
            state,
            host,
            Some(user_ref.clone()),
            FederationWsRequest::MessagesCreate {
                server_id,
                channel_id,
                new_message: new_message.clone(),
            },
        )
        .await?;
        let FederationWsReply::MessagesCreate(message) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for messages.create"
            )));
        };
        Ok(message)
    }
}

/// Get all messages.
pub async fn get_all(
    state: &AppState,
    session: &Session,
    target_host: Option<&str>,
) -> ApiResult<Vec<Message>> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let messages = queries::messages::get_all(&state.db_pool).await?;
        Ok(messages)
    } else {
        // Fetch from remote host using federation
        let host = target_host.unwrap();
        let user_ref = session.user_ref.as_ref().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated message fetching"
                    .to_string(),
            )
        })?;
        let reply = federation::request(
            state,
            host,
            Some(user_ref.clone()),
            FederationWsRequest::MessagesGetAll,
        )
        .await?;
        let FederationWsReply::MessagesGetAll(messages) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for messages.get_all"
            )));
        };
        Ok(messages)
    }
}

/// Get messages in a server.
pub async fn get_by_server(
    state: &AppState,
    session: &Session,
    server_id: Uuid,
    target_host: Option<&str>,
) -> ApiResult<Vec<Message>> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let messages =
            queries::messages::get_by_server(&state.db_pool, server_id).await?;
        Ok(messages)
    } else {
        // Fetch from remote host using federation
        let host = target_host.unwrap();
        let user_ref = session.user_ref.as_ref().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated message fetching"
                    .to_string(),
            )
        })?;
        let reply = federation::request(
            state,
            host,
            Some(user_ref.clone()),
            FederationWsRequest::MessagesGetByServer { server_id },
        )
        .await?;
        let FederationWsReply::MessagesGetByServer(messages) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for messages.get_by_server"
            )));
        };
        Ok(messages)
    }
}

/// Get messages in a channel.
pub async fn get_by_channel(
    state: &AppState,
    session: &Session,
    server_id: Uuid,
    channel_id: Uuid,
    target_host: Option<&str>,
) -> ApiResult<Vec<Message>> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let messages =
            queries::messages::get_by_channel(&state.db_pool, channel_id)
                .await?;
        Ok(messages)
    } else {
        // Fetch from remote host using federation
        let host = target_host.unwrap();
        let user_ref = session.user_ref.as_ref().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated message fetching"
                    .to_string(),
            )
        })?;
        let reply = federation::request(
            state,
            host,
            Some(user_ref.clone()),
            FederationWsRequest::MessagesGetByChannel {
                server_id,
                channel_id,
            },
        )
        .await?;
        let FederationWsReply::MessagesGetByChannel(messages) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for messages.get_by_channel"
            )));
        };
        Ok(messages)
    }
}

/// Get a message by its ID.
pub async fn get_by_id(
    state: &AppState,
    session: &Session,
    server_id: Uuid,
    channel_id: Uuid,
    message_id: Uuid,
    target_host: Option<&str>,
) -> ApiResult<Message> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let message =
            queries::messages::get_by_id(&state.db_pool, message_id).await?;
        if message.channel_id != channel_id {
            return Err(ApiError::AuthError(
                "Message not found in specified channel".into(),
            ));
        }
        let channel =
            queries::channels::get_by_id(&state.db_pool, channel_id).await?;
        if channel.server_id != server_id {
            return Err(ApiError::AuthError(
                "Message not found in specified server".into(),
            ));
        }
        Ok(message)
    } else {
        // Fetch from remote host using federation
        let host = target_host.unwrap();
        let user_ref = session.user_ref.as_ref().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated message fetching"
                    .to_string(),
            )
        })?;
        let reply = federation::request(
            state,
            host,
            Some(user_ref.clone()),
            FederationWsRequest::MessagesGetById {
                server_id,
                channel_id,
                message_id,
            },
        )
        .await?;
        let FederationWsReply::MessagesGetById(message) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for messages.get_by_id"
            )));
        };
        Ok(message)
    }
}

/// Delete a message by ID.
pub async fn delete(
    state: &AppState,
    session: &Session,
    server_id: Uuid,
    channel_id: Uuid,
    message_id: Uuid,
    target_host: Option<&str>,
) -> ApiResult<()> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        // Verify the message belongs to the channel and server
        // TODO: This should be done with one database query
        let message =
            queries::messages::get_by_id(&state.db_pool, message_id).await?;
        if message.channel_id != channel_id {
            return Err(ApiError::AuthError(
                "Message not found in specified channel".into(),
            ));
        }
        let channel =
            queries::channels::get_by_id(&state.db_pool, channel_id).await?;
        if channel.server_id != server_id {
            return Err(ApiError::AuthError(
                "Message not found in specified server".into(),
            ));
        }
        queries::messages::delete(&state.db_pool, message_id).await?;
        fanout::fanout_update(
            state,
            fanout::resolve_server_targets(state, server_id).await?,
            ClientWsUpdate::MessageDeleted {
                server_id,
                channel_id,
                message_id,
            },
            FederationWsUpdate::MessageDeleted {
                server_id,
                channel_id,
                message_id,
            },
        )
        .await;
        Ok(())
    } else {
        // Delete on remote host using federation
        let host = target_host.unwrap();
        let user_ref = session.user_ref.as_ref().ok_or_else(|| {
            ApiError::Internal(
                "User reference required for federated message deletion"
                    .to_string(),
            )
        })?;
        let reply = federation::request(
            state,
            host,
            Some(user_ref.clone()),
            FederationWsRequest::MessagesDelete {
                server_id,
                channel_id,
                message_id,
            },
        )
        .await?;
        let FederationWsReply::MessagesDelete = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for messages.delete"
            )));
        };
        Ok(())
    }
}

/// Auth requirements for message operations.
pub mod auth {
    use super::*;
    use crate::auth::Requirement as Req;
    use crate::or;

    pub fn create(server_id: Uuid) -> Req {
        Req::ServerMember(server_id).or_admin().client_only()
    }

    pub fn get_all() -> Req {
        Req::HostAdmin.client_only()
    }

    pub fn get_by_server(server_id: Uuid) -> Req {
        Req::ServerMember(server_id).or_admin().client_only()
    }

    pub fn get_by_channel(server_id: Uuid) -> Req {
        Req::ServerMember(server_id).or_admin().client_only()
    }

    pub fn get_by_id(server_id: Uuid) -> Req {
        Req::ServerMember(server_id).or_admin().client_only()
    }

    async fn delete_base(
        state: &AppState,
        server_id: Uuid,
        message_id: Uuid,
    ) -> ApiResult<Req> {
        let message =
            queries::messages::get_by_id(&state.db_pool, message_id).await?;
        if let Some(author) = message.author {
            Ok(or!(Req::User(author.into()), Req::ServerAdmin(server_id)))
        } else {
            Ok(Req::ServerAdmin(server_id))
        }
    }

    pub async fn delete(
        state: &AppState,
        server_id: Uuid,
        message_id: Uuid,
    ) -> ApiResult<Req> {
        let base = delete_base(state, server_id, message_id).await?;
        Ok(base.or_admin().client_only())
    }

    pub mod federated {
        use super::*;

        pub fn create(server_id: Uuid) -> Req {
            Req::ServerMember(server_id).federated_only()
        }

        pub fn get_all() -> Req {
            Req::Never.federated_only()
        }

        pub fn get_by_server(server_id: Uuid) -> Req {
            Req::ServerMember(server_id).federated_only()
        }

        pub fn get_by_channel(server_id: Uuid) -> Req {
            Req::ServerMember(server_id).federated_only()
        }

        pub fn get_by_id(server_id: Uuid) -> Req {
            Req::ServerMember(server_id).federated_only()
        }

        pub async fn delete(
            state: &AppState,
            server_id: Uuid,
            message_id: Uuid,
        ) -> ApiResult<Req> {
            // TODO: Check if the author is from the same host as the server
            let base = delete_base(state, server_id, message_id).await?;
            Ok(base.federated_only())
        }
    }
}
