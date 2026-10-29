use runelink_types::server::{NewServer, Server, ServerId};
use time::OffsetDateTime;

use crate::{
    config::ServerConfig, db::DbPool, error::ApiResult, state::AppState,
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

pub async fn upsert_remote(pool: &DbPool, server: &Server) -> ApiResult<()> {
    sqlx::query!(
        r#"
        INSERT INTO cached_remote_servers (
            id, host, title, description, remote_created_at,
            remote_updated_at, synced_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, NOW())
        ON CONFLICT(id) DO UPDATE
            SET host = EXCLUDED.host,
                title = EXCLUDED.title,
                description = EXCLUDED.description,
                remote_created_at = EXCLUDED.remote_created_at,
                remote_updated_at = EXCLUDED.remote_updated_at,
                synced_at = NOW()
        WHERE cached_remote_servers.remote_updated_at <= EXCLUDED.remote_updated_at
        "#,
        server.id.as_uuid(),
        server.host,
        server.title,
        server.description,
        server.created_at,
        server.updated_at,
    )
    .execute(pool)
    .await?;
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
    use crate::error::ApiError;
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
