//! The server actor: the server itself, its channel tree, and its memberships.

use std::sync::Arc;

use runelink_types::{
    channel::{
        NewServerChannel, ServerChannel, ServerChannelContent, ServerChannelId,
        ServerChannelKind, ServerChannelMove, ServerChannelUpdate,
        TextChannelId,
    },
    server::{
        FullServerMembership, NewServer, NewServerMembership, Server, ServerId,
        ServerRole, ServerUpdate,
    },
    user::{User, UserRef},
    ws::{ClientWsUpdate, FederationWsUpdate},
};
use sqlx::{PgConnection, Postgres, Transaction};
use tokio::sync::{Mutex, RwLockWriteGuard, mpsc};

use super::{
    ActorCtx, ActorFacts, Envelope, MAILBOX_CAPACITY, Registry, ServerMailbox,
    ServerState, SharedState, Update,
};
use crate::{
    auth::Authorization,
    auth_service::NAME_REUSE_DELAY,
    channel_tree,
    error::{ApiError, ApiResult},
    queries,
};

#[derive(Debug)]
pub(super) enum ServerOp {
    Create {
        new_server: NewServer,
        creator: UserRef,
    },
    Update(ServerUpdate),
    Delete,
    CreateChannel(NewServerChannel),
    UpdateChannel {
        channel_id: ServerChannelId,
        update: ServerChannelUpdate,
    },
    MoveChannel {
        channel_id: ServerChannelId,
        channel_move: ServerChannelMove,
    },
    DeleteChannel(ServerChannelId),
    UpsertMembership {
        new_membership: NewServerMembership,
        remote_user: Option<User>,
    },
    DeleteMembership(UserRef),
}

#[derive(Debug)]
pub(super) enum ServerOutput {
    Server(Server),
    Channel(ServerChannel),
    Channels(Vec<ServerChannel>),
    Membership(Box<FullServerMembership>),
    Done,
}

/// How a committed operation changes the server's state.
enum Change {
    None,
    /// The server was created with its admin and any default text channel.
    Create {
        admin: UserRef,
        text_channel: Option<TextChannelId>,
    },
    AddMember(UserRef, ServerRole),
    RemoveMember(UserRef),
    AddTextChannel(TextChannelId),
    RemoveTextChannel(TextChannelId),
    Delete,
}

/// The result of an operation, applied once its transaction commits.
struct Outcome {
    output: ServerOutput,
    updates: Vec<Update>,
    change: Change,
}

impl Outcome {
    fn new(output: ServerOutput, update: Update) -> Self {
        Self {
            output,
            updates: vec![update],
            change: Change::None,
        }
    }

    fn with_change(mut self, change: Change) -> Self {
        self.change = change;
        self
    }
}

pub(super) fn spawn(
    ctx: ActorCtx,
    registry: Arc<Mutex<Registry>>,
    server_id: ServerId,
    state: SharedState,
) -> ServerMailbox {
    let (mailbox, inbox) = mpsc::channel(MAILBOX_CAPACITY);
    tokio::spawn(run(ctx, registry, server_id, state, inbox));
    mailbox
}

