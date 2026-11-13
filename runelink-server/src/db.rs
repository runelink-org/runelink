use sqlx::{Pool, Postgres, Transaction, postgres::PgPoolOptions};
use std::time::Duration;

use crate::config::ServerConfig;

pub type DbPool = Pool<Postgres>;

/// Begin a read-only transaction whose reads all see one snapshot.
pub async fn begin_snapshot(
    pool: &DbPool,
) -> sqlx::Result<Transaction<'static, Postgres>> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

pub async fn get_pool(config: &ServerConfig) -> sqlx::Result<DbPool> {
    PgPoolOptions::new()
        .max_connections(50)
        // Includes checking idle connections and reconnecting. Allow time for
        // a suspended database to wake up before failing the request.
        .acquire_timeout(Duration::from_secs(15))
        .connect(&config.database_url)
        .await
}
