//! A client websocket connection that negotiates capabilities, matches replies
//! to requests, streams pushed updates, and reconnects. It follows the
//! JavaScript SDK's `RunelinkConnection`, so both behave the same way.
//!
//! ```no_run
//! use runelink_client::ws::{Connection, ConnectionOptions};
//! use runelink_types::ws::{ClientWsReply, ClientWsRequest};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let connection = Connection::new(ConnectionOptions::host("localhost"))?;
//! let mut updates = connection.updates();
//! tokio::spawn(async move {
//!     while let Some(update) = updates.recv().await {
//!         println!("{update:?}");
//!     }
//! });
//! // Connects if needed. The reply always matches the request type.
//! let reply = connection.request(ClientWsRequest::Ping).await?;
//! assert!(matches!(reply, ClientWsReply::Pong));
//! # Ok(())
//! # }
//! ```
//!
//! [`ConnectionOptions::host`] connects to `wss://host:7000/ws/client`. Set
//! [`ConnectionOptions::transport_security`] to
//! [`TransportSecurity::PreferSecure`] to fall back to `ws://` if the secure
//! socket never opens, or use [`ConnectionOptions::url`] for a websocket URL.
//!
//! Connections reconnect with exponential backoff and jitter after a socket
//! fails to open or closes. Requests in flight when a socket closes fail with
//! [`Error::Connection`] and are never resent. Each reconnect negotiates a new
//! socket, so authenticate again after reconnecting. If the host rejects
//! capability negotiation, or this client rejects the host's reply, the
//! connection fails with [`Error::IncompatibleHost`] and stops reconnecting.
//! A host that does not finish negotiating within 5 seconds is retried
//! normally.
//!
//! Requests whose capability was not negotiated fail with
//! [`Error::UnsupportedCapability`] without closing the connection.
//!
//! Requests have no built-in timeout, and nothing detects a connection that
//! silently stopped delivering, such as after a laptop sleeps, so wrap
//! requests in [`tokio::time::timeout`] where that matters.

mod error;
mod issues;

use std::{
    collections::HashMap,
    mem,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::Duration,
};

use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use runelink_types::{
    capability::{
        Capability, CapabilityHello, CapabilityWelcome, NegotiatedCapabilities,
        SupportedCapabilities, supported_capabilities,
        versions::NEGOTIATION_VERSION,
    },
    host::canonical_host,
    ids::RequestId,
    ws::{
        ClientWsEnvelope, ClientWsReply, ClientWsRequest, ClientWsUpdate,
        WsError,
    },
};
use rustls::ClientConfig;
use rustls_platform_verifier::BuilderVerifierExt;
use serde::{Deserialize, Serialize, de::IgnoredAny};
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot, watch},
    time,
};
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};

pub use error::*;
pub use issues::*;

use crate::{
    capabilities::{client_websocket, preferred_client_websocket_version},
    util::get_client_ws_url,
};

const DEFAULT_RECONNECT_INITIAL_DELAY: Duration = Duration::from_secs(1);
const DEFAULT_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);
const DEFAULT_RECONNECT_BACKOFF_MULTIPLIER: f64 = 2.0;
const DEFAULT_RECONNECT_JITTER_RATIO: f64 = 0.2;
// How long to wait for a socket to open, including TLS and the HTTP upgrade
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);
const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(5);
// How long to wait for the host's close reply after closing a socket
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);
// Close code the host sends when capability negotiation fails
const PROTOCOL_ERROR_CLOSE_CODE: u16 = 1002;
// Close code for a close frame without a code
const NO_STATUS_CLOSE_CODE: u16 = 1005;
// Close code for sockets that fail without a close frame
const ABNORMAL_CLOSE_CODE: u16 = 1006;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Sink = SplitSink<Socket, Message>;
type Stream = SplitStream<Socket>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

/// Whether a host connection may fall back to `ws` if `wss` never opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransportSecurity {
    #[default]
    SecureOnly,
    /// Falls back to `ws` when `wss` fails to open for any reason, including
    /// certificate errors, after which credentials are sent unencrypted. Each
    /// reconnect tries `wss` again until a socket opens, and then the
    /// connection keeps that scheme. Only use this when unencrypted
    /// connections to the host are acceptable, such as after the user agrees
    /// to them.
    PreferSecure,
}

#[derive(Clone, Debug)]
pub enum Target {
    /// Connects to the host's client websocket over `wss`, or `ws` as set by
    /// [`ConnectionOptions::transport_security`].
    Host(String),
    /// Connects to this websocket URL directly.
    Url(String),
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ConnectionOptions {
    pub target: Target,
    pub auto_reconnect: bool,
    pub reconnect_initial_delay: Duration,
    pub reconnect_max_delay: Duration,
    pub reconnect_backoff_multiplier: f64,
    pub reconnect_jitter_ratio: f64,
    /// The number of reconnects to try after a socket closes or fails to
    /// open, or `None` for no limit.
    pub max_reconnect_attempts: Option<u32>,
    pub transport_security: TransportSecurity,
    /// The capability versions to offer, or `None` for every version this
    /// client implements.
    pub capabilities: Option<SupportedCapabilities>,
}

impl ConnectionOptions {
    pub fn host(host: impl Into<String>) -> Self {
        Self::new(Target::Host(host.into()))
    }

    pub fn url(url: impl Into<String>) -> Self {
        Self::new(Target::Url(url.into()))
    }

