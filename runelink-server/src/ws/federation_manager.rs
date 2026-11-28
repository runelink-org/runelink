#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    fmt,
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
    time::Duration as StdDuration,
};

use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use jsonwebtoken::{Algorithm, Header};
use log::{debug, info, warn};
use runelink_client::util::{get_api_url, get_federation_ws_url, pad_host};
use runelink_types::{
    FederationClaims,
    capability::{
        Capability, CapabilityHello, NegotiatedCapabilities,
        VersionedCapability, preferred_version, supported_capabilities,
        versions::NEGOTIATION_VERSION,
    },
    host::Host,
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
    connect_async_with_config, tungstenite::client::IntoClientRequest,
};

use super::{
    error::{FederationRequestError, FederationRequestResult},
    pools::{FederationWsPool, HostSend},
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
#[derive(Clone)]
pub struct FederationWsManager {
    pool: FederationWsPool,
    pending: Arc<Mutex<HashMap<RequestId, PendingFederationReplySender>>>,
    /// Connection attempts in progress, by canonical host. Concurrent
    /// requests to a host share one attempt, since each new connection would
    /// take over sending to that host, and messages sent before and after a
    /// switch can arrive out of order.
    connecting: Arc<Mutex<HashMap<String, Shared<BoxFuture<'static, ()>>>>>,
    /// Updates waiting for a connection to their host, and the responses sent
    /// after them, by canonical host, in order. Every message to a host is
    /// queued on a connection, and every connection authenticated, under this
    /// lock, so that a new connection is given the waiting messages before
    /// any others. It is never held across a connection attempt.
    waiting: Arc<Mutex<HashMap<String, Vec<FederationWsEnvelope>>>>,
    /// This host's state, for connecting to the hosts updates are sent to.
    state: Arc<OnceLock<AppState>>,
}

impl fmt::Debug for FederationWsManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The state holds this manager.
        f.debug_struct("FederationWsManager")
            .field("pool", &self.pool)
            .finish_non_exhaustive()
    }
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
            waiting: Arc::default(),
            state: Arc::default(),
        }
    }

    /// Sets this host's state, so that updates can connect to their hosts.
    pub fn set_state(&self, state: AppState) {
        assert!(self.state.set(state).is_ok(), "state set twice");
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

    /// Authenticates a connection for a given host, and sends the host the
    /// messages waiting for a connection to it.
    pub async fn authenticate_connection(
        &self,
        conn_id: ConnId,
        host: String,
        issuer: String,
    ) -> bool {
        let Ok(host) = canonical_host(&host) else {
            return false;
        };
        let mut waiting = self.waiting.lock().await;
        if !self
            .pool
            .authenticate_connection(conn_id, host.clone(), issuer)
            .await
        {
            return false;
        }
        if let Some(queue) = waiting.remove(&host) {
            self.send_queue(&host, queue).await;
        }
        true
    }

    /// Sends messages that waited for `host`, in order, or drops them if it
    /// has no connection. The caller holds the `waiting` lock.
    async fn send_queue(&self, host: &str, queue: Vec<FederationWsEnvelope>) {
        let mut dropped = 0;
        for envelope in queue {
            if self.pool.send_to_host(host, envelope).await
                == HostSend::NotConnected
            {
                dropped += 1;
            }
        }
        if dropped > 0 {
            warn!(
                "Dropped {dropped} federation messages for unreachable host {host}"
            );
        }
    }

    /// Deregisters a connection from the manager.
    pub async fn deregister_connection(&self, conn_id: ConnId) -> bool {
        self.pool.deregister_connection(conn_id).await
    }

    pub async fn authenticated_host(&self, conn_id: ConnId) -> Option<Host> {
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

        let sent = {
            let _waiting = self.waiting.lock().await;
            self.pool.send_to_host(&host, envelope).await
        };
        if sent != HostSend::Sent {
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

    /// Sends an update to the given hosts without waiting on the network. A
    /// host without a connection, such as one that restarted, is connected to
    /// in the background and sent its waiting updates in order once it is
    /// connected, or they are dropped if it cannot be reached.
    pub async fn send_update_to_hosts<I, S>(
        &self,
        hosts: I,
        update: FederationWsUpdate,
    ) where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let envelope = FederationWsEnvelope::Update {
            event_id: EventId::new(),
            update,
        };
        let hosts = hosts
            .into_iter()
            .filter_map(|host| canonical_host(host.as_ref()).ok())
            .collect::<HashSet<_>>();
        if hosts.is_empty() {
            return;
        }
        let mut waiting = self.waiting.lock().await;
        for host in hosts {
            if let Some(envelopes) = waiting.get_mut(&host) {
                envelopes.push(envelope.clone());
                continue;
            }
            if self.pool.send_to_host(&host, envelope.clone()).await
                == HostSend::NotConnected
                && let Some(state) = self.state.get()
            {
                waiting.insert(host.clone(), vec![envelope.clone()]);
                tokio::spawn(self.clone().connect_waiting(state.clone(), host));
            }
        }
    }

    /// Connects to `host` and sends it the messages waiting for it, or drops
    /// them if it can't be reached. A connection that authenticates meanwhile
    /// is given them first (see [`Self::authenticate_connection`]).
    async fn connect_waiting(self, state: AppState, host: String) {
        self.ensure_connection(&state, &pad_host(&host)).await;
        let mut waiting = self.waiting.lock().await;
        if let Some(queue) = waiting.remove(&host) {
            self.send_queue(&host, queue).await;
        }
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
        self.send_response(
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
        self.send_response(
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

    /// Sends a response to a request from `host` behind any updates waiting
    /// for the host, since it may follow from them.
    async fn send_response(
        &self,
        conn_id: ConnId,
        host: Option<&str>,
        envelope: FederationWsEnvelope,
    ) -> bool {
        let mut waiting = self.waiting.lock().await;
        if let Some(queue) =
            host.and_then(|host| waiting.get_mut(&canonical_host(host).ok()?))
        {
            queue.push(envelope);
            return true;
        }
        self.pool.send_response(conn_id, host, envelope).await
    }

    /// Resolves a response envelope into a request ID and outcome.
    pub async fn resolve_response_envelope(
        &self,
        envelope: FederationWsEnvelope,
    ) -> bool {
        debug!(
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
        let _waiting = self.waiting.lock().await;
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
        let Ok(key) = canonical_host(host) else {
            return false;
        };
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
                connect_async_with_config(request, None, true),
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use runelink_types::{
        ids::EventId,
        ws::{FederationWsEnvelope, WsError},
    };
    use tokio::sync::mpsc;

    use super::FederationWsManager;

    fn error(code: &str) -> WsError {
        WsError {
            code: code.into(),
            message: String::new(),
            details: None,
        }
    }

    #[tokio::test]
    async fn new_connections_send_waiting_messages_first() {
        let manager = FederationWsManager::new();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let conn_id =
            manager.register_connection(sender, BTreeMap::new()).await;
        manager.waiting.lock().await.insert(
            "example.com".into(),
            vec![FederationWsEnvelope::Error {
                request_id: None,
                event_id: EventId::new(),
                error: error("waiting"),
            }],
        );
        // A response waits behind the waiting messages.
        manager
            .send_error(conn_id, Some("example.com"), None, error("queued"))
            .await;
        manager
            .authenticate_connection(
                conn_id,
                "example.com".into(),
                "issuer".into(),
            )
            .await;
        manager
            .send_error(conn_id, Some("example.com"), None, error("direct"))
            .await;
        for code in ["waiting", "queued", "direct"] {
            let Ok(FederationWsEnvelope::Error { error, .. }) =
                receiver.try_recv()
            else {
                panic!("expected an error envelope");
            };
            assert_eq!(error.code, code);
        }
        assert!(manager.waiting.lock().await.is_empty());
    }
}
