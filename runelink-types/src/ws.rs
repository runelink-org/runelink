use crate::{
    channel::ServerChannelUpdate, message::MessageUpdate, server::ServerUpdate,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    auth::{
        AuthTokenPasswordRequest, AuthTokenRefreshRequest, JwksResponse,
        OidcDiscoveryDocument, SignupRequest, TokenResponse,
    },
    channel::{
        NewServerChannel, ServerChannel, ServerChannelId, ServerChannelMove,
        ServerChannelRef, TextChannelId, TextChannelRef,
    },
    host::Host,
    message::{Message, MessageId, MessageRef, NewMessage},
    presence::{
        ChosenPresenceStatus, RelayedPresenceStatus, UserPresenceStatus,
    },
    server::{
        FullServerMembership, NewServer, NewServerMembership,
        NewServerMembershipFull, Server, ServerId, ServerMember,
        ServerMembership, ServerRef, ServerWithChannels,
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
    Authenticated { host: Host },
}

/// Request enum for websocket client traffic. Variants map to existing API endpoints.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ClientWsRequest {
    ServersUpdate {
        server_ref: ServerRef,
        server_update: ServerUpdate,
    },
    ChannelsUpdate {
        channel_ref: ServerChannelRef,
        channel_update: ServerChannelUpdate,
    },
    MessagesUpdate {
        message_ref: MessageRef,
        message_update: MessageUpdate,
    },
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
        target_host: Option<Host>,
    },
    UsersGetByRef {
        user_ref: UserRef,
    },
    UsersGetAssociatedHosts {
        user_ref: UserRef,
        target_host: Option<Host>,
    },
    UsersDelete {
        user_ref: UserRef,
    },
    MembershipsGetByUser {
        user_ref: UserRef,
    },
    MembershipsGetMembersByServer {
        server_ref: ServerRef,
    },
    MembershipsGetByUserAndServer {
        server_ref: ServerRef,
        user_ref: UserRef,
    },
    MembershipsUpsert {
        new_membership: NewServerMembership,
    },
    MembershipsDelete {
        server_ref: ServerRef,
        user_ref: UserRef,
    },
    ServersCreate {
        new_server: NewServer,
        target_host: Option<Host>,
    },
    ServersGetAll {
        target_host: Option<Host>,
    },
    ServersGetById {
        server_ref: ServerRef,
    },
    ServersGetWithChannels {
        server_ref: ServerRef,
    },
    ServersDelete {
        server_ref: ServerRef,
    },
    ChannelsCreate {
        server_ref: ServerRef,
        new_channel: NewServerChannel,
    },
    ChannelsGetAll {
        target_host: Option<Host>,
    },
    ChannelsGetByServer {
        server_ref: ServerRef,
    },
    ChannelsGetById {
        channel_ref: ServerChannelRef,
    },
    ChannelsDelete {
        channel_ref: ServerChannelRef,
    },
    ChannelsMove {
        channel_ref: ServerChannelRef,
        channel_move: ServerChannelMove,
    },
    MessagesCreate {
        text_channel_ref: TextChannelRef,
        new_message: NewMessage,
    },
    MessagesGetAll {
        target_host: Option<Host>,
    },
    MessagesGetByServer {
        server_ref: ServerRef,
    },
    MessagesGetByTextChannel {
        text_channel_ref: TextChannelRef,
    },
    MessagesGetById {
        message_ref: MessageRef,
    },
    MessagesDelete {
        message_ref: MessageRef,
    },
    /// Chooses the authenticated user's presence status.
    PresenceStatusUpdate {
        status: ChosenPresenceStatus,
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
            Self::ServersUpdate { .. } => Capability::ServersUpdate,
            Self::ServersCreate { .. } => Capability::ServersCreate,
            Self::ServersGetAll { .. }
            | Self::ServersGetById { .. }
            | Self::ServersGetWithChannels { .. } => Capability::ServersRead,
            Self::ServersDelete { .. } => Capability::ServersDelete,
            Self::ChannelsUpdate { .. } => Capability::ChannelsUpdate,
            Self::ChannelsCreate { .. } => Capability::ChannelsCreate,
            Self::ChannelsGetAll { .. }
            | Self::ChannelsGetByServer { .. }
            | Self::ChannelsGetById { .. } => Capability::ChannelsRead,
            Self::ChannelsDelete { .. } => Capability::ChannelsDelete,
            Self::ChannelsMove { .. } => Capability::ChannelsMove,
            Self::MessagesUpdate { .. } => Capability::MessagesUpdate,
            Self::MessagesCreate { .. } => Capability::MessagesCreate,
            Self::MessagesGetAll { .. }
            | Self::MessagesGetByServer { .. }
            | Self::MessagesGetByTextChannel { .. }
            | Self::MessagesGetById { .. } => Capability::MessagesRead,
            Self::MessagesDelete { .. } => Capability::MessagesDelete,
            Self::PresenceStatusUpdate { .. } => Capability::PresenceStatus,
        }
    }

    /// Whether a reply is the kind the host sends for this request.
    pub fn expects_reply(&self, reply: &ClientWsReply) -> bool {
        use ClientWsReply as Reply;
        match self {
            Self::Ping => matches!(reply, Reply::Pong),
            Self::OidcDiscovery => matches!(reply, Reply::OidcDiscovery(_)),
            Self::OidcJwks => matches!(reply, Reply::OidcJwks(_)),
            Self::ConnectionState => matches!(reply, Reply::ConnectionState(_)),
            Self::AuthSignup(_) => matches!(reply, Reply::AuthSignup(_)),
            Self::AuthTokenPassword(_) | Self::AuthTokenRefresh(_) => {
                matches!(reply, Reply::AuthToken(_))
            }
            Self::AuthTokenAccess(_) => {
                matches!(reply, Reply::AuthTokenAccess(_))
            }
            // These have no replies yet
            Self::AuthUserinfo | Self::AuthRegisterClient => false,
            Self::UsersCreate(_) => matches!(reply, Reply::UsersCreate(_)),
            Self::UsersGetAll { .. } => matches!(reply, Reply::UsersGetAll(_)),
            Self::UsersGetByRef { .. } => {
                matches!(reply, Reply::UsersGetByRef(_))
            }
            Self::UsersGetAssociatedHosts { .. } => {
                matches!(reply, Reply::UsersGetAssociatedHosts(_))
            }
            Self::UsersDelete { .. } => matches!(reply, Reply::UsersDelete),
            Self::MembershipsGetByUser { .. } => {
                matches!(reply, Reply::MembershipsGetByUser(_))
            }
            Self::MembershipsGetMembersByServer { .. } => {
                matches!(reply, Reply::MembershipsGetMembersByServer(_))
            }
            Self::MembershipsGetByUserAndServer { .. } => {
                matches!(reply, Reply::MembershipsGetByUserAndServer(_))
            }
            Self::MembershipsUpsert { .. } => {
                matches!(reply, Reply::MembershipsUpsert(_))
            }
            Self::MembershipsDelete { .. } => {
                matches!(reply, Reply::MembershipsDelete)
            }
            Self::ServersUpdate { .. } => {
                matches!(reply, Reply::ServersUpdate(_))
            }
            Self::ServersCreate { .. } => {
                matches!(reply, Reply::ServersCreate(_))
            }
            Self::ServersGetAll { .. } => {
                matches!(reply, Reply::ServersGetAll(_))
            }
            Self::ServersGetById { .. } => {
                matches!(reply, Reply::ServersGetById(_))
            }
            Self::ServersGetWithChannels { .. } => {
                matches!(reply, Reply::ServersGetWithChannels(_))
            }
            Self::ServersDelete { .. } => matches!(reply, Reply::ServersDelete),
            Self::ChannelsUpdate { .. } => {
                matches!(reply, Reply::ChannelsUpdate(_))
            }
            Self::ChannelsCreate { .. } => {
                matches!(reply, Reply::ChannelsCreate(_))
            }
            Self::ChannelsGetAll { .. } => {
                matches!(reply, Reply::ChannelsGetAll(_))
            }
            Self::ChannelsGetByServer { .. } => {
                matches!(reply, Reply::ChannelsGetByServer(_))
            }
            Self::ChannelsGetById { .. } => {
                matches!(reply, Reply::ChannelsGetById(_))
            }
            Self::ChannelsDelete { .. } => {
                matches!(reply, Reply::ChannelsDelete)
            }
            Self::ChannelsMove { .. } => {
                matches!(reply, Reply::ChannelsMove(_))
            }
            Self::MessagesUpdate { .. } => {
                matches!(reply, Reply::MessagesUpdate(_))
            }
            Self::MessagesCreate { .. } => {
                matches!(reply, Reply::MessagesCreate(_))
            }
            Self::MessagesGetAll { .. } => {
                matches!(reply, Reply::MessagesGetAll(_))
            }
            Self::MessagesGetByServer { .. } => {
                matches!(reply, Reply::MessagesGetByServer(_))
            }
            Self::MessagesGetByTextChannel { .. } => {
                matches!(reply, Reply::MessagesGetByTextChannel(_))
            }
            Self::MessagesGetById { .. } => {
                matches!(reply, Reply::MessagesGetById(_))
            }
            Self::MessagesDelete { .. } => {
                matches!(reply, Reply::MessagesDelete(_))
            }
            Self::PresenceStatusUpdate { .. } => {
                matches!(reply, Reply::PresenceStatusUpdate)
            }
        }
    }
}