    fn new(target: Target) -> Self {
        Self {
            target,
            auto_reconnect: true,
            reconnect_initial_delay: DEFAULT_RECONNECT_INITIAL_DELAY,
            reconnect_max_delay: DEFAULT_RECONNECT_MAX_DELAY,
            reconnect_backoff_multiplier: DEFAULT_RECONNECT_BACKOFF_MULTIPLIER,
            reconnect_jitter_ratio: DEFAULT_RECONNECT_JITTER_RATIO,
            max_reconnect_attempts: None,
            transport_security: TransportSecurity::default(),
            capabilities: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RequestOptions {
    /// Whether to connect if needed. When false, the request is sent only on
    /// the socket that is connected when it starts, and fails with
    /// [`Error::Disconnected`] if there is none.
    pub connect: bool,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self { connect: true }
    }
}

/// A client websocket connection. Clones share the connection, which
/// disconnects once every clone is dropped. It needs a Tokio runtime.
#[derive(Clone)]
pub struct Connection {
    handle: Arc<Handle>,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Connection")
            .field("status", &self.status())
            .finish()
    }
}

// Disconnects when the last clone of a connection drops
struct Handle(Arc<Shared>);

struct Shared {
    config: Config,
    state: Mutex<State>,
}

struct Config {
    // The host, or the websocket URL without secrets for URL connections
    host: String,
    auto_reconnect: bool,
    reconnect_initial_delay: Duration,
    reconnect_max_delay: Duration,
    reconnect_backoff_multiplier: f64,
    reconnect_jitter_ratio: f64,
    max_reconnect_attempts: Option<u32>,
    capabilities: SupportedCapabilities,
    url: String,
    // Host connections may fall back to this if the secure socket never opens
    insecure_url: Option<String>,
}

struct State {
    status: Status,
    // The negotiated session, if any
    session: Option<Arc<Session>>,
    // Callers waiting for the next negotiation
    ready: Vec<oneshot::Sender<Result<(), Error>>>,
    // The task that owns reconnects, while one runs
    supervisor: Option<Supervisor>,
    // Closes once the latest supervisor's task ends
    closing: watch::Receiver<()>,
    // Set during a reconnect delay. The first caller to connect then takes it
    // to skip the delay, while callers during a dial join it.
    wake: Option<oneshot::Sender<()>>,
    // Which URL the first socket to open used, which later attempts start
    // from
    opened: Option<Dial>,
    has_connected_once: bool,
    next_supervisor_id: u64,
    statuses: Subscribers<Status>,
    updates: Subscribers<ClientWsUpdate>,
    issues: Subscribers<Error>,
}

struct Supervisor {
    id: u64,
    // Dropping this stops the supervisor
    _cancel: oneshot::Sender<()>,
}

struct Session {
    negotiated: NegotiatedCapabilities,
    rejected: Option<SupportedCapabilities>,
    outbound: mpsc::UnboundedSender<String>,
    pending: Mutex<Pending>,
}

#[derive(Default)]
struct Pending {
    // Requests awaiting replies. Abandoned requests stay as `None` so their
    // late replies are not reported as unknown.
    waiting: HashMap<RequestId, Option<oneshot::Sender<Outcome>>>,
    // Why the session ended, once it has
    ended: Option<Error>,
}

enum Outcome {
    Reply(Box<ClientWsReply>),
    HostError(WsError),
    Failed(Error),
}

// Which URL a dial uses
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dial {
    Primary,
    // The insecure URL a host connection may fall back to
    Fallback,
}

struct Subscribers<T>(Vec<mpsc::UnboundedSender<T>>);

impl<T: Clone> Subscribers<T> {
    fn subscribe(&mut self, initial: Option<T>) -> mpsc::UnboundedReceiver<T> {
        let (sender, receiver) = mpsc::unbounded_channel();
        if let Some(initial) = initial {
            let _ = sender.send(initial);
        }
        self.0.retain(|sender| !sender.is_closed());
        self.0.push(sender);
        receiver
    }

    fn publish(&mut self, value: &T) {
        self.0.retain(|sender| sender.send(value.clone()).is_ok());
    }
}

impl<T> Default for Subscribers<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl Connection {
    /// Creates a disconnected connection.
    pub fn new(options: ConnectionOptions) -> Result<Self, OptionsError> {
        let capabilities = match options.capabilities {
            None => supported_capabilities(client_websocket),
            Some(capabilities) => {
                let capabilities: SupportedCapabilities = capabilities
                    .into_iter()
                    .filter(|(_, versions)| !versions.is_empty())
                    .collect();
                for (capability, versions) in &capabilities {
                    let implemented = client_websocket(capability);
                    if versions.iter().any(|v| !implemented.contains(v)) {
                        return Err(OptionsError(format!(
                            "Capability {capability} includes a version not implemented by this client"
                        )));
                    }
                }
                capabilities
            }
        };
        let (url, insecure_url, host) = match options.target {
            Target::Host(host) => {
                let host = canonical_host(&host).map_err(|error| {
                    OptionsError(format!("Invalid host: {error}"))
                })?;
                (
                    get_client_ws_url(&host, true),
                    (options.transport_security
                        == TransportSecurity::PreferSecure)
                        .then(|| get_client_ws_url(&host, false)),
                    host,
                )
            }
            Target::Url(url) => {
                let invalid = |error: &dyn std::fmt::Display| {
                    OptionsError(format!("Invalid websocket URL: {error}"))
                };
                // Stricter than the handshake's parser, which can let a
                // malformed authority dial the scheme's default port
                let parsed =
                    url::Url::parse(&url).map_err(|error| invalid(&error))?;
                if !matches!(parsed.scheme(), "ws" | "wss") {
                    return Err(invalid(&"the scheme must be ws or wss"));
                }
                // The handshake only accepts lowercase schemes, which parsing
                // normalizes
                let url = parsed.to_string();
                url.as_str()
                    .into_client_request()
                    .map_err(|error| invalid(&error))?;
                let host = redact(&url, parsed);
                (url, None, host)
            }
        };
        let reconnect_initial_delay = options.reconnect_initial_delay;
        let config = Config {
            host,
            auto_reconnect: options.auto_reconnect,
            reconnect_initial_delay,
            reconnect_max_delay: options
                .reconnect_max_delay
                .max(reconnect_initial_delay),
            reconnect_backoff_multiplier: options
                .reconnect_backoff_multiplier
                .max(1.0),
            reconnect_jitter_ratio: options
                .reconnect_jitter_ratio
                .clamp(0.0, 1.0),
            max_reconnect_attempts: options.max_reconnect_attempts,
            capabilities,
            url,
            insecure_url,
        };
        let state = State {
            status: Status::Disconnected,
            session: None,
            ready: Vec::new(),
            supervisor: None,
            closing: watch::channel(()).1,
            wake: None,
            opened: None,
            has_connected_once: false,
            next_supervisor_id: 0,
            statuses: Subscribers::default(),
            updates: Subscribers::default(),
            issues: Subscribers::default(),
        };
        let shared = Arc::new(Shared {
            config,
            state: Mutex::new(state),
        });
        Ok(Self {
            handle: Arc::new(Handle(shared)),
        })
    }

