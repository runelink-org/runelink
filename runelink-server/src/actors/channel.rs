//! The channel actor: the messages of one text channel.

use std::sync::Arc;

use runelink_types::{
    channel::TextChannelId,
    message::{Message, MessageId, MessageUpdate, NewMessage},
    server::ServerId,
    ws::{ClientWsUpdate, FederationWsUpdate},
};
use sqlx::PgConnection;
use time::{Duration, OffsetDateTime};
use tokio::sync::{Mutex, mpsc};

use super::{
    ActorCtx, ActorFacts, ChannelMailbox, Envelope, MAILBOX_CAPACITY, Registry,
    ServerState, SharedState, Update,
};
use crate::{
    auth::Authorization,
    error::{ApiError, ApiResult},
    queries,
};

#[derive(Debug)]
pub(super) enum ChannelOp {
    Create(NewMessage),
    Update {
        message_id: MessageId,
        update: MessageUpdate,
    },
    Delete(MessageId),
}

pub(super) fn spawn(
    ctx: ActorCtx,
    registry: Arc<Mutex<Registry>>,
    server_id: ServerId,
    text_channel_id: TextChannelId,
    state: SharedState,
) -> ChannelMailbox {
    let (mailbox, mut inbox) =
        mpsc::channel::<Envelope<ChannelOp, Message>>(MAILBOX_CAPACITY);
    tokio::spawn(async move {
        let mut clock = Clock::default();
        while let Some(Envelope { op, auth, reply }) = inbox.recv().await {
            // Held until the message is fanned out, so that server operations
            // never interleave with it.
            let state = state.read().await;
            if !state.text_channels.contains(&text_channel_id) {
                let _ = reply.send(Err(ApiError::NotFound));
                drop(state);
                // The channel is gone for good. Its server actor clears it
                // from the registry, unless it was deleted before this actor
                // was registered.
                inbox.close();
                let mut registry = registry.lock().await;
                if registry
                    .channels
                    .get(&text_channel_id)
                    .is_some_and(|mailbox| mailbox.is_closed())
                {
                    registry.channels.remove(&text_channel_id);
                }
                continue;
            }
            let result = execute(
                &ctx,
                server_id,
                text_channel_id,
                &state,
                &mut clock,
                op,
                auth,
            )
            .await;
            let result = match result {
                Ok(message) => {
                    let update = Update {
                        client: ClientWsUpdate::MessageUpserted(
                            message.clone(),
                        ),
                        federation: FederationWsUpdate::MessageUpserted {
                            server_id,
                            message: message.clone(),
                        },
                    };
                    ctx.fanout(&state.targets(&ctx.config), vec![update]).await;
                    Ok(message)
                }
                Err(error) => Err(error),
            };
            let _ = reply.send(result);
        }
    });
    mailbox
}

/// Run one operation in its own transaction.
async fn execute(
    ctx: &ActorCtx,
    server_id: ServerId,
    text_channel_id: TextChannelId,
    state: &ServerState,
    clock: &mut Clock,
    op: ChannelOp,
    auth: Authorization,
) -> ApiResult<Message> {
    let mut tx = ctx.db_pool.begin().await?;
    let mut facts = ActorFacts {
        conn: &mut tx,
        server_id,
        state,
    };
    auth.check(&ctx.config, &mut facts).await?;
    let now = clock.next(&mut tx, text_channel_id).await?;
    let message = apply(&mut tx, text_channel_id, op, now).await?;
    tx.commit().await?;
    Ok(message)
}

async fn apply(
    conn: &mut PgConnection,
    text_channel_id: TextChannelId,
    op: ChannelOp,
    now: OffsetDateTime,
) -> ApiResult<Message> {
    match op {
        ChannelOp::Create(new_message) => {
            if let Some(reply_to) = new_message.reply_to {
                check_reply_target(conn, text_channel_id, reply_to).await?;
            }
            // The actor, not the database, assigns IDs and times.
            let message_id = MessageId::new();
            queries::messages::insert(
                conn,
                message_id,
                text_channel_id,
                &new_message,
                now,
            )
            .await
        }
        ChannelOp::Update { message_id, update } => {
            queries::messages::update(
                conn,
                text_channel_id,
                message_id,
                &update,
                now,
            )
            .await
        }
        ChannelOp::Delete(message_id) => {
            queries::messages::delete(conn, text_channel_id, message_id, now)
                .await
        }
    }
}

/// Deleted targets remain valid. Channel restrictions belong here, rather than
/// in the schema, to allow broader scopes in the future.
async fn check_reply_target(
    conn: &mut PgConnection,
    text_channel_id: TextChannelId,
    reply_to: MessageId,
) -> ApiResult<()> {
    let not_found = || {
        ApiError::BadRequest(
            "Reply target not found in this text channel".into(),
        )
    };
    let target = queries::messages::get_by_id(conn, reply_to).await.map_err(
        |error| match error {
            ApiError::NotFound => not_found(),
            error => error,
        },
    )?;
    if target.text_channel_id != text_channel_id {
        return Err(not_found());
    }
    Ok(())
}

/// Message times, which the actor chooses so that they only move forward
/// within the channel, even if the system clock steps back.
#[derive(Default)]
struct Clock {
    last: Option<OffsetDateTime>,
}

impl Clock {
    /// A time later than any already stored on the channel's messages, at the
    /// database's microsecond precision so that the actor knows the stored
    /// value without reading it back.
    async fn next(
        &mut self,
        conn: &mut PgConnection,
        text_channel_id: TextChannelId,
    ) -> ApiResult<OffsetDateTime> {
        let last = match self.last {
            Some(last) => Some(last),
            None => {
                queries::messages::latest_time(conn, text_channel_id).await?
            }
        };
        let now = OffsetDateTime::now_utc();
        let now = now
            .replace_nanosecond(now.nanosecond() / 1_000 * 1_000)
            .expect("truncating nanoseconds keeps them in range");
        let next = match last {
            Some(last) => now.max(last + Duration::microseconds(1)),
            None => now,
        };
        self.last = Some(next);
        Ok(next)
    }
}
