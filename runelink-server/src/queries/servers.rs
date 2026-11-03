use runelink_types::server::{NewServer, Server, ServerId};
use time::OffsetDateTime;

use crate::{
    config::{ServerConfig, canonical_host},
    db::DbPool,
    error::{ApiError, ApiResult},
    state::AppState,
};

#[derive(sqlx::FromRow, Debug)]
struct LocalServerRow {
    pub id: ServerId,
    pub title: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    // No 'host' field
}

impl LocalServerRow {
    fn into_server(self, config: &ServerConfig) -> Server {
        Server {
            id: self.id,
            host: config.public_host(),
            title: self.title,
            description: self.description,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

pub async fn insert(
    state: &AppState,
    new_server: &NewServer,
) -> ApiResult<Server> {
    let row = sqlx::query_as!(
        LocalServerRow,
        r#"
        INSERT INTO servers (title, description)
        VALUES ($1, $2)
        RETURNING *;
        "#,
        new_server.title,
        new_server.description,
    )
    .fetch_one(state.db_pool.as_ref())
    .await?;
    Ok(row.into_server(&state.config))
}

/// Cache a remote server reported by `source_host`.
///
/// Only a server's own host may report it, and a cached server's host never
/// changes, so the first host to claim a server ID keeps it. Local server IDs
/// are never cached.
pub async fn upsert_remote(
    pool: &DbPool,
    server: &Server,
    source_host: &str,
) -> ApiResult<()> {
    let host = canonical_host(&server.host);
    if host != canonical_host(source_host) {
        return Err(ApiError::Conflict(format!(
            "{source_host} cannot report servers on {}",
            server.host
        )));
    }
    let result = sqlx::query!(
        r#"
        INSERT INTO cached_remote_servers (
            id, host, title, description, remote_created_at,
            remote_updated_at, synced_at
        )
        SELECT $1, $2, $3, $4, $5, $6, NOW()
        WHERE NOT EXISTS (SELECT 1 FROM servers WHERE id = $1)
        ON CONFLICT(id) DO UPDATE
            SET title = EXCLUDED.title,
                description = EXCLUDED.description,
                remote_created_at = EXCLUDED.remote_created_at,
                remote_updated_at = EXCLUDED.remote_updated_at,
                synced_at = NOW()
        WHERE cached_remote_servers.remote_updated_at <= EXCLUDED.remote_updated_at
            AND cached_remote_servers.host = EXCLUDED.host
        "#,
        server.id.as_uuid(),
        host,
        server.title,
        server.description,
        server.created_at,
        server.updated_at,
    )
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        // The update is stale, or another host claimed this ID first.
        let cached_host = sqlx::query_scalar!(
            "SELECT host FROM cached_remote_servers WHERE id = $1",
            server.id.as_uuid(),
        )
        .fetch_optional(pool)
        .await?;
        if cached_host.as_deref() != Some(host.as_str()) {
            return Err(ApiError::Conflict(format!(
                "Server {} belongs to another host",
                server.id
            )));
        }
    }
    Ok(())
}

pub async fn get_by_id(
    state: &AppState,
    server_id: ServerId,
) -> ApiResult<Server> {
    let row = sqlx::query_as!(
        LocalServerRow,
        "SELECT * FROM servers WHERE id = $1;",
        server_id.as_uuid(),
    )
    .fetch_one(state.db_pool.as_ref())
    .await?;
    Ok(row.into_server(&state.config))
}

pub async fn get_all(state: &AppState) -> ApiResult<Vec<Server>> {
    let rows = sqlx::query_as!(LocalServerRow, "SELECT * FROM servers",)
        .fetch_all(state.db_pool.as_ref())
        .await?;
    let servers = rows
        .into_iter()
        .map(|row| row.into_server(&state.config))
        .collect();
    Ok(servers)
}

pub async fn delete(state: &AppState, server_id: ServerId) -> ApiResult<()> {
    sqlx::query!("DELETE FROM servers WHERE id = $1;", server_id.as_uuid())
        .execute(state.db_pool.as_ref())
        .await?;
    Ok(())
}

pub async fn update(
    state: &AppState,
    server_id: ServerId,
    update: &runelink_types::edit::DetailsUpdate,
) -> ApiResult<Server> {
    if update
        .title
        .as_ref()
        .is_some_and(|title| title.trim().is_empty())
    {
        return Err(ApiError::BadRequest("Title cannot be empty".into()));
    }
    let mut tx = state.db_pool.begin().await?;
    let mut row = sqlx::query_as::<_, LocalServerRow>(
        "SELECT * FROM servers WHERE id = $1 FOR UPDATE",
    )
    .bind(server_id)
    .fetch_one(&mut *tx)
    .await?;
    if row.updated_at != update.expected_updated_at {
        return Err(ApiError::Conflict(
            "Resource changed. Reload it before saving.".into(),
        ));
    }
    let title = update.title.as_ref().unwrap_or(&row.title);
    let description = update.description.as_ref().unwrap_or(&row.description);
    if title != &row.title || description != &row.description {
        row = sqlx::query_as::<_, LocalServerRow>("UPDATE servers SET title = $2, description = $3 WHERE id = $1 RETURNING *")
            .bind(server_id).bind(title).bind(description).fetch_one(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(row.into_server(&state.config))
}