    fn shared(&self) -> &Arc<Shared> {
        &self.handle.0
    }

    /// Opens the websocket if needed and waits for capability negotiation.
    /// While reconnecting is allowed, it waits through failed attempts.
    pub async fn connect(&self) -> Result<(), Error> {
        let shared = self.shared();
        let (waiting, launch) = {
            let mut state = shared.lock();
            if state.session.is_some() {
                return Ok(());
            }
            state.ready.retain(|sender| !sender.is_closed());
            if state.ready.is_empty()
                && let Some(wake) = state.wake.take()
            {
                let _ = wake.send(());
            }
            let (sender, receiver) = oneshot::channel();
            state.ready.push(sender);
            let task = state
                .supervisor
                .is_none()
                .then(|| shared.register(&mut state));
            (receiver, task)
        };
        // Spawned once the state is unlocked, since dropping the task locks it
        if let Some(task) = launch {
            tokio::spawn(task.run());
        }
        waiting.await.unwrap_or(Err(Error::Disconnected))
    }

    /// Closes the websocket, stops reconnecting, and fails waiting calls with
    /// [`Error::Disconnected`]. A later `connect` or `request` opens it again.
    pub async fn disconnect(&self) {
        // Fails once the supervisor's task ends
        let _ = self.shared().stop(None).changed().await;
    }

    /// Connects if needed, then sends a request and waits for its reply,
    /// which is checked to match the request type.
    ///
    /// Requests have no built-in timeout. Dropping the future stops waiting
    /// for the reply, but a request that was already queued may still be sent
    /// and take effect.
    pub async fn request(
        &self,
        request: ClientWsRequest,
    ) -> Result<ClientWsReply, Error> {
        self.request_with(request, RequestOptions::default()).await
    }

    /// [`Connection::request`] with options.
    pub async fn request_with(
        &self,
        request: ClientWsRequest,
        options: RequestOptions,
    ) -> Result<ClientWsReply, Error> {
        let shared = self.shared();
        let config = &shared.config;
        // A request that does not connect stays on the socket connected when
        // it starts
        let initial = shared.lock().session.clone();
        let request_id = RequestId::new();
        // Encoded before connecting, so an invalid request does not connect
        let text = serde_json::to_string(&Outgoing::Request {
            request_id,
            request: &request,
        })
        .map_err(|error| Error::InvalidRequest {
            cause: Arc::new(error),
        })?;
        let session = if options.connect {
            self.connect().await?;
            shared.lock().session.clone()
        } else {
            initial.filter(|initial| shared.lock().is_active(initial))
        }
        .ok_or(Error::Disconnected)?;

        let capability = request.capability();
        if !session.supports(&capability) {
            // Without an issue when this client does not offer it
            let offered = preferred_client_websocket_version(&capability)
                .is_some_and(|version| {
                    config
                        .capabilities
                        .get(&capability)
                        .is_some_and(|versions| versions.contains(&version))
                });
            return Err(Error::UnsupportedCapability {
                host: config.host.clone(),
                issue: offered
                    .then(|| session.issue(config, &capability).message),
                capability,
            });
        }

        let received = session.register(request_id)?;
        let _abandon = Abandon {
            session: &session,
            request_id,
        };
        // A failed send means the socket closed, which fails the request
        let _ = session.outbound.send(text);
        match received
            .await
            .unwrap_or(Outcome::Failed(Error::Disconnected))
        {
            Outcome::Reply(reply) if request.expects_reply(&reply) => {
                Ok(*reply)
            }
            Outcome::Reply(reply) => Err(Error::protocol(format!(
                "Unexpected reply type for {}: received {}",
                type_name(&request),
                type_name(&reply)
            ))),
            Outcome::HostError(error) => {
                Err(Error::from_reply(error, &config.host, capability))
            }
            Outcome::Failed(error) => Err(error),
        }
    }

    pub fn status(&self) -> Status {
        self.shared().lock().status
    }

    /// The current status, then each change.
    pub fn status_changes(&self) -> mpsc::UnboundedReceiver<Status> {
        let mut state = self.shared().lock();
        let status = state.status;
        state.statuses.subscribe(Some(status))
    }

    /// Updates the host pushes from now on, across reconnects. The receiver
    /// ends once every clone of the connection is dropped.
    pub fn updates(&self) -> mpsc::UnboundedReceiver<ClientWsUpdate> {
        self.shared().lock().updates.subscribe(None)
    }