/// Reply enum for websocket client traffic. Variants map 1:1 with request outcomes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ClientWsReply {
    ServersUpdate(Server),
    ChannelsUpdate(ServerChannel),
    MessagesUpdate(Message),
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
    UsersGetAssociatedHosts(Vec<Host>),
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
    ChannelsCreate(ServerChannel),
    ChannelsGetAll(Vec<ServerChannel>),
    ChannelsGetByServer(Vec<ServerChannel>),
    ChannelsGetById(ServerChannel),
    ChannelsDelete,
    ChannelsMove(Vec<ServerChannel>),
    MessagesCreate(Message),
    MessagesGetAll(Vec<Message>),
    MessagesGetByServer(Vec<Message>),
    MessagesGetByTextChannel(Vec<Message>),
    MessagesGetById(Message),
    MessagesDelete(Message),
    PresenceStatusUpdate,
}

/// Request enum for federation websocket traffic.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum FederationWsRequest {
    ServersUpdate {
        server_id: ServerId,
        server_update: ServerUpdate,
    },
    ChannelsUpdate {
        channel_id: ServerChannelId,
        channel_update: ServerChannelUpdate,
    },
    MessagesUpdate {
        message_id: MessageId,
        message_update: MessageUpdate,
    },
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
        new_channel: NewServerChannel,
    },
    ChannelsGetAll,
    ChannelsGetByServer {
        server_id: ServerId,
    },
    ChannelsGetById {
        channel_id: ServerChannelId,
    },
    ChannelsDelete {
        channel_id: ServerChannelId,
    },
    ChannelsMove {
        channel_id: ServerChannelId,
        channel_move: ServerChannelMove,
    },
    MessagesCreate {
        text_channel_id: TextChannelId,
        new_message: NewMessage,
    },
    MessagesGetAll,
    MessagesGetByServer {
        server_id: ServerId,
    },
    MessagesGetByTextChannel {
        text_channel_id: TextChannelId,
    },
    MessagesGetById {
        message_id: MessageId,
    },
    MessagesDelete {
        message_id: MessageId,
    },
    /// The presence statuses of the users who share a server on this host
    /// with the delegated user, leaving out offline users, or of those who
    /// share one of the given servers, including offline users.
    PresenceStatusesGet {
        server_ids: Option<Vec<ServerId>>,
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
            Self::MembershipsGetMembersByServer { .. }
            | Self::MembershipsGetByUserAndServer { .. } => {
                Capability::MembershipsRead
            }
            Self::MembershipsUpsert { .. } | Self::MembershipsDelete { .. } => {
                Capability::MembershipsWrite
            }
            Self::ServersUpdate { .. } => Capability::ServersUpdate,
            Self::ServersCreate(_) => Capability::ServersCreate,
            Self::ServersGetAll
            | Self::ServersGetById { .. }
            | Self::ServersGetWithChannels { .. } => Capability::ServersRead,
            Self::ServersDelete { .. } => Capability::ServersDelete,
            Self::ChannelsUpdate { .. } => Capability::ChannelsUpdate,
            Self::ChannelsCreate { .. } => Capability::ChannelsCreate,
            Self::ChannelsGetAll
            | Self::ChannelsGetByServer { .. }
            | Self::ChannelsGetById { .. } => Capability::ChannelsRead,
            Self::ChannelsDelete { .. } => Capability::ChannelsDelete,
            Self::ChannelsMove { .. } => Capability::ChannelsMove,
            Self::MessagesUpdate { .. } => Capability::MessagesUpdate,
            Self::MessagesCreate { .. } => Capability::MessagesCreate,
            Self::MessagesGetAll
            | Self::MessagesGetByServer { .. }
            | Self::MessagesGetByTextChannel { .. }
            | Self::MessagesGetById { .. } => Capability::MessagesRead,
            Self::MessagesDelete { .. } => Capability::MessagesDelete,
            Self::PresenceStatusesGet { .. } => Capability::PresenceStatus,
        }
    }
}

