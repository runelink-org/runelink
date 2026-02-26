#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use runelink_types::{
    user::UserRef,
    ws::{
        FederationWsEnvelope, FederationWsReply, FederationWsRequest,
        FederationWsUpdate, WsError,
    },
};
use tokio::sync::{Mutex, mpsc, oneshot};
use uuid::Uuid;

use super::{
    error::{FederationRequestError, FederationRequestResult},
    pools::FederationWsPool,
};

type PendingFederationReplySender =
    oneshot::Sender<Result<FederationWsReply, WsError>>;

/// High-level manager for federation websocket connections.
///
/// This manager combines connection-pool responsibilities with request/reply
/// correlation so callers can send typed requests/updates without constructing
/// websocket envelopes directly.
#[derive(Clone, Debug)]
pub struct FederationWsManager {
    pool: FederationWsPool,
    pending: Arc<Mutex<HashMap<Uuid, PendingFederationReplySender>>>,
}

impl Default for FederationWsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FederationWsManager {
    /// Creates a new federation websocket manager.
    pub fn new() -> Self {
        let pool = FederationWsPool::new();
        Self {
            pool,
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Registers a new connection with the manager.
    pub async fn register_connection(
        &self,
        sender: mpsc::UnboundedSender<FederationWsEnvelope>,
    ) -> Uuid {
        let conn_id = Uuid::new_v4();
        self.pool.register_connection(conn_id, sender).await;
        conn_id
    }

    /// Authenticates a connection for a given host.
    pub async fn authenticate_connection(
        &self,
        conn_id: Uuid,
        host: String,
    ) -> bool {
        self.pool.authenticate_connection(conn_id, host).await
    }

    /// Deregisters a connection from the manager.
    pub async fn deregister_connection(&self, conn_id: Uuid) -> bool {
        self.pool.deregister_connection(conn_id).await
    }

    pub async fn authenticated_host(&self, conn_id: Uuid) -> Option<String> {
        self.pool.authenticated_host(conn_id).await
    }

    /// Sends a request to the given host and waits for a reply with a timeout.
    pub async fn send_request_to_host(
        &self,
        host: &str,
        delegated_user_ref: Option<UserRef>,
        request: FederationWsRequest,
        timeout: Duration,
    ) -> FederationRequestResult<FederationWsReply> {
        let request_id = Uuid::new_v4();
        let event_id = Uuid::new_v4();
        let envelope = FederationWsEnvelope::Request {
            request_id,
            event_id,
            delegated_user_ref,
            request,
        };
        let (tx, rx) = oneshot::channel();
        let mut pending = self.pending.lock().await;
        pending.insert(request_id, tx);

        let sent = self.pool.send_to_host(host, envelope).await;
        if !sent {
            let mut pending = self.pending.lock().await;
            pending.remove(&request_id);
            return Err(FederationRequestError::HostUnavailable {
                host: host.to_owned(),
            });
        }

        let result = tokio::time::timeout(timeout, rx).await;
        match result {
            Ok(Ok(Ok(reply))) => Ok(reply),
            Ok(Ok(Err(remote_error))) => Err(FederationRequestError::Remote {
                code: remote_error.code.clone(),
                message: remote_error.message.clone(),
                error: remote_error,
            }),
            Ok(Err(_)) => {
                Err(FederationRequestError::ChannelClosed { request_id })
            }
            Err(_) => {
                let mut pending = self.pending.lock().await;
                pending.remove(&request_id);
                Err(FederationRequestError::Timeout {
                    host: host.to_owned(),
                    request_id,
                })
            }
        }
    }

    /// Sends an update to the given host.
    pub async fn send_update_to_host(
        &self,
        host: &str,
        update: FederationWsUpdate,
    ) -> bool {
        self.pool
            .send_to_host(
                host,
                FederationWsEnvelope::Update {
                    event_id: Uuid::new_v4(),
                    update,
                },
            )
            .await
    }

    /// Sends an update to the given hosts.
    pub async fn send_update_to_hosts<I, S>(
        &self,
        hosts: I,
        update: FederationWsUpdate,
    ) -> usize
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.pool
            .send_to_hosts(
                hosts,
                FederationWsEnvelope::Update {
                    event_id: Uuid::new_v4(),
                    update,
                },
            )
            .await
    }

    /// Sends a reply to the given connection.
    pub async fn send_reply_to_connection(
        &self,
        conn_id: Uuid,
        request_id: Uuid,
        reply: FederationWsReply,
    ) -> bool {
        self.pool
            .send_to_connection(
                conn_id,
                FederationWsEnvelope::Reply {
                    request_id,
                    event_id: Uuid::new_v4(),
                    reply,
                },
            )
            .await
    }

    /// Sends an error to the given connection.
    pub async fn send_error_to_connection(
        &self,
        conn_id: Uuid,
        request_id: Option<Uuid>,
        error: WsError,
    ) -> bool {
        self.pool
            .send_to_connection(
                conn_id,
                FederationWsEnvelope::Error {
                    request_id,
                    event_id: Uuid::new_v4(),
                    error,
                },
            )
            .await
    }

    /// Resolves a response envelope into a request ID and outcome.
    pub async fn resolve_response_envelope(
        &self,
        envelope: FederationWsEnvelope,
    ) -> bool {
        let (request_id, outcome) = match envelope {
            FederationWsEnvelope::Reply {
                request_id, reply, ..
            } => (request_id, Ok(reply)),
            FederationWsEnvelope::Error {
                request_id: Some(request_id),
                error,
                ..
            } => (request_id, Err(error)),
            _ => return false,
        };

        let sender = {
            let mut pending = self.pending.lock().await;
            pending.remove(&request_id)
        };

        let Some(sender) = sender else {
            return false;
        };

        sender.send(outcome).is_ok()
    }

    /// Broadcasts an update to all active connections.
    pub async fn broadcast_update(&self, update: FederationWsUpdate) -> usize {
        self.pool
            .broadcast(FederationWsEnvelope::Update {
                event_id: Uuid::new_v4(),
                update,
            })
            .await
    }
}