async fn run(
    ctx: ActorCtx,
    registry: Arc<Mutex<Registry>>,
    server_id: ServerId,
    state: SharedState,
    mut inbox: mpsc::Receiver<Envelope<ServerOp, ServerOutput>>,
) {
    while let Some(Envelope { op, auth, reply }) = inbox.recv().await {
        let mut guard = StopOnPanic(state.write().await);
        let state = &mut *guard.0;
        if state.stopped {
            let _ = reply.send(Err(ApiError::NotFound));
            continue;
        }
        let creating = matches!(op, ServerOp::Create { .. });
        let mut untrusted = false;
        let result = match execute(&ctx, server_id, state, op, auth).await {
            Ok((tx, outcome)) => match tx.commit().await {
                Ok(()) => Ok(outcome),
                Err(error) => {
                    // The commit may have gone through anyway, so the state
                    // is reloaded rather than trusted. If that fails too, the
                    // server's actors stop and start again from the database.
                    match ServerState::load(&ctx, server_id).await {
                        Ok(loaded) => *state = loaded,
                        Err(error) => {
                            log::warn!(
                                "Stopping server {server_id}'s actors after a failed commit: {error}"
                            );
                            untrusted = true;
                        }
                    }
                    Err(error.into())
                }
            },
            Err(error) => Err(error),
        };
        let mut stop = untrusted || (creating && result.is_err());
        let mut removed_channels = Vec::new();
        match result {
            Ok(Outcome {
                output,
                updates,
                change,
            }) => {
                // Members who leave still hear about it, and new ones hear
                // about joining.
                let mut targets = state.targets(&ctx.config);
                match change {
                    Change::None => {}
                    Change::Create {
                        admin,
                        text_channel,
                    } => {
                        targets.add(&ctx.config, &admin);
                        state.members.insert(admin, ServerRole::Admin);
                        state.text_channels.extend(text_channel);
                    }
                    Change::AddMember(user, role) => {
                        if !state.members.contains_key(&user) {
                            targets.add(&ctx.config, &user);
                        }
                        state.members.insert(user, role);
                    }
                    Change::RemoveMember(user) => {
                        state.members.remove(&user);
                    }
                    Change::AddTextChannel(id) => {
                        state.text_channels.insert(id);
                    }
                    Change::RemoveTextChannel(id) => {
                        state.text_channels.remove(&id);
                        removed_channels.push(id);
                    }
                    Change::Delete => stop = true,
                }
                ctx.fanout(&targets, updates).await;
                let _ = reply.send(Ok(output));
            }
            Err(error) => {
                let _ = reply.send(Err(error));
            }
        }
        if stop {
            state.stopped = true;
            removed_channels.extend(state.text_channels.drain());
        }
        drop(guard);

        if stop || !removed_channels.is_empty() {
            let mut registry = registry.lock().await;
            for id in &removed_channels {
                registry.channels.remove(id);
            }
            if stop {
                registry.servers.remove(&server_id);
                registry.stopped_servers += 1;
                // Operations already queued find the server gone.
                inbox.close();
            }
        }
    }
}

/// The server actor's hold on its state. If the actor panics, possibly after a
/// commit that its state doesn't reflect yet, the state is marked stopped as it
/// would be for an untrusted state. Its channel actors then refuse operations
/// and stop until the actor is replaced and the state reloaded.
struct StopOnPanic<'a>(RwLockWriteGuard<'a, ServerState>);

impl Drop for StopOnPanic<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.stopped = true;
            self.0.text_channels.clear();
        }
    }
}

/// Run one operation in its own transaction, which is left to commit.
async fn execute(
    ctx: &ActorCtx,
    server_id: ServerId,
    state: &ServerState,
    op: ServerOp,
    auth: Authorization,
) -> ApiResult<(Transaction<'static, Postgres>, Outcome)> {
    let mut tx = ctx.db_pool.begin().await?;
    let mut facts = ActorFacts {
        conn: &mut tx,
        server_id,
        state,
    };
    auth.check(&ctx.config, &mut facts).await?;
    let outcome = apply(ctx, server_id, &mut tx, op).await?;
    Ok((tx, outcome))
}