    /// Problems outside of any one request, such as closes and bad messages:
    /// [`Error::Connection`], [`Error::IncompatibleHost`], and
    /// [`Error::Protocol`].
    pub fn errors(&self) -> mpsc::UnboundedReceiver<Error> {
        self.shared().lock().issues.subscribe(None)
    }

    /// Offered capabilities the host did not agree to, or `None` until
    /// negotiation completes.
    pub fn capability_issues(&self) -> Option<Vec<CapabilityIssue>> {
        let shared = self.shared();
        let session = shared.lock().session.clone()?;
        Some(
            shared
                .config
                .capabilities
                .keys()
                .filter(|id| !session.negotiated.contains_key(*id))
                .map(|capability| session.issue(&shared.config, capability))
                .collect(),
        )
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // The old socket closes in the background
        drop(self.0.stop(None));
        let mut state = self.0.lock();
        state.statuses = Subscribers::default();
        state.updates = Subscribers::default();
        state.issues = Subscribers::default();
    }
}

// A request envelope that borrows its request
#[derive(Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum Outgoing<'a> {
    Request {
        request_id: RequestId,
        request: &'a ClientWsRequest,
    },
}

// A registered supervisor's task, which owns its cleanup before it is first
// polled, so the connection recovers even if the runtime drops it unpolled
struct Task {
    cleanup: Cleanup,
    cancelled: oneshot::Receiver<()>,
}

impl Task {
    async fn run(self) {
        let Self {
            cleanup,
            mut cancelled,
        } = self;
        cleanup.shared.supervise(cleanup.id, &mut cancelled).await;
    }
}

// Disconnects if a supervisor ends without retiring itself, such as when it
// panics or its runtime shuts down
struct Cleanup {
    shared: Arc<Shared>,
    id: u64,
    // Dropped after the cleanup, which closes `State::closing`
    _finish: watch::Sender<()>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        drop(self.shared.stop(Some(self.id)));
    }
}

// Marks a request abandoned if it is dropped before it is settled
struct Abandon<'a> {
    session: &'a Session,
    request_id: RequestId,
}

impl Drop for Abandon<'_> {
    fn drop(&mut self) {
        let mut pending = self.session.lock();
        if let Some(waiting) = pending.waiting.get_mut(&self.request_id) {
            *waiting = None;
        }
    }
}

impl State {
    fn set_status(&mut self, status: Status) {
        if self.status != status {
            self.status = status;
            self.statuses.publish(&status);
        }
    }

    fn is_active(&self, session: &Session) -> bool {
        self.session
            .as_ref()
            .is_some_and(|active| std::ptr::eq(Arc::as_ptr(active), session))
    }

    fn is_current(&self, id: u64) -> bool {
        self.supervisor
            .as_ref()
            .is_some_and(|supervisor| supervisor.id == id)
    }
}

impl Session {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// Whether the host agreed to the version of a capability this client
    /// uses.
    fn supports(&self, capability: &Capability) -> bool {
        preferred_client_websocket_version(capability).is_some_and(|version| {
            self.negotiated.get(capability) == Some(&version)
        })
    }

    fn register(
        &self,
        request_id: RequestId,
    ) -> Result<oneshot::Receiver<Outcome>, Error> {
        let mut pending = self.lock();
        if let Some(error) = &pending.ended {
            return Err(error.clone());
        }
        let (sender, receiver) = oneshot::channel();
        pending.waiting.insert(request_id, Some(sender));
        Ok(receiver)
    }

    /// Settles a request with what the host sent for it, returning whether
    /// the request was waiting or abandoned.
    fn settle(&self, request_id: Option<RequestId>, outcome: Outcome) -> bool {
        let Some(waiting) =
            request_id.and_then(|id| self.lock().waiting.remove(&id))
        else {
            return false;
        };
        if let Some(sender) = waiting {
            let _ = sender.send(outcome);
        }
        true
    }

    /// Fails every waiting request with why the session ended, and any later
    /// ones.
    fn release(&self, error: &Error) {
        let mut pending = self.lock();
        if pending.ended.is_some() {
            return;
        }
        pending.ended = Some(error.clone());
        for sender in pending.waiting.drain().filter_map(|(_, sender)| sender) {
            let _ = sender.send(Outcome::Failed(error.clone()));
        }
    }

