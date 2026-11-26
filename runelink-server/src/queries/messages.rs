use runelink_types::{
    channel::TextChannelId,
    message::{Message, MessageContent, MessageId, NewMessage},
    server::ServerId,
    user::User,
};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgExecutor, types::Json};
use time::OffsetDateTime;

use crate::{
    db::DbPool,
    error::{ApiError, ApiResult},
};

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct DbMessage {
    pub id: MessageId,
    pub text_channel_id: TextChannelId,
    pub author: Option<Json<User>>,
    pub body: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub reply_to: Option<MessageId>,
    pub deleted_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub edited_at: Option<OffsetDateTime>,
}

impl TryFrom<DbMessage> for Message {
    type Error = ApiError;

    fn try_from(msg: DbMessage) -> ApiResult<Self> {
        let content = match (msg.body, msg.deleted_at) {
            (Some(body), None) => MessageContent::Active {
                body,
                deleted_at: (),
            },
            (None, Some(deleted_at)) => MessageContent::Deleted {
                body: (),
                deleted_at,
            },
            _ => {
                return Err(ApiError::Internal(
                    "Invalid message content state".into(),
                ));
            }
        };
        Ok(Message {
            id: msg.id,
            text_channel_id: msg.text_channel_id,
            author: msg.author.map(|json_user| json_user.0),
            reply_to: msg.reply_to,
            metadata: match msg.metadata {
                None => None,
                Some(serde_json::Value::Object(metadata)) => Some(metadata),
                _ => {
                    return Err(ApiError::Internal(
                        "Invalid message metadata".into(),
                    ));
                }
            },
            content,
            created_at: msg.created_at,
            updated_at: msg.updated_at,
            edited_at: msg.edited_at,
        })
    }
}

/// The latest time stored on any of a text channel's messages.
pub async fn latest_time(
    conn: &mut PgConnection,
    text_channel_id: TextChannelId,
) -> ApiResult<Option<OffsetDateTime>> {
    let time = sqlx::query_scalar!(
        "SELECT max(updated_at) FROM messages WHERE text_channel_id = $1;",
        text_channel_id.as_uuid(),
    )
    .fetch_one(conn)
    .await?;
    Ok(time)
}

