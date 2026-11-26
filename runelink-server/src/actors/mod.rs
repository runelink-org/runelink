//! Actors that serialize every mutation of local servers and users.
//!
//! Each local server has a server actor for the server itself, its channel
//! tree, and its memberships, and each of its text channels has a channel
//! actor for that channel's messages. Every actor runs one operation at a time,
//! so its changes commit and fan out in the same order.
//!
//! A server's actors share its [`ServerState`] behind a read-write lock. The
//! server actor holds the write side for each operation and channel actors hold
//! the read side, so channels run in parallel with each other, while a server
//! operation runs alone and every message lands entirely before or after it.
//!
//! Operations get an [`ActorCtx`] rather than the `AppState`, which leaves them
//! no way to send a federation request: an actor must never wait on the
//! network, because that would stall every other change it serializes. Reads
//! do not go through actors.
//!
//! Each user has a user actor for creating and deleting them, which may wait
//! on server actors to remove the user's memberships first. Server actors
//! never wait on user actors.
//!
//! Lock order: the registry, then a server's state. An actor never takes the
//! registry while it holds its server's state.
//!
//! Actors serialize changes within one process only, and no database locks
//! back them up, so only one process may serve a database at a time.

mod channel;
mod server;
mod user;

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::Arc,
};

use runelink_types::{
    channel::{
        NewServerChannel, ServerChannel, ServerChannelId, ServerChannelMove,
        ServerChannelUpdate, TextChannelId,
    },
    message::{Message, MessageId, MessageUpdate, NewMessage},
    server::{
        FullServerMembership, NewServer, NewServerMembership, Server, ServerId,
        ServerRole, ServerUpdate,
    },
    user::{NewUser, User, UserRef},
    ws::{ClientWsUpdate, FederationWsUpdate},
};
use sqlx::PgConnection;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};

use crate::{
    auth::{Authorization, Facts},
    config::ServerConfig,
    db::{self, DbPool},
    error::{ApiError, ApiResult},
    queries,
    ws::{ClientWsManager, FederationWsManager},
};

use channel::ChannelOp;
use server::{ServerOp, ServerOutput};
use user::{UserOp, UserOutput};

/// Callers wait for room once this many operations are queued for an actor.
const MAILBOX_CAPACITY: usize = 256;

/// Everything an operation may use: the database and non-blocking fanout.
#[derive(Clone, Debug)]
pub struct ActorCtx {
    db_pool: Arc<DbPool>,
    config: Arc<ServerConfig>,
    client_ws_manager: ClientWsManager,
    federation_ws_manager: FederationWsManager,
}

impl ActorCtx {
    pub fn new(
        db_pool: Arc<DbPool>,
        config: Arc<ServerConfig>,
        client_ws_manager: ClientWsManager,
        federation_ws_manager: FederationWsManager,
    ) -> Self {
        Self {
            db_pool,
            config,
            client_ws_manager,
            federation_ws_manager,
        }
    }

    /// Send updates to their recipients (best effort, never waits on the
    /// network).
    async fn fanout(&self, targets: &Targets, updates: Vec<Update>) {
        for update in updates {
            let _ = self
                .client_ws_manager
                .send_update_to_users(&targets.local_users, update.client)
                .await;
            let _ = self
                .federation_ws_manager
                .send_update_to_hosts(&targets.remote_hosts, update.federation)
                .await;
        }
    }
}

/// An update for clients and the matching one for peer hosts.
struct Update {
    client: ClientWsUpdate,
    federation: FederationWsUpdate,
}

/// The recipients of a server's updates.
#[derive(Debug, Default)]
struct Targets {
    local_users: Vec<UserRef>,
    remote_hosts: BTreeSet<String>,
}

impl Targets {
    fn add(&mut self, config: &ServerConfig, user: &UserRef) {
        if config.locality(&user.host).is_local() {
            self.local_users.push(user.clone());
        } else {
            self.remote_hosts.insert(user.host.clone());
        }
    }
}