/// Reply enum for federation websocket traffic. Variants map 1:1 with request outcomes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum FederationWsReply {
    ServersUpdate(Server),
    ChannelsUpdate(ServerChannel),
    MessagesUpdate(Message),
    ConnectionState(FederationWsConnectionState),
    UsersGetAll(Vec<User>),
    UsersGetByRef(User),
    UsersGetAssociatedHosts(Vec<Host>),
    UsersDelete,
    MembershipsUpsert(FullServerMembership),
    MembershipsDelete,
    MembershipsGetMembersByServer(Vec<ServerMember>),
    MembershipsGetByUserAndServer(ServerMember),
    ServersCreate(Server),
    ServersDelete,
    ServersGetAll(Vec<Server>),
    ServersGetById(Server),
    ServersGetWithChannels(ServerWithChannels),
    ChannelsCreate(ServerChannel),
    ChannelsGetAll(Vec<ServerChannel>),
    ChannelsGetByServer(Vec<ServerChannel>),
    ChannelsGetById(ServerChannel),
    ChannelsDelete,
    ChannelsMove(Vec<ServerChannel>),
    MessagesCreate(Message),
    MessagesGetAll(Vec<Message>),
    MessagesGetByServer(Vec<Message>),
    MessagesGetByTextChannel(Vec<Message>),
    MessagesGetById(Message),
    MessagesDelete(Message),
    PresenceStatusesGet(Vec<UserPresenceStatus>),
}

