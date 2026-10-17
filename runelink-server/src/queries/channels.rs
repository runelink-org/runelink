use runelink_types::{
    channel::{
        NewServerChannel, ServerChannel, ServerChannelId, ServerChannelKind,
        TextChannelId,
    },
    server::ServerId,
};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{db::DbPool, error::ApiResult};

/// Insert a server channel after its siblings, along with the text channel
/// it owns if it is a text channel.
pub async fn insert(
    pool: &DbPool,
    server_id: ServerId,
    new_channel: &NewServerChannel,
) -> ApiResult<ServerChannel> {
    let mut tx = pool.begin().await?;
    let text_channel_id = match new_channel.kind {
        ServerChannelKind::Text => Some(
            sqlx::query_scalar!(
                "INSERT INTO text_channels DEFAULT VALUES RETURNING id;"
            )
            .fetch_one(&mut *tx)
            .await?,
        ),
        ServerChannelKind::Category => None,
    };
    let channel = sqlx::query_as!(
        ServerChannel,
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
        RETURNING
            id, server_id, kind AS "kind: ServerChannelKind", title,
            description,
            text_channel_id AS "text_channel_id: TextChannelId",
            parent_channel_id AS "parent_channel_id: ServerChannelId",
            position, created_at, updated_at;
        "#,
        server_id.as_uuid(),
        new_channel.kind as ServerChannelKind,
        new_channel.title,
        new_channel.description,
        text_channel_id,
        new_channel.parent_channel_id.map(|id| id.as_uuid()),
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(channel)
}

pub async fn get_by_id(
    pool: &DbPool,
    channel_id: ServerChannelId,
) -> ApiResult<ServerChannel> {
    let channel = sqlx::query_as!(
        ServerChannel,
        r#"
        SELECT
            id, server_id, kind AS "kind: ServerChannelKind", title,
            description,
            text_channel_id AS "text_channel_id: TextChannelId",
            parent_channel_id AS "parent_channel_id: ServerChannelId",
            position, created_at, updated_at
        FROM server_channels
        WHERE id = $1;
        "#,
        channel_id.as_uuid(),
    )
    .fetch_one(pool)
    .await?;
    Ok(channel)
}

/// Get the server channel that owns a text channel.
pub async fn get_by_text_channel(
    pool: &DbPool,
    text_channel_id: TextChannelId,
) -> ApiResult<ServerChannel> {
    let channel = sqlx::query_as!(
        ServerChannel,
        r#"
        SELECT
            id, server_id, kind AS "kind: ServerChannelKind", title,
            description,
            text_channel_id AS "text_channel_id: TextChannelId",
            parent_channel_id AS "parent_channel_id: ServerChannelId",
            position, created_at, updated_at
        FROM server_channels
        WHERE text_channel_id = $1;
        "#,
        text_channel_id.as_uuid(),
    )
    .fetch_one(pool)
    .await?;
    Ok(channel)
}

pub async fn get_all(pool: &DbPool) -> ApiResult<Vec<ServerChannel>> {
    let channels = sqlx::query_as!(
        ServerChannel,
        r#"
        SELECT
            id, server_id, kind AS "kind: ServerChannelKind", title,
            description,
            text_channel_id AS "text_channel_id: TextChannelId",
            parent_channel_id AS "parent_channel_id: ServerChannelId",
            position, created_at, updated_at
        FROM server_channels
        ORDER BY server_id, position, id;
        "#
    )
    .fetch_all(pool)
    .await?;
    Ok(channels)
}

pub async fn get_by_server(
    pool: &DbPool,
    server_id: ServerId,
) -> ApiResult<Vec<ServerChannel>> {
    let channels = sqlx::query_as!(
        ServerChannel,
        r#"
        SELECT
            id, server_id, kind AS "kind: ServerChannelKind", title,
            description,
            text_channel_id AS "text_channel_id: TextChannelId",
            parent_channel_id AS "parent_channel_id: ServerChannelId",
            position, created_at, updated_at
        FROM server_channels
        WHERE server_id = $1
        ORDER BY position, id;
        "#,
        server_id.as_uuid(),
    )
    .fetch_all(pool)
    .await?;
    Ok(channels)
}

/// Get and lock a server's channels, serializing changes to its tree.
pub async fn get_by_server_for_update(
    conn: &mut PgConnection,
    server_id: ServerId,
) -> ApiResult<Vec<ServerChannel>> {
    let channels = sqlx::query_as!(
        ServerChannel,
        r#"
        SELECT
            id, server_id, kind AS "kind: ServerChannelKind", title,
            description,
            text_channel_id AS "text_channel_id: TextChannelId",
            parent_channel_id AS "parent_channel_id: ServerChannelId",
            position, created_at, updated_at
        FROM server_channels
        WHERE server_id = $1
        ORDER BY position, id
        FOR UPDATE;
        "#,
        server_id.as_uuid(),
    )
    .fetch_all(conn)
    .await?;
    Ok(channels)
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
        ServerChannel,
        r#"
        UPDATE server_channels sc
        SET parent_channel_id = p.parent_channel_id, position = p.position
        FROM UNNEST($1::UUID[], $2::UUID[], $3::INTEGER[])
            AS p (id, parent_channel_id, position)
        WHERE sc.id = p.id
        RETURNING
            sc.id, sc.server_id, sc.kind AS "kind: ServerChannelKind",
            sc.title, sc.description,
            sc.text_channel_id AS "text_channel_id: TextChannelId",
            sc.parent_channel_id AS "parent_channel_id: ServerChannelId",
            sc.position, sc.created_at, sc.updated_at;
        "#,
        &ids,
        &parent_ids as &[Option<Uuid>],
        &positions,
    )
    .fetch_all(conn)
    .await?;
    Ok(channels)
}

pub async fn has_children(
    pool: &DbPool,
    channel_id: ServerChannelId,
) -> ApiResult<bool> {
    let has_children = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM server_channels WHERE parent_channel_id = $1
        ) AS "exists!";
        "#,
        channel_id.as_uuid(),
    )
    .fetch_one(pool)
    .await?;
    Ok(has_children)
}

/// Delete a server channel. Its text channel and messages are deleted by
/// a database trigger.
pub async fn delete(
    pool: &DbPool,
    channel_id: ServerChannelId,
) -> ApiResult<()> {
    sqlx::query!(
        "DELETE FROM server_channels WHERE id = $1;",
        channel_id.as_uuid()
    )
    .execute(pool)
    .await?;
    Ok(())
}