/// What a server's actors know about it, kept current by its server actor.
#[derive(Debug, Default)]
struct ServerState {
    /// Set once the server actor stops: the server is gone, its creation
    /// failed, or its state can no longer be trusted. Every operation then
    /// fails and the server's actors stop, and later operations start new ones
    /// from the database.
    stopped: bool,
    members: HashMap<UserRef, ServerRole>,
    text_channels: HashSet<TextChannelId>,
}

impl ServerState {
    /// Load a server's state. Fails with `NotFound` for unknown servers.
    async fn load(ctx: &ActorCtx, server_id: ServerId) -> ApiResult<Self> {
        let mut tx = db::begin_snapshot(&ctx.db_pool).await?;
        queries::servers::get_by_id(&mut *tx, &ctx.config, server_id).await?;
        let members =
            queries::memberships::get_local_roles(&mut *tx, server_id).await?;
        let text_channels =
            queries::channels::get_text_channel_ids(&mut *tx, server_id)
                .await?;
        tx.commit().await?;
        Ok(Self {
            stopped: false,
            members: members.into_iter().collect(),
            text_channels: text_channels.into_iter().collect(),
        })
    }

    fn targets(&self, config: &ServerConfig) -> Targets {
        let mut targets = Targets::default();
        for user in self.members.keys() {
            targets.add(config, user);
        }
        targets
    }
}

type SharedState = Arc<RwLock<ServerState>>;

/// An operation, what it was authorized with, and where to send its result.
struct Envelope<Op, Output> {
    op: Op,
    auth: Authorization,
    reply: oneshot::Sender<ApiResult<Output>>,
}

/// Facts for checking an operation's authorization again inside its actor,
/// against the actor's current state.
struct ActorFacts<'a> {
    conn: &'a mut PgConnection,
    server_id: ServerId,
    state: &'a ServerState,
}

impl Facts for ActorFacts<'_> {
    async fn user(&mut self, user_ref: &UserRef) -> ApiResult<Option<User>> {
        queries::users::find_by_ref(&mut *self.conn, user_ref.clone()).await
    }

    async fn server_role(
        &mut self,
        server_id: ServerId,
        user_ref: &UserRef,
    ) -> ApiResult<Option<ServerRole>> {
        // Actors only know their own server's members, so requirements on any
        // other server fail.
        if server_id != self.server_id {
            return Ok(None);
        }
        Ok(self.state.members.get(user_ref).copied())
    }
}

type ServerMailbox = mpsc::Sender<Envelope<ServerOp, ServerOutput>>;
type ChannelMailbox = mpsc::Sender<Envelope<ChannelOp, Message>>;
type UserMailbox = mpsc::Sender<Envelope<UserOp, UserOutput>>;

#[derive(Clone, Debug)]
struct ServerEntry {
    mailbox: ServerMailbox,
    state: SharedState,
}

/// The running actors. Idle server and channel actors are only a parked task
/// and a channel, so they stay around until their server or channel is
/// deleted (or a server's state can no longer be trusted), while user actors
/// stop once idle. A closed mailbox belongs to an actor that stopped or
/// panicked and is replaced.
#[derive(Debug, Default)]
struct Registry {
    servers: HashMap<ServerId, ServerEntry>,
    /// Counts server actors that stopped, to tell whether one did during a
    /// load.
    stopped_servers: u64,
    channels: HashMap<TextChannelId, ChannelMailbox>,
    users: HashMap<UserRef, UserMailbox>,
}

/// The actors of a host's local servers, started on first use.
#[derive(Clone, Debug)]
pub struct Actors {
    ctx: ActorCtx,
    registry: Arc<Mutex<Registry>>,
}

impl Actors {
    pub fn new(ctx: ActorCtx) -> Self {
        Self {
            ctx,
            registry: Arc::default(),
        }
    }

    /// Create a server with `creator` as its admin.
    pub async fn create_server(
        &self,
        new_server: NewServer,
        creator: UserRef,
        auth: Authorization,
    ) -> ApiResult<Server> {
        // The actor exists before the server does, so nothing can overtake
        // the creation's updates. Nobody else knows the new ID yet.
        let server_id = ServerId::new();
        let entry = self.spawn_server(server_id, SharedState::default());
        self.registry
            .lock()
            .await
            .servers
            .insert(server_id, entry.clone());
        let op = ServerOp::Create {
            new_server,
            creator,
        };
        match send(entry.mailbox, op, auth).await? {
            ServerOutput::Server(server) => Ok(server),
            output => Err(unexpected(output)),
        }
    }