impl FederationWsReply {
    /// Whether every resource in the reply that has an owning host is owned
    /// by `host`, which is what a host may reply with about its own
    /// resources. Users and members may be other hosts' users.
    pub fn is_owned_by(&self, host: &Host) -> bool {
        let servers =
            |servers: &[Server]| servers.iter().all(|s| s.host == *host);
        let channels = |channels: &[ServerChannel]| {
            channels.iter().all(|c| c.host == *host)
        };
        let messages =
            |messages: &[Message]| messages.iter().all(|m| m.host == *host);
        match self {
            Self::ServersCreate(server)
            | Self::ServersGetById(server)
            | Self::ServersUpdate(server) => server.host == *host,
            Self::ServersGetAll(list) => servers(list),
            Self::ServersGetWithChannels(ServerWithChannels {
                server,
                channels: list,
            }) => server.host == *host && channels(list),
            Self::MembershipsUpsert(membership) => {
                membership.server.host == *host
            }
            Self::ChannelsCreate(channel)
            | Self::ChannelsGetById(channel)
            | Self::ChannelsUpdate(channel) => channel.host == *host,
            Self::ChannelsGetAll(list)
            | Self::ChannelsGetByServer(list)
            | Self::ChannelsMove(list) => channels(list),
            Self::MessagesCreate(message)
            | Self::MessagesGetById(message)
            | Self::MessagesUpdate(message)
            | Self::MessagesDelete(message) => message.host == *host,
            Self::MessagesGetAll(list)
            | Self::MessagesGetByServer(list)
            | Self::MessagesGetByTextChannel(list) => messages(list),
            // Peers answer for their own users only.
            Self::UsersGetByRef(user) => user.host == *host,
            // Peers answer for the members of their own servers.
            Self::PresenceStatusesGet(_)
            | Self::ConnectionState(_)
            | Self::UsersGetAll(_)
            | Self::UsersGetAssociatedHosts(_)
            | Self::UsersDelete
            | Self::MembershipsDelete
            | Self::MembershipsGetMembersByServer(_)
            | Self::MembershipsGetByUserAndServer(_)
            | Self::ServersDelete
            | Self::ChannelsDelete => true,
        }
    }
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
        server_ref: ServerRef,
        user_ref: UserRef,
    },
    ServerUpserted(Server),
    ServerDeleted {
        server_ref: ServerRef,
    },
    ChannelUpserted(ServerChannel),
    ChannelDeleted {
        server_ref: ServerRef,
        channel_id: ServerChannelId,
    },
    MessageUpserted(Message),
    /// Another user's presence status.
    PresenceStatusUpdated(UserPresenceStatus),
    /// The presence status the authenticated user chose.
    ChosenPresenceStatusUpdated {
        status: ChosenPresenceStatus,
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
            Self::MessageUpserted(_) => Capability::MessagesEvents,
            Self::PresenceStatusUpdated(_)
            | Self::ChosenPresenceStatusUpdated { .. } => {
                Capability::PresenceStatus
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
        server_ref: ServerRef,
        user_ref: UserRef,
    },
    ServerUpserted(Server),
    ServerDeleted {
        server_ref: ServerRef,
    },
    ChannelUpserted(ServerChannel),
    ChannelDeleted {
        server_ref: ServerRef,
        channel_id: ServerChannelId,
    },
    /// A message in a text channel of the server on the message's host.
    MessageUpserted {
        server_id: ServerId,
        message: Message,
    },
    RemoteUserDeleted {
        user_ref: UserRef,
    },
    /// A user's presence status, sent by their home host to the hosts of the
    /// servers they are in.
    PresenceStatusUpdated(UserPresenceStatus),
    /// Presence statuses of the members of the sending host's servers, for
    /// users on the receiving host.
    PresenceStatusesRelayed(Vec<RelayedPresenceStatus>),
}

