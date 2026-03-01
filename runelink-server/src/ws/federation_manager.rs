#![allow(dead_code)]

use std::{
    collections::HashMap, future::Future, pin::Pin, sync::Arc,
    time::Duration as StdDuration,
};

use jsonwebtoken::{Algorithm, Header};
use log::{info, warn};
use runelink_client::util::{get_api_url, get_federation_ws_url, pad_host};
use runelink_types::{
    FederationClaims,
    user::UserRef,
    ws::{
        FederationWsEnvelope, FederationWsReply, FederationWsRequest,
        FederationWsUpdate, WsError,
    },
};
use time::Duration;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_tungstenite::{
    connect_async, tungstenite::client::IntoClientRequest,
};
use uuid::Uuid;

use super::{
    error::{FederationRequestError, FederationRequestResult},
    pools::FederationWsPool,
    socket_loops::{FederationSocket, federation_socket_loop},
};
use crate::state::AppState;

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
        state: &AppState,
        host: &str,
        delegated_user_ref: Option<UserRef>,
        request: FederationWsRequest,
        timeout: StdDuration,
    ) -> FederationRequestResult<FederationWsReply> {
        let host = pad_host(host);
        if !self.ensure_connection(state, &host).await {
            return Err(FederationRequestError::HostUnavailable { host });
        }

        let request_id = Uuid::new_v4();
        let event_id = Uuid::new_v4();
        let envelope = FederationWsEnvelope::Request {
            request_id,
            event_id,
            delegated_user_ref,
            request,
        };
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(request_id, tx);
        }

        let sent = self.pool.send_to_host(&host, envelope).await;
        if !sent {
            warn!("Failed to send federation request to {host}");
            let mut pending = self.pending.lock().await;
            pending.remove(&request_id);
            return Err(FederationRequestError::HostUnavailable { host });
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
                Err(FederationRequestError::Timeout { host, request_id })
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

    async fn ensure_connection(&self, state: &AppState, host: &str) -> bool {
        if self.pool.has_host(host).await {
            return true;
        }

        if !self.connect_to_host(state, host).await {
            return false;
        }

        self.pool.has_host(host).await
    }

    fn connect_to_host<'a>(
        &'a self,
        state: &'a AppState,
        host: &'a str,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            info!("Opening federation websocket to {host}");
            let claims = FederationClaims::new_server_only(
                state.config.api_url(),
                get_api_url(host),
                Duration::minutes(5),
            );
            let token = match jsonwebtoken::encode(
                &Header::new(Algorithm::EdDSA),
                &claims,
                &state.key_manager.private_key,
            ) {
                Ok(token) => token,
                Err(error) => {
                    warn!("Failed creating federation token for {host}: {error}");
                    return false;
                }
            };
            let ws_url = get_federation_ws_url(host);
            let mut request = match ws_url.as_str().into_client_request() {
                Ok(request) => request,
                Err(error) => {
                    warn!(
                        "Failed building federation websocket request for {host}: {error}"
                    );
                    return false;
                }
            };
            let auth_header = match format!("Bearer {token}").parse() {
                Ok(value) => value,
                Err(error) => {
                    warn!(
                        "Failed building federation auth header for {host}: {error}"
                    );
                    return false;
                }
            };
            request.headers_mut().insert("Authorization", auth_header);

            let stream = match connect_async(request).await {
                Ok((stream, _)) => stream,
                Err(error) => {
                    warn!("Failed opening federation websocket to {host}: {error}");
                    return false;
                }
            };

            let (sender, outbound_rx) =
                mpsc::unbounded_channel::<FederationWsEnvelope>();
            let conn_id = self.register_connection(sender).await;
            let _ = self
                .authenticate_connection(conn_id, host.to_string())
                .await;
            let state = state.clone();
            let host = host.to_string();

            let loop_task: Pin<Box<dyn Future<Output = ()> + Send>> =
                Box::pin(async move {
                    federation_socket_loop(
                        state,
                        conn_id,
                        FederationSocket::Outbound(stream),
                        outbound_rx,
                    )
                    .await;
                    info!("Federation websocket closed for {host}");
                });
            tokio::spawn(loop_task);
            true
        })
    }
}