    pub async fn update_server(
        &self,
        server_id: ServerId,
        update: ServerUpdate,
        auth: Authorization,
    ) -> ApiResult<Server> {
        let op = ServerOp::Update(update);
        match self.server_op(server_id, op, auth).await? {
            ServerOutput::Server(server) => Ok(server),
            output => Err(unexpected(output)),
        }
    }

    pub async fn delete_server(
        &self,
        server_id: ServerId,
        auth: Authorization,
    ) -> ApiResult<()> {
        match self.server_op(server_id, ServerOp::Delete, auth).await? {
            ServerOutput::Done => Ok(()),
            output => Err(unexpected(output)),
        }
    }

    pub async fn create_channel(
        &self,
        server_id: ServerId,
        new_channel: NewServerChannel,
        auth: Authorization,
    ) -> ApiResult<ServerChannel> {
        let op = ServerOp::CreateChannel(new_channel);
        match self.server_op(server_id, op, auth).await? {
            ServerOutput::Channel(channel) => Ok(channel),
            output => Err(unexpected(output)),
        }
    }

    pub async fn update_channel(
        &self,
        server_id: ServerId,
        channel_id: ServerChannelId,
        update: ServerChannelUpdate,
        auth: Authorization,
    ) -> ApiResult<ServerChannel> {
        let op = ServerOp::UpdateChannel { channel_id, update };
        match self.server_op(server_id, op, auth).await? {
            ServerOutput::Channel(channel) => Ok(channel),
            output => Err(unexpected(output)),
        }
    }

    /// Move a channel, returning every channel whose placement changed.
    pub async fn move_channel(
        &self,
        server_id: ServerId,
        channel_id: ServerChannelId,
        channel_move: ServerChannelMove,
        auth: Authorization,
    ) -> ApiResult<Vec<ServerChannel>> {
        let op = ServerOp::MoveChannel {
            channel_id,
            channel_move,
        };
        match self.server_op(server_id, op, auth).await? {
            ServerOutput::Channels(channels) => Ok(channels),
            output => Err(unexpected(output)),
        }
    }

    pub async fn delete_channel(
        &self,
        server_id: ServerId,
        channel_id: ServerChannelId,
        auth: Authorization,
    ) -> ApiResult<()> {
        let op = ServerOp::DeleteChannel(channel_id);
        match self.server_op(server_id, op, auth).await? {
            ServerOutput::Done => Ok(()),
            output => Err(unexpected(output)),
        }
    }

    /// Add a member or change their role. A remote user's record is stored
    /// along with the membership.
    pub async fn upsert_membership(
        &self,
        new_membership: NewServerMembership,
        remote_user: Option<User>,
        auth: Authorization,
    ) -> ApiResult<FullServerMembership> {
        let server_id = new_membership.server_id;
        let op = ServerOp::UpsertMembership {
            new_membership,
            remote_user,
        };
        match self.server_op(server_id, op, auth).await? {
            ServerOutput::Membership(membership) => Ok(*membership),
            output => Err(unexpected(output)),
        }
    }

    pub async fn delete_membership(
        &self,
        server_id: ServerId,
        user_ref: UserRef,
        auth: Authorization,
    ) -> ApiResult<()> {
        let op = ServerOp::DeleteMembership(user_ref);
        match self.server_op(server_id, op, auth).await? {
            ServerOutput::Done => Ok(()),
            output => Err(unexpected(output)),
        }
    }

    pub async fn create_message(
        &self,
        text_channel_id: TextChannelId,
        new_message: NewMessage,
        auth: Authorization,
    ) -> ApiResult<Message> {
        let mailbox = self.channel(text_channel_id).await?;
        send(mailbox, ChannelOp::Create(new_message), auth).await
    }

    pub async fn update_message(
        &self,
        text_channel_id: TextChannelId,
        message_id: MessageId,
        update: MessageUpdate,
        auth: Authorization,
    ) -> ApiResult<Message> {
        let mailbox = self.channel(text_channel_id).await?;
        let op = ChannelOp::Update { message_id, update };
        send(mailbox, op, auth).await
    }

