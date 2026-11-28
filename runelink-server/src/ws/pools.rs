#![allow(dead_code)]

use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use runelink_types::{
    capability::{
        NegotiatedCapabilities, VersionedCapability, preferred_version,
        supports,
    },
    user::UserRef,
    ws::{ClientWsEnvelope, FederationWsEnvelope},
};
use time::OffsetDateTime;
use tokio::sync::{RwLock, mpsc};

use runelink_types::host::Host;

use crate::{ids::ConnId, presence::Presence};

/// Tracks active client websocket connections and provides safe send helpers.
/// It tells presence whenever a connection starts or stops being
/// authenticated as a user.
#[derive(Clone, Debug)]
pub struct ClientWsPool {
    inner: Arc<RwLock<ClientPoolState>>,
    presence: Presence,
}

#[derive(Debug, Default)]
struct ClientPoolState {
    connections: HashMap<ConnId, ClientConn>,
    by_user: HashMap<UserRef, HashSet<ConnId>>,
}

#[derive(Clone, Debug)]
pub struct ClientConn {
    pub sender: mpsc::UnboundedSender<ClientWsEnvelope>,
    pub user_ref: Option<UserRef>,
    pub connected_at: OffsetDateTime,
    pub capabilities: NegotiatedCapabilities,
}

impl ClientWsPool {
    /// Creates a new client websocket pool.
    pub fn new(presence: Presence) -> Self {
        Self {
            inner: Arc::default(),
            presence,
        }
    }

    /// Registers a new connection with the pool.
    pub async fn register_connection(
        &self,
        conn_id: ConnId,
        sender: mpsc::UnboundedSender<ClientWsEnvelope>,
        capabilities: NegotiatedCapabilities,
    ) {
        let mut state = self.inner.write().await;
        self.remove_client_connection(&mut state, conn_id);
        state.connections.insert(
            conn_id,
            ClientConn {
                sender,
                user_ref: None,
                connected_at: OffsetDateTime::now_utc(),
                capabilities,
            },
        );
    }

    /// Authenticates a connection for a given user.
    pub async fn authenticate_connection(
        &self,
        conn_id: ConnId,
        user_ref: UserRef,
    ) -> bool {
        let mut state = self.inner.write().await;
        let old_user = match state.connections.get_mut(&conn_id) {
            Some(conn) => conn.user_ref.replace(user_ref.clone()),
            None => return false,
        };
        if old_user.as_ref() == Some(&user_ref) {
            return true;
        }
        if let Some(previous_user) = old_user {
            Self::remove_conn_from_user_index(
                &mut state.by_user,
                &previous_user,
                conn_id,
            );
            self.presence.disconnected(conn_id, previous_user);
        }
        state
            .by_user
            .entry(user_ref.clone())
            .or_default()
            .insert(conn_id);
        self.presence.connected(conn_id, user_ref);
        true
    }

    /// Deregisters a connection from the pool.
    pub async fn deregister_connection(&self, conn_id: ConnId) -> bool {
        let mut state = self.inner.write().await;
        self.remove_client_connection(&mut state, conn_id)
    }

    /// Removes authentication from a connection.
    pub async fn deauthenticate_connection(&self, conn_id: ConnId) {
        let mut state = self.inner.write().await;
        if let Some(user_ref) = state
            .connections
            .get_mut(&conn_id)
            .and_then(|conn| conn.user_ref.take())
        {
            Self::remove_conn_from_user_index(
                &mut state.by_user,
                &user_ref,
                conn_id,
            );
            self.presence.disconnected(conn_id, user_ref);
        }
    }

    /// Removes authentication from all connections for a user.
    pub async fn deauthenticate_user(&self, user_ref: &UserRef) {
        let mut state = self.inner.write().await;
        let conn_ids = state.by_user.remove(user_ref).unwrap_or_default();
        for conn_id in conn_ids {
            if let Some(conn) = state.connections.get_mut(&conn_id) {
                conn.user_ref = None;
            }
            self.presence.disconnected(conn_id, user_ref.clone());
        }
    }

