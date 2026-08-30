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

use crate::ids::ConnId;

/// Tracks active client websocket connections and provides safe send helpers.
#[derive(Clone, Debug, Default)]
pub struct ClientWsPool {
    inner: Arc<RwLock<ClientPoolState>>,
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new connection with the pool.
    pub async fn register_connection(
        &self,
        conn_id: ConnId,
        sender: mpsc::UnboundedSender<ClientWsEnvelope>,
        capabilities: NegotiatedCapabilities,
    ) {
        let mut state = self.inner.write().await;
        if let Some(previous) = state.connections.remove(&conn_id) {
            if let Some(previous_user) = previous.user_ref {
                Self::remove_conn_from_user_index(
                    &mut state.by_user,
                    &previous_user,
                    conn_id,
                );
            }
        }
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
        if let Some(previous_user) = old_user {
            Self::remove_conn_from_user_index(
                &mut state.by_user,
                &previous_user,
                conn_id,
            );
        }
        state.by_user.entry(user_ref).or_default().insert(conn_id);
        true
    }

    /// Deregisters a connection from the pool.
    pub async fn deregister_connection(&self, conn_id: ConnId) -> bool {
        let mut state = self.inner.write().await;
        Self::remove_client_connection(&mut state, conn_id)
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

    pub async fn send_to_user(
        &self,
        user_ref: &UserRef,
        envelope: ClientWsEnvelope,
    ) -> usize {
        let targets = {
            let state = self.inner.read().await;
            state
                .by_user
                .get(user_ref)
                .into_iter()
                .flat_map(|conn_ids| conn_ids.iter())
                .filter_map(|conn_id| {
                    state
                        .connections
                        .get(conn_id)
                        .filter(|conn| {
                            client_supports_envelope(conn, &envelope)
                        })
                        .map(|conn| (*conn_id, conn.sender.clone()))
                })
                .collect::<Vec<_>>()
        };
        Self::send_to_many_client(targets, envelope, self).await
    }

    /// Sends an envelope to the active connections for the given users.
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
        let targets = {
            let state = self.inner.read().await;
            let mut conn_ids = HashSet::new();
            for user in users {
                if let Some(user_conn_ids) = state.by_user.get(&user) {
                    conn_ids.extend(user_conn_ids.iter().copied());
                }
            }
            conn_ids
                .into_iter()
                .filter_map(|conn_id| {
                    state
                        .connections
                        .get(&conn_id)
                        .filter(|conn| {
                            client_supports_envelope(conn, &envelope)
                        })
                        .map(|conn| (conn_id, conn.sender.clone()))
                })
                .collect::<Vec<_>>()
        };
        Self::send_to_many_client(targets, envelope, self).await
    }

    /// Broadcasts an envelope to all active connections.
    pub async fn broadcast(&self, envelope: ClientWsEnvelope) -> usize {
        let targets = {
            let state = self.inner.read().await;
            state
                .connections
                .iter()
                .filter(|(_, conn)| client_supports_envelope(conn, &envelope))
                .map(|(conn_id, conn)| (*conn_id, conn.sender.clone()))
                .collect::<Vec<_>>()
        };
        Self::send_to_many_client(targets, envelope, self).await
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
        }
        true
    }

    async fn send_to_many_client(
        targets: Vec<(ConnId, mpsc::UnboundedSender<ClientWsEnvelope>)>,
        envelope: ClientWsEnvelope,
        pool: &ClientWsPool,
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
                let _ = Self::remove_client_connection(&mut state, conn_id);
            }
        }
        sent
    }
}

/// Tracks active federation websocket connections and provides safe send
/// helpers. Maintains one authenticated federation connection per host and
/// manages safe message delivery.
#[derive(Clone, Debug, Default)]
pub struct FederationWsPool {
    inner: Arc<RwLock<FederationPoolState>>,
}

#[derive(Debug, Default)]
struct FederationPoolState {
    connections: HashMap<ConnId, FederationConn>,
    by_host: HashMap<String, ConnId>,
}

#[derive(Clone, Debug)]
pub struct FederationConn {
    pub sender: mpsc::UnboundedSender<FederationWsEnvelope>,
    pub host: Option<String>,
    pub issuer: Option<String>,
    pub connected_at: OffsetDateTime,
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
        if let Some(previous) = state.connections.remove(&conn_id) {
            if let Some(previous_host) = previous.host {
                Self::remove_conn_from_host_index(
                    &mut state.by_host,
                    &previous_host,
                    conn_id,
                );
            }
        }

