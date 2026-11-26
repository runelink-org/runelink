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
#[derive(sqlx::FromRow)]
struct ServerChannelRow {
    id: Uuid,
    server_id: Uuid,
    kind: String,
    title: String,
    description: Option<String>,
    metadata: Option<serde_json::Value>,
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
            metadata: match row.metadata {
                None => None,
                Some(serde_json::Value::Object(metadata)) => Some(metadata),
                _ => {
                    return Err(ApiError::Internal(
                        "Invalid channel metadata".into(),
                    ));
                }
            },
            parent_channel_id: row.parent_channel_id.map(Into::into),
            position: row.position,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// Insert a server channel after its siblings, along with the text channel
/// it owns if it is a text channel.
pub async fn insert(
    conn: &mut PgConnection,
    server_id: ServerId,
    new_channel: &NewServerChannel,
) -> ApiResult<ServerChannel> {
    if let Some(metadata) = &new_channel.metadata {
        runelink_types::metadata::validate_metadata(metadata)
            .map_err(ApiError::BadRequest)?;
    }
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
            parent_channel_id, position, metadata
        )
        VALUES ($1, $2, $3, $4, $5, $6, (
            SELECT COALESCE(MAX(position) + 1, 0)
            FROM server_channels
            WHERE server_id = $1
                AND parent_channel_id IS NOT DISTINCT FROM $6
        ), $7)
        RETURNING *;
        "#,
        server_id.as_uuid(),
        new_channel.kind as ServerChannelKind,
        new_channel.title,
        new_channel.description,
        text_channel_id,
        new_channel.parent_channel_id.map(|id| id.as_uuid()),
        new_channel.metadata.as_ref().map(sqlx::types::Json) as _,
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

/// Delete a server channel, returning the text channel it owned, if any. The
/// text channel and its messages are deleted by a database trigger.
pub async fn delete(
    executor: impl PgExecutor<'_>,
    server_id: ServerId,
    channel_id: ServerChannelId,
) -> ApiResult<Option<TextChannelId>> {
    let text_channel_id = sqlx::query_scalar!(
        r#"
        DELETE FROM server_channels WHERE id = $1 AND server_id = $2
        RETURNING text_channel_id;
        "#,
        channel_id.as_uuid(),
        server_id.as_uuid(),
    )
    .fetch_one(executor)
    .await?;
    Ok(text_channel_id.map(Into::into))
}

/// The text channels in a server.
pub async fn get_text_channel_ids(
    executor: impl PgExecutor<'_>,
    server_id: ServerId,
) -> ApiResult<Vec<TextChannelId>> {
    let ids = sqlx::query_scalar!(
        r#"
        SELECT text_channel_id AS "text_channel_id!" FROM server_channels
        WHERE server_id = $1 AND text_channel_id IS NOT NULL;
        "#,
        server_id.as_uuid(),
    )
    .fetch_all(executor)
    .await?;
    Ok(ids.into_iter().map(Into::into).collect())
}

pub async fn update(
    conn: &mut PgConnection,
    server_id: ServerId,
    channel_id: ServerChannelId,
    update: &runelink_types::channel::ServerChannelUpdate,
) -> ApiResult<ServerChannel> {
    if update
        .title
        .as_ref()
        .is_some_and(|title| title.trim().is_empty())
    {
        return Err(ApiError::BadRequest("Title cannot be empty".into()));
    }
    if let Some(Some(metadata)) = &update.metadata {
        runelink_types::metadata::validate_metadata(metadata)
            .map_err(ApiError::BadRequest)?;
    }
    let mut row = sqlx::query_as::<_, ServerChannelRow>(
        "SELECT * FROM server_channels WHERE id = $1 AND server_id = $2",
    )
    .bind(channel_id)
    .bind(server_id)
    .fetch_one(&mut *conn)
    .await?;
    if row.updated_at != update.expected_updated_at {
        return Err(ApiError::Conflict(
            "Resource changed. Reload it before saving.".into(),
        ));
    }
    let title = update.title.as_ref().unwrap_or(&row.title);
    let description = update.description.as_ref().unwrap_or(&row.description);
    let metadata = update
        .metadata
        .as_ref()
        .map(|value| value.clone().map(serde_json::Value::Object))
        .unwrap_or_else(|| row.metadata.clone());
    if title != &row.title
        || description != &row.description
        || metadata != row.metadata
    {
        row = sqlx::query_as::<_, ServerChannelRow>("UPDATE server_channels SET title = $2, description = $3, metadata = $4 WHERE id = $1 RETURNING *")
            .bind(channel_id).bind(title).bind(description).bind(metadata).fetch_one(&mut *conn).await?;
    }
    row.try_into()
}
