//! Presence statuses: of this host's users, which only their home host knows,
//! and of other hosts' users who are in this host's servers, which their home
//! hosts send here and which are kept only in memory.
//!
//! One task handles every change in order, so the statuses it sends for a
//! user are sent in the order they changed. A host sends a user's status to
//! the users who share one of its servers with them: to its own users
//! directly, and to other hosts' users through their home hosts, naming the
//! users who should receive it. Home hosts send their users' statuses to the
//! hosts of every server their users are in.
//!
//! Clients never ask for statuses: a host sends a client the statuses it
//! should know when it connects, when its user joins a server, and whenever
//! they change. Statuses of users that have none sent are offline.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::Duration,
};

use log::{debug, warn};
use runelink_types::{
    capability::Capability,
    host::Host,
    presence::{
        ChosenPresenceStatus, PresenceStatus, RelayedPresenceStatus,
        UserPresenceStatus,
    },
    server::{ServerId, ServerRef},
    user::UserRef,
    ws::{
        ClientWsUpdate, FederationWsReply, FederationWsRequest,
        FederationWsUpdate,
    },
};
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};

use crate::{
    error::{ApiError, ApiResult},
    ids::ConnId,
    queries,
    state::AppState,
};

/// How long a user stays online after their last connection closes, so that
/// reconnecting, such as by reloading a page, doesn't show them offline or
/// reset their chosen status.
const GRACE_PERIOD: Duration = Duration::from_secs(10);
/// How long to wait before trying again to reconnect to a host that the
/// statuses of this host's users are sent to, doubling up to the maximum.
const RECONNECT_INITIAL_DELAY: Duration = Duration::from_secs(1);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Sends changes to the task that keeps presence statuses.
#[derive(Clone, Debug)]
pub struct Presence {
    events: mpsc::UnboundedSender<Event>,
}

/// Changes waiting for [`run`].
pub type PresenceEvents = mpsc::UnboundedReceiver<Event>;

#[derive(Debug)]
pub enum Event {
    Connected {
        conn_id: ConnId,
        user_ref: UserRef,
    },
    Disconnected {
        conn_id: ConnId,
        user_ref: UserRef,
    },
    GraceEnded {
        user_ref: UserRef,
        generation: u64,
    },
    Chosen {
        user_ref: UserRef,
        status: ChosenPresenceStatus,
    },
    /// A status from a remote user's home host.
    HomeStatusUpdated(UserPresenceStatus),
    /// Statuses a server's host sent for local users.
    Relayed {
        sender: Host,
        relayed: Vec<RelayedPresenceStatus>,
    },
    /// A reply to a request for a server host's statuses, empty if it failed.
    RemoteStatuses {
        request: u64,
        statuses: Vec<UserPresenceStatus>,
    },
    PeerConnected(Host),
    PeerDisconnected(Host),
    Joined {
        user_ref: UserRef,
        server_ref: ServerRef,
    },
    /// A user left a local server, or a remote user was deleted. Only remote
    /// users' statuses are kept for local servers.
    Left(UserRef),
    StatusesRequested {
        user_ref: UserRef,
        server_ids: Option<Vec<ServerId>>,
        reply: oneshot::Sender<ApiResult<Vec<UserPresenceStatus>>>,
    },
    NeedsPeer {
        host: Host,
        reply: oneshot::Sender<bool>,
    },
    ReconnectEnded {
        host: Host,
        /// The delay the next attempt would have waited
        delay: Duration,
    },
}

impl Presence {
    /// A handle, and the events it sends, which wait until [`run`] handles
    /// them.
    pub fn new() -> (Self, PresenceEvents) {
        let (events, receiver) = mpsc::unbounded_channel();
        (Self { events }, receiver)
    }

    fn send(&self, event: Event) {
        // Fails only once the host is shutting down.
        let _ = self.events.send(event);
    }

