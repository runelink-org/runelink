use runelink_types::{
    channel::{TextChannelId, TextChannelRef},
    host::Host,
    message::{Message, MessageId, MessageRef, NewMessage},
    server::ServerRef,
    user::UserRef,
    ws::{FederationWsReply, FederationWsRequest},
};

use super::federation;
use crate::{
    auth::Session,
    config::Locality,
    error::ApiResult,
    queries::{self, messages::MessageLocation},
    state::AppState,
};

/// Create a new message in a channel.
pub async fn create(
    state: &AppState,
    session: &Session,
    text_channel_ref: &TextChannelRef,
    new_message: &NewMessage,
) -> ApiResult<Message> {
    match state.config.locality(&text_channel_ref.host) {
        Locality::Local => {
            state
                .actors
                .create_message(
                    text_channel_ref.id,
                    new_message.clone(),
                    session.authorization(),
                )
                .await
        }
        Locality::Remote(host) => federation::request!(
            state,
            host,
            Some(session),
            FederationWsRequest::MessagesCreate {
                text_channel_id: text_channel_ref.id,
                new_message: new_message.clone(),
            },
            FederationWsReply::MessagesCreate(message) => message
        ),
    }
}

/// Get all messages.
pub async fn get_all(
    state: &AppState,
    session: &Session,
    target_host: Option<&Host>,
) -> ApiResult<Vec<Message>> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            let messages = queries::messages::get_all(
                &state.db_pool,
                state.config.public_host(),
            )
            .await?;
            Ok(messages)
        }
        Locality::Remote(host) => federation::request!(
            state,
            host,
            Some(session),
            FederationWsRequest::MessagesGetAll,
            FederationWsReply::MessagesGetAll(messages) => messages
        ),
    }
}

/// Get messages in a server.
pub async fn get_by_server(
    state: &AppState,
    session: &Session,
    server_ref: &ServerRef,
) -> ApiResult<Vec<Message>> {
    match state.config.locality(&server_ref.host) {
        Locality::Local => {
            let messages = queries::messages::get_by_server(
                &state.db_pool,
                state.config.public_host(),
                server_ref.id,
            )
            .await?;
            Ok(messages)
        }
        Locality::Remote(host) => federation::request!(
            state,
            host,
            Some(session),
            FederationWsRequest::MessagesGetByServer {
                server_id: server_ref.id,
            },
            FederationWsReply::MessagesGetByServer(messages) => messages
        ),
    }
}

/// Get messages in a text channel.
pub async fn get_by_text_channel(
    state: &AppState,
    session: &Session,
    text_channel_ref: &TextChannelRef,
) -> ApiResult<Vec<Message>> {
    match state.config.locality(&text_channel_ref.host) {
        Locality::Local => {
            let messages = queries::messages::get_by_text_channel(
                &state.db_pool,
                state.config.public_host(),
                text_channel_ref.id,
            )
            .await?;
            Ok(messages)
        }
        Locality::Remote(host) => federation::request!(
            state,
            host,
            Some(session),
            FederationWsRequest::MessagesGetByTextChannel {
                text_channel_id: text_channel_ref.id,
            },
            FederationWsReply::MessagesGetByTextChannel(messages) => messages
        ),
    }
}

/// Get a message by its ID.
pub async fn get_by_id(
    state: &AppState,
    session: &Session,
    message_ref: &MessageRef,
) -> ApiResult<Message> {
    match state.config.locality(&message_ref.host) {
        Locality::Local => {
            let message = queries::messages::get_by_id(
                &*state.db_pool,
                state.config.public_host(),
                message_ref.id,
            )
            .await?;
            Ok(message)
        }
        Locality::Remote(host) => federation::request!(
            state,
            host,
            Some(session),
            FederationWsRequest::MessagesGetById {
                message_id: message_ref.id,
            },
            FederationWsReply::MessagesGetById(message) => message
        ),
    }
}

/// Where an edit or delete of a message goes.
#[derive(Debug)]
pub enum MessageTarget<'a> {
    /// A message on this host.
    Local(MessageLocation),
    /// A message on another host, which authorizes the request itself.
    Remote(&'a MessageRef),
}

/// Find where an edit or delete goes. A local message's location serves both
/// its permission check and its routing, so it is looked up once.
pub async fn locate<'a>(
    state: &AppState,
    message_ref: &'a MessageRef,
) -> ApiResult<MessageTarget<'a>> {
    Ok(match state.config.locality(&message_ref.host) {
        Locality::Local => MessageTarget::Local(
            queries::messages::get_location(
                &state.db_pool,
                state.config.public_host(),
                message_ref.id,
            )
            .await?,
        ),
        Locality::Remote(_) => MessageTarget::Remote(message_ref),
    })
}