    pub async fn delete_message(
        &self,
        text_channel_id: TextChannelId,
        message_id: MessageId,
        auth: Authorization,
    ) -> ApiResult<Message> {
        let mailbox = self.channel(text_channel_id).await?;
        send(mailbox, ChannelOp::Delete(message_id), auth).await
    }

    /// Create a home user, with a local account if a password hash is given.
    pub async fn create_user(
        &self,
        new_user: NewUser,
        password_hash: Option<String>,
        auth: Authorization,
    ) -> ApiResult<User> {
        let user_ref =
            UserRef::new(new_user.name.clone(), new_user.host.clone());
        let op = UserOp::Create {
            new_user,
            password_hash,
        };
        match self.user_op(user_ref, op, auth).await? {
            UserOutput::User(user) => Ok(user),
            output => Err(unexpected(output)),
        }
    }

    /// Delete a home user along with their memberships.
    pub async fn delete_home_user(
        &self,
        user_ref: UserRef,
        auth: Authorization,
    ) -> ApiResult<()> {
        match self.user_op(user_ref, UserOp::DeleteHome, auth).await? {
            UserOutput::Done => Ok(()),
            output => Err(unexpected(output)),
        }
    }

    /// Delete a remote user's record along with their memberships here.
    pub async fn delete_remote_user(
        &self,
        user_ref: UserRef,
        auth: Authorization,
    ) -> ApiResult<()> {
        // Unknown users are recorded as deleted too.
        match self.user_op(user_ref, UserOp::DeleteRemote, auth).await? {
            UserOutput::Done => Ok(()),
            output => Err(unexpected(output)),
        }
    }

    /// Queue an operation on a user's actor, starting it if needed.
    async fn user_op(
        &self,
        user_ref: UserRef,
        op: UserOp,
        auth: Authorization,
    ) -> ApiResult<UserOutput> {
        let mailbox = {
            let mut registry = self.registry.lock().await;
            match registry.users.get(&user_ref) {
                Some(mailbox) if !mailbox.is_closed() => mailbox.clone(),
                _ => {
                    let mailbox = user::spawn(self.clone(), user_ref.clone());
                    registry.users.insert(user_ref, mailbox.clone());
                    mailbox
                }
            }
        };
        send(mailbox, op, auth).await
    }

    async fn server_op(
        &self,
        server_id: ServerId,
        op: ServerOp,
        auth: Authorization,
    ) -> ApiResult<ServerOutput> {
        let mailbox = self.server(server_id).await?.mailbox;
        send(mailbox, op, auth).await
    }

    /// Get a server's actor, starting it if needed.
    async fn server(&self, server_id: ServerId) -> ApiResult<ServerEntry> {
        loop {
            let stopped = {
                let mut registry = self.registry.lock().await;
                match registry.servers.get(&server_id) {
                    Some(entry) if !entry.mailbox.is_closed() => {
                        return Ok(entry.clone());
                    }
                    // An actor that panicked is replaced while holding the
                    // registry, and its channel actors keep its state.
                    Some(entry) => {
                        let state = entry.state.clone();
                        *state.write().await =
                            ServerState::load(&self.ctx, server_id).await?;
                        let entry = self.spawn_server(server_id, state);
                        registry.servers.insert(server_id, entry.clone());
                        return Ok(entry);
                    }
                    None => registry.stopped_servers,
                }
            };
            // Loaded without holding the registry, so that unknown IDs and
            // slow loads never hold up other lookups. Only a server's actor
            // changes it, so what was loaded is still current unless an actor
            // started and then stopped meanwhile.
            let loaded = ServerState::load(&self.ctx, server_id).await?;
            let mut registry = self.registry.lock().await;
            match registry.servers.get(&server_id) {
                Some(entry) if !entry.mailbox.is_closed() => {
                    return Ok(entry.clone());
                }
                None if registry.stopped_servers == stopped => {
                    let entry = self
                        .spawn_server(server_id, Arc::new(RwLock::new(loaded)));
                    registry.servers.insert(server_id, entry.clone());
                    return Ok(entry);
                }
                _ => {}
            }
        }
    }