    /// A connection authenticated as a local user.
    pub fn connected(&self, conn_id: ConnId, user_ref: UserRef) {
        self.send(Event::Connected { conn_id, user_ref });
    }

    /// A connection stopped being authenticated as a local user.
    pub fn disconnected(&self, conn_id: ConnId, user_ref: UserRef) {
        self.send(Event::Disconnected { conn_id, user_ref });
    }

    /// A local user chose their status.
    pub fn choose(&self, user_ref: UserRef, status: ChosenPresenceStatus) {
        self.send(Event::Chosen { user_ref, status });
    }

    /// A remote user's home host sent their status.
    pub fn home_status_updated(&self, status: UserPresenceStatus) {
        self.send(Event::HomeStatusUpdated(status));
    }

    pub fn peer_connected(&self, host: Host) {
        self.send(Event::PeerConnected(host));
    }

    /// A connection with a peer closed. Ignored if the peer is still
    /// connected another way.
    pub fn peer_disconnected(&self, host: Host) {
        self.send(Event::PeerDisconnected(host));
    }

    /// A server's host sent statuses for local users.
    pub fn relayed(&self, sender: Host, relayed: Vec<RelayedPresenceStatus>) {
        self.send(Event::Relayed { sender, relayed });
    }

    /// A user joined a local server, or a local user joined a remote one.
    pub fn joined(&self, user_ref: UserRef, server_ref: ServerRef) {
        self.send(Event::Joined {
            user_ref,
            server_ref,
        });
    }

    /// A user left a local server, or a remote user was deleted. Only remote
    /// users' statuses are kept for local servers.
    pub fn left(&self, user_ref: UserRef) {
        self.send(Event::Left(user_ref));
    }

    /// The statuses of the users who share a local server with `user_ref`,
    /// leaving out offline users, or of those who share one of the given ones,
    /// including offline users.
    pub async fn statuses(
        &self,
        user_ref: UserRef,
        server_ids: Option<Vec<ServerId>>,
    ) -> ApiResult<Vec<UserPresenceStatus>> {
        let (reply, receiver) = oneshot::channel();
        self.send(Event::StatusesRequested {
            user_ref,
            server_ids,
            reply,
        });
        receiver.await.map_err(|_| {
            ApiError::Internal("Presence statuses are unavailable".into())
        })?
    }

    async fn needs_peer(&self, host: Host) -> bool {
        let (reply, receiver) = oneshot::channel();
        self.send(Event::NeedsPeer { host, reply });
        receiver.await.unwrap_or(false)
    }
}

/// Handles presence changes until the host shuts down.
pub async fn run(state: AppState, mut events: PresenceEvents) {
    let mut task = PresenceTask {
        state,
        local: HashMap::new(),
        remote: HashMap::new(),
        reconnects: HashMap::new(),
        relayed: HashMap::new(),
        requests: HashMap::new(),
        next_request: 0,
    };
    while let Some(event) = events.recv().await {
        if let Err(error) = task.handle(event).await {
            warn!("Failed handling a presence change: {error}");
        }
    }
}

/// A local user who is online, or whose last connection closed less than
/// the grace period ago.
#[derive(Debug, Default)]
struct LocalUser {
    chosen: ChosenPresenceStatus,
    connections: HashSet<ConnId>,
    /// Counts disconnections, so that a grace period that ended after the
    /// user reconnected and disconnected again is ignored.
    generation: u64,
}

/// Reconnecting to a host, or when it last ended.
enum Reconnect {
    Running,
    /// When it ended, and the delay it reached, so that a host whose
    /// connections keep closing is retried less often.
    Ended {
        at: Instant,
        delay: Duration,
    },
}

