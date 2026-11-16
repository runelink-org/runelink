#![allow(dead_code)]

use std::{
    collections::HashMap, future::Future, pin::Pin, sync::Arc,
    time::Duration as StdDuration,
};

use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use jsonwebtoken::{Algorithm, Header};
use log::{info, warn};
use runelink_client::util::{get_api_url, get_federation_ws_url, pad_host};
use runelink_types::{
    FederationClaims,
    capability::{
        Capability, CapabilityHello, NegotiatedCapabilities,
        VersionedCapability, preferred_version, supported_capabilities,
        versions::NEGOTIATION_VERSION,
    },
    ids::{EventId, RequestId},
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

use super::{
    error::{FederationRequestError, FederationRequestResult},
    pools::FederationWsPool,
    socket_loops::{
        FederationIncomingEvent, FederationSocket, HELLO_TIMEOUT,
        federation_socket_loop,
    },
};
use crate::{
    capabilities::federation_websocket, config::canonical_host, ids::ConnId,
    state::AppState,
};

type PendingFederationReplySender =
    oneshot::Sender<Result<FederationWsReply, WsError>>;

/// The updates that cache a request's result here. Only the host's updates
/// write these caches, so a request is refused unless the host sends them.
fn cached_by(request: &FederationWsRequest) -> &'static [Capability] {
    match request {
        FederationWsRequest::ServersCreate(_) => {
            &[Capability::ServersEvents, Capability::MembershipsEvents]
        }
        FederationWsRequest::ServersUpdate { .. }
        | FederationWsRequest::ServersDelete { .. } => {
            &[Capability::ServersEvents]
        }
        FederationWsRequest::MembershipsUpsert { .. }
        | FederationWsRequest::MembershipsDelete { .. } => {
            &[Capability::MembershipsEvents]
        }
        _ => &[],
    }
}

