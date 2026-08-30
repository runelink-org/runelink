use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    auth::{
        AuthTokenPasswordRequest, AuthTokenRefreshRequest, JwksResponse,
        OidcDiscoveryDocument, SignupRequest, TokenResponse,
    },
    channel::{Channel, ChannelId, NewChannel},
    message::{Message, MessageId, NewMessage},
    server::{
        FullServerMembership, NewServer, NewServerMembership,
        NewServerMembershipFull, Server, ServerId, ServerMember,
        ServerMembership, ServerWithChannels,
    },
    user::{NewUser, User, UserRef},
};

use crate::capability::{Capability, CapabilityHello, CapabilityWelcome};

pub use crate::ids::{EventId, RequestId};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WsError {
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthTokenAccessRequest {
    pub access_token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ClientWsConnectionState {
    Unauthenticated,
    Authenticated { user_ref: UserRef },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum FederationWsConnectionState {
    Unauthenticated,
    Authenticated { host: String },
}

/// Request enum for websocket client traffic. Variants map to existing API endpoints.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ClientWsRequest {
    Ping,
    OidcDiscovery,
    OidcJwks,
    ConnectionState,
    AuthSignup(SignupRequest),
    AuthTokenPassword(AuthTokenPasswordRequest),
    AuthTokenRefresh(AuthTokenRefreshRequest),
    AuthTokenAccess(AuthTokenAccessRequest),
    AuthUserinfo,
    AuthRegisterClient,
    UsersCreate(NewUser),
    UsersGetAll {
        target_host: Option<String>,
    },
    UsersGetByRef {
        user_ref: UserRef,
        target_host: Option<String>,
    },
    UsersGetAssociatedHosts {
        user_ref: UserRef,
        target_host: Option<String>,
    },
    UsersDelete {
        user_ref: UserRef,
    },
    MembershipsGetByUser {
        user_ref: UserRef,
    },
    MembershipsGetMembersByServer {
        server_id: ServerId,
        target_host: Option<String>,
    },
    MembershipsGetByUserAndServer {
        server_id: ServerId,
        user_ref: UserRef,
        target_host: Option<String>,
    },
    MembershipsUpsert {
        server_id: ServerId,
        new_membership: NewServerMembership,
    },
    MembershipsDelete {
        server_id: ServerId,
        user_ref: UserRef,
        target_host: Option<String>,
    },
    ServersCreate {
        new_server: NewServer,
        target_host: Option<String>,
    },
    ServersGetAll {
        target_host: Option<String>,
    },
    ServersGetById {
        server_id: ServerId,
        target_host: Option<String>,
    },
    ServersGetWithChannels {
        server_id: ServerId,
        target_host: Option<String>,
    },
    ServersDelete {
        server_id: ServerId,
        target_host: Option<String>,
    },
    ChannelsCreate {
        server_id: ServerId,
        new_channel: NewChannel,
        target_host: Option<String>,
    },
    ChannelsGetAll {
        target_host: Option<String>,
    },
    ChannelsGetByServer {
        server_id: ServerId,
        target_host: Option<String>,
    },
    ChannelsGetById {
        server_id: ServerId,
        channel_id: ChannelId,
        target_host: Option<String>,
    },
    ChannelsDelete {
        server_id: ServerId,
        channel_id: ChannelId,
        target_host: Option<String>,
    },
    MessagesCreate {
        server_id: ServerId,
        channel_id: ChannelId,
        new_message: NewMessage,
        target_host: Option<String>,
    },
    MessagesGetAll {
        target_host: Option<String>,
    },
    MessagesGetByServer {
        server_id: ServerId,
        target_host: Option<String>,
    },
    MessagesGetByChannel {
        server_id: ServerId,
        channel_id: ChannelId,
        target_host: Option<String>,
    },
    MessagesGetById {
        server_id: ServerId,
        channel_id: ChannelId,
        message_id: MessageId,
        target_host: Option<String>,
    },
    MessagesDelete {
        server_id: ServerId,
        channel_id: ChannelId,
        message_id: MessageId,
        target_host: Option<String>,
    },
}

impl ClientWsRequest {
    pub fn capability(&self) -> Capability {
        match self {
            Self::Ping => Capability::Ping,
            Self::OidcDiscovery | Self::OidcJwks => Capability::AuthDiscovery,
            Self::ConnectionState => Capability::ConnectionState,
            Self::AuthSignup(_) => Capability::AuthSignup,
            Self::AuthTokenPassword(_) | Self::AuthTokenRefresh(_) => {
                Capability::AuthToken
            }
            Self::AuthTokenAccess(_) => Capability::AuthAuthenticate,
            Self::AuthUserinfo => Capability::AuthUserinfo,
            Self::AuthRegisterClient => Capability::AuthRegisterClient,
            Self::UsersCreate(_) => Capability::UsersCreate,
            Self::UsersGetAll { .. }
            | Self::UsersGetByRef { .. }
            | Self::UsersGetAssociatedHosts { .. } => Capability::UsersRead,
            Self::UsersDelete { .. } => Capability::UsersDelete,
            Self::MembershipsGetByUser { .. }
            | Self::MembershipsGetMembersByServer { .. }
            | Self::MembershipsGetByUserAndServer { .. } => {
                Capability::MembershipsRead
            }
            Self::MembershipsUpsert { .. } | Self::MembershipsDelete { .. } => {
                Capability::MembershipsWrite
            }
            Self::ServersCreate { .. } => Capability::ServersCreate,
            Self::ServersGetAll { .. }
            | Self::ServersGetById { .. }
            | Self::ServersGetWithChannels { .. } => Capability::ServersRead,
            Self::ServersDelete { .. } => Capability::ServersDelete,
            Self::ChannelsCreate { .. } => Capability::ChannelsCreate,
            Self::ChannelsGetAll { .. }
            | Self::ChannelsGetByServer { .. }
            | Self::ChannelsGetById { .. } => Capability::ChannelsRead,
            Self::ChannelsDelete { .. } => Capability::ChannelsDelete,
            Self::MessagesCreate { .. } => Capability::MessagesCreate,
            Self::MessagesGetAll { .. }
            | Self::MessagesGetByServer { .. }
            | Self::MessagesGetByChannel { .. }
            | Self::MessagesGetById { .. } => Capability::MessagesRead,
            Self::MessagesDelete { .. } => Capability::MessagesDelete,
        }
    }
}

/// Reply enum for websocket client traffic. Variants map 1:1 with request outcomes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ClientWsReply {
    Pong,
    OidcDiscovery(OidcDiscoveryDocument),
    OidcJwks(JwksResponse),
    ConnectionState(ClientWsConnectionState),
    AuthSignup(User),
    AuthToken(TokenResponse),
    AuthTokenAccess(ClientWsConnectionState),
    UsersCreate(User),
    UsersGetAll(Vec<User>),
    UsersGetByRef(User),
    UsersGetAssociatedHosts(Vec<String>),
    UsersDelete,
    MembershipsGetByUser(Vec<ServerMembership>),
    MembershipsGetMembersByServer(Vec<ServerMember>),
    MembershipsGetByUserAndServer(ServerMember),
    MembershipsUpsert(FullServerMembership),
    MembershipsDelete,
    ServersCreate(Server),
    ServersGetAll(Vec<Server>),
    ServersGetById(Server),
    ServersGetWithChannels(ServerWithChannels),
    ServersDelete,
    ChannelsCreate(Channel),
    ChannelsGetAll(Vec<Channel>),
    ChannelsGetByServer(Vec<Channel>),
    ChannelsGetById(Channel),
    ChannelsDelete,
    MessagesCreate(Message),
    MessagesGetAll(Vec<Message>),
    MessagesGetByServer(Vec<Message>),
    MessagesGetByChannel(Vec<Message>),
    MessagesGetById(Message),
    MessagesDelete,
}

/// Request enum for federation websocket traffic.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum FederationWsRequest {
    ConnectionState,
    UsersGetAll,
    UsersGetByRef {
        user_ref: UserRef,
    },
    UsersGetAssociatedHosts {
        user_ref: UserRef,
    },
    UsersDelete {
        user_ref: UserRef,
    },
    MembershipsUpsert {
        new_membership: NewServerMembershipFull,
    },
    MembershipsGetByUser {
        user_ref: UserRef,
    },
    MembershipsDelete {
        server_id: ServerId,
        user_ref: UserRef,
    },
    MembershipsGetMembersByServer {
        server_id: ServerId,
    },
    MembershipsGetByUserAndServer {
        server_id: ServerId,
        user_ref: UserRef,
    },
    ServersCreate(NewServer),
    ServersDelete {
        server_id: ServerId,
    },
    ServersGetAll,
    ServersGetById {
        server_id: ServerId,
    },
    ServersGetWithChannels {
        server_id: ServerId,
    },
    ChannelsCreate {
        server_id: ServerId,
        new_channel: NewChannel,
    },
    ChannelsGetAll,
    ChannelsGetByServer {
        server_id: ServerId,
    },
    ChannelsGetById {
        server_id: ServerId,
        channel_id: ChannelId,
    },
    ChannelsDelete {
        server_id: ServerId,
        channel_id: ChannelId,
    },
    MessagesCreate {
        server_id: ServerId,
        channel_id: ChannelId,
        new_message: NewMessage,
    },
    MessagesGetAll,
    MessagesGetByServer {
        server_id: ServerId,
    },
    MessagesGetByChannel {
        server_id: ServerId,
        channel_id: ChannelId,
    },
    MessagesGetById {
        server_id: ServerId,
        channel_id: ChannelId,
        message_id: MessageId,
    },
    MessagesDelete {
        server_id: ServerId,
        channel_id: ChannelId,
        message_id: MessageId,
    },
}