/// What the presence task keeps.
struct PresenceTask {
    state: AppState,
    local: HashMap<UserRef, LocalUser>,
    /// Remote members of local servers who aren't offline, as their home hosts
    /// last said.
    remote: HashMap<UserRef, PresenceStatus>,
    reconnects: HashMap<Host, Reconnect>,
    /// The users who aren't offline that each host relayed to each local user,
    /// to show them offline if that host disconnects.
    relayed: HashMap<Host, HashMap<UserRef, HashSet<UserRef>>>,
    /// Requests for a server host's statuses waiting for their replies, by ID.
    requests: HashMap<u64, PendingSnapshot>,
    next_request: u64,
}

/// A request for the statuses a server's host knows for a local user.
struct PendingSnapshot {
    host: Host,
    user_ref: UserRef,
    target: SnapshotTarget,
    /// The users whose statuses the host sent the user meanwhile, which are
    /// newer than those in the reply.
    relayed: HashSet<UserRef>,
}

#[derive(Clone, Copy)]
enum SnapshotTarget {
    Connection(ConnId),
    /// All of the user's connections
    User,
}

impl PresenceTask {
    async fn handle(&mut self, event: Event) -> ApiResult<()> {
        match event {
            Event::Connected { conn_id, user_ref } => {
                let new = !self.local.contains_key(&user_ref);
                let user = self.local.entry(user_ref.clone()).or_default();
                if !user.connections.insert(conn_id) {
                    return Ok(());
                }
                let chosen = user.chosen;
                let sent = self
                    .send_connection_statuses(conn_id, &user_ref, chosen)
                    .await;
                if new {
                    self.announce(&user_ref, chosen.shown()).await?;
                }
                sent?;
            }

            Event::Disconnected { conn_id, user_ref } => {
                let Some(user) = self.local.get_mut(&user_ref) else {
                    return Ok(());
                };
                if !user.connections.remove(&conn_id)
                    || !user.connections.is_empty()
                {
                    return Ok(());
                }
                user.generation += 1;
                let generation = user.generation;
                let presence = self.state.presence.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(GRACE_PERIOD).await;
                    presence.send(Event::GraceEnded {
                        user_ref,
                        generation,
                    });
                });
            }

            Event::GraceEnded {
                user_ref,
                generation,
            } => {
                let Some(user) = self.local.get(&user_ref) else {
                    return Ok(());
                };
                if user.generation != generation || !user.connections.is_empty()
                {
                    return Ok(());
                }
                let shown = user.chosen.shown();
                // Their chosen status goes with them, as do the statuses
                // relayed to them.
                self.local.remove(&user_ref);
                for relayed in self.relayed.values_mut() {
                    relayed.remove(&user_ref);
                }
                if shown != PresenceStatus::Offline {
                    self.announce(&user_ref, PresenceStatus::Offline).await?;
                }
            }

            Event::Chosen { user_ref, status } => {
                let Some(user) = self.local.get_mut(&user_ref) else {
                    return Ok(());
                };
                let shown = user.chosen.shown();
                user.chosen = status;
                let _ = self
                    .state
                    .client_ws_manager
                    .send_update_to_users(
                        [&user_ref],
                        ClientWsUpdate::ChosenPresenceStatusUpdated { status },
                    )
                    .await;
                if status.shown() != shown {
                    self.announce(&user_ref, status.shown()).await?;
                }
            }

            Event::HomeStatusUpdated(UserPresenceStatus {
                user_ref,
                status,
            }) => {
                // Only the statuses of members are kept.
                if self.forget_unless_member(&user_ref).await? {
                    return Ok(());
                }
                let previous = if status == PresenceStatus::Offline {
                    self.remote.remove(&user_ref)
                } else {
                    self.remote.insert(user_ref.clone(), status)
                };
                if previous.unwrap_or(PresenceStatus::Offline) != status {
                    self.fan_out(&user_ref, status, None, None).await?;
                }
            }