/// Delete a message by ID.
pub async fn delete(
    state: &AppState,
    session: &Session,
    target: MessageTarget<'_>,
) -> ApiResult<Message> {
    match target {
        MessageTarget::Local(location) => {
            state
                .actors
                .delete_message(
                    location.text_channel_id,
                    location.message_id,
                    session.authorization(),
                )
                .await
        }
        MessageTarget::Remote(message_ref) => federation::request!(
            state,
            &message_ref.host,
            Some(session),
            FederationWsRequest::MessagesDelete {
                message_id: message_ref.id,
            },
            FederationWsReply::MessagesDelete(message) => message
        ),
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
        Ok(Req::ServerMember(state.config.local_ref(server_id)))
    }

    async fn message_member(
        state: &AppState,
        message_id: MessageId,
    ) -> ApiResult<Req> {
        let location = queries::messages::get_location(
            &state.db_pool,
            state.config.public_host(),
            message_id,
        )
        .await?;
        Ok(Req::ServerMember(location.server_ref))
    }

    fn delete_base(location: &MessageLocation) -> Req {
        let admin = Req::ServerAdmin(location.server_ref.clone());
        match &location.author {
            Some(author) => or!(Req::User(author.clone()), admin),
            None => admin,
        }
    }

    fn update_base(location: &MessageLocation) -> Req {
        match &location.author {
            Some(author) => crate::and!(
                Req::User(author.clone()),
                Req::ServerMember(location.server_ref.clone())
            ),
            None => Req::Never,
        }
    }

    pub fn update(target: &MessageTarget) -> Req {
        match target {
            MessageTarget::Local(location) => {
                update_base(location).client_only()
            }
            MessageTarget::Remote(_) => Req::Client,
        }
    }
    /// Members post as themselves only.
    pub async fn create(
        state: &AppState,
        text_channel_ref: &TextChannelRef,
        author: &UserRef,
    ) -> ApiResult<Req> {
        let base = get_by_text_channel(state, text_channel_ref).await?;
        Ok(crate::and!(Req::User(author.clone()), base))
    }

    pub fn get_all() -> Req {
        Req::HostAdmin.client_only()
    }

    pub fn get_by_server(server_ref: &ServerRef) -> Req {
        Req::ServerMember(server_ref.clone())
            .or_admin()
            .client_only()
    }

    pub async fn get_by_text_channel(
        state: &AppState,
        text_channel_ref: &TextChannelRef,
    ) -> ApiResult<Req> {
        if state.config.locality(&text_channel_ref.host).is_remote() {
            return Ok(Req::Client);
        }
        let base = text_channel_member(state, text_channel_ref.id).await?;
        Ok(base.or_admin().client_only())
    }

    pub async fn get_by_id(
        state: &AppState,
        message_ref: &MessageRef,
    ) -> ApiResult<Req> {
        if state.config.locality(&message_ref.host).is_remote() {
            return Ok(Req::Client);
        }
        let base = message_member(state, message_ref.id).await?;
        Ok(base.or_admin().client_only())
    }

    pub fn delete(target: &MessageTarget) -> Req {
        match target {
            MessageTarget::Local(location) => {
                delete_base(location).or_admin().client_only()
            }
            MessageTarget::Remote(_) => Req::Client,
        }
    }

    pub mod federated {
        use super::*;

        pub fn update(location: &MessageLocation) -> Req {
            update_base(location).federated_only()
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

        pub fn get_by_server(server_ref: ServerRef) -> Req {
            Req::ServerMember(server_ref).federated_only()
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

        pub fn delete(location: &MessageLocation) -> Req {
            // TODO: Check if the author is from the same host as the server
            delete_base(location).federated_only()
        }
    }
}

/// Edit a message by ID.
pub async fn update(
    state: &AppState,
    session: &Session,
    message_update: &runelink_types::message::MessageUpdate,
    target: MessageTarget<'_>,
) -> ApiResult<Message> {
    match target {
        MessageTarget::Local(location) => {
            state
                .actors
                .update_message(
                    location.text_channel_id,
                    location.message_id,
                    message_update.clone(),
                    session.authorization(),
                )
                .await
        }
        MessageTarget::Remote(message_ref) => federation::request!(
            state,
            &message_ref.host,
            Some(session),
            FederationWsRequest::MessagesUpdate {
                message_id: message_ref.id,
                message_update: message_update.clone(),
            },
            FederationWsReply::MessagesUpdate(message) => message
        ),
    }
}