impl FederationWsRequest {
    pub fn capability(&self) -> Capability {
        match self {
            Self::ConnectionState => Capability::ConnectionState,
            Self::UsersGetAll
            | Self::UsersGetByRef { .. }
            | Self::UsersGetAssociatedHosts { .. } => Capability::UsersRead,
            Self::UsersDelete { .. } => Capability::UsersDelete,
            Self::MembershipsGetByUser { .. }
            | Self::MembershipsGetMembersByServer { .. }
            | Self::MembershipsGetByUserAndServer { .. } => {
                Capability::MembershipsRead
            }
            Self::MembershipsUpsert { .. } | Self::MembershipsDelete { .. } => {
                Capability::MembershipsWrite
            }
            Self::ServersCreate(_) => Capability::ServersCreate,
            Self::ServersGetAll
            | Self::ServersGetById { .. }
            | Self::ServersGetWithChannels { .. } => Capability::ServersRead,
            Self::ServersDelete { .. } => Capability::ServersDelete,
            Self::ChannelsCreate { .. } => Capability::ChannelsCreate,
            Self::ChannelsGetAll
            | Self::ChannelsGetByServer { .. }
            | Self::ChannelsGetById { .. } => Capability::ChannelsRead,
            Self::ChannelsDelete { .. } => Capability::ChannelsDelete,
            Self::MessagesCreate { .. } => Capability::MessagesCreate,
            Self::MessagesGetAll
            | Self::MessagesGetByServer { .. }
            | Self::MessagesGetByChannel { .. }
            | Self::MessagesGetById { .. } => Capability::MessagesRead,
            Self::MessagesDelete { .. } => Capability::MessagesDelete,
        }
    }
}

