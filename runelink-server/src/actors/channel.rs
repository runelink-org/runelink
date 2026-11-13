//! The channel actor: the messages of one text channel.

use std::sync::Arc;

use runelink_types::{
    channel::TextChannelId,
    message::{Message, MessageId, MessageUpdate, NewMessage},
    server::ServerId,
    ws::{ClientWsUpdate, FederationWsUpdate},
};
use sqlx::PgConnection;
use time::OffsetDateTime;
use tokio::sync::{Mutex, mpsc};

use super::{
    ActorCtx, ChannelMailbox, Envelope, MAILBOX_CAPACITY, Registry,
    SharedState, Update,
};
use crate::{
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
        while let Some(Envelope { op, reply }) = inbox.recv().await {
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
            let result = execute(&ctx, text_channel_id, op).await;
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
    text_channel_id: TextChannelId,
    op: ChannelOp,
) -> ApiResult<Message> {
    let mut tx = ctx.db_pool.begin().await?;
    let message = apply(&mut tx, text_channel_id, op).await?;
    tx.commit().await?;
    Ok(message)
}

async fn apply(
    conn: &mut PgConnection,
    text_channel_id: TextChannelId,
    op: ChannelOp,
) -> ApiResult<Message> {
    match op {
        ChannelOp::Create(new_message) => {
            if let Some(reply_to) = new_message.reply_to {
                check_reply_target(conn, text_channel_id, reply_to).await?;
            }
            // The actor, not the database, assigns IDs and creation times.
            let message_id = MessageId::new();
            queries::messages::insert(
                conn,
                message_id,
                text_channel_id,
                &new_message,
                now(),
            )
            .await
        }
        ChannelOp::Update { message_id, update } => {
            queries::messages::update(
                conn,
                text_channel_id,
                message_id,
                &update,
            )
            .await
        }
        ChannelOp::Delete(message_id) => {
            queries::messages::delete(conn, text_channel_id, message_id).await
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

/// The current time at the database's microsecond precision, so that the
/// actor knows the stored value without reading it back (the database would
/// round it otherwise).
fn now() -> OffsetDateTime {
    let now = OffsetDateTime::now_utc();
    now.replace_nanosecond(now.nanosecond() / 1_000 * 1_000)
        .expect("truncating nanoseconds keeps them in range")
}
