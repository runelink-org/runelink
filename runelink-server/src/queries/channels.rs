use runelink_types::{
    channel::{
        NewServerChannel, ServerChannel, ServerChannelContent, ServerChannelId,
        ServerChannelKind, TextChannelId,
    },
    server::ServerId,
};
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::{
    db::DbPool,
    error::{ApiError, ApiResult},
};

// Nullable database columns are confined to this boundary.
struct ServerChannelRow {
    id: Uuid,
    server_id: Uuid,
    kind: String,
    title: String,
    description: Option<String>,
    text_channel_id: Option<Uuid>,
    parent_channel_id: Option<Uuid>,
    position: i32,
    created_at: time::OffsetDateTime,
    updated_at: time::OffsetDateTime,
}

impl TryFrom<ServerChannelRow> for ServerChannel {
    type Error = ApiError;

    fn try_from(row: ServerChannelRow) -> ApiResult<Self> {
        let content = match (row.kind.as_str(), row.text_channel_id) {
            ("text", Some(id)) => ServerChannelContent::Text {
                text_channel_id: id.into(),
            },
            ("category", None) => ServerChannelContent::Category,
            _ => {
                return Err(ApiError::Internal(format!(
                    "Invalid channel content for {}",
                    row.id
                )));
            }
        };
        Ok(Self {
            id: row.id.into(),
            server_id: row.server_id.into(),
            content,
            title: row.title,
            description: row.description,
            parent_channel_id: row.parent_channel_id.map(Into::into),
            position: row.position,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// Lock a server's channel tree until the transaction ends, serializing
/// changes to it. Fails with `NotFound` if the server does not exist.
pub async fn lock_tree(
    conn: &mut PgConnection,
    server_id: ServerId,
) -> ApiResult<()> {
    sqlx::query_scalar!(
        "SELECT id FROM servers WHERE id = $1 FOR NO KEY UPDATE;",
        server_id.as_uuid(),
    )
    .fetch_one(conn)
    .await?;
    Ok(())
}

/// Insert a server channel after its siblings, along with the text channel
/// it owns if it is a text channel.
pub async fn insert(
    conn: &mut PgConnection,
    server_id: ServerId,
    new_channel: &NewServerChannel,
) -> ApiResult<ServerChannel> {
    let text_channel_id = match new_channel.kind {
        ServerChannelKind::Text => Some(
            sqlx::query_scalar!(
                "INSERT INTO text_channels DEFAULT VALUES RETURNING id;"
            )
            .fetch_one(&mut *conn)
            .await?,
        ),
        ServerChannelKind::Category => None,
    };
    let channel = sqlx::query_as!(
        ServerChannelRow,
        r#"
        INSERT INTO server_channels (
            server_id, kind, title, description, text_channel_id,
            parent_channel_id, position
        )
        VALUES ($1, $2, $3, $4, $5, $6, (
            SELECT COALESCE(MAX(position) + 1, 0)
            FROM server_channels
            WHERE server_id = $1
                AND parent_channel_id IS NOT DISTINCT FROM $6
        ))
        RETURNING *;
        "#,
        server_id.as_uuid(),
        new_channel.kind as ServerChannelKind,
        new_channel.title,
        new_channel.description,
        text_channel_id,
        new_channel.parent_channel_id.map(|id| id.as_uuid()),
    )
    .fetch_one(conn)
    .await?;
    channel.try_into()
}

pub async fn get_by_id(
    executor: impl PgExecutor<'_>,
    channel_id: ServerChannelId,
) -> ApiResult<ServerChannel> {
    let channel = sqlx::query_as!(
        ServerChannelRow,
        r#"
        SELECT * FROM server_channels
        WHERE id = $1;
        "#,
        channel_id.as_uuid(),
    )
    .fetch_one(executor)
    .await?;
    channel.try_into()
}

/// Get the server that owns a server channel.
pub async fn get_server_id(
    executor: impl PgExecutor<'_>,
    channel_id: ServerChannelId,
) -> ApiResult<ServerId> {
    let server_id = sqlx::query_scalar!(
        "SELECT server_id FROM server_channels WHERE id = $1;",
        channel_id.as_uuid(),
    )
    .fetch_one(executor)
    .await?;
    Ok(server_id.into())
}

/// Get the server whose channel owns a text channel.
pub async fn get_server_id_by_text_channel(
    pool: &DbPool,
    text_channel_id: TextChannelId,
) -> ApiResult<ServerId> {
    let server_id = sqlx::query_scalar!(
        "SELECT server_id FROM server_channels WHERE text_channel_id = $1;",
        text_channel_id.as_uuid(),
    )
    .fetch_one(pool)
    .await?;
    Ok(server_id.into())
}

/// List all channels, optionally restricted to one server.
pub async fn list(
    executor: impl PgExecutor<'_>,
    server_id: Option<ServerId>,
) -> ApiResult<Vec<ServerChannel>> {
    let channels = sqlx::query_as!(
        ServerChannelRow,
        r#"
        SELECT * FROM server_channels
        WHERE $1::UUID IS NULL OR server_id = $1
        ORDER BY server_id, position, id;
        "#,
        server_id.map(|id| id.as_uuid()),
    )
    .fetch_all(executor)
    .await?;
    channels.into_iter().map(TryInto::try_into).collect()
}

/// Store the parent and position of each channel.
pub async fn update_placements(
    conn: &mut PgConnection,
    channels: &[ServerChannel],
) -> ApiResult<Vec<ServerChannel>> {
    let ids: Vec<Uuid> = channels.iter().map(|c| c.id.as_uuid()).collect();
    let parent_ids: Vec<Option<Uuid>> = channels
        .iter()
        .map(|c| c.parent_channel_id.map(|id| id.as_uuid()))
        .collect();
    let positions: Vec<i32> = channels.iter().map(|c| c.position).collect();
    let channels = sqlx::query_as!(
        ServerChannelRow,
        r#"
        UPDATE server_channels sc
        SET parent_channel_id = p.parent_channel_id, position = p.position
        FROM UNNEST($1::UUID[], $2::UUID[], $3::INTEGER[])
            AS p (id, parent_channel_id, position)
        WHERE sc.id = p.id
        RETURNING sc.*;
        "#,
        &ids,
        &parent_ids as &[Option<Uuid>],
        &positions,
    )
    .fetch_all(conn)
    .await?;
    channels.into_iter().map(TryInto::try_into).collect()
}

pub async fn has_children(
    executor: impl PgExecutor<'_>,
    server_id: ServerId,
    channel_id: ServerChannelId,
) -> ApiResult<bool> {
    let has_children = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM server_channels
            WHERE server_id = $1 AND parent_channel_id = $2
        ) AS "exists!";
        "#,
        server_id.as_uuid(),
        channel_id.as_uuid(),
    )
    .fetch_one(executor)
    .await?;
    Ok(has_children)
}

/// Delete a server channel. Its text channel and messages are deleted by
/// a database trigger.
pub async fn delete(
    executor: impl PgExecutor<'_>,
    channel_id: ServerChannelId,
) -> ApiResult<()> {
    let result = sqlx::query!(
        "DELETE FROM server_channels WHERE id = $1;",
        channel_id.as_uuid()
    )
    .execute(executor)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(())
}