    /// Get a text channel's actor, starting it if needed.
    async fn channel(
        &self,
        text_channel_id: TextChannelId,
    ) -> ApiResult<ChannelMailbox> {
        if let Some(mailbox) =
            self.registry.lock().await.channels.get(&text_channel_id)
            && !mailbox.is_closed()
        {
            return Ok(mailbox.clone());
        }
        // Only needed to start the actor, and looked up without holding the
        // registry. A channel never moves to another server.
        let server_id = queries::channels::get_server_id_by_text_channel(
            &self.ctx.db_pool,
            text_channel_id,
        )
        .await?;
        let state = self.server(server_id).await?.state;
        // Checked without holding the registry, which a busy server operation
        // would otherwise hold up for every other actor. A channel deleted
        // right after this check gets an actor that answers `NotFound` and
        // leaves the registry.
        if !state.read().await.text_channels.contains(&text_channel_id) {
            return Err(ApiError::NotFound);
        }
        let mut registry = self.registry.lock().await;
        if let Some(mailbox) = registry.channels.get(&text_channel_id)
            && !mailbox.is_closed()
        {
            return Ok(mailbox.clone());
        }
        let mailbox = channel::spawn(
            self.ctx.clone(),
            self.registry.clone(),
            server_id,
            text_channel_id,
            state,
        );
        registry.channels.insert(text_channel_id, mailbox.clone());
        Ok(mailbox)
    }

    fn spawn_server(
        &self,
        server_id: ServerId,
        state: SharedState,
    ) -> ServerEntry {
        let mailbox = server::spawn(
            self.ctx.clone(),
            self.registry.clone(),
            server_id,
            state.clone(),
        );
        ServerEntry { mailbox, state }
    }
}

/// Queue an operation and wait for its result. The mailbox is dropped once the
/// operation is queued, which user actors rely on to tell when they are idle.
async fn send<Op, Output>(
    mailbox: mpsc::Sender<Envelope<Op, Output>>,
    op: Op,
    auth: Authorization,
) -> ApiResult<Output> {
    let (reply, result) = oneshot::channel();
    // Server and channel actors stop for good once their server or channel
    // is gone, and also when a server's state can no longer be trusted.
    mailbox
        .send(Envelope { op, auth, reply })
        .await
        .map_err(|_| ApiError::NotFound)?;
    drop(mailbox);
    // Later requests from the same connection may now be read.
    crate::admission::admit();
    result
        .await
        .map_err(|_| ApiError::Internal("Actor stopped unexpectedly".into()))?
}