/// Reply enum for federation websocket traffic. Variants map 1:1 with request outcomes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum FederationWsReply {
    ConnectionState(FederationWsConnectionState),
    UsersGetAll(Vec<User>),
    UsersGetByRef(User),
    UsersGetAssociatedHosts(Vec<String>),
    UsersDelete,
    MembershipsUpsert(FullServerMembership),
    MembershipsGetByUser(Vec<ServerMembership>),
    MembershipsDelete,
    MembershipsGetMembersByServer(Vec<ServerMember>),
    MembershipsGetByUserAndServer(ServerMember),
    ServersCreate(Server),
    ServersDelete,
    ServersGetAll(Vec<Server>),
    ServersGetById(Server),
    ServersGetWithChannels(ServerWithChannels),
    ChannelsCreate(Channel),
    ChannelsGetAll(Vec<Channel>),
    ChannelsGetByServer(Vec<Channel>),
    ChannelsGetById(Channel),
    ChannelsDelete,
    MessagesCreate(Message),
    MessagesGetAll(Vec<Message>),
    MessagesGetByServer(Vec<Message>),
    MessagesGetByChannel(Vec<Message>),
    MessagesGetById(Message),
    MessagesDelete,
}

/// Client websocket updates are push-only events and do not map 1:1 with requests.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ClientWsUpdate {
    UserUpserted(User),
    UserDeleted {
        user_ref: UserRef,
    },
    MembershipUpserted(FullServerMembership),
    MembershipDeleted {
        server_id: ServerId,
        user_ref: UserRef,
    },
    ServerUpserted(Server),
    ServerDeleted {
        server_id: ServerId,
    },
    ChannelUpserted(Channel),
    ChannelDeleted {
        server_id: ServerId,
        channel_id: ChannelId,
    },
    MessageUpserted(Message),
    MessageDeleted {
        server_id: ServerId,
        channel_id: ChannelId,
        message_id: MessageId,
    },
}

