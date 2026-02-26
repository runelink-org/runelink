#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use runelink_types::{ClientWsEnvelope, FederationWsEnvelope, UserRef};
use time::OffsetDateTime;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

/// Tracks active client websocket connections and provides safe send helpers.
///
/// This pool only manages connection lifecycle and direct addressability
/// (by connection id or authenticated user). It intentionally does not contain
/// subscription state, fanout/routing decisions, persistence, or DB access.
///
/// Send strategy:
/// - collect target senders under a read lock
/// - drop lock before `send`
/// - prune stale connections under a write lock after failed sends
#[derive(Clone, Debug, Default)]
pub struct ClientWsPool {
    inner: Arc<RwLock<ClientPoolState>>,
}

#[derive(Debug, Default)]
struct ClientPoolState {
    connections: HashMap<Uuid, ClientConn>,
    by_user: HashMap<UserRef, HashSet<Uuid>>,
}

#[derive(Clone, Debug)]
pub struct ClientConn {
    pub sender: mpsc::UnboundedSender<ClientWsEnvelope>,
    pub user_ref: Option<UserRef>,
    pub connected_at: OffsetDateTime,
}

impl ClientWsPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register_connection(
        &self,
        conn_id: Uuid,
        sender: mpsc::UnboundedSender<ClientWsEnvelope>,
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
            },
        );
    }

    pub async fn authenticate_connection(
        &self,
        conn_id: Uuid,
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

    pub async fn deregister_connection(&self, conn_id: Uuid) -> bool {
        let mut state = self.inner.write().await;
        Self::remove_client_connection(&mut state, conn_id)
    }

    pub async fn send_to_connection(
        &self,
        conn_id: Uuid,
        envelope: ClientWsEnvelope,
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
                        .map(|conn| (*conn_id, conn.sender.clone()))
                })
                .collect::<Vec<_>>()
        };

        Self::send_to_many_client(targets, envelope, self).await
    }

    pub async fn broadcast(&self, envelope: ClientWsEnvelope) -> usize {
        let targets = {
            let state = self.inner.read().await;
            state
                .connections
                .iter()
                .map(|(conn_id, conn)| (*conn_id, conn.sender.clone()))
                .collect::<Vec<_>>()
        };

        Self::send_to_many_client(targets, envelope, self).await
    }

    fn remove_conn_from_user_index(
        by_user: &mut HashMap<UserRef, HashSet<Uuid>>,
        user_ref: &UserRef,
        conn_id: Uuid,
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
        conn_id: Uuid,
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
        targets: Vec<(Uuid, mpsc::UnboundedSender<ClientWsEnvelope>)>,
        envelope: ClientWsEnvelope,
        pool: &ClientWsPool,
    ) -> usize {
        let mut sent = 0usize;
        let mut stale = Vec::new();

        for (conn_id, sender) in targets {
            if sender.send(envelope.clone()).is_ok() {
                sent += 1;
            } else {
                stale.push(conn_id);
            }
        }

        if !stale.is_empty() {
            let stale_set: HashSet<Uuid> = stale.into_iter().collect();
            let mut state = pool.inner.write().await;
            for conn_id in stale_set {
                let _ = Self::remove_client_connection(&mut state, conn_id);
            }
        }

        sent
    }
}

/// Tracks active federation websocket connections and provides safe send
/// helpers.
///
/// This pool manages only connection lifecycle and addressing primitives. It
/// does not compute fanout targets, maintain subscriptions, handle replay/ack,
/// or touch persistence.
///
/// Send strategy:
/// - collect target senders under a read lock
/// - drop lock before `send`
/// - prune stale connections under a write lock after failed sends
///
/// Host send semantics:
/// - exactly one active authenticated connection is tracked per host
/// - `send_to_host` uses that single active connection
/// - `send_to_hosts` sends once per unique host
#[derive(Clone, Debug, Default)]
pub struct FederationWsPool {
    inner: Arc<RwLock<FederationPoolState>>,
}

#[derive(Debug, Default)]
struct FederationPoolState {
    connections: HashMap<Uuid, FederationConn>,
    by_host: HashMap<String, Uuid>,
}

#[derive(Clone, Debug)]
pub struct FederationConn {
    pub sender: mpsc::UnboundedSender<FederationWsEnvelope>,
    pub host: Option<String>,
    pub connected_at: OffsetDateTime,
}

impl FederationWsPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register_connection(
        &self,
        conn_id: Uuid,
        sender: mpsc::UnboundedSender<FederationWsEnvelope>,
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
                connected_at: OffsetDateTime::now_utc(),
            },
        );
    }

    pub async fn authenticate_connection(
        &self,
        conn_id: Uuid,
        host: String,
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
        }
        true
    }

    pub async fn deregister_connection(&self, conn_id: Uuid) -> bool {
        let mut state = self.inner.write().await;
        Self::remove_federation_connection(&mut state, conn_id)
    }

    pub async fn send_to_connection(
        &self,
        conn_id: Uuid,
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
                if let Some(conn) = state.connections.get(&conn_id) {
                    let sender = conn.sender.clone();
                    out.push((conn_id, sender));
                }
            }
            out
        };

        Self::send_to_many_federation(targets, envelope, self).await
    }

    pub async fn broadcast(&self, envelope: FederationWsEnvelope) -> usize {
        let targets = {
            let state = self.inner.read().await;
            state
                .connections
                .iter()
                .map(|(conn_id, conn)| (*conn_id, conn.sender.clone()))
                .collect::<Vec<_>>()
        };
        Self::send_to_many_federation(targets, envelope, self).await
    }

    fn remove_conn_from_host_index(
        by_host: &mut HashMap<String, Uuid>,
        host: &str,
        conn_id: Uuid,
    ) {
        if by_host.get(host).copied() == Some(conn_id) {
            by_host.remove(host);
        }
    }

    fn remove_federation_connection(
        state: &mut FederationPoolState,
        conn_id: Uuid,
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
        targets: Vec<(Uuid, mpsc::UnboundedSender<FederationWsEnvelope>)>,
        envelope: FederationWsEnvelope,
        pool: &FederationWsPool,
    ) -> usize {
        let mut sent = 0usize;
        let mut stale = Vec::new();

        for (conn_id, sender) in targets {
            if sender.send(envelope.clone()).is_ok() {
                sent += 1;
            } else {
                stale.push(conn_id);
            }
        }

        if !stale.is_empty() {
            let stale_set: HashSet<Uuid> = stale.into_iter().collect();
            let mut state = pool.inner.write().await;
            for conn_id in stale_set {
                let _ = Self::remove_federation_connection(&mut state, conn_id);
            }
        }

        sent
    }
}