            Event::Relayed { sender, relayed } => {
                // Only to users who are in one of the sender's servers
                let members =
                    queries::memberships::get_local_users_in_servers_on(
                        &*self.state.db_pool,
                        &sender,
                    )
                    .await?
                    .into_iter()
                    .collect::<HashSet<_>>();
                let mut by_recipient =
                    HashMap::<UserRef, Vec<UserPresenceStatus>>::new();
                for RelayedPresenceStatus {
                    user_ref,
                    status,
                    recipients,
                } in relayed
                {
                    for recipient in recipients {
                        // Users who aren't online get the statuses once they connect.
                        if members.contains(&recipient)
                            && self.local.contains_key(&recipient)
                        {
                            by_recipient.entry(recipient).or_default().push(
                                UserPresenceStatus {
                                    user_ref: user_ref.clone(),
                                    status,
                                },
                            );
                        }
                    }
                }
                for (recipient, statuses) in by_recipient {
                    for request in self.requests.values_mut() {
                        if request.host == sender
                            && request.user_ref == recipient
                        {
                            request.relayed.extend(
                                statuses
                                    .iter()
                                    .map(|status| status.user_ref.clone()),
                            );
                        }
                    }
                    let co_members = self.co_members(&recipient).await?;
                    for UserPresenceStatus { user_ref, status } in statuses {
                        if let Some(status) = self.relayed_status_for(
                            &sender,
                            &recipient,
                            &user_ref,
                            status,
                            &co_members,
                        ) {
                            self.send_status(
                                &recipient,
                                UserPresenceStatus { user_ref, status },
                            )
                            .await;
                        }
                    }
                }
            }

            Event::RemoteStatuses { request, statuses } => {
                let Some(request) = self.requests.remove(&request) else {
                    return Ok(());
                };
                // Unless they left the host's servers meanwhile
                if !queries::memberships::get_local_users_in_servers_on(
                    &*self.state.db_pool,
                    &request.host,
                )
                .await?
                .contains(&request.user_ref)
                {
                    return Ok(());
                }
                let co_members = self.co_members(&request.user_ref).await?;
                for UserPresenceStatus { user_ref, status } in statuses {
                    if request.relayed.contains(&user_ref) {
                        continue;
                    }
                    let Some(status) = self.relayed_status_for(
                        &request.host,
                        &request.user_ref,
                        &user_ref,
                        status,
                        &co_members,
                    ) else {
                        continue;
                    };
                    let status = UserPresenceStatus { user_ref, status };
                    match request.target {
                        SnapshotTarget::Connection(conn_id) => {
                            self.state
                                .client_ws_manager
                                .send_update_to_connection_as(
                                    conn_id,
                                    &request.user_ref,
                                    ClientWsUpdate::PresenceStatusUpdated(
                                        status,
                                    ),
                                )
                                .await;
                        }
                        SnapshotTarget::User => {
                            self.send_status(&request.user_ref, status).await;
                        }
                    }
                }
            }

            Event::PeerConnected(host) => self.peer_connected(&host).await?,

            Event::PeerDisconnected(host) => {
                self.peer_disconnected(&host).await?
            }

            Event::Joined {
                user_ref,
                server_ref,
            } => {
                let status = self.shown(&user_ref);
                let is_local_user =
                    self.state.config.locality(&user_ref.host).is_local();
                if self.state.config.locality(&server_ref.host).is_local() {
                    // Their status may be known already, from their other
                    // servers here, and members may still show an old one.
                    self.fan_out(
                        &user_ref,
                        status,
                        None,
                        Some(&[server_ref.id]),
                    )
                    .await?;
                    if is_local_user && self.local.contains_key(&user_ref) {
                        let statuses = self
                            .co_member_statuses(
                                &user_ref,
                                Some(&[server_ref.id]),
                            )
                            .await?;
                        for status in statuses {
                            self.send_status(&user_ref, status).await;
                        }
                    }
                } else if is_local_user {
                    let host = server_ref.host.clone();
                    // Offline too, which the host may not know yet.
                    self.send_if_connected(
                        &host,
                        FederationWsUpdate::PresenceStatusUpdated(
                            UserPresenceStatus {
                                user_ref: user_ref.clone(),
                                status,
                            },
                        ),
                    )
                    .await;
                    self.keep_connected(&host).await?;
                    if self.local.contains_key(&user_ref) {
                        self.request_statuses(
                            host,
                            user_ref,
                            Some(vec![server_ref.id]),
                            SnapshotTarget::User,
                        )
                        .await;
                    }
                }
            }