    fn issue(
        &self,
        config: &Config,
        capability: &Capability,
    ) -> CapabilityIssue {
        CapabilityIssue::new(
            capability.clone(),
            config
                .capabilities
                .get(capability)
                .cloned()
                .unwrap_or_default(),
            self.rejected
                .as_ref()
                .and_then(|rejected| rejected.get(capability).cloned()),
        )
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Installs a supervisor, returning the task to spawn for it.
    fn register(self: &Arc<Self>, state: &mut State) -> Task {
        let id = state.next_supervisor_id;
        state.next_supervisor_id += 1;
        let (cancel, cancelled) = oneshot::channel();
        let (finish, finished) = watch::channel(());
        state.supervisor = Some(Supervisor {
            id,
            _cancel: cancel,
        });
        state.closing = finished;
        state.set_status(Status::Connecting);
        Task {
            cleanup: Cleanup {
                shared: Arc::clone(self),
                id,
                _finish: finish,
            },
            cancelled,
        }
    }

    /// Stops the supervisor, or only the one with this ID, and reports
    /// `disconnected` before failing anything, returning the supervisor's
    /// task's completion, once it has closed its socket.
    fn stop(&self, only: Option<u64>) -> watch::Receiver<()> {
        let mut state = self.lock();
        let closing = state.closing.clone();
        if let Some(id) = only
            && !state.is_current(id)
        {
            return closing;
        }
        // Signals the supervisor before releasing anyone, so its socket stops
        // first
        state.supervisor = None;
        state.has_connected_once = false;
        state.wake = None;
        let waiting = mem::take(&mut state.ready);
        state.set_status(Status::Disconnected);
        if let Some(session) = state.session.take() {
            session.release(&Error::Disconnected);
        }
        drop(state);
        for sender in waiting {
            let _ = sender.send(Err(Error::Disconnected));
        }
        closing
    }

    /// Publishes a problem with a session's socket while it is the active
    /// one.
    fn publish_issue(&self, session: &Session, error: Error) {
        let mut state = self.lock();
        if state.is_active(session) {
            state.issues.publish(&error);
        }
    }

    /// The sole owner of reconnects, running until it gives up or is stopped.
    async fn supervise(&self, id: u64, cancelled: &mut oneshot::Receiver<()>) {
        let mut attempt = 0;
        loop {
            let Some(error) = self.run_attempt(id, cancelled).await else {
                return;
            };
            let Some((delay, woken)) =
                self.settle_attempt(id, error, &mut attempt)
            else {
                return;
            };
            tokio::select! {
                _ = &mut *cancelled => return,
                _ = woken => {}
                () = time::sleep(delay) => {}
            }
        }
    }

    /// One socket, from opening through negotiation until it closes,
    /// returning why it ended, or `None` if the supervisor was stopped.
    async fn run_attempt(
        &self,
        id: u64,
        cancelled: &mut oneshot::Receiver<()>,
    ) -> Option<Error> {
        let mut dial = {
            let mut state = self.lock();
            if !state.is_current(id) {
                return None;
            }
            state.wake = None;
            state.opened.unwrap_or(Dial::Primary)
        };
        loop {
            let url = match (dial, &self.config.insecure_url) {
                (Dial::Fallback, Some(insecure_url)) => insecure_url,
                _ => &self.config.url,
            };
            let dialed = tokio::select! {
                _ = &mut *cancelled => return None,
                dialed = time::timeout(OPEN_TIMEOUT, self::dial(url)) => dialed,
            };
            let cause: Cause = match dialed {
                Ok(Ok(socket)) => {
                    return self.run_socket(id, dial, socket, cancelled).await;
                }
                Ok(Err(cause)) => cause,
                Err(elapsed) => Arc::new(elapsed),
            };
            // Falls back right away while no socket has opened, but starts
            // each later attempt from `wss` until one does, so a host that is
            // down does not leave the connection on `ws`
            if dial == Dial::Fallback
                || self.config.insecure_url.is_none()
                || self.lock().opened.is_some()
            {
                return Some(SocketEnd::Failed(Some(cause)).into_error());
            }
            dial = Dial::Fallback;
        }
    }

    /// Negotiates an open socket and handles it until it closes.
    async fn run_socket(
        &self,
        id: u64,
        dial: Dial,
        socket: Socket,
        cancelled: &mut oneshot::Receiver<()>,
    ) -> Option<Error> {
        {
            let mut state = self.lock();
            if !state.is_current(id) {
                return None;
            }
            state.opened.get_or_insert(dial);
        }
        let (mut sink, mut stream) = socket.split();
        let negotiated = tokio::select! {
            _ = &mut *cancelled => return None,
            negotiated = self.negotiate(&mut sink, &mut stream) => negotiated,
        };
        let welcome = match negotiated {
            Ok(welcome) => welcome,
            Err(error) => return Some(error),
        };

        let (outbound, outgoing) = mpsc::unbounded_channel();
        let session = Arc::new(Session {
            negotiated: welcome.capabilities,
            rejected: welcome.rejected_capabilities,
            outbound,
            pending: Mutex::default(),
        });
        {
            let mut state = self.lock();
            if !state.is_current(id) {
                return None;
            }
            state.has_connected_once = true;
            // Installing the session as `connected` is published, so
            // subscribers see both together
            state.session = Some(Arc::clone(&session));
            state.set_status(Status::Connected);
            for sender in mem::take(&mut state.ready) {
                let _ = sender.send(Ok(()));
            }
        }
        self.run_session(&session, sink, stream, outgoing, cancelled)
            .await
    }

    async fn negotiate(
        &self,
        sink: &mut Sink,
        stream: &mut Stream,
    ) -> Result<CapabilityWelcome, Error> {
        let hello = ClientWsEnvelope::Hello(CapabilityHello {
            negotiation_version: NEGOTIATION_VERSION,
            capabilities: self.config.capabilities.clone(),
        });
        let hello = serde_json::to_string(&hello)
            .expect("capability hellos serialize to JSON");
        let first = time::timeout(NEGOTIATION_TIMEOUT, async {
            sink.send(Message::text(hello))
                .await
                .map_err(|error| SocketEnd::Failed(Some(Arc::new(error))))?;
            next_data(stream).await
        })
        .await;
        let first = match first {
            Ok(Ok(first)) => first,
            Ok(Err(end)) => {
                acknowledge_close(&end, sink).await;
                return Err(self.negotiation_error(end));
            }
            // A slow host is not evidence of incompatibility, so this ends with
            // however the socket closes, normally following the reconnect path
            Err(_) => {
                let end =
                    close(sink, stream, "Capability negotiation timed out")
                        .await;
                return Err(self.negotiation_error(end));
            }
        };
        let welcome = match first {
            Message::Text(text) => parse_welcome(&text),
            _ => None,
        };
        match welcome {
            Some(welcome)
                if is_valid_welcome(&self.config.capabilities, &welcome) =>
            {
                Ok(welcome)
            }
            _ => {
                const REASON: &str = "Invalid capability negotiation response";
                close(sink, stream, REASON).await;
                Err(self.incompatible(REASON))
            }
        }
    }

    fn incompatible(&self, reason: &str) -> Error {
        Error::IncompatibleHost {
            host: self.config.host.clone(),
            reason: reason.into(),
        }
    }

    /// A 1002 only means an incompatible host during negotiation. Later, it is
    /// an ordinary protocol error that reconnecting can recover from.
    fn negotiation_error(&self, end: SocketEnd) -> Error {
        match end {
            SocketEnd::Closed { code, reason }
                if code == PROTOCOL_ERROR_CLOSE_CODE =>
            {
                self.incompatible(&reason)
            }
            end => end.into_error(),
        }
    }

    /// Handles a negotiated socket until it ends, returning why, or `None` if
    /// the supervisor was stopped.
    async fn run_session(
        &self,
        session: &Session,
        mut sink: Sink,
        mut stream: Stream,
        mut outgoing: mpsc::UnboundedReceiver<String>,
        cancelled: &mut oneshot::Receiver<()>,
    ) -> Option<Error> {
        // Writes run alongside reads, so a host that stops reading cannot hold
        // up replies, its close frame, or stopping
        let write = async {
            while let Some(text) = outgoing.recv().await {
                if let Err(error) = sink.send(Message::text(text)).await {
                    return SocketEnd::Failed(Some(Arc::new(error)));
                }
            }
            std::future::pending().await
        };
        let read = async {
            loop {
                match next_data(&mut stream).await {
                    Ok(Message::Text(text)) => {
                        self.handle_frame(session, &text)
                    }
                    Ok(_) => self.publish_issue(
                        session,
                        Error::protocol(
                            "Unsupported websocket message type: binary",
                        ),
                    ),
                    Err(end) => return end,
                }
            }
        };
        // Stopping comes first, while reads and writes take turns
        let ended = async {
            tokio::select! {
                end = read => end,
                end = write => end,
            }
        };
        let end = tokio::select! {
            biased;
            _ = &mut *cancelled => None,
            end = ended => Some(end),
        };
        let Some(end) = end else {
            close(&mut sink, &mut stream, "").await;
            return None;
        };
        acknowledge_close(&end, &mut sink).await;
        Some(end.into_error())
    }

    fn handle_frame(&self, session: &Session, text: &str) {
        let envelope = match serde_json::from_str::<ClientWsEnvelope>(text) {
            Ok(envelope) => envelope,
            Err(error) => {
                let error = Error::Protocol {
                    reason: "Invalid message received".into(),
                    cause: Some(Arc::new(error)),
                };
                // A reply that fails to decode still settles its request,
                // which would otherwise wait forever
                if !session.settle(
                    unparsed_request_id(text),
                    Outcome::Failed(error.clone()),
                ) {
                    self.publish_issue(session, error);
                }
                return;
            }
        };
        let (kind, request_id, outcome) = match envelope {
            ClientWsEnvelope::Update { update, .. } => {
                let capability = update.capability();
                let mut state = self.lock();
                if !state.is_active(session) {
                    return;
                }
                if !session.supports(&capability) {
                    state.issues.publish(&Error::protocol(format!(
                        "Received update for unnegotiated capability {capability}"
                    )));
                } else {
                    state.updates.publish(&update);
                }
                return;
            }
            ClientWsEnvelope::Reply {
                request_id, reply, ..
            } => ("reply", Some(request_id), Outcome::Reply(Box::new(reply))),
            ClientWsEnvelope::Error {
                request_id, error, ..
            } => ("error", request_id, Outcome::HostError(error)),
            ClientWsEnvelope::Hello(_)
            | ClientWsEnvelope::Welcome(_)
            | ClientWsEnvelope::Request { .. } => {
                return self.publish_issue(
                    session,
                    Error::protocol("Invalid message received"),
                );
            }
        };
        if !session.settle(request_id, outcome) {
            let request_id =
                request_id.map_or("null".into(), |id| id.as_uuid().to_string());
            self.publish_issue(
                session,
                Error::protocol(format!(
                    "Received {kind} for unknown request_id: {request_id}"
                )),
            );
        }
    }

    /// Reports how an attempt ended, returning how long to wait before the
    /// next one and what can wake it early, or `None` to stop.
    fn settle_attempt(
        &self,
        id: u64,
        error: Error,
        attempt: &mut u32,
    ) -> Option<(Duration, oneshot::Receiver<()>)> {
        let mut state = self.lock();
        // A stopped supervisor's session was released when it stopped
        if !state.is_current(id) {
            return None;
        }
        let session = state.session.take();
        if session.is_some() {
            *attempt = 0;
        }
        let retry = matches!(error, Error::Connection { .. })
            && self.config.auto_reconnect
            && self
                .config
                .max_reconnect_attempts
                .is_none_or(|max| *attempt < max);
        state.issues.publish(&error);
        let mut waiting = Vec::new();
        let next = if retry {
            *attempt += 1;
            let status = if state.has_connected_once {
                Status::Reconnecting
            } else {
                Status::Connecting
            };
            state.set_status(status);
            let (wake, woken) = oneshot::channel();
            state.wake = Some(wake);
            Some((self.reconnect_delay(*attempt), woken))
        } else {
            state.supervisor = None;
            waiting = mem::take(&mut state.ready);
            state.set_status(Status::Disconnected);
            None
        };
        // Requests and waiters fail after the status is reported
        if let Some(session) = session {
            session.release(&error);
        }
        drop(state);
        for sender in waiting {
            let _ = sender.send(Err(error.clone()));
        }
        next
    }

    fn reconnect_delay(&self, attempt: u32) -> Duration {
        let config = &self.config;
        let exponent =
            i32::try_from(attempt.saturating_sub(1)).unwrap_or(i32::MAX);
        let initial = config.reconnect_initial_delay.as_secs_f64();
        let max = config.reconnect_max_delay.as_secs_f64();
        // Zero stays zero, rather than becoming NaN once the growth overflows
        let base = if initial == 0.0 {
            0.0
        } else {
            (initial * config.reconnect_backoff_multiplier.powi(exponent))
                .min(max)
        };
        let jitter = base * config.reconnect_jitter_ratio;
        let min = (base - jitter).max(0.0);
        let delay = min + rand::random::<f64>() * (base + jitter - min);
        Duration::try_from_secs_f64(delay).unwrap_or(config.reconnect_max_delay)
    }
}

// How a socket stopped delivering messages
enum SocketEnd {
    Closed { code: u16, reason: String },
    Failed(Option<Cause>),
}

impl SocketEnd {
    fn into_error(self) -> Error {
        match self {
            Self::Closed { code, reason } => Error::connection(code, reason),
            Self::Failed(cause) => Error::Connection {
                code: ABNORMAL_CLOSE_CODE,
                reason: String::new(),
                cause,
            },
        }
    }
}

/// Opens a socket, verifying TLS certificates as `reqwest` does.
async fn dial(url: &str) -> Result<Socket, Cause> {
    let connector = if url.starts_with("wss:") {
        Connector::Rustls(
            tls_config().map_err(|error| -> Cause { Arc::new(error) })?,
        )
    } else {
        Connector::Plain
    };
    let (socket, _) =
        // Nagle's algorithm would hold back small requests sent together
        connect_async_tls_with_config(url, None, true, Some(connector))
            .await
            .map_err(|error| -> Cause { Arc::new(error) })?;
    Ok(socket)
}

/// The TLS configuration, built once it first succeeds.
fn tls_config() -> Result<Arc<ClientConfig>, rustls::Error> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    if let Some(config) = CONFIG.get() {
        return Ok(Arc::clone(config));
    }
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()?
        .with_no_client_auth();
    Ok(Arc::clone(CONFIG.get_or_init(|| Arc::new(config))))
}