    /// Sends an envelope to the active connection for the given connection ID.
    pub async fn send_to_connection(
        &self,
        conn_id: ConnId,
        envelope: ClientWsEnvelope,
    ) -> bool {
        let sender = {
            let state = self.inner.read().await;
            state
                .connections
                .get(&conn_id)
                .filter(|conn| client_supports_envelope(conn, &envelope))
                .map(|conn| conn.sender.clone())
        };
        let Some(sender) = sender else {
            return false;
        };
        if sender.send(envelope).is_ok() {
            return true;
        }
        let _ = self.deregister_connection(conn_id).await;
        false
    }

    /// Sends an envelope to a connection only while it is authenticated as
    /// `user_ref`.
    pub async fn send_to_connection_as(
        &self,
        conn_id: ConnId,
        user_ref: &UserRef,
        envelope: ClientWsEnvelope,
    ) -> bool {
        let state = self.inner.read().await;
        state
            .connections
            .get(&conn_id)
            .filter(|conn| {
                conn.user_ref.as_ref() == Some(user_ref)
                    && client_supports_envelope(conn, &envelope)
            })
            // A closed connection is deregistered as its loop ends.
            .is_some_and(|conn| conn.sender.send(envelope).is_ok())
    }

    /// Returns the authenticated user for a connection, if any.
    pub async fn authenticated_user_ref(
        &self,
        conn_id: ConnId,
    ) -> Option<UserRef> {
        let state = self.inner.read().await;
        state
            .connections
            .get(&conn_id)
            .and_then(|conn| conn.user_ref.clone())
    }

    pub async fn supports_capability(
        &self,
        conn_id: ConnId,
        capability: &VersionedCapability,
    ) -> bool {
        let state = self.inner.read().await;
        state
            .connections
            .get(&conn_id)
            .is_some_and(|conn| supports(&conn.capabilities, capability))
    }

    /// Sends an envelope to the active connections for the given users. It is
    /// queued while the pool is locked, so it can't reach a connection that
    /// authenticates as someone else meanwhile.
    pub async fn send_to_users<I, S>(
        &self,
        users: I,
        envelope: ClientWsEnvelope,
    ) -> usize
    where
        I: IntoIterator<Item = S>,
        S: Borrow<UserRef>,
    {
        let users = users
            .into_iter()
            .map(|user| user.borrow().clone())
            .collect::<HashSet<UserRef>>();
        let (sent, stale) = {
            let state = self.inner.read().await;
            let conn_ids = users
                .iter()
                .filter_map(|user| state.by_user.get(user))
                .flatten()
                .copied()
                .collect::<HashSet<_>>();
            Self::send_to_many(&state, conn_ids, &envelope)
        };
        self.remove_stale(stale).await;
        sent
    }

    /// Broadcasts an envelope to all active connections.
    pub async fn broadcast(&self, envelope: ClientWsEnvelope) -> usize {
        let (sent, stale) = {
            let state = self.inner.read().await;
            let conn_ids =
                state.connections.keys().copied().collect::<Vec<_>>();
            Self::send_to_many(&state, conn_ids, &envelope)
        };
        self.remove_stale(stale).await;
        sent
    }

    /// Queues an envelope on the connections that support it, returning how
    /// many it was queued on and the closed ones.
    fn send_to_many(
        state: &ClientPoolState,
        conn_ids: impl IntoIterator<Item = ConnId>,
        envelope: &ClientWsEnvelope,
    ) -> (usize, Vec<ConnId>) {
        let mut sent = 0;
        let mut stale = Vec::new();
        for conn_id in conn_ids {
            let Some(conn) = state.connections.get(&conn_id) else {
                continue;
            };
            if !client_supports_envelope(conn, envelope) {
                continue;
            }
            if conn.sender.send(envelope.clone()).is_ok() {
                sent += 1;
            } else {
                stale.push(conn_id);
            }
        }
        (sent, stale)
    }

    async fn remove_stale(&self, stale: Vec<ConnId>) {
        if stale.is_empty() {
            return;
        }
        let mut state = self.inner.write().await;
        for conn_id in stale {
            let _ = self.remove_client_connection(&mut state, conn_id);
        }
    }