impl FederationWsUpdate {
    /// The host that owns what the update is about, which is the only host
    /// that may send it, or `None` for updates any host may send about what
    /// it relays.
    pub fn owner(&self) -> Option<&Host> {
        Some(match self {
            Self::MembershipUpserted(membership) => &membership.server.host,
            Self::MembershipDeleted { server_ref, .. }
            | Self::ServerDeleted { server_ref }
            | Self::ChannelDeleted { server_ref, .. } => &server_ref.host,
            Self::ServerUpserted(server) => &server.host,
            Self::ChannelUpserted(channel) => &channel.host,
            Self::MessageUpserted { message, .. } => &message.host,
            Self::RemoteUserDeleted { user_ref }
            | Self::PresenceStatusUpdated(UserPresenceStatus {
                user_ref,
                ..
            }) => &user_ref.host,
            Self::PresenceStatusesRelayed(_) => return None,
        })
    }

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
            Self::MessageUpserted { .. } => Capability::MessagesEvents,
            Self::PresenceStatusUpdated(_)
            | Self::PresenceStatusesRelayed(_) => Capability::PresenceStatus,
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

    use super::*;
    use crate::capability::{
        CapabilityHello,
        versions::{NEGOTIATION_VERSION, messages},
    };
    use crate::{channel::ServerChannelContent, message::MessageContent};
    use time::OffsetDateTime;

    fn host(host: &str) -> Host {
        Host::parse(host).unwrap()
    }