impl ClientWsUpdate {
    pub fn capability(&self) -> Capability {
        match self {
            Self::UserUpserted(_) | Self::UserDeleted { .. } => {
                Capability::UsersEvents
            }
            Self::MembershipUpserted(_) | Self::MembershipDeleted { .. } => {
                Capability::MembershipsEvents
            }
            Self::ServerUpserted(_) | Self::ServerDeleted { .. } => {
                Capability::ServersEvents
            }
            Self::ChannelUpserted(_) | Self::ChannelDeleted { .. } => {
                Capability::ChannelsEvents
            }
            Self::MessageUpserted(_) | Self::MessageDeleted { .. } => {
                Capability::MessagesEvents
            }
        }
    }
}

/// Federation websocket updates are push-only events
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum FederationWsUpdate {
    MembershipUpserted(FullServerMembership),
    MembershipDeleted {
        server_id: ServerId,
        user_ref: UserRef,
    },
    ServerUpserted(Server),
    ServerDeleted {
        server_id: ServerId,
    },
    ChannelUpserted(Channel),
    ChannelDeleted {
        server_id: ServerId,
        channel_id: ChannelId,
    },
    MessageUpserted {
        server_id: ServerId,
        message: Message,
    },
    MessageDeleted {
        server_id: ServerId,
        channel_id: ChannelId,
        message_id: MessageId,
    },
    RemoteUserDeleted {
        user_ref: UserRef,
    },
}

impl FederationWsUpdate {
    pub fn capability(&self) -> Capability {
        match self {
            Self::RemoteUserDeleted { .. } => Capability::UsersEvents,
            Self::MembershipUpserted(_) | Self::MembershipDeleted { .. } => {
                Capability::MembershipsEvents
            }
            Self::ServerUpserted(_) | Self::ServerDeleted { .. } => {
                Capability::ServersEvents
            }
            Self::ChannelUpserted(_) | Self::ChannelDeleted { .. } => {
                Capability::ChannelsEvents
            }
            Self::MessageUpserted { .. } | Self::MessageDeleted { .. } => {
                Capability::MessagesEvents
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ClientWsEnvelope {
    Hello(CapabilityHello),
    Welcome(CapabilityWelcome),
    Request {
        request_id: RequestId,
        request: ClientWsRequest,
    },
    Reply {
        request_id: RequestId,
        event_id: EventId,
        reply: ClientWsReply,
    },
    Error {
        request_id: Option<RequestId>,
        event_id: EventId,
        error: WsError,
    },
    Update {
        event_id: EventId,
        update: ClientWsUpdate,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum FederationWsEnvelope {
    Hello(CapabilityHello),
    Welcome(CapabilityWelcome),
    Request {
        request_id: RequestId,
        event_id: EventId,
        delegated_user_ref: Option<UserRef>,
        request: FederationWsRequest,
    },
    Reply {
        request_id: RequestId,
        event_id: EventId,
        reply: FederationWsReply,
    },
    Error {
        request_id: Option<RequestId>,
        event_id: EventId,
        error: WsError,
    },
    Update {
        event_id: EventId,
        update: FederationWsUpdate,
    },
}

impl std::fmt::Debug for AuthTokenAccessRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthTokenAccessRequest")
            .field("access_token", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{AuthTokenAccessRequest, ClientWsEnvelope};
    use crate::capability::{CapabilityHello, versions::NEGOTIATION_VERSION};

    #[test]
    fn auth_token_access_request_debug_redacts_access_token() {
        let request = AuthTokenAccessRequest {
            access_token: "secret-token".into(),
        };

        let debug = format!("{request:?}");

        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("secret-token"));
    }

    #[test]
    fn capability_hello_has_stable_bootstrap_shape() {
        let envelope = ClientWsEnvelope::Hello(CapabilityHello {
            negotiation_version: NEGOTIATION_VERSION,
            capabilities: BTreeMap::from([("messages.read".into(), vec![1])]),
        });

        assert_eq!(
            serde_json::to_value(envelope).unwrap(),
            serde_json::json!({
                "type": "hello",
                "data": {
                    "negotiation_version": 1,
                    "capabilities": { "messages.read": [1] }
                }
            })
        );
    }
}