            // Its home host stops sending its status once it shares no server
            // here.
            Event::Left(user_ref) => {
                self.forget_unless_member(&user_ref).await?;
            }

            Event::StatusesRequested {
                user_ref,
                server_ids,
                reply,
            } => {
                let statuses = self
                    .co_member_statuses(&user_ref, server_ids.as_deref())
                    .await;
                let _ = reply.send(statuses);
            }

            Event::NeedsPeer { host, reply } => {
                let _ = reply.send(self.needs_peer(&host).await?);
            }

            Event::ReconnectEnded { host, delay } => {
                self.reconnects.insert(
                    host.clone(),
                    Reconnect::Ended {
                        at: Instant::now(),
                        delay,
                    },
                );
                // It may have closed again before this was handled.
                self.keep_connected(&host).await?;
            }
        }
        Ok(())
    }

    /// Shows the statuses a peer kept current as offline once it has no
    /// connection left, until it reconnects.
    async fn peer_disconnected(&mut self, host: &Host) -> ApiResult<()> {
        // A connection that replaced it keeps statuses current.
        if self.state.federation_ws_manager.is_connected(host).await {
            return Ok(());
        }
        // Replies to come from it would be older than this.
        self.requests.retain(|_, request| request.host != *host);
        let users = self
            .remote
            .keys()
            .filter(|user_ref| user_ref.host == *host)
            .cloned()
            .collect::<Vec<_>>();
        for user_ref in &users {
            self.remote.remove(user_ref);
        }
        let relayed = self.relayed.remove(host).unwrap_or_default();
        self.keep_connected(host).await?;

        // Its users
        for user_ref in users {
            self.fan_out(&user_ref, PresenceStatus::Offline, Some(host), None)
                .await?;
        }
        // The statuses it relayed, unless another host relays them too or
        // this host now sends them directly
        for (recipient, subjects) in relayed {
            let co_members = self.co_members(&recipient).await?;
            for subject in subjects {
                let relayed_elsewhere = self.relayed.values().any(|relayed| {
                    relayed
                        .get(&recipient)
                        .is_some_and(|other| other.contains(&subject))
                });
                if !relayed_elsewhere && !co_members.contains(&subject) {
                    self.send_status(
                        &recipient,
                        UserPresenceStatus {
                            user_ref: subject,
                            status: PresenceStatus::Offline,
                        },
                    )
                    .await;
                }
            }
        }
        Ok(())
    }

    /// What other users see for a user.
    fn shown(&self, user_ref: &UserRef) -> PresenceStatus {
        if self.state.config.locality(&user_ref.host).is_local() {
            self.local
                .get(user_ref)
                .map_or(PresenceStatus::Offline, |user| user.chosen.shown())
        } else {
            self.remote
                .get(user_ref)
                .copied()
                .unwrap_or(PresenceStatus::Offline)
        }
    }

    /// Sends a user's status to another user's connections.
    async fn send_status(
        &self,
        recipient: &UserRef,
        status: UserPresenceStatus,
    ) {
        let _ = self
            .state
            .client_ws_manager
            .send_update_to_users(
                [recipient],
                ClientWsUpdate::PresenceStatusUpdated(status),
            )
            .await;
    }

    /// The other members of the local servers a user is in.
    async fn co_members(
        &self,
        user_ref: &UserRef,
    ) -> ApiResult<HashSet<UserRef>> {
        Ok(queries::memberships::get_local_co_members(
            &*self.state.db_pool,
            user_ref,
            None,
        )
        .await?
        .into_iter()
        .collect())
    }

    /// Drops a remote user's status unless they are in a local server,
    /// returning whether it was dropped.
    async fn forget_unless_member(
        &mut self,
        user_ref: &UserRef,
    ) -> ApiResult<bool> {
        let is_member = !queries::memberships::get_local_server_ids_for_user(
            &*self.state.db_pool,
            user_ref,
        )
        .await?
        .is_empty();
        if !is_member {
            self.remote.remove(user_ref);
        }
        Ok(!is_member)
    }

    /// The status to send a local user that a server's host relayed to them,
    /// if any, which is noted as relayed by that host. This host knows its own
    /// users' statuses better, and sends those of the members of its own
    /// servers directly, given as `co_members`.
    fn relayed_status_for(
        &mut self,
        sender: &Host,
        recipient: &UserRef,
        subject: &UserRef,
        status: PresenceStatus,
        co_members: &HashSet<UserRef>,
    ) -> Option<PresenceStatus> {
        if subject == recipient || co_members.contains(subject) {
            return None;
        }
        let status = if self.state.config.locality(&subject.host).is_local() {
            self.shown(subject)
        } else {
            status
        };
        if status == PresenceStatus::Offline {
            if let Some(relayed) = self.relayed.get_mut(sender)
                && let Some(subjects) = relayed.get_mut(recipient)
            {
                subjects.remove(subject);
                if subjects.is_empty() {
                    relayed.remove(recipient);
                }
            }
        } else {
            self.relayed
                .entry(sender.clone())
                .or_default()
                .entry(recipient.clone())
                .or_default()
                .insert(subject.clone());
        }
        Some(status)
    }

    /// The statuses of the users who share a local server with `user_ref`,
    /// leaving out offline users, or of those who share one of the given ones,
    /// including offline users. A join's statuses correct ones its user may
    /// still show from before, while a connection starts out with everyone
    /// offline.
    async fn co_member_statuses(
        &self,
        user_ref: &UserRef,
        server_ids: Option<&[ServerId]>,
    ) -> ApiResult<Vec<UserPresenceStatus>> {
        let include_offline = server_ids.is_some();
        let co_members = queries::memberships::get_local_co_members(
            &*self.state.db_pool,
            user_ref,
            server_ids,
        )
        .await?;
        Ok(co_members
            .into_iter()
            .filter_map(|user_ref| {
                let status = self.shown(&user_ref);
                (include_offline || status != PresenceStatus::Offline)
                    .then_some(UserPresenceStatus { user_ref, status })
            })
            .collect())
    }

    /// Sends a local user's new status to everyone who should know it.
    async fn announce(
        &mut self,
        user_ref: &UserRef,
        status: PresenceStatus,
    ) -> ApiResult<()> {
        self.fan_out(user_ref, status, None, None).await?;
        for host in queries::memberships::get_remote_server_hosts_for_user(
            &*self.state.db_pool,
            user_ref,
        )
        .await?
        {
            self.send_if_connected(
                &host,
                FederationWsUpdate::PresenceStatusUpdated(UserPresenceStatus {
                    user_ref: user_ref.clone(),
                    status,
                }),
            )
            .await;
            self.keep_connected(&host).await?;
        }
        Ok(())
    }

    /// Sends a user's status to the other members of the local servers they
    /// are in, or of the given ones, except to the users of `except_host`.
    async fn fan_out(
        &self,
        user_ref: &UserRef,
        status: PresenceStatus,
        except_host: Option<&Host>,
        server_ids: Option<&[ServerId]>,
    ) -> ApiResult<()> {
        let co_members = queries::memberships::get_local_co_members(
            &*self.state.db_pool,
            user_ref,
            server_ids,
        )
        .await?;
        let mut local = Vec::new();
        let mut remote = BTreeMap::<Host, Vec<UserRef>>::new();
        for member in co_members {
            if self.state.config.locality(&member.host).is_local() {
                local.push(member);
            } else if Some(&member.host) != except_host {
                remote.entry(member.host.clone()).or_default().push(member);
            }
        }
        let _ = self
            .state
            .client_ws_manager
            .send_update_to_users(
                &local,
                ClientWsUpdate::PresenceStatusUpdated(UserPresenceStatus {
                    user_ref: user_ref.clone(),
                    status,
                }),
            )
            .await;
        // Their home hosts keep connected to this host, and catch up when
        // they reconnect.
        for (host, recipients) in remote {
            self.send_if_connected(
                &host,
                FederationWsUpdate::PresenceStatusesRelayed(vec![
                    RelayedPresenceStatus {
                        user_ref: user_ref.clone(),
                        status,
                        recipients,
                    },
                ]),
            )
            .await;
        }
        Ok(())
    }

    /// Sends an update to `host` if it is connected. Presence only connects to
    /// hosts through [`Self::keep_connected`], since hosts catch up with each
    /// other's statuses once connected.
    async fn send_if_connected(&self, host: &Host, update: FederationWsUpdate) {
        self.state
            .federation_ws_manager
            .send_update_if_connected(host, update)
            .await;
    }

    /// Brings a peer up to date with this host's statuses once connected,
    /// including offline ones, since statuses may have changed while they were
    /// disconnected.
    async fn peer_connected(&mut self, host: &Host) -> ApiResult<()> {
        if !self
            .state
            .federation_ws_manager
            .host_supports(host, Capability::PresenceStatus)
            .await
        {
            return Ok(());
        }
        let db_pool = &*self.state.db_pool;
        // The statuses of local users in the peer's servers
        for user_ref in
            queries::memberships::get_local_users_in_servers_on(db_pool, host)
                .await?
        {
            let status = self.shown(&user_ref);
            self.send_if_connected(
                host,
                FederationWsUpdate::PresenceStatusUpdated(UserPresenceStatus {
                    user_ref,
                    status,
                }),
            )
            .await;
        }
        // The statuses of the members of local servers, for the peer's users
        // who share one with them
        let mut recipients = HashMap::<UserRef, Vec<UserRef>>::new();
        for (user_ref, recipient) in
            queries::memberships::get_local_co_members_from(db_pool, host)
                .await?
        {
            recipients.entry(user_ref).or_default().push(recipient);
        }
        let relayed = recipients
            .into_iter()
            .map(|(user_ref, recipients)| RelayedPresenceStatus {
                status: self.shown(&user_ref),
                user_ref,
                recipients,
            })
            .collect::<Vec<_>>();
        if !relayed.is_empty() {
            self.send_if_connected(
                host,
                FederationWsUpdate::PresenceStatusesRelayed(relayed),
            )
            .await;
        }
        Ok(())
    }

    /// Sends a new connection its user's chosen status and the statuses of
    /// the users who share a server with them.
    async fn send_connection_statuses(
        &mut self,
        conn_id: ConnId,
        user_ref: &UserRef,
        chosen: ChosenPresenceStatus,
    ) -> ApiResult<()> {
        let clients = &self.state.client_ws_manager;
        if !clients.supports(conn_id, Capability::PresenceStatus).await {
            return Ok(());
        }
        // Sent only while the connection is still this user's, since it may
        // have authenticated as someone else since.
        clients
            .send_update_to_connection_as(
                conn_id,
                user_ref,
                ClientWsUpdate::ChosenPresenceStatusUpdated { status: chosen },
            )
            .await;
        for status in self.co_member_statuses(user_ref, None).await? {
            clients
                .send_update_to_connection_as(
                    conn_id,
                    user_ref,
                    ClientWsUpdate::PresenceStatusUpdated(status),
                )
                .await;
        }
        for host in queries::memberships::get_remote_server_hosts_for_user(
            &*self.state.db_pool,
            user_ref,
        )
        .await?
        {
            self.request_statuses(
                host,
                user_ref.clone(),
                None,
                SnapshotTarget::Connection(conn_id),
            )
            .await;
        }
        Ok(())
    }

    /// Asks a server's host for the statuses it knows for a local user, which
    /// are sent to the user once it replies. A host that isn't connected
    /// sends them once it reconnects instead.
    async fn request_statuses(
        &mut self,
        host: Host,
        user_ref: UserRef,
        server_ids: Option<Vec<ServerId>>,
        target: SnapshotTarget,
    ) {
        if !self.state.federation_ws_manager.is_connected(&host).await {
            return;
        }
        let request = self.next_request;
        self.next_request += 1;
        self.requests.insert(
            request,
            PendingSnapshot {
                host: host.clone(),
                user_ref: user_ref.clone(),
                target,
                relayed: HashSet::new(),
            },
        );
        let state = self.state.clone();
        tokio::spawn(async move {
            let statuses =
                remote_statuses(&state, &host, user_ref, server_ids).await;
            state
                .presence
                .send(Event::RemoteStatuses { request, statuses });
        });
    }

    /// Whether a local user who is online is in one of `host`'s servers, so
    /// that their status should be sent there.
    async fn needs_peer(&self, host: &Host) -> ApiResult<bool> {
        Ok(queries::memberships::get_local_users_in_servers_on(
            &*self.state.db_pool,
            host,
        )
        .await?
        .iter()
        .any(|user_ref| self.local.contains_key(user_ref)))
    }

    /// Reconnects to `host` in the background while it is needed.
    async fn keep_connected(&mut self, host: &Host) -> ApiResult<()> {
        if matches!(self.reconnects.get(host), Some(Reconnect::Running))
            || self.state.federation_ws_manager.is_connected(host).await
            || !self.needs_peer(host).await?
        {
            return Ok(());
        }
        // Picks up where the last reconnection left off if it ended recently,
        // since the connection it made has closed again.
        let delay = match self.reconnects.get(host) {
            Some(Reconnect::Ended { at, delay })
                if at.elapsed() < RECONNECT_MAX_DELAY * 2 =>
            {
                Some(*delay)
            }
            _ => None,
        };
        self.reconnects.insert(host.clone(), Reconnect::Running);
        tokio::spawn(reconnect(self.state.clone(), host.clone(), delay));
        Ok(())
    }
}