/// Sends the reply tungstenite queued to the host's close frame, if it sent
/// one.
async fn acknowledge_close(end: &SocketEnd, sink: &mut Sink) {
    if matches!(end, SocketEnd::Closed { .. }) {
        let _ = time::timeout(CLOSE_TIMEOUT, sink.flush()).await;
    }
}

/// Sends a close frame and waits briefly for the socket to close, returning
/// how it ended.
async fn close(
    sink: &mut Sink,
    stream: &mut Stream,
    reason: &str,
) -> SocketEnd {
    let closed = async {
        let _ = sink
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Normal,
                reason: reason.into(),
            })))
            .await;
        loop {
            if let Err(end) = next_data(stream).await {
                return end;
            }
        }
    };
    time::timeout(CLOSE_TIMEOUT, closed)
        .await
        .unwrap_or(SocketEnd::Failed(None))
}

/// The next text or binary message, skipping control frames, or how the
/// socket ended.
async fn next_data(stream: &mut Stream) -> Result<Message, SocketEnd> {
    loop {
        match stream.next().await {
            Some(Ok(message @ (Message::Text(_) | Message::Binary(_)))) => {
                return Ok(message);
            }
            Some(Ok(Message::Close(frame))) => {
                return Err(match frame {
                    Some(frame) => SocketEnd::Closed {
                        code: frame.code.into(),
                        reason: frame.reason.trim().into(),
                    },
                    None => SocketEnd::Closed {
                        code: NO_STATUS_CLOSE_CODE,
                        reason: String::new(),
                    },
                });
            }
            Some(Ok(_)) => {}
            Some(Err(error)) => {
                return Err(SocketEnd::Failed(Some(Arc::new(error))));
            }
            None => return Err(SocketEnd::Failed(None)),
        }
    }
}

