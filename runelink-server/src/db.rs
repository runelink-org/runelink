use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use std::time::Duration;

use crate::config::ServerConfig;

pub type DbPool = Pool<Postgres>;

pub async fn get_pool(config: &ServerConfig) -> sqlx::Result<DbPool> {
    PgPoolOptions::new()
        .max_connections(50)
        .acquire_timeout(Duration::from_secs(2))
        .connect(&config.database_url)
        .await
}
