use runelink_types::{
    channel::{Channel, ChannelId, NewChannel},
    server::ServerId,
};

use crate::{db::DbPool, error::ApiResult};

pub async fn insert(
    pool: &DbPool,
    server_id: ServerId,
    new_channel: &NewChannel,
) -> ApiResult<Channel> {
    let channel = sqlx::query_as!(
        Channel,
        r#"
        INSERT INTO channels (server_id, title, description)
        VALUES ($1, $2, $3)
        RETURNING *;
        "#,
        server_id.as_uuid(),
        new_channel.title,
        new_channel.description,
    )
    .fetch_one(pool)
    .await?;
    Ok(channel)
}

pub async fn get_by_id(
    pool: &DbPool,
    channel_id: ChannelId,
) -> ApiResult<Channel> {
    let channel = sqlx::query_as!(
        Channel,
        "SELECT * FROM channels WHERE id = $1;",
        channel_id.as_uuid(),
    )
    .fetch_one(pool)
    .await?;
    Ok(channel)
}

pub async fn get_all(pool: &DbPool) -> ApiResult<Vec<Channel>> {
    let channels = sqlx::query_as!(Channel, "SELECT * FROM channels")
        .fetch_all(pool)
        .await?;
    Ok(channels)
}

pub async fn get_by_server(
    pool: &DbPool,
    server_id: ServerId,
) -> ApiResult<Vec<Channel>> {
    let channels = sqlx::query_as!(
        Channel,
        r#"
        SELECT * FROM channels
        WHERE server_id = $1
        ORDER BY created_at;
        "#,
        server_id.as_uuid(),
    )
    .fetch_all(pool)
    .await?;
    Ok(channels)
}

pub async fn delete(pool: &DbPool, channel_id: ChannelId) -> ApiResult<()> {
    sqlx::query!("DELETE FROM channels WHERE id = $1;", channel_id.as_uuid())
        .execute(pool)
        .await?;
    Ok(())
}
