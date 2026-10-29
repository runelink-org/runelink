use runelink_types::{
    channel::TextChannelId,
    message::{Message, MessageId, NewMessage},
    server::ServerId,
    ws::{
        ClientWsUpdate, FederationWsReply, FederationWsRequest,
        FederationWsUpdate,
    },
};

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
    text_channel_id: TextChannelId,
    new_message: &NewMessage,
    target_host: Option<&str>,
) -> ApiResult<Message> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let server_id = queries::channels::get_server_id_by_text_channel(
            &state.db_pool,
            text_channel_id,
        )
        .await?;
        if let Some(reply_to) = new_message.reply_to {
            // Deleted targets remain valid. Channel restrictions belong here,
            // rather than in the schema, to allow broader scopes in the future.
            let target = queries::messages::get_by_id(&state.db_pool, reply_to)
                .await
                .map_err(|error| match error {
                    ApiError::NotFound => ApiError::BadRequest(
                        "Reply target not found in this text channel".into(),
                    ),
                    error => error,
                })?;
            if target.text_channel_id != text_channel_id {
                return Err(ApiError::BadRequest(
                    "Reply target not found in this text channel".into(),
                ));
            }
        }
        let message = queries::messages::insert(
            &state.db_pool,
            text_channel_id,
            new_message,
        )
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
                text_channel_id,
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
    server_id: ServerId,
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

/// Get messages in a text channel.
pub async fn get_by_text_channel(
    state: &AppState,
    session: &Session,
    text_channel_id: TextChannelId,
    target_host: Option<&str>,
) -> ApiResult<Vec<Message>> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let messages = queries::messages::get_by_text_channel(
            &state.db_pool,
            text_channel_id,
        )
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
            FederationWsRequest::MessagesGetByTextChannel { text_channel_id },
        )
        .await?;
        let FederationWsReply::MessagesGetByTextChannel(messages) = reply
        else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for messages.get_by_text_channel"
            )));
        };
        Ok(messages)
    }
}

/// Get a message by its ID.
pub async fn get_by_id(
    state: &AppState,
    session: &Session,
    message_id: MessageId,
    target_host: Option<&str>,
) -> ApiResult<Message> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let message =
            queries::messages::get_by_id(&state.db_pool, message_id).await?;
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
            FederationWsRequest::MessagesGetById { message_id },
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
    message_id: MessageId,
    target_host: Option<&str>,
) -> ApiResult<Message> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let message =
            queries::messages::get_by_id(&state.db_pool, message_id).await?;
        let text_channel_id = message.text_channel_id;
        let server_id = queries::channels::get_server_id_by_text_channel(
            &state.db_pool,
            text_channel_id,
        )
        .await?;
        let message =
            queries::messages::delete(&state.db_pool, message_id).await?;
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
            FederationWsRequest::MessagesDelete { message_id },
        )
        .await?;
        let FederationWsReply::MessagesDelete(message) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for messages.delete"
            )));
        };
        Ok(message)
    }
}

/// Auth requirements for message operations.
///
/// Requests for another host only require client auth here, since the
/// owning host authorizes them.
pub mod auth {
    use super::*;
    use crate::auth::Requirement as Req;
    use crate::or;

    async fn text_channel_member(
        state: &AppState,
        text_channel_id: TextChannelId,
    ) -> ApiResult<Req> {
        let server_id = queries::channels::get_server_id_by_text_channel(
            &state.db_pool,
            text_channel_id,
        )
        .await?;
        Ok(Req::ServerMember(server_id))
    }

    async fn message_member(
        state: &AppState,
        message_id: MessageId,
    ) -> ApiResult<Req> {
        let message =
            queries::messages::get_by_id(&state.db_pool, message_id).await?;
        text_channel_member(state, message.text_channel_id).await
    }

    async fn delete_base(
        state: &AppState,
        message_id: MessageId,
    ) -> ApiResult<Req> {
        let message =
            queries::messages::get_by_id(&state.db_pool, message_id).await?;
        let server_id = queries::channels::get_server_id_by_text_channel(
            &state.db_pool,
            message.text_channel_id,
        )
        .await?;
        if let Some(author) = message.author {
            Ok(or!(Req::User(author.into()), Req::ServerAdmin(server_id)))
        } else {
            Ok(Req::ServerAdmin(server_id))
        }
    }