    fn remove_conn_from_user_index(
        by_user: &mut HashMap<UserRef, HashSet<ConnId>>,
        user_ref: &UserRef,
        conn_id: ConnId,
    ) {
        if let Some(conn_ids) = by_user.get_mut(user_ref) {
            conn_ids.remove(&conn_id);
            if conn_ids.is_empty() {
                by_user.remove(user_ref);
            }
        }
    }

    fn remove_client_connection(
        &self,
        state: &mut ClientPoolState,
        conn_id: ConnId,
    ) -> bool {
        let Some(connection) = state.connections.remove(&conn_id) else {
            return false;
        };
        if let Some(user_ref) = connection.user_ref {
            Self::remove_conn_from_user_index(
                &mut state.by_user,
                &user_ref,
                conn_id,
            );
            self.presence.disconnected(conn_id, user_ref);
        }
        true
    }
}

/// What happened to an envelope sent to a host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostSend {
    Sent,
    /// The host's connection doesn't support the envelope.
    Unsupported,
    NotConnected,
}

/// Tracks active federation websocket connections and provides safe send
/// helpers.
///
/// Each host has one connection that messages to it are sent on. Two hosts
/// may connect to each other at once, so a host can have other connections
/// too, which stay open and are read. The most recently authenticated
/// connection sends: a host only connects again once it has lost its
/// connection, which may not have closed on this side yet. When the sending
/// connection closes, the most recently authenticated remaining one takes
/// over.
///
/// Messages already queued on a connection that stops sending can arrive
/// after later ones sent on its replacement. Between two working connections,
/// that only happens when two hosts first connect to each other at once.
#[derive(Clone, Debug, Default)]
pub struct FederationWsPool {
    inner: Arc<RwLock<FederationPoolState>>,
}

#[derive(Debug, Default)]
struct FederationPoolState {
    connections: HashMap<ConnId, FederationConn>,
    by_host: HashMap<Host, ConnId>,
    /// Counts authentications, to order connections by when they
    /// authenticated.
    authentications: u64,
}

impl FederationPoolState {
    /// The connection that a host is sent messages on, if any.
    fn host_conn(&self, host: &Host) -> Option<ConnId> {
        self.by_host.get(host).copied()
    }
}

#[derive(Clone, Debug)]
pub struct FederationConn {
    pub sender: mpsc::UnboundedSender<FederationWsEnvelope>,
    pub host: Option<Host>,
    /// When the connection authenticated, from the pool's count.
    authenticated: u64,
    pub capabilities: NegotiatedCapabilities,
}

impl FederationWsPool {
    /// Creates a new federation websocket pool.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new connection with the pool.
    pub async fn register_connection(
        &self,
        conn_id: ConnId,
        sender: mpsc::UnboundedSender<FederationWsEnvelope>,
        capabilities: NegotiatedCapabilities,
    ) {
        let mut state = self.inner.write().await;
        Self::remove_federation_connection(&mut state, conn_id);
        state.connections.insert(
            conn_id,
            FederationConn {
                sender,
                host: None,
                authenticated: 0,
                capabilities,
            },
        );
    }

    /// Authenticates a connection for a given host.
    pub async fn authenticate_connection(
        &self,
        conn_id: ConnId,
        host: Host,
    ) -> bool {
        let mut state = self.inner.write().await;
        let previous_host_for_conn = match state.connections.get(&conn_id) {
            Some(conn) => conn.host.clone(),
            None => return false,
        };

        if let Some(previous_host) = &previous_host_for_conn {
            Self::release_host(&mut state, previous_host, conn_id);
        }

        state.authentications += 1;
        let authenticated = state.authentications;
        state.by_host.insert(host.clone(), conn_id);
        if let Some(conn) = state.connections.get_mut(&conn_id) {
            conn.host = Some(host);
            conn.authenticated = authenticated;
        }
        true
    }

    pub async fn deregister_connection(&self, conn_id: ConnId) -> bool {
        let mut state = self.inner.write().await;
        Self::remove_federation_connection(&mut state, conn_id)
    }