/// High-level manager for federation websocket connections.
///
/// This manager combines connection-pool responsibilities with request/reply
/// correlation so callers can send typed requests/updates without constructing
/// websocket envelopes directly.
#[derive(Clone, Debug)]
pub struct FederationWsManager {
    pool: FederationWsPool,
    pending: Arc<Mutex<HashMap<RequestId, PendingFederationReplySender>>>,
    /// Connection attempts in progress, by canonical host. Concurrent
    /// requests to a host share one attempt, since each new connection would
    /// take over sending to that host, and messages sent before and after a
    /// switch can arrive out of order.
    connecting: Arc<Mutex<HashMap<String, Shared<BoxFuture<'static, ()>>>>>,
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
            connecting: Arc::default(),
        }
    }

    /// Registers a new connection with the manager.
    pub async fn register_connection(
        &self,
        sender: mpsc::UnboundedSender<FederationWsEnvelope>,
        capabilities: NegotiatedCapabilities,
    ) -> ConnId {
        let conn_id = ConnId::new();
        self.pool
            .register_connection(conn_id, sender, capabilities)
            .await;
        conn_id
    }

    /// Authenticates a connection for a given host.
    pub async fn authenticate_connection(
        &self,
        conn_id: ConnId,
        host: String,
        issuer: String,
    ) -> bool {
        self.pool
            .authenticate_connection(conn_id, host, issuer)
            .await
    }

    /// Deregisters a connection from the manager.
    pub async fn deregister_connection(&self, conn_id: ConnId) -> bool {
        self.pool.deregister_connection(conn_id).await
    }

    pub async fn authenticated_host(&self, conn_id: ConnId) -> Option<String> {
        self.pool.authenticated_host(conn_id).await
    }

    pub async fn authenticated_issuer(
        &self,
        conn_id: ConnId,
    ) -> Option<String> {
        self.pool.authenticated_issuer(conn_id).await
    }

    pub async fn supports_capability(
        &self,
        conn_id: ConnId,
        capability: &VersionedCapability,
    ) -> bool {
        self.pool.supports_capability(conn_id, capability).await
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

        for capability in std::iter::once(request.capability())
            .chain(cached_by(&request).iter().cloned())
        {
            let Some(capability) = preferred_version(
                crate::capabilities::federation_websocket,
                &capability,
            )
            .map(|version| capability.with_version(version)) else {
                return Err(FederationRequestError::UnsupportedCapability {
                    host,
                    capability: capability.to_string(),
                });
            };
            if !self.pool.host_supports_capability(&host, &capability).await {
                return Err(FederationRequestError::UnsupportedCapability {
                    host,
                    capability: capability.capability.to_string(),
                });
            }
        }

        let request_id = RequestId::new();
        let event_id = EventId::new();
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
        // The request is now queued on the peer's connection in order, so
        // later requests on the caller's connection may be read.
        crate::admission::admit();

        let result = tokio::time::timeout(timeout, rx).await;
        match result {
            Ok(Ok(Ok(reply))) => Ok(reply),
            Ok(Ok(Err(remote_error))) => Err(FederationRequestError::Remote {
                code: remote_error.code.clone(),
                message: remote_error.message.clone(),
                error: Box::new(remote_error),
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
                    event_id: EventId::new(),
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
                    event_id: EventId::new(),
                    update,
                },
            )
            .await
    }

    /// Sends a reply to a request from `host` that arrived on `conn_id`, on
    /// the connection the host is sent messages on.
    pub async fn send_reply(
        &self,
        conn_id: ConnId,
        host: Option<&str>,
        request_id: RequestId,
        reply: FederationWsReply,
    ) -> bool {
        self.pool
            .send_response(
                conn_id,
                host,
                FederationWsEnvelope::Reply {
                    request_id,
                    event_id: EventId::new(),
                    reply,
                },
            )
            .await
    }

    /// Sends an error for a request from `host` that arrived on `conn_id`, on
    /// the connection the host is sent messages on.
    pub async fn send_error(
        &self,
        conn_id: ConnId,
        host: Option<&str>,
        request_id: Option<RequestId>,
        error: WsError,
    ) -> bool {
        self.pool
            .send_response(
                conn_id,
                host,
                FederationWsEnvelope::Error {
                    request_id,
                    event_id: EventId::new(),
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
        info!(
            "Resolving federation websocket response envelope: {:#?}",
            envelope
        );
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
                event_id: EventId::new(),
                update,
            })
            .await
    }

    async fn ensure_connection(&self, state: &AppState, host: &str) -> bool {
        if self.pool.has_host(host).await {
            return true;
        }
        let key = canonical_host(host);
        let attempt = {
            let mut connecting = self.connecting.lock().await;
            match connecting.get(&key) {
                Some(attempt) => attempt.clone(),
                None => {
                    // An attempt registers its connection before it leaves
                    // the map, so one may have finished since the check above.
                    if self.pool.has_host(host).await {
                        return true;
                    }
                    let manager = self.clone();
                    let state = state.clone();
                    let host = host.to_owned();
                    let attempt_key = key.clone();
                    // Spawned, so that it finishes and leaves the map even if
                    // every caller waiting on it gives up.
                    let task = tokio::spawn(async move {
                        manager.connect_to_host(&state, &host).await;
                        manager.connecting.lock().await.remove(&attempt_key);
                    });
                    let connecting_map = self.connecting.clone();
                    let panicked_key = key.clone();
                    let attempt = async move {
                        // A panic skipped the removal above.
                        if task.await.is_err() {
                            connecting_map.lock().await.remove(&panicked_key);
                        }
                    }
                    .boxed()
                    .shared();
                    connecting.insert(key, attempt.clone());
                    attempt
                }
            }
        };
        // Whether it worked or not, the host may also have connected to us.
        attempt.await;
        self.pool.has_host(host).await
    }

    fn connect_to_host<'a>(
        &'a self,
        state: &'a AppState,
        host: &'a str,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            info!("Opening federation websocket to {host}");
            let claims = FederationClaims::new_server_only(
                state.config.api_url(),
                get_api_url(host, state.config.secure),
                Duration::minutes(5),
            );
            let token = match jsonwebtoken::encode(
                &Header::new(Algorithm::EdDSA),
                &claims,
                &state.key_manager.private_key,
            ) {
                Ok(token) => token,
                Err(error) => {
                    warn!(
                        "Failed creating federation token for {host}: {error}"
                    );
                    return;
                }
            };
            let ws_url = get_federation_ws_url(host, state.config.secure);
            let mut request = match ws_url.as_str().into_client_request() {
                Ok(request) => request,
                Err(error) => {
                    warn!(
                        "Failed building federation websocket request for {host}: {error}"
                    );
                    return;
                }
            };
            let auth_header = match format!("Bearer {token}").parse() {
                Ok(value) => value,
                Err(error) => {
                    warn!(
                        "Failed building federation auth header for {host}: {error}"
                    );
                    return;
                }
            };
            request.headers_mut().insert("Authorization", auth_header);

            let stream = match tokio::time::timeout(
                StdDuration::from_secs(5),
                connect_async(request),
            )
            .await
            {
                Ok(Ok((stream, _))) => stream,
                Err(_) => {
                    warn!("Timed out opening federation websocket to {host}");
                    return;
                }
                Ok(Err(error)) => {
                    warn!(
                        "Failed opening federation websocket to {host}: {error}"
                    );
                    return;
                }
            };

            let offered_capabilities =
                supported_capabilities(federation_websocket);
            let hello = FederationWsEnvelope::Hello(CapabilityHello {
                negotiation_version: NEGOTIATION_VERSION,
                capabilities: offered_capabilities.clone(),
            });
            let Ok(payload) = serde_json::to_string(&hello) else {
                return;
            };
            let mut socket = FederationSocket::Outbound(Box::new(stream));
            if !matches!(
                tokio::time::timeout(
                    StdDuration::from_secs(5),
                    socket.send_text(payload),
                )
                .await,
                Ok(Ok(()))
            ) {
                warn!("Failed sending federation capability hello to {host}");
                return;
            }
            let event =
                tokio::time::timeout(HELLO_TIMEOUT, socket.recv_event()).await;
            let Ok(FederationIncomingEvent::Text(payload)) = event else {
                // Timeouts and closes are not evidence of incompatibility.
                warn!(
                    "Federation peer {host} did not send a capability welcome"
                );
                return;
            };
            let capabilities = match serde_json::from_str(&payload) {
                Ok(FederationWsEnvelope::Welcome(welcome))
                    if welcome.negotiation_version == NEGOTIATION_VERSION
                        && welcome.capabilities.iter().all(
                            |(capability, version)| {
                                offered_capabilities
                                    .get(capability)
                                    .is_some_and(|versions| {
                                        versions.contains(version)
                                    })
                            },
                        ) =>
                {
                    welcome.capabilities
                }
                _ => {
                    warn!(
                        "Federation peer {host} sent an invalid capability welcome"
                    );
                    socket
                        .close_protocol_error("Invalid capability welcome")
                        .await;
                    return;
                }
            };

            let (sender, outbound_rx) =
                mpsc::unbounded_channel::<FederationWsEnvelope>();
            let conn_id = self.register_connection(sender, capabilities).await;
            let issuer = get_api_url(host, state.config.secure);
            let _ = self
                .authenticate_connection(conn_id, host.to_string(), issuer)
                .await;
            let state = state.clone();
            let host = host.to_string();

            let loop_task: Pin<Box<dyn Future<Output = ()> + Send>> =
                Box::pin(async move {
                    federation_socket_loop(state, conn_id, socket, outbound_rx)
                        .await;
                    info!("Federation websocket closed for {host}");
                });
            tokio::spawn(loop_task);
        })
    }
}