/// Insert a message with an ID and creation time chosen by the caller.
pub async fn insert(
    conn: &mut PgConnection,
    message_id: MessageId,
    text_channel_id: TextChannelId,
    new_message: &NewMessage,
    created_at: OffsetDateTime,
) -> ApiResult<Message> {
    if let Some(metadata) = &new_message.metadata {
        runelink_types::metadata::validate_metadata(metadata)
            .map_err(ApiError::BadRequest)?;
    }
    let row = sqlx::query_as!(
        DbMessage,
        r#"
        WITH m AS (
            INSERT INTO messages (
                id, text_channel_id, author_name, author_host, body, reply_to,
                metadata, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
            RETURNING *
        )
        SELECT
            m.id AS "id!",
            m.text_channel_id AS "text_channel_id!",
            m.body,
            m.metadata,
            m.reply_to AS "reply_to: MessageId",
            m.deleted_at,
            m.created_at AS "created_at!",
            m.updated_at AS "updated_at!",
            m.edited_at,
            to_jsonb(a) AS "author: Json<User>"
        FROM m
        LEFT JOIN users a ON a.name = m.author_name AND a.host = m.author_host;
        "#,
        message_id.as_uuid(),
        text_channel_id.as_uuid(),
        new_message.author.name,
        new_message.author.host,
        new_message.body,
        new_message.reply_to.map(|id| id.as_uuid()),
        new_message.metadata.as_ref().map(Json) as _,
        created_at,
    )
    .fetch_one(conn)
    .await?;
    row.try_into()
}

pub async fn get_all(pool: &DbPool) -> ApiResult<Vec<Message>> {
    let rows = sqlx::query_as!(
        DbMessage,
        r#"
        SELECT
            m.id AS "id!",
            m.text_channel_id AS "text_channel_id!",
            m.body,
            m.metadata,
            m.reply_to AS "reply_to: MessageId",
            m.deleted_at,
            m.created_at AS "created_at!",
            m.updated_at AS "updated_at!",
            m.edited_at,
            to_jsonb(a) AS "author: Json<User>"
        FROM messages m
        LEFT JOIN users a ON a.name = m.author_name AND a.host = m.author_host
        ORDER BY m.created_at DESC;
        "#
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(Message::try_from).collect()
}

pub async fn get_by_server(
    pool: &DbPool,
    server_id: ServerId,
) -> ApiResult<Vec<Message>> {
    let rows = sqlx::query_as!(
        DbMessage,
        r#"
        SELECT
            m.id AS "id!",
            m.text_channel_id AS "text_channel_id!",
            m.body,
            m.metadata,
            m.reply_to AS "reply_to: MessageId",
            m.deleted_at,
            m.created_at AS "created_at!",
            m.updated_at AS "updated_at!",
            m.edited_at,
            to_jsonb(a) AS "author: Json<User>"
        FROM messages m
        LEFT JOIN users a ON a.name = m.author_name AND a.host = m.author_host
        JOIN server_channels c ON c.text_channel_id = m.text_channel_id
        WHERE c.server_id = $1
        ORDER BY m.created_at DESC;
        "#,
        server_id.as_uuid(),
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(Message::try_from).collect()
}

pub async fn get_by_text_channel(
    pool: &DbPool,
    text_channel_id: TextChannelId,
) -> ApiResult<Vec<Message>> {
    let rows = sqlx::query_as!(
        DbMessage,
        r#"
        SELECT
            m.id AS "id!",
            m.text_channel_id AS "text_channel_id!",
            m.body,
            m.metadata,
            m.reply_to AS "reply_to: MessageId",
            m.deleted_at,
            m.created_at AS "created_at!",
            m.updated_at AS "updated_at!",
            m.edited_at,
            to_jsonb(a) AS "author: Json<User>"
        FROM messages m
        LEFT JOIN users a ON a.name = m.author_name AND a.host = m.author_host
        WHERE m.text_channel_id = $1
        ORDER BY m.created_at DESC;
        "#,
        text_channel_id.as_uuid(),
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(Message::try_from).collect()
}

pub async fn get_by_id(
    executor: impl PgExecutor<'_>,
    msg_id: MessageId,
) -> ApiResult<Message> {
    let db_message = sqlx::query_as!(
        DbMessage,
        r#"
        SELECT
            m.id AS "id!",
            m.text_channel_id AS "text_channel_id!",
            m.body,
            m.metadata,
            m.reply_to AS "reply_to: MessageId",
            m.deleted_at,
            m.created_at AS "created_at!",
            m.updated_at AS "updated_at!",
            m.edited_at,
            to_jsonb(a) AS "author: Json<User>"
        FROM messages m
        LEFT JOIN users a ON a.name = m.author_name AND a.host = m.author_host
        WHERE m.id = $1;
        "#,
        msg_id.as_uuid(),
    )
    .fetch_one(executor)
    .await?;
    db_message.try_into()
}

/// Get the text channel that holds a message.
pub async fn get_text_channel_id(
    pool: &DbPool,
    message_id: MessageId,
) -> ApiResult<TextChannelId> {
    let text_channel_id = sqlx::query_scalar!(
        "SELECT text_channel_id FROM messages WHERE id = $1;",
        message_id.as_uuid(),
    )
    .fetch_one(pool)
    .await?;
    Ok(text_channel_id.into())
}

/// Retain identity and relationships, clear text, and preserve the first
/// deletion time. `now` is the deletion time, chosen by the caller.
pub async fn delete(
    conn: &mut PgConnection,
    text_channel_id: TextChannelId,
    message_id: MessageId,
    now: OffsetDateTime,
) -> ApiResult<Message> {
    sqlx::query!(
        r#"
        UPDATE messages
        SET body = NULL, metadata = NULL, deleted_at = $3, updated_at = $3
        WHERE id = $1 AND text_channel_id = $2 AND deleted_at IS NULL;
        "#,
        message_id.as_uuid(),
        text_channel_id.as_uuid(),
        now,
    )
    .execute(&mut *conn)
    .await?;
    let message = get_by_id(conn, message_id).await?;
    if message.text_channel_id != text_channel_id {
        return Err(ApiError::NotFound);
    }
    Ok(message)
}

/// Edit a message. `now` is the edit time, chosen by the caller.
pub async fn update(
    conn: &mut PgConnection,
    text_channel_id: TextChannelId,
    message_id: MessageId,
    update: &runelink_types::message::MessageUpdate,
    now: OffsetDateTime,
) -> ApiResult<Message> {
    if update
        .body
        .as_ref()
        .is_some_and(|body| body.trim().is_empty())
    {
        return Err(ApiError::BadRequest("Message cannot be empty".into()));
    }
    if let Some(Some(metadata)) = &update.metadata {
        runelink_types::metadata::validate_metadata(metadata)
            .map_err(ApiError::BadRequest)?;
    }
    let mut row = sqlx::query_as::<_, DbMessage>(
        r#"
        SELECT m.id, m.text_channel_id, m.body, m.metadata, m.reply_to, m.deleted_at,
            m.created_at, m.updated_at, m.edited_at, to_jsonb(a) AS author
        FROM messages m
        LEFT JOIN users a ON a.name = m.author_name AND a.host = m.author_host
        WHERE m.id = $1 AND m.text_channel_id = $2
    "#,
    )
    .bind(message_id)
    .bind(text_channel_id)
    .fetch_one(&mut *conn)
    .await?;
    if row.deleted_at.is_some() {
        return Err(ApiError::Conflict("Message was deleted".into()));
    }
    if row.updated_at != update.expected_updated_at {
        return Err(ApiError::Conflict(
            "Message changed. Reload it before saving.".into(),
        ));
    }
    let body = update.body.as_ref().or(row.body.as_ref());
    let metadata = update
        .metadata
        .as_ref()
        .map(|value| value.clone().map(serde_json::Value::Object))
        .unwrap_or_else(|| row.metadata.clone());
    if body != row.body.as_ref() || metadata != row.metadata {
        sqlx::query(
            r#"
            UPDATE messages
            SET body = $2, metadata = $3, edited_at = $4, updated_at = $4
            WHERE id = $1
        "#,
        )
        .bind(message_id)
        .bind(body)
        .bind(&metadata)
        .bind(now)
        .execute(&mut *conn)
        .await?;
        row.body = body.cloned();
        row.metadata = metadata;
        row.updated_at = now;
        row.edited_at = Some(now);
    }
    row.try_into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;
    use uuid::Uuid;

    // Use one connection and roll back every test mutation. No database creation
    // privileges are needed, and other host connections cannot see the fixtures.
    async fn test_pool() -> PgPool {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(
                &std::env::var("DATABASE_URL")
                    .expect("Set DATABASE_URL to a migrated test database"),
            )
            .await
            .unwrap();
        sqlx::query("BEGIN").execute(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL pointing to a migrated PostgreSQL database"]
    async fn deletion_clears_text_and_preserves_replies() {
        let pool = test_pool().await;
        let channel: Uuid = sqlx::query_scalar(
            "INSERT INTO text_channels DEFAULT VALUES RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let original: Uuid = sqlx::query_scalar("INSERT INTO messages (text_channel_id, body) VALUES ($1, 'sensitive text') RETURNING id")
            .bind(channel).fetch_one(&pool).await.unwrap();
        let reply: Uuid = sqlx::query_scalar("INSERT INTO messages (text_channel_id, body, reply_to) VALUES ($1, 'reply', $2) RETURNING id")
            .bind(channel).bind(original).fetch_one(&pool).await.unwrap();
        let before = get_by_id(&pool, original.into()).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        let deleted_at = before.created_at + time::Duration::seconds(1);
        let deleted =
            delete(&mut conn, channel.into(), original.into(), deleted_at)
                .await
                .unwrap();
        assert!(matches!(
            deleted.content,
            MessageContent::Deleted { deleted_at: at, .. } if at == deleted_at
        ));
        assert_eq!(before.id, deleted.id);
        assert_eq!(before.created_at, deleted.created_at);
        // A later deletion keeps the first deletion time.
        let later = deleted_at + time::Duration::seconds(1);
        assert_eq!(
            delete(&mut conn, channel.into(), original.into(), later)
                .await
                .unwrap(),
            deleted
        );
        drop(conn);
        assert_eq!(
            get_by_id(&pool, reply.into()).await.unwrap().reply_to,
            Some(original.into())
        );
        assert_eq!(
            get_by_text_channel(&pool, channel.into())
                .await
                .unwrap()
                .len(),
            2
        );

        // The schema permits cross-channel replies, including to deleted targets.
        let other: Uuid = sqlx::query_scalar(
            "INSERT INTO text_channels DEFAULT VALUES RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO messages (text_channel_id, body, reply_to) VALUES ($1, 'future cross-channel reply', $2)")
            .bind(other).bind(original).execute(&pool).await.unwrap();
        // Physical channel deletion must not cascade into replies in other channels.
        sqlx::query("DELETE FROM text_channels WHERE id = $1")
            .bind(channel)
            .execute(&pool)
            .await
            .unwrap();
        let remaining = get_by_text_channel(&pool, other.into()).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].reply_to, None);
        sqlx::query("ROLLBACK").execute(&pool).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL pointing to a migrated PostgreSQL database"]
    async fn database_rejects_inconsistent_content_states() {
        let pool = test_pool().await;
        let channel: Uuid = sqlx::query_scalar(
            "INSERT INTO text_channels DEFAULT VALUES RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        for (body, deleted_at) in [
            (None, None),
            (Some("leaked text"), Some(OffsetDateTime::now_utc())),
        ] {
            sqlx::query("SAVEPOINT invalid_state")
                .execute(&pool)
                .await
                .unwrap();
            assert!(sqlx::query("INSERT INTO messages (text_channel_id, body, deleted_at) VALUES ($1, $2, $3)")
                .bind(channel).bind(body).bind(deleted_at).execute(&pool).await.is_err());
            sqlx::query("ROLLBACK TO SAVEPOINT invalid_state")
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("ROLLBACK").execute(&pool).await.unwrap();
    }
}