async fn apply(
    ctx: &ActorCtx,
    server_id: ServerId,
    conn: &mut PgConnection,
    op: ServerOp,
) -> ApiResult<Outcome> {
    match op {
        ServerOp::Create {
            new_server,
            creator,
        } => create(ctx, server_id, conn, new_server, creator).await,

        ServerOp::Update(update) => {
            let server =
                queries::servers::update(conn, &ctx.config, server_id, &update)
                    .await?;
            Ok(Outcome::new(
                ServerOutput::Server(server.clone()),
                server_upserted(server),
            ))
        }

        ServerOp::Delete => {
            queries::servers::delete(conn, server_id).await?;
            Ok(Outcome::new(
                ServerOutput::Done,
                Update {
                    client: ClientWsUpdate::ServerDeleted { server_id },
                    federation: FederationWsUpdate::ServerDeleted { server_id },
                },
            )
            .with_change(Change::Delete))
        }

        ServerOp::CreateChannel(new_channel) => {
            if let Some(parent_id) = new_channel.parent_channel_id {
                let parent =
                    queries::channels::get_by_id(&mut *conn, parent_id)
                        .await
                        .map_err(|error| match error {
                        ApiError::NotFound => ApiError::BadRequest(
                            "Parent channel not found in specified server"
                                .into(),
                        ),
                        error => error,
                    })?;
                channel_tree::check_parent(server_id, &parent)?;
            }
            let channel =
                queries::channels::insert(conn, server_id, &new_channel)
                    .await?;
            let change = match channel.content {
                ServerChannelContent::Text { text_channel_id } => {
                    Change::AddTextChannel(text_channel_id)
                }
                ServerChannelContent::Category => Change::None,
            };
            Ok(Outcome::new(
                ServerOutput::Channel(channel.clone()),
                channel_upserted(channel),
            )
            .with_change(change))
        }

        ServerOp::UpdateChannel { channel_id, update } => {
            let channel =
                queries::channels::update(conn, server_id, channel_id, &update)
                    .await?;
            Ok(Outcome::new(
                ServerOutput::Channel(channel.clone()),
                channel_upserted(channel),
            ))
        }

        ServerOp::MoveChannel {
            channel_id,
            channel_move,
        } => {
            let channels =
                queries::channels::list(&mut *conn, Some(server_id)).await?;
            let planned =
                channel_tree::plan_move(&channels, channel_id, &channel_move)?;
            let changed =
                queries::channels::update_placements(conn, &planned).await?;
            let updates =
                changed.iter().cloned().map(channel_upserted).collect();
            Ok(Outcome {
                output: ServerOutput::Channels(changed),
                updates,
                change: Change::None,
            })
        }

        ServerOp::DeleteChannel(channel_id) => {
            if queries::channels::has_children(
                &mut *conn, server_id, channel_id,
            )
            .await?
            {
                return Err(ApiError::BadRequest(
                    "Only empty categories can be deleted".into(),
                ));
            }
            let text_channel_id =
                queries::channels::delete(conn, server_id, channel_id).await?;
            let change = match text_channel_id {
                Some(id) => Change::RemoveTextChannel(id),
                None => Change::None,
            };
            Ok(Outcome::new(
                ServerOutput::Done,
                Update {
                    client: ClientWsUpdate::ChannelDeleted {
                        server_id,
                        channel_id,
                    },
                    federation: FederationWsUpdate::ChannelDeleted {
                        server_id,
                        channel_id,
                    },
                },
            )
            .with_change(change))
        }

        ServerOp::UpsertMembership {
            new_membership,
            remote_user,
        } => {
            if let Some(remote_user) = &remote_user {
                queries::users::upsert_remote(
                    &mut *conn,
                    remote_user,
                    NAME_REUSE_DELAY,
                )
                .await?;
            }
            let membership =
                upsert_membership(ctx, conn, &new_membership).await?;
            let change =
                Change::AddMember(membership.user.as_ref(), membership.role);
            Ok(Outcome::new(
                ServerOutput::Membership(Box::new(membership.clone())),
                membership_upserted(membership),
            )
            .with_change(change))
        }

        ServerOp::DeleteMembership(user_ref) => {
            queries::memberships::delete_local(
                &mut *conn,
                server_id,
                user_ref.clone(),
            )
            .await?;
            Ok(Outcome::new(
                ServerOutput::Done,
                Update {
                    client: ClientWsUpdate::MembershipDeleted {
                        server_id,
                        user_ref: user_ref.clone(),
                    },
                    federation: FederationWsUpdate::MembershipDeleted {
                        server_id,
                        user_ref: user_ref.clone(),
                    },
                },
            )
            .with_change(Change::RemoveMember(user_ref)))
        }
    }
}

