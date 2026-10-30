use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use std::time::Duration;

use crate::config::ServerConfig;

pub type DbPool = Pool<Postgres>;

pub async fn get_pool(config: &ServerConfig) -> sqlx::Result<DbPool> {
    PgPoolOptions::new()
        .max_connections(50)
        // Includes checking idle connections and reconnecting. Allow time for
        // a suspended database to wake up before failing the request.
        .acquire_timeout(Duration::from_secs(15))
        .connect(&config.database_url)
        .await
}