        state.connections.insert(
            conn_id,
            FederationConn {
                sender,
                host: None,
                issuer: None,
                connected_at: OffsetDateTime::now_utc(),
                capabilities,
            },
        );
    }

    /// Authenticates a connection for a given host.
    pub async fn authenticate_connection(
        &self,
        conn_id: ConnId,
        host: String,
        issuer: String,
    ) -> bool {
        let mut state = self.inner.write().await;
        let previous_host_for_conn = match state.connections.get(&conn_id) {
            Some(conn) => conn.host.clone(),
            None => return false,
        };

        if let Some(previous_host) = previous_host_for_conn.as_deref() {
            Self::remove_conn_from_host_index(
                &mut state.by_host,
                previous_host,
                conn_id,
            );
        }

        if let Some(existing_conn_id) = state.by_host.get(&host).copied() {
            if existing_conn_id != conn_id {
                let _ = Self::remove_federation_connection(
                    &mut state,
                    existing_conn_id,
                );
            }
        }

        state.by_host.insert(host.clone(), conn_id);
        if let Some(conn) = state.connections.get_mut(&conn_id) {
            conn.host = Some(host);
            conn.issuer = Some(issuer);
        }
        true
    }

    pub async fn deregister_connection(&self, conn_id: ConnId) -> bool {
        let mut state = self.inner.write().await;
        Self::remove_federation_connection(&mut state, conn_id)
    }

    /// Sends an envelope to the active connection for the given connection ID.
    pub async fn send_to_connection(
        &self,
        conn_id: ConnId,
        envelope: FederationWsEnvelope,
    ) -> bool {
        let sender = {
            let state = self.inner.read().await;
            state
                .connections
                .get(&conn_id)
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

    /// Returns the authenticated host for a connection, if any.
    pub async fn authenticated_host(&self, conn_id: ConnId) -> Option<String> {
        let state = self.inner.read().await;
        state
            .connections
            .get(&conn_id)
            .and_then(|conn| conn.host.clone())
    }

    pub async fn authenticated_issuer(
        &self,
        conn_id: ConnId,
    ) -> Option<String> {
        let state = self.inner.read().await;
        state
            .connections
            .get(&conn_id)
            .and_then(|conn| conn.issuer.clone())
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
    pub async fn has_host(&self, host: &str) -> bool {
        let state = self.inner.read().await;
        let Some(conn_id) = state.by_host.get(host).copied() else {
            return false;
        };
        state.connections.contains_key(&conn_id)
    }

    pub async fn host_supports_capability(
        &self,
        host: &str,
        capability: &VersionedCapability,
    ) -> bool {
        let state = self.inner.read().await;
        let Some(conn_id) = state.by_host.get(host) else {
            return false;
        };
        state
            .connections
            .get(conn_id)
            .is_some_and(|conn| supports(&conn.capabilities, capability))
    }

    /// Sends an envelope to the active connection for the given host.
    pub async fn send_to_host(
        &self,
        host: &str,
        envelope: FederationWsEnvelope,
    ) -> bool {
        let target = {
            let state = self.inner.read().await;
            let Some(conn_id) = state.by_host.get(host).copied() else {
                return false;
            };
            state
                .connections
                .get(&conn_id)
                .filter(|conn| federation_supports_envelope(conn, &envelope))
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

    /// Sends an envelope to the active connection for the given hosts.
    pub async fn send_to_hosts<I, S>(
        &self,
        hosts: I,
        envelope: FederationWsEnvelope,
    ) -> usize
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let hosts = hosts
            .into_iter()
            .map(|host| host.as_ref().to_owned())
            .collect::<HashSet<String>>();
        let targets = {
            let state = self.inner.read().await;
            let mut out = Vec::new();
            for host in hosts {
                let Some(conn_id) = state.by_host.get(&host).copied() else {
                    continue;
                };
                if let Some(conn) =
                    state.connections.get(&conn_id).filter(|conn| {
                        federation_supports_envelope(conn, &envelope)
                    })
                {
                    let sender = conn.sender.clone();
                    out.push((conn_id, sender));
                }
            }
            out
        };
        Self::send_to_many_federation(targets, envelope, self).await
    }

    /// Sends an envelope to all active connections.
    pub async fn broadcast(&self, envelope: FederationWsEnvelope) -> usize {
        let targets = {
            let state = self.inner.read().await;
            state
                .connections
                .iter()
                .filter(|(_, conn)| {
                    federation_supports_envelope(conn, &envelope)
                })
                .map(|(conn_id, conn)| (*conn_id, conn.sender.clone()))
                .collect::<Vec<_>>()
        };
        Self::send_to_many_federation(targets, envelope, self).await
    }

    fn remove_conn_from_host_index(
        by_host: &mut HashMap<String, ConnId>,
        host: &str,
        conn_id: ConnId,
    ) {
        if by_host.get(host).copied() == Some(conn_id) {
            by_host.remove(host);
        }
    }

    fn remove_federation_connection(
        state: &mut FederationPoolState,
        conn_id: ConnId,
    ) -> bool {
        let Some(connection) = state.connections.remove(&conn_id) else {
            return false;
        };
        if let Some(host) = connection.host {
            Self::remove_conn_from_host_index(
                &mut state.by_host,
                &host,
                conn_id,
            );
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
        capability::Capability,
        ids::{ChannelId, MessageId, ServerId},
        ws::{ClientWsEnvelope, ClientWsUpdate},
    };
    use tokio::sync::mpsc;

    use super::ClientWsPool;
    use crate::ids::ConnId;

    #[tokio::test]
    async fn client_updates_only_reach_negotiated_capabilities() {
        let pool = ClientWsPool::new();
        let (subscribed_tx, mut subscribed_rx) = mpsc::unbounded_channel();
        let (unsubscribed_tx, mut unsubscribed_rx) = mpsc::unbounded_channel();
        pool.register_connection(
            ConnId::new(),
            subscribed_tx,
            BTreeMap::from([(Capability::MessagesEvents, 1)]),
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
            update: ClientWsUpdate::MessageDeleted {
                server_id: ServerId::new(),
                channel_id: ChannelId::new(),
                message_id: MessageId::new(),
            },
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
}