/// Create the server along with its creator's admin membership, so that it is
/// never visible without an admin, and any default channels.
async fn create(
    ctx: &ActorCtx,
    server_id: ServerId,
    conn: &mut PgConnection,
    new_server: NewServer,
    creator: UserRef,
) -> ApiResult<Outcome> {
    if ctx.config.locality(&creator.host).is_local() {
        // Never recreate a home user deleted while this request waited
        queries::users::get_by_ref(&mut *conn, creator.clone()).await?;
    } else {
        // Creates the record for federated users from other hosts
        queries::users::ensure_exists(
            &mut *conn,
            creator.clone(),
            NAME_REUSE_DELAY,
        )
        .await?;
    }
    let server = queries::servers::insert(
        &mut *conn,
        &ctx.config,
        server_id,
        &new_server,
    )
    .await?;
    let new_membership = NewServerMembership {
        user_ref: creator,
        server_id,
        server_host: server.host.clone(),
        role: ServerRole::Admin,
    };
    let membership = upsert_membership(ctx, conn, &new_membership).await?;
    // The creator hears about the server before their membership.
    let mut updates = vec![
        server_upserted(server.clone()),
        membership_upserted(membership),
    ];
    let mut text_channel = None;
    if new_server.default_channels {
        let new_channel = NewServerChannel {
            kind: ServerChannelKind::Text,
            title: "general".into(),
            description: None,
            metadata: None,
            parent_channel_id: None,
        };
        let channel =
            queries::channels::insert(conn, server_id, &new_channel).await?;
        if let ServerChannelContent::Text { text_channel_id } = channel.content
        {
            text_channel = Some(text_channel_id);
        }
        updates.push(channel_upserted(channel));
    }
    Ok(Outcome {
        output: ServerOutput::Server(server),
        updates,
        change: Change::Create {
            admin: new_membership.user_ref,
            text_channel,
        },
    })
}

async fn upsert_membership(
    ctx: &ActorCtx,
    conn: &mut PgConnection,
    new_membership: &NewServerMembership,
) -> ApiResult<FullServerMembership> {
    let member =
        queries::memberships::upsert_local(conn, new_membership).await?;
    let membership = queries::memberships::get_local_by_user_and_server(
        &mut *conn,
        &ctx.config,
        new_membership.server_id,
        new_membership.user_ref.clone(),
    )
    .await?;
    Ok(FullServerMembership {
        server: membership.server,
        user: member.user,
        role: membership.role,
        joined_at: membership.joined_at,
        updated_at: membership.updated_at,
        synced_at: membership.synced_at,
    })
}

fn server_upserted(server: Server) -> Update {
    Update {
        client: ClientWsUpdate::ServerUpserted(server.clone()),
        federation: FederationWsUpdate::ServerUpserted(server),
    }
}

fn channel_upserted(channel: ServerChannel) -> Update {
    Update {
        client: ClientWsUpdate::ChannelUpserted(channel.clone()),
        federation: FederationWsUpdate::ChannelUpserted(channel),
    }
}

fn membership_upserted(membership: FullServerMembership) -> Update {
    Update {
        client: ClientWsUpdate::MembershipUpserted(membership.clone()),
        federation: FederationWsUpdate::MembershipUpserted(membership),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_panic_while_holding_the_state_marks_it_stopped() {
        let state = SharedState::default();
        let text_channel_id = TextChannelId::new();
        state.write().await.text_channels.insert(text_channel_id);

        // Released normally, the state is left as it is.
        drop(StopOnPanic(state.write().await));
        {
            let state = state.read().await;
            assert!(!state.stopped);
            assert!(state.text_channels.contains(&text_channel_id));
        }

        // Held across an await, like the actor holds it, so that it is
        // dropped along with the panicked task.
        let held = state.clone();
        let task = tokio::spawn(async move {
            let _state = StopOnPanic(held.write().await);
            tokio::task::yield_now().await;
            panic!("panic while holding the state");
        });
        assert!(task.await.unwrap_err().is_panic());
        let state = state.read().await;
        assert!(state.stopped);
        assert!(state.text_channels.is_empty());
    }
}
