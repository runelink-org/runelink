use runelink_types::{
    channel::TextChannelId,
    message::{Message, MessageId, NewMessage},
    server::ServerId,
    user::UserRef,
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

/// Create a new message in a channel.
pub async fn create(
    state: &AppState,
    session: &Session,
    text_channel_id: TextChannelId,
    new_message: &NewMessage,
    target_host: Option<&str>,
) -> ApiResult<Message> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            state
                .actors
                .create_message(
                    text_channel_id,
                    new_message.clone(),
                    session.authorization(),
                )
                .await
        }
        Locality::Remote(host) => {
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
}

/// Get all messages.
pub async fn get_all(
    state: &AppState,
    session: &Session,
    target_host: Option<&str>,
) -> ApiResult<Vec<Message>> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            let messages = queries::messages::get_all(&state.db_pool).await?;
            Ok(messages)
        }
        Locality::Remote(host) => {
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
}

/// Get messages in a server.
pub async fn get_by_server(
    state: &AppState,
    session: &Session,
    server_id: ServerId,
    target_host: Option<&str>,
) -> ApiResult<Vec<Message>> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            let messages =
                queries::messages::get_by_server(&state.db_pool, server_id)
                    .await?;
            Ok(messages)
        }
        Locality::Remote(host) => {
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
}

/// Get messages in a text channel.
pub async fn get_by_text_channel(
    state: &AppState,
    session: &Session,
    text_channel_id: TextChannelId,
    target_host: Option<&str>,
) -> ApiResult<Vec<Message>> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            let messages = queries::messages::get_by_text_channel(
                &state.db_pool,
                text_channel_id,
            )
            .await?;
            Ok(messages)
        }
        Locality::Remote(host) => {
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
                FederationWsRequest::MessagesGetByTextChannel {
                    text_channel_id,
                },
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
}

/// Get a message by its ID.
pub async fn get_by_id(
    state: &AppState,
    session: &Session,
    message_id: MessageId,
    target_host: Option<&str>,
) -> ApiResult<Message> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            let message =
                queries::messages::get_by_id(&*state.db_pool, message_id)
                    .await?;
            Ok(message)
        }
        Locality::Remote(host) => {
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
}

/// Delete a message by ID.
pub async fn delete(
    state: &AppState,
    session: &Session,
    message_id: MessageId,
    target_host: Option<&str>,
) -> ApiResult<Message> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            let text_channel_id = queries::messages::get_text_channel_id(
                &state.db_pool,
                message_id,
            )
            .await?;
            state
                .actors
                .delete_message(
                    text_channel_id,
                    message_id,
                    session.authorization(),
                )
                .await
        }
        Locality::Remote(host) => {
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
            queries::messages::get_by_id(&*state.db_pool, message_id).await?;
        text_channel_member(state, message.text_channel_id).await
    }

    async fn delete_base(
        state: &AppState,
        message_id: MessageId,
    ) -> ApiResult<Req> {
        let message =
            queries::messages::get_by_id(&*state.db_pool, message_id).await?;
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
            queries::messages::get_by_id(&*state.db_pool, message_id).await?;
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
        if state.config.target_locality(target_host).is_remote() {
            return Ok(Req::Client);
        }
        Ok(update_base(state, message_id).await?.client_only())
    }
    /// Members post as themselves only.
    pub async fn create(
        state: &AppState,
        text_channel_id: TextChannelId,
        author: &UserRef,
        target_host: Option<&str>,
    ) -> ApiResult<Req> {
        let base =
            get_by_text_channel(state, text_channel_id, target_host).await?;
        Ok(crate::and!(Req::User(author.clone()), base))
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
        if state.config.target_locality(target_host).is_remote() {
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
        if state.config.target_locality(target_host).is_remote() {
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
        if state.config.target_locality(target_host).is_remote() {
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
        /// Members post as themselves only.
        pub async fn create(
            state: &AppState,
            text_channel_id: TextChannelId,
            author: &UserRef,
        ) -> ApiResult<Req> {
            let base = get_by_text_channel(state, text_channel_id).await?;
            Ok(crate::and!(Req::User(author.clone()), base))
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
    if let Locality::Remote(host) = state.config.target_locality(target_host) {
        let user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::AuthError("User identity required".into())
        })?;
        let reply = federation::request(
            state,
            host,
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
    let text_channel_id =
        queries::messages::get_text_channel_id(&state.db_pool, message_id)
            .await?;
    state
        .actors
        .update_message(
            text_channel_id,
            message_id,
            message_update.clone(),
            session.authorization(),
        )
        .await
}