fn unexpected(output: impl std::fmt::Debug) -> ApiError {
    ApiError::Internal(format!("Unexpected actor output: {output:?}"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use futures_util::future::join_all;
    use runelink_types::{
        capability::{Capability, NegotiatedCapabilities, preferred_version},
        channel::{NewServerChannel, ServerChannelContent, ServerChannelKind},
        ws::ClientWsEnvelope,
    };

    use super::*;
    use crate::auth::Requirement;

    struct Fixture {
        actors: Actors,
        pool: Arc<DbPool>,
        user: UserRef,
        updates: mpsc::UnboundedReceiver<ClientWsEnvelope>,
    }

    /// Unlike the query tests, actors commit their own transactions, so this
    /// creates a uniquely named user and deletes everything it made at the end.
    async fn fixture() -> Fixture {
        let url = std::env::var("DATABASE_URL")
            .expect("Set DATABASE_URL to a migrated test database");
        let pool = Arc::new(DbPool::connect(&url).await.unwrap());
        let config = ServerConfig {
            public_host_raw: "localhost".into(),
            database_url: url,
            public_port: 7000,
            bind_host: "127.0.0.1".into(),
            bind_port: 7000,
            secure: false,
            key_dir: PathBuf::new(),
        };
        let user = UserRef::new(
            format!("actors-test-{}", ServerId::new().as_uuid().simple()),
            config.public_host(),
        );
        sqlx::query(
            "INSERT INTO users (name, host, role) VALUES ($1, $2, 'user')",
        )
        .bind(&user.name)
        .bind(&user.host)
        .execute(&*pool)
        .await
        .unwrap();

        let clients = ClientWsManager::new();
        let (sender, updates) = mpsc::unbounded_channel();
        let capabilities: NegotiatedCapabilities = [
            Capability::ServersEvents,
            Capability::ChannelsEvents,
            Capability::MembershipsEvents,
            Capability::MessagesEvents,
            Capability::UsersEvents,
        ]
        .into_iter()
        .map(|capability| {
            let version = preferred_version(
                crate::capabilities::client_websocket,
                &capability,
            )
            .unwrap();
            (capability, version)
        })
        .collect();
        let conn_id = clients.register_connection(sender, capabilities).await;
        clients.authenticate_connection(conn_id, user.clone()).await;

        let actors = Actors::new(ActorCtx::new(
            pool.clone(),
            Arc::new(config),
            clients,
            FederationWsManager::new(),
        ));
        Fixture {
            actors,
            pool,
            user,
            updates,
        }
    }

    impl Fixture {
        fn received(&mut self) -> Vec<ClientWsUpdate> {
            std::iter::from_fn(|| self.updates.try_recv().ok())
                .filter_map(|envelope| match envelope {
                    ClientWsEnvelope::Update { update, .. } => Some(update),
                    _ => None,
                })
                .collect()
        }

        async fn text_channel(
            &self,
            server_id: ServerId,
            title: &str,
        ) -> (ServerChannelId, TextChannelId) {
            let channel = self
                .actors
                .create_channel(
                    server_id,
                    NewServerChannel {
                        kind: ServerChannelKind::Text,
                        title: title.into(),
                        description: None,
                        metadata: None,
                        parent_channel_id: None,
                    },
                    Authorization::host(),
                )
                .await
                .unwrap();
            let ServerChannelContent::Text { text_channel_id } =
                channel.content
            else {
                panic!("expected a text channel");
            };
            (channel.id, text_channel_id)
        }

        fn message(&self, body: String) -> NewMessage {
            NewMessage {
                author: self.user.clone(),
                body,
                reply_to: None,
                metadata: None,
            }
        }

        async fn cleanup(self) {
            for query in [
                "DELETE FROM users WHERE name = $1 AND host = $2",
                "DELETE FROM deleted_users WHERE name = $1 AND host = $2",
            ] {
                sqlx::query(query)
                    .bind(&self.user.name)
                    .bind(&self.user.host)
                    .execute(&*self.pool)
                    .await
                    .unwrap();
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL pointing to a migrated PostgreSQL database"]
    async fn updates_follow_commit_order_around_server_operations() {
        let mut f = fixture().await;
        let server = f
            .actors
            .create_server(
                NewServer {
                    title: "actors test".into(),
                    description: None,
                },
                f.user.clone(),
                Authorization::host(),
            )
            .await
            .unwrap();
        let (doomed, doomed_text) = f.text_channel(server.id, "doomed").await;
        let (_, kept_text) = f.text_channel(server.id, "kept").await;
        // The creator hears about the server before their own membership.
        assert!(matches!(
            f.received().as_slice(),
            [
                ClientWsUpdate::ServerUpserted(_),
                ClientWsUpdate::MembershipUpserted(_),
                ClientWsUpdate::ChannelUpserted(_),
                ClientWsUpdate::ChannelUpserted(_),
            ]
        ));

        // Messages race a deletion of one of their channels.
        let sends = (0..40).map(|index| {
            let text_channel_id = if index % 2 == 0 {
                doomed_text
            } else {
                kept_text
            };
            f.actors.create_message(
                text_channel_id,
                f.message(format!("message {index}")),
                Authorization::host(),
            )
        });
        let (sent, deleted) = tokio::join!(
            join_all(sends),
            f.actors
                .delete_channel(server.id, doomed, Authorization::host()),
        );
        deleted.unwrap();
        for (index, result) in sent.iter().enumerate() {
            match result {
                Ok(_) => {}
                Err(ApiError::NotFound) => assert_eq!(index % 2, 0),
                Err(error) => panic!("message {index} failed: {error:?}"),
            }
        }

        let received = f.received();
        let deleted_at = received
            .iter()
            .position(|update| {
                matches!(update, ClientWsUpdate::ChannelDeleted { channel_id, .. } if *channel_id == doomed)
            })
            .expect("channel deletion was fanned out");
        let mut last_created = HashMap::new();
        for (position, update) in received.iter().enumerate() {
            let ClientWsUpdate::MessageUpserted(message) = update else {
                continue;
            };
            if message.text_channel_id == doomed_text {
                assert!(
                    position < deleted_at,
                    "message after its channel was deleted"
                );
            }
            // Updates arrive in commit order within each channel.
            let last = last_created
                .insert(message.text_channel_id, message.created_at);
            assert!(last.is_none_or(|last| last <= message.created_at));
        }
        let delivered = received
            .iter()
            .filter(|update| {
                matches!(update, ClientWsUpdate::MessageUpserted(_))
            })
            .count();
        assert_eq!(
            delivered,
            sent.iter().filter(|result| result.is_ok()).count()
        );

        f.actors
            .delete_server(server.id, Authorization::host())
            .await
            .unwrap();
        assert!(matches!(
            f.actors
                .create_message(
                    kept_text,
                    f.message("late".into()),
                    Authorization::host(),
                )
                .await,
            Err(ApiError::NotFound)
        ));
        let registry = f.actors.registry.lock().await;
        assert!(registry.servers.is_empty() && registry.channels.is_empty());
        drop(registry);
        f.cleanup().await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL pointing to a migrated PostgreSQL database"]
    async fn actors_check_authorization_against_their_own_state() {
        let f = fixture().await;
        let server = f
            .actors
            .create_server(
                NewServer {
                    title: "actors test".into(),
                    description: None,
                },
                f.user.clone(),
                Authorization::host(),
            )
            .await
            .unwrap();
        let (_, text_channel_id) = f.text_channel(server.id, "general").await;
        let as_member = || {
            Authorization::for_test_user(
                f.user.clone(),
                Requirement::ServerMember(server.id),
            )
        };
        f.actors
            .create_message(
                text_channel_id,
                f.message("member".into()),
                as_member(),
            )
            .await
            .unwrap();

        f.actors
            .delete_membership(server.id, f.user.clone(), Authorization::host())
            .await
            .unwrap();
        let result = f
            .actors
            .create_message(
                text_channel_id,
                f.message("former member".into()),
                as_member(),
            )
            .await;
        assert!(matches!(result, Err(ApiError::AuthError(_))), "{result:?}");

        f.actors
            .delete_server(server.id, Authorization::host())
            .await
            .unwrap();
        f.cleanup().await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL pointing to a migrated PostgreSQL database"]
    async fn deleting_a_user_removes_their_memberships_first() {
        let mut f = fixture().await;
        let server = f
            .actors
            .create_server(
                NewServer {
                    title: "actors test".into(),
                    description: None,
                },
                f.user.clone(),
                Authorization::host(),
            )
            .await
            .unwrap();
        f.actors
            .delete_home_user(f.user.clone(), Authorization::host())
            .await
            .unwrap();
        assert!(matches!(
            f.received().as_slice(),
            [
                ClientWsUpdate::ServerUpserted(_),
                ClientWsUpdate::MembershipUpserted(_),
                ClientWsUpdate::MembershipDeleted { .. },
                ClientWsUpdate::UserDeleted { .. },
            ]
        ));
        let registry = f.actors.registry.lock().await;
        let state = registry.servers[&server.id].state.read().await;
        assert!(state.members.is_empty());
        drop(state);
        drop(registry);
        // The name stays reserved.
        let recreated = f
            .actors
            .create_user(
                NewUser {
                    name: f.user.name.clone(),
                    host: f.user.host.clone(),
                    role: runelink_types::user::UserRole::User,
                },
                None,
                Authorization::host(),
            )
            .await;
        assert!(
            matches!(recreated, Err(ApiError::Conflict(_))),
            "{recreated:?}"
        );

        f.actors
            .delete_server(server.id, Authorization::host())
            .await
            .unwrap();
        f.cleanup().await;
    }
}
