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
