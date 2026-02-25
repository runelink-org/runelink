use crate::{
    AuthTokenPasswordRequest, AuthTokenRefreshRequest, Channel,
    FullServerMembership, JwksResponse, Message, NewChannel, NewMessage,
    NewServer, NewServerMembership, NewUser, OidcDiscoveryDocument, Server,
    ServerMember, ServerMembership, ServerWithChannels, SignupRequest,
    TokenResponse, User, UserRef,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WsError {
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthTokenAccessRequest {
    pub access_token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ClientWsConnectionState {
    Unauthenticated,
    Authenticated { user_ref: UserRef },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FederationWsConnectionState {
    Unauthenticated,
    Authenticated { host: String },
}

/// Request enum for websocket client traffic. Variants map to existing API endpoints.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
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
        server_id: Uuid,
        target_host: Option<String>,
    },
    MembershipsGetByUserAndServer {
        server_id: Uuid,
        user_ref: UserRef,
        target_host: Option<String>,
    },
    MembershipsCreate {
        server_id: Uuid,
        new_membership: NewServerMembership,
    },
    MembershipsDelete {
        server_id: Uuid,
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
        server_id: Uuid,
        target_host: Option<String>,
    },
    ServersGetWithChannels {
        server_id: Uuid,
        target_host: Option<String>,
    },
    ServersDelete {
        server_id: Uuid,
        target_host: Option<String>,
    },
    ChannelsCreate {
        server_id: Uuid,
        new_channel: NewChannel,
        target_host: Option<String>,
    },
    ChannelsGetAll {
        target_host: Option<String>,
    },
    ChannelsGetByServer {
        server_id: Uuid,
        target_host: Option<String>,
    },
    ChannelsGetById {
        server_id: Uuid,
        channel_id: Uuid,
        target_host: Option<String>,
    },
    ChannelsDelete {
        server_id: Uuid,
        channel_id: Uuid,
        target_host: Option<String>,
    },
    MessagesCreate {
        server_id: Uuid,
        channel_id: Uuid,
        new_message: NewMessage,
        target_host: Option<String>,
    },
    MessagesGetAll {
        target_host: Option<String>,
    },
    MessagesGetByServer {
        server_id: Uuid,
        target_host: Option<String>,
    },
    MessagesGetByChannel {
        server_id: Uuid,
        channel_id: Uuid,
        target_host: Option<String>,
    },
    MessagesGetById {
        server_id: Uuid,
        channel_id: Uuid,
        message_id: Uuid,
        target_host: Option<String>,
    },
    MessagesDelete {
        server_id: Uuid,
        channel_id: Uuid,
        message_id: Uuid,
        target_host: Option<String>,
    },
}

/// Reply enum for websocket client traffic. Variants map 1:1 with request outcomes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ClientWsReply {
    Ping,
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
    MembershipsCreate(FullServerMembership),
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
#[serde(tag = "action", rename_all = "snake_case")]
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
    MembershipsCreate {
        server_id: Uuid,
        new_membership: NewServerMembership,
    },
    MembershipsGetByUser {
        user_ref: UserRef,
    },
    MembershipsDelete {
        server_id: Uuid,
        user_ref: UserRef,
    },
    MembershipsGetMembersByServer {
        server_id: Uuid,
    },
    MembershipsGetByUserAndServer {
        server_id: Uuid,
        user_ref: UserRef,
    },
    ServersCreate(NewServer),
    ServersDelete {
        server_id: Uuid,
    },
    ServersGetAll,
    ServersGetById {
        server_id: Uuid,
    },
    ServersGetWithChannels {
        server_id: Uuid,
    },
    ChannelsCreate {
        server_id: Uuid,
        new_channel: NewChannel,
    },
    ChannelsGetAll,
    ChannelsGetByServer {
        server_id: Uuid,
    },
    ChannelsGetById {
        server_id: Uuid,
        channel_id: Uuid,
    },
    ChannelsDelete {
        server_id: Uuid,
        channel_id: Uuid,
    },
    MessagesCreate {
        server_id: Uuid,
        channel_id: Uuid,
        new_message: NewMessage,
    },
    MessagesGetAll,
    MessagesGetByServer {
        server_id: Uuid,
    },
    MessagesGetByChannel {
        server_id: Uuid,
        channel_id: Uuid,
    },
    MessagesGetById {
        server_id: Uuid,
        channel_id: Uuid,
        message_id: Uuid,
    },
    MessagesDelete {
        server_id: Uuid,
        channel_id: Uuid,
        message_id: Uuid,
    },
}

/// Reply enum for federation websocket traffic. Variants map 1:1 with request outcomes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum FederationWsReply {
    ConnectionState(FederationWsConnectionState),
    UsersGetAll(Vec<User>),
    UsersGetByRef(User),
    UsersGetAssociatedHosts(Vec<String>),
    UsersDelete,
    MembershipsCreate(FullServerMembership),
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
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ClientWsUpdate {
    UserUpserted(User),
    UserDeleted {
        user_ref: UserRef,
    },
    MembershipUpserted(FullServerMembership),
    MembershipDeleted {
        server_id: Uuid,
        user_ref: UserRef,
    },
    ServerUpserted(Server),
    ServerDeleted {
        server_id: Uuid,
    },
    ChannelUpserted(Channel),
    ChannelDeleted {
        server_id: Uuid,
        channel_id: Uuid,
    },
    MessageUpserted(Message),
    MessageDeleted {
        server_id: Uuid,
        channel_id: Uuid,
        message_id: Uuid,
    },
}

/// Federation websocket updates are push-only events
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum FederationWsUpdate {
    MembershipUpserted(FullServerMembership),
    MembershipDeleted {
        server_id: Uuid,
        user_ref: UserRef,
    },
    ServerUpserted(Server),
    ServerDeleted {
        server_id: Uuid,
    },
    ChannelUpserted(Channel),
    ChannelDeleted {
        server_id: Uuid,
        channel_id: Uuid,
    },
    MessageUpserted(Message),
    MessageDeleted {
        server_id: Uuid,
        channel_id: Uuid,
        message_id: Uuid,
    },
    RemoteUserDeleted {
        user_ref: UserRef,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientWsEnvelope {
    Request {
        request_id: Uuid,
        request: ClientWsRequest,
    },
    Reply {
        request_id: Uuid,
        event_id: Uuid,
        reply: ClientWsReply,
    },
    Error {
        request_id: Option<Uuid>,
        event_id: Uuid,
        error: WsError,
    },
    Update {
        event_id: Uuid,
        update: ClientWsUpdate,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FederationWsEnvelope {
    Request {
        request_id: Uuid,
        event_id: Uuid,
        delegated_user_ref: Option<UserRef>,
        request: FederationWsRequest,
    },
    Reply {
        request_id: Uuid,
        event_id: Uuid,
        reply: FederationWsReply,
    },
    Error {
        request_id: Option<Uuid>,
        event_id: Uuid,
        error: WsError,
    },
    Update {
        event_id: Uuid,
        update: FederationWsUpdate,
    },
}