    fn server(host: &Host) -> Server {
        Server {
            id: ServerId::new(),
            host: host.clone(),
            title: "test".into(),
            description: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn channel(host: &Host) -> ServerChannel {
        ServerChannel {
            id: ServerChannelId::new(),
            host: host.clone(),
            server_id: ServerId::new(),
            content: ServerChannelContent::Category,
            title: "test".into(),
            description: None,
            metadata: None,
            parent_channel_id: None,
            position: 0,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn message(host: &Host) -> Message {
        Message {
            id: MessageId::new(),
            host: host.clone(),
            text_channel_id: TextChannelId::new(),
            author: None,
            reply_to: None,
            metadata: None,
            content: MessageContent::Active {
                body: "hi".into(),
                deleted_at: (),
            },
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            edited_at: None,
        }
    }

    fn user(host: &Host) -> User {
        User {
            name: "alice".into(),
            host: host.clone(),
            role: crate::user::UserRole::User,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            synced_at: None,
        }
    }

    /// A membership of a server on `server_host` by a user on `user_host`.
    fn membership(
        server_host: &Host,
        user_host: &Host,
    ) -> FullServerMembership {
        FullServerMembership {
            server: server(server_host),
            user: user(user_host),
            role: crate::server::ServerRole::Member,
            joined_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            synced_at: None,
        }
    }

    #[test]
    fn updates_are_owned_by_the_host_of_their_resource() {
        let (a, b) = (host("a.example"), host("b.example"));
        let user_ref = UserRef::parse_subject("alice@b.example").unwrap();
        let server_ref = ServerRef::new(a.clone(), ServerId::new());
        for update in [
            FederationWsUpdate::ServerUpserted(server(&a)),
            FederationWsUpdate::ServerDeleted {
                server_ref: server_ref.clone(),
            },
            FederationWsUpdate::ChannelUpserted(channel(&a)),
            FederationWsUpdate::ChannelDeleted {
                server_ref: server_ref.clone(),
                channel_id: ServerChannelId::new(),
            },
            FederationWsUpdate::MembershipDeleted {
                server_ref: server_ref.clone(),
                user_ref: user_ref.clone(),
            },
            FederationWsUpdate::MessageUpserted {
                server_id: ServerId::new(),
                message: message(&a),
            },
            // Owned by the server's host, whoever the member is
            FederationWsUpdate::MembershipUpserted(membership(&a, &b)),
        ] {
            assert_eq!(update.owner(), Some(&a), "{update:?}");
        }
        // A user's deletion and presence status belong to their home host.
        assert_eq!(
            FederationWsUpdate::RemoteUserDeleted {
                user_ref: user_ref.clone()
            }
            .owner(),
            Some(&b)
        );
        assert_eq!(
            FederationWsUpdate::PresenceStatusUpdated(UserPresenceStatus {
                user_ref,
                status: crate::presence::PresenceStatus::Online,
            })
            .owner(),
            Some(&b)
        );
    }

    #[test]
    fn replies_are_owned_only_if_all_their_resources_are() {
        let (a, b) = (host("a.example"), host("b.example"));
        assert!(FederationWsReply::ServersGetById(server(&a)).is_owned_by(&a));
        assert!(!FederationWsReply::ServersGetById(server(&b)).is_owned_by(&a));
        assert!(
            !FederationWsReply::ServersGetAll(vec![server(&a), server(&b)])
                .is_owned_by(&a)
        );
        assert!(
            !FederationWsReply::ServersGetWithChannels(ServerWithChannels {
                server: server(&a),
                channels: vec![channel(&a), channel(&b)],
            })
            .is_owned_by(&a)
        );
        assert!(
            !FederationWsReply::ChannelsMove(vec![channel(&b)]).is_owned_by(&a)
        );
        assert!(
            !FederationWsReply::MessagesGetByTextChannel(vec![message(&b)])
                .is_owned_by(&a)
        );
        assert!(FederationWsReply::MessagesDelete(message(&a)).is_owned_by(&a));
        assert!(
            FederationWsReply::MembershipsUpsert(membership(&a, &b))
                .is_owned_by(&a)
        );
        assert!(
            !FederationWsReply::MembershipsUpsert(membership(&b, &a))
                .is_owned_by(&a)
        );
        assert!(FederationWsReply::UsersGetByRef(user(&a)).is_owned_by(&a));
        assert!(!FederationWsReply::UsersGetByRef(user(&b)).is_owned_by(&a));
    }

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
            capabilities: BTreeMap::from([(
                "messages.read".into(),
                vec![messages::READ_VERSION],
            )]),
        });

        assert_eq!(
            serde_json::to_value(envelope).unwrap(),
            serde_json::json!({
                "type": "hello",
                "data": {
                    "negotiation_version": 1,
                    "capabilities": { "messages.read": ["2026.11.27"] }
                }
            })
        );
    }
}