    /// Sends a response to a request from `host` that arrived on `conn_id`.
    /// It goes out on the connection that the host is sent updates on, so that
    /// it follows the updates its request caused, even if the request's own
    /// connection has closed since.
    pub async fn send_response(
        &self,
        conn_id: ConnId,
        host: Option<&Host>,
        envelope: FederationWsEnvelope,
    ) -> bool {
        let target = {
            let state = self.inner.read().await;
            let conn_id = host
                .and_then(|host| state.host_conn(host))
                .unwrap_or(conn_id);
            state
                .connections
                .get(&conn_id)
                .map(|conn| (conn_id, conn.sender.clone()))
        };

        let Some((conn_id, sender)) = target else {
            return false;
        };

        if sender.send(envelope).is_ok() {
            return true;
        }

        let _ = self.deregister_connection(conn_id).await;
        false
    }

    /// Returns the authenticated host for a connection, if any.
    pub async fn authenticated_host(&self, conn_id: ConnId) -> Option<Host> {
        let state = self.inner.read().await;
        state
            .connections
            .get(&conn_id)
            .and_then(|conn| conn.host.clone())
    }

    pub async fn supports_capability(
        &self,
        conn_id: ConnId,
        capability: &VersionedCapability,
    ) -> bool {
        let state = self.inner.read().await;
        state
            .connections
            .get(&conn_id)
            .is_some_and(|conn| supports(&conn.capabilities, capability))
    }

    /// Returns whether the given host currently has an authenticated connection.
    pub async fn has_host(&self, host: &Host) -> bool {
        let state = self.inner.read().await;
        let Some(conn_id) = state.host_conn(host) else {
            return false;
        };
        state.connections.contains_key(&conn_id)
    }

    pub async fn host_supports_capability(
        &self,
        host: &Host,
        capability: &VersionedCapability,
    ) -> bool {
        let state = self.inner.read().await;
        let Some(conn_id) = state.host_conn(host) else {
            return false;
        };
        state
            .connections
            .get(&conn_id)
            .is_some_and(|conn| supports(&conn.capabilities, capability))
    }

    /// Sends an envelope to the active connection for the given host, or to
    /// the next one while that has closed.
    pub async fn send_to_host(
        &self,
        host: &Host,
        mut envelope: FederationWsEnvelope,
    ) -> HostSend {
        loop {
            let (conn_id, sender) = {
                let state = self.inner.read().await;
                let Some(conn_id) = state.host_conn(host) else {
                    return HostSend::NotConnected;
                };
                let Some(conn) = state.connections.get(&conn_id) else {
                    return HostSend::NotConnected;
                };
                if !conn.sender.is_closed()
                    && !federation_supports_envelope(conn, &envelope)
                {
                    return HostSend::Unsupported;
                }
                (conn_id, conn.sender.clone())
            };
            match sender.send(envelope) {
                Ok(()) => return HostSend::Sent,
                Err(error) => envelope = error.0,
            }
            let _ = self.deregister_connection(conn_id).await;
        }
    }

    /// Sends an envelope to every authenticated host, on the connection each
    /// is sent messages on.
    pub async fn broadcast(&self, envelope: FederationWsEnvelope) -> usize {
        let targets = {
            let state = self.inner.read().await;
            state
                .by_host
                .values()
                .filter_map(|conn_id| {
                    state
                        .connections
                        .get(conn_id)
                        .filter(|conn| {
                            federation_supports_envelope(conn, &envelope)
                        })
                        .map(|conn| (*conn_id, conn.sender.clone()))
                })
                .collect::<Vec<_>>()
        };
        Self::send_to_many_federation(targets, envelope, self).await
    }

    /// Stop sending to `host` on a connection, switching to the host's most
    /// recently authenticated other connection if it has one.
    fn release_host(
        state: &mut FederationPoolState,
        host: &Host,
        conn_id: ConnId,
    ) {
        if state.by_host.get(host) != Some(&conn_id) {
            return;
        }
        let next = state
            .connections
            .iter()
            .filter(|(id, conn)| {
                **id != conn_id && conn.host.as_ref() == Some(host)
            })
            .max_by_key(|(_, conn)| conn.authenticated)
            .map(|(id, _)| *id);
        match next {
            Some(next) => state.by_host.insert(host.clone(), next),
            None => state.by_host.remove(host),
        };
    }