/// Whether a welcome agrees to and resolves exactly the offered capabilities.
fn is_valid_welcome(
    offered: &SupportedCapabilities,
    welcome: &CapabilityWelcome,
) -> bool {
    let agreed = &welcome.capabilities;
    if welcome.negotiation_version != NEGOTIATION_VERSION
        || !agreed.iter().all(|(id, version)| {
            offered
                .get(id)
                .is_some_and(|versions| versions.contains(version))
        })
    {
        return false;
    }
    // New hosts must resolve every offer exactly once. Legacy hosts omit the
    // entire field, so omission cannot be mistaken for missing support.
    let Some(rejected) = &welcome.rejected_capabilities else {
        return true;
    };
    let mut unagreed =
        offered.iter().filter(|(id, _)| !agreed.contains_key(*id));
    rejected.len() == unagreed.clone().count()
        && unagreed.all(|(id, offered_versions)| {
            rejected.get(id).is_some_and(|versions| {
                !versions.iter().any(|v| offered_versions.contains(v))
            })
        })
}

/// A URL without its credentials, query, or fragment, which may hold secrets.
fn redact(url: &str, mut parsed: url::Url) -> String {
    if parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
    {
        return url.into();
    }
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.set_query(None);
    parsed.set_fragment(None);
    parsed.into()
}

/// A welcome, which must have a map of rejected capabilities if it has the
/// field at all.
fn parse_welcome(text: &str) -> Option<CapabilityWelcome> {
    #[derive(Deserialize)]
    struct Envelope {
        data: Data,
    }
    #[derive(Deserialize)]
    struct Data {
        // Whether the field is present, since serde reads `null` as absent
        #[serde(default, deserialize_with = "present")]
        rejected_capabilities: bool,
    }
    fn present<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<bool, D::Error> {
        IgnoredAny::deserialize(deserializer).map(|_| true)
    }
    let ClientWsEnvelope::Welcome(welcome) = serde_json::from_str(text).ok()?
    else {
        return None;
    };
    let explicit = serde_json::from_str::<Envelope>(text).ok()?;
    if welcome.rejected_capabilities.is_none()
        && explicit.data.rejected_capabilities
    {
        return None;
    }
    Some(welcome)
}