    async fn update_base(
        state: &AppState,
        message_id: MessageId,
    ) -> ApiResult<Req> {
        let message =
            queries::messages::get_by_id(&state.db_pool, message_id).await?;
        let Some(author) = message.author else {
            return Ok(Req::Never);
        };
        let membership = message_member(state, message_id).await?;
        Ok(crate::and!(Req::User(author.into()), membership))
    }
    pub async fn update(
        state: &AppState,
        message_id: MessageId,
        target_host: Option<&str>,
    ) -> ApiResult<Req> {
        if state.config.is_remote_host(target_host) {
            return Ok(Req::Client);
        }
        Ok(update_base(state, message_id).await?.client_only())
    }
    pub async fn create(
        state: &AppState,
        text_channel_id: TextChannelId,
        target_host: Option<&str>,
    ) -> ApiResult<Req> {
        get_by_text_channel(state, text_channel_id, target_host).await
    }

    pub fn get_all() -> Req {
        Req::HostAdmin.client_only()
    }

    pub fn get_by_server(server_id: ServerId) -> Req {
        Req::ServerMember(server_id).or_admin().client_only()
    }

    pub async fn get_by_text_channel(
        state: &AppState,
        text_channel_id: TextChannelId,
        target_host: Option<&str>,
    ) -> ApiResult<Req> {
        if state.config.is_remote_host(target_host) {
            return Ok(Req::Client);
        }
        let base = text_channel_member(state, text_channel_id).await?;
        Ok(base.or_admin().client_only())
    }

    pub async fn get_by_id(
        state: &AppState,
        message_id: MessageId,
        target_host: Option<&str>,
    ) -> ApiResult<Req> {
        if state.config.is_remote_host(target_host) {
            return Ok(Req::Client);
        }
        let base = message_member(state, message_id).await?;
        Ok(base.or_admin().client_only())
    }

    pub async fn delete(
        state: &AppState,
        message_id: MessageId,
        target_host: Option<&str>,
    ) -> ApiResult<Req> {
        if state.config.is_remote_host(target_host) {
            return Ok(Req::Client);
        }
        let base = delete_base(state, message_id).await?;
        Ok(base.or_admin().client_only())
    }

    pub mod federated {
        use super::*;

        pub async fn update(
            state: &AppState,
            message_id: MessageId,
        ) -> ApiResult<Req> {
            Ok(update_base(state, message_id).await?.federated_only())
        }
        pub async fn create(
            state: &AppState,
            text_channel_id: TextChannelId,
        ) -> ApiResult<Req> {
            get_by_text_channel(state, text_channel_id).await
        }

        pub fn get_all() -> Req {
            Req::Never.federated_only()
        }

        pub fn get_by_server(server_id: ServerId) -> Req {
            Req::ServerMember(server_id).federated_only()
        }

        pub async fn get_by_text_channel(
            state: &AppState,
            text_channel_id: TextChannelId,
        ) -> ApiResult<Req> {
            let base = text_channel_member(state, text_channel_id).await?;
            Ok(base.federated_only())
        }

        pub async fn get_by_id(
            state: &AppState,
            message_id: MessageId,
        ) -> ApiResult<Req> {
            let base = message_member(state, message_id).await?;
            Ok(base.federated_only())
        }

        pub async fn delete(
            state: &AppState,
            message_id: MessageId,
        ) -> ApiResult<Req> {
            // TODO: Check if the author is from the same host as the server
            let base = delete_base(state, message_id).await?;
            Ok(base.federated_only())
        }
    }
}

pub async fn update(
    state: &AppState,
    session: &Session,
    message_id: MessageId,
    message_update: &runelink_types::message::MessageUpdate,
    target_host: Option<&str>,
) -> ApiResult<Message> {
    if state.config.is_remote_host(target_host) {
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::AuthError("User identity required".into())
        })?;
        let reply = federation::request(
            state,
            target_host.unwrap(),
            Some(user_ref),
            FederationWsRequest::MessagesUpdate {
                message_id,
                message_update: message_update.clone(),
            },
        )
        .await?;
        let FederationWsReply::MessagesUpdate(resource) = reply else {
            return Err(ApiError::Internal("Unexpected edit reply".into()));
        };
        return Ok(resource);
    }
    let resource =
        queries::messages::update(&state.db_pool, message_id, message_update)
            .await?;
    let server_id = queries::channels::get_server_id_by_text_channel(
        &state.db_pool,
        resource.text_channel_id,
    )
    .await?;
    fanout::fanout_update(
        state,
        fanout::resolve_server_targets(state, server_id).await?,
        ClientWsUpdate::MessageUpserted(resource.clone()),
        FederationWsUpdate::MessageUpserted {
            server_id,
            message: resource.clone(),
        },
    )
    .await;
    Ok(resource)
}
