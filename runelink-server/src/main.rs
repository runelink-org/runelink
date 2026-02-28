use config::ServerConfig;
use sqlx::migrate::Migrator;
use state::AppState;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::task::JoinSet;

use crate::key_manager::KeyManager;

mod api;
mod auth;
mod bearer_auth;
mod config;
mod db;
mod error;
mod jwks_resolver;
mod key_manager;
mod ops;
mod queries;
mod state;

// Embed all sql migrations in binary
static MIGRATOR: Migrator = sqlx::migrate!();

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logger - reads RUST_LOG environment variable
    // Examples: RUST_LOG=info, RUST_LOG=debug, RUST_LOG=runelink_server=debug
    // Defaults to info level if RUST_LOG is not set
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();

    let config_path = std::env::var("RUNELINK_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("config.toml"));
    let server_configs = ServerConfig::from_toml_file(&config_path)?;

    if server_configs.len() > 1 {
        log::info!(
            "Cluster mode inferred from config: starting {} server instances",
            server_configs.len()
        );
    } else {
        log::info!("Starting single server instance");
    }

    let http_client = reqwest::Client::new();
    let mut join_set = JoinSet::new();

    for config in server_configs {
        let config = Arc::new(config);
        let pool = Arc::new(db::get_pool(config.as_ref()).await?);
        let key_manager = KeyManager::load_or_generate(config.key_dir.clone())?;

        let app_state = AppState {
            config: config.clone(),
            db_pool: pool.clone(),
            http_client: http_client.clone(),
            key_manager,
            jwks_cache: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
        };

        MIGRATOR.run(pool.as_ref()).await?;
        log::info!(
            "Migrations are up to date for {}.",
            config.local_host_with_explicit_port()
        );

        let app = api::router().with_state(app_state);

        let ip_addr = format!("0.0.0.0:{}", config.port);
        let listener = TcpListener::bind(&ip_addr).await?;
        let host = config.local_host_with_explicit_port();

        log::info!("Starting server {host} on {ip_addr}");
        join_set.spawn(async move {
            axum::serve(listener, app)
                .await
                .map_err(|e| format!("server {host} exited with error: {e}"))
        });
    }

    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok(Ok(())) => {}
            Ok(Err(err_msg)) => return Err(err_msg.into()),
            Err(join_err) => {
                return Err(
                    format!("server task join failure: {join_err}").into()
                );
            }
        }
    }

    Ok(())
}