/// The request ID of a reply or error that failed to decode, if it has one.
/// The payload is skipped, since it may be what failed.
fn unparsed_request_id(text: &str) -> Option<RequestId> {
    // A plain struct skips the payload whatever order the fields are in
    #[derive(Deserialize)]
    struct Envelope {
        #[serde(rename = "type")]
        _kind: Kind,
        data: Data,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Kind {
        Reply,
        Error,
    }
    #[derive(Deserialize)]
    struct Data {
        request_id: Option<RequestId>,
    }
    let envelope = serde_json::from_str::<Envelope>(text).ok()?;
    envelope.data.request_id
}

/// The `type` of a request or reply, for error messages.
fn type_name(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| Some(value.get("type")?.as_str()?.to_owned()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use runelink_types::capability::CapabilityVersion;

    use super::*;

    const V1: CapabilityVersion = CapabilityVersion::new(2026, 10, 16);
    const V2: CapabilityVersion = CapabilityVersion::new(2026, 10, 17);

    fn welcome(
        capabilities: NegotiatedCapabilities,
        rejected: Option<SupportedCapabilities>,
    ) -> CapabilityWelcome {
        CapabilityWelcome {
            negotiation_version: NEGOTIATION_VERSION,
            capabilities,
            rejected_capabilities: rejected,
            server_version: "test".into(),
        }
    }

    #[test]
    fn welcomes_must_resolve_exactly_the_offered_capabilities() {
        let offered = SupportedCapabilities::from([
            (Capability::Ping, vec![V1]),
            (Capability::MessagesRead, vec![V2]),
        ]);
        let agreed = NegotiatedCapabilities::from([(Capability::Ping, V1)]);
        let valid = [
            None,
            Some(SupportedCapabilities::from([(
                Capability::MessagesRead,
                vec![V1],
            )])),
            Some(SupportedCapabilities::from([(
                Capability::MessagesRead,
                vec![],
            )])),
        ];
        for rejected in valid {
            assert!(is_valid_welcome(
                &offered,
                &welcome(agreed.clone(), rejected)
            ));
        }
        let invalid = [
            (agreed.clone(), Some(SupportedCapabilities::new())),
            // Rejects a version it could have agreed to
            (
                agreed.clone(),
                Some(SupportedCapabilities::from([(
                    Capability::MessagesRead,
                    vec![V2],
                )])),
            ),
            // Rejects an agreed capability
            (
                agreed.clone(),
                Some(SupportedCapabilities::from([
                    (Capability::Ping, vec![]),
                    (Capability::MessagesRead, vec![]),
                ])),
            ),
            // Agrees to a version that was not offered
            (NegotiatedCapabilities::from([(Capability::Ping, V2)]), None),
        ];
        for (agreed, rejected) in invalid {
            assert!(!is_valid_welcome(&offered, &welcome(agreed, rejected)));
        }
    }

    #[test]
    fn options_reject_unimplemented_versions_and_non_websocket_urls() {
        let mut options = ConnectionOptions::url("ws://localhost:7000");
        options.capabilities =
            Some(SupportedCapabilities::from([(Capability::Ping, vec![V2])]));
        assert!(Connection::new(options).is_err());
        for url in [
            "http://localhost:7000",
            "ws://localhost:70000",
            "ws://localhost:http/ws",
            "wss://example.com:7000[]/ws",
        ] {
            assert!(Connection::new(ConnectionOptions::url(url)).is_err());
        }
        for url in ["ws://localhost", "ws://[::1]:7000", "WSS://a.b:1/ws"] {
            assert!(Connection::new(ConnectionOptions::url(url)).is_ok());
        }
        assert!(Connection::new(ConnectionOptions::host("localhost")).is_ok());
        assert!(Connection::new(ConnectionOptions::host("a.b/path")).is_err());
        let url = "wss://user:pass@a.b:1/ws?token=secret#x";
        assert_eq!(
            redact(url, url::Url::parse(url).unwrap()),
            "wss://a.b:1/ws"
        );
    }

    #[test]
    fn undecodable_replies_are_matched_by_their_request_id_alone() {
        let id = "00000000-0000-4000-8000-000000000001";
        let reply = format!(
            r#"{{"type":"reply","data":{{"request_id":"{id}","event_id":"{id}","reply":{{"n":1e400}}}}}}"#
        );
        assert_eq!(
            unparsed_request_id(&reply).map(|id| id.as_uuid().to_string()),
            Some(id.to_string())
        );
        let data_first = format!(
            r#"{{"data":{{"reply":{{"n":1e400}},"request_id":"{id}"}},"type":"error"}}"#
        );
        assert!(unparsed_request_id(&data_first).is_some());
        let duplicate = format!(
            r#"{{"type":"reply","data":{{"request_id":"{id}","request_id":"{id}"}}}}"#
        );
        assert!(unparsed_request_id(&duplicate).is_none());
        assert!(
            unparsed_request_id(r#"{"type":"update","data":{}}"#).is_none()
        );
    }

    #[test]
    fn welcomes_with_null_rejections_are_invalid() {
        let welcome = |rejected: &str| {
            format!(
                r#"{{"type":"welcome","data":{{"negotiation_version":1,"capabilities":{{}},{rejected}"server_version":"x"}}}}"#
            )
        };
        assert!(parse_welcome(&welcome("")).is_some());
        assert!(
            parse_welcome(&welcome(r#""rejected_capabilities":{},"#)).is_some()
        );
        assert!(
            parse_welcome(&welcome(r#""rejected_capabilities":null,"#))
                .is_none()
        );
    }
}