    fn remove_federation_connection(
        state: &mut FederationPoolState,
        conn_id: ConnId,
    ) -> bool {
        let Some(connection) = state.connections.remove(&conn_id) else {
            return false;
        };
        if let Some(host) = connection.host {
            Self::release_host(state, &host, conn_id);
        }
        true
    }

    async fn send_to_many_federation(
        targets: Vec<(ConnId, mpsc::UnboundedSender<FederationWsEnvelope>)>,
        envelope: FederationWsEnvelope,
        pool: &FederationWsPool,
    ) -> usize {
        let mut sent = 0usize;
        let mut stale = Vec::<ConnId>::new();
        for (conn_id, sender) in targets {
            if sender.send(envelope.clone()).is_ok() {
                sent += 1;
            } else {
                stale.push(conn_id);
            }
        }
        if !stale.is_empty() {
            let stale_set = stale.into_iter().collect::<HashSet<_>>();
            let mut state = pool.inner.write().await;
            for conn_id in stale_set {
                let _ = Self::remove_federation_connection(&mut state, conn_id);
            }
        }
        sent
    }
}

fn client_supports_envelope(
    conn: &ClientConn,
    envelope: &ClientWsEnvelope,
) -> bool {
    let ClientWsEnvelope::Update { update, .. } = envelope else {
        return true;
    };
    let capability = update.capability();
    preferred_version(crate::capabilities::client_websocket, &capability)
        .is_some_and(|version| {
            supports(&conn.capabilities, &capability.with_version(version))
        })
}