/// Reconnects to `host`, backing off, until it is connected or no longer
/// needed. It tries right away unless given a delay to start with.
async fn reconnect(state: AppState, host: Host, delay: Option<Duration>) {
    let mut delay = match delay {
        Some(delay) => {
            tokio::time::sleep(delay).await;
            (delay * 2).min(RECONNECT_MAX_DELAY)
        }
        None => RECONNECT_INITIAL_DELAY,
    };
    loop {
        let manager = &state.federation_ws_manager;
        if manager.is_connected(&host).await
            || !state.presence.needs_peer(host.clone()).await
            || manager.ensure_connection(&state, &host).await
        {
            break;
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(RECONNECT_MAX_DELAY);
    }
    state.presence.send(Event::ReconnectEnded { host, delay });
}

/// The statuses `host` knows for the users who share its servers with a
/// local user, or the given ones, or none if it can't say.
async fn remote_statuses(
    state: &AppState,
    host: &Host,
    user_ref: UserRef,
    server_ids: Option<Vec<ServerId>>,
) -> Vec<UserPresenceStatus> {
    let reply = state
        .federation_ws_manager
        .send_request_to_host(
            state,
            host,
            Some(user_ref),
            FederationWsRequest::PresenceStatusesGet { server_ids },
            REQUEST_TIMEOUT,
        )
        .await;
    match reply {
        Ok(FederationWsReply::PresenceStatusesGet(statuses)) => statuses,
        Ok(_) => {
            warn!("Unexpected presence statuses reply from {host}");
            Vec::new()
        }
        // Such as a host that doesn't support presence statuses
        Err(error) => {
            debug!("Could not get presence statuses from {host}: {error}");
            Vec::new()
        }
    }
}