fn federation_supports_envelope(
    conn: &FederationConn,
    envelope: &FederationWsEnvelope,
) -> bool {
    let capability = match envelope {
        FederationWsEnvelope::Request { request, .. } => request.capability(),
        FederationWsEnvelope::Update { update, .. } => update.capability(),
        _ => return true,
    };
    preferred_version(crate::capabilities::federation_websocket, &capability)
        .is_some_and(|version| {
            supports(&conn.capabilities, &capability.with_version(version))
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use runelink_types::{
        capability::{
            Capability,
            versions::{messages, presence},
        },
        ids::{MessageId, TextChannelId},
        presence::ChosenPresenceStatus,
        user::UserRef,
        ws::{ClientWsEnvelope, ClientWsUpdate, FederationWsEnvelope, WsError},
    };
    use tokio::sync::mpsc;

    use super::{ClientWsPool, FederationWsPool, HostSend};
    use crate::ids::ConnId;
    use runelink_types::host::Host;

    #[tokio::test]
    async fn newer_federation_connections_send_for_their_host() {
        let pool = FederationWsPool::new();
        let host = Host::parse("example.com").unwrap();
        let mut connections = Vec::new();
        for _ in 0..2 {
            let (sender, receiver) = mpsc::unbounded_channel();
            let conn_id = ConnId::new();
            pool.register_connection(conn_id, sender, BTreeMap::new())
                .await;
            assert!(pool.authenticate_connection(conn_id, host.clone()).await);
            connections.push((conn_id, receiver));
        }
        let (older, older_receiver) = &connections[0];
        let (newer, _) = &connections[1];
        let sending =
            || async { pool.inner.read().await.by_host.get(&host).copied() };
        assert!(pool.has_host(&host).await);
        assert_eq!(sending().await, Some(*newer));
        // The older connection stays open.
        assert!(!older_receiver.is_closed());

        pool.deregister_connection(*newer).await;
        assert_eq!(sending().await, Some(*older));
        pool.deregister_connection(*older).await;
        assert!(!pool.has_host(&host).await);
    }

    #[tokio::test]
    async fn federation_sends_skip_closed_connections() {
        let pool = FederationWsPool::new();
        let host = Host::parse("example.com").unwrap();
        let mut receivers = Vec::new();
        for _ in 0..2 {
            let (sender, receiver) = mpsc::unbounded_channel();
            let conn_id = ConnId::new();
            pool.register_connection(conn_id, sender, BTreeMap::new())
                .await;
            pool.authenticate_connection(conn_id, host.clone()).await;
            receivers.push(receiver);
        }
        let envelope = FederationWsEnvelope::Error {
            request_id: None,
            event_id: runelink_types::EventId::new(),
            error: WsError {
                code: "test".into(),
                message: "test".into(),
                details: None,
            },
        };
        // The newer connection sends, but has closed.
        drop(receivers.pop());
        assert_eq!(
            pool.send_to_host(&host, envelope.clone()).await,
            HostSend::Sent
        );
        assert_eq!(receivers[0].try_recv(), Ok(envelope.clone()));
        drop(receivers.pop());
        assert_eq!(
            pool.send_to_host(&host, envelope).await,
            HostSend::NotConnected
        );
        assert!(!pool.has_host(&host).await);
    }

    #[tokio::test]
    async fn client_updates_only_reach_negotiated_capabilities() {
        let pool = ClientWsPool::new(crate::presence::Presence::new().0);
        let (subscribed_tx, mut subscribed_rx) = mpsc::unbounded_channel();
        let (unsubscribed_tx, mut unsubscribed_rx) = mpsc::unbounded_channel();
        pool.register_connection(
            ConnId::new(),
            subscribed_tx,
            BTreeMap::from([(
                Capability::MessagesEvents,
                messages::EVENTS_VERSION,
            )]),
        )
        .await;
        let unsubscribed_id = ConnId::new();
        pool.register_connection(
            unsubscribed_id,
            unsubscribed_tx,
            BTreeMap::new(),
        )
        .await;
        let envelope = ClientWsEnvelope::Update {
            event_id: runelink_types::EventId::new(),
            update: ClientWsUpdate::MessageUpserted(
                runelink_types::message::Message {
                    id: MessageId::new(),
                    host: runelink_types::host::Host::parse("localhost")
                        .unwrap(),
                    text_channel_id: TextChannelId::new(),
                    author: None,
                    reply_to: None,
                    metadata: None,
                    content: runelink_types::message::MessageContent::Deleted {
                        body: (),
                        deleted_at: time::OffsetDateTime::now_utc(),
                    },
                    created_at: time::OffsetDateTime::now_utc(),
                    updated_at: time::OffsetDateTime::now_utc(),
                    edited_at: None,
                },
            ),
        };

        assert!(
            !pool
                .send_to_connection(unsubscribed_id, envelope.clone())
                .await
        );
        assert_eq!(pool.broadcast(envelope.clone()).await, 1);
        assert_eq!(subscribed_rx.recv().await, Some(envelope));
        assert!(unsubscribed_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn updates_for_a_user_never_reach_the_next_user_of_a_connection() {
        let pool = ClientWsPool::new(crate::presence::Presence::new().0);
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let conn_id = ConnId::new();
        pool.register_connection(
            conn_id,
            sender,
            BTreeMap::from([(
                Capability::PresenceStatus,
                presence::STATUS_VERSION,
            )]),
        )
        .await;
        let user = |name: &str| {
            UserRef::new(name.into(), Host::parse("localhost").unwrap())
        };
        let update = |status| ClientWsEnvelope::Update {
            event_id: runelink_types::EventId::new(),
            update: ClientWsUpdate::ChosenPresenceStatusUpdated { status },
        };
        pool.authenticate_connection(conn_id, user("alice")).await;
        pool.authenticate_connection(conn_id, user("bob")).await;
        let alices = update(ChosenPresenceStatus::Invisible);
        assert!(
            !pool
                .send_to_connection_as(conn_id, &user("alice"), alices.clone())
                .await
        );
        assert_eq!(pool.send_to_users([user("alice")], alices).await, 0);
        let bobs = update(ChosenPresenceStatus::Online);
        assert!(
            pool.send_to_connection_as(conn_id, &user("bob"), bobs.clone())
                .await
        );
        assert_eq!(receiver.try_recv().unwrap(), bobs);
        assert!(receiver.try_recv().is_err());
    }
}
