use std::{collections::HashMap, sync::Arc};

use sqlx::migrate::Migrator;
use tokio::{net::TcpListener, sync::RwLock, task::JoinSet};

use crate::{config::ServerConfig, key_manager::KeyManager, state::AppState};

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
mod ws;

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

    let mut join_set = JoinSet::new();

    for config in server_configs {
        let config = Arc::new(config);
        let db_pool = Arc::new(db::get_pool(&config).await?);

        let app_state = AppState {
            config: config.clone(),
            db_pool: db_pool.clone(),
            http_client: reqwest::Client::new(),
            client_ws_manager: ws::ClientWsManager::new(),
            federation_ws_manager: ws::FederationWsManager::new(),
            key_manager: KeyManager::load_or_generate(config.key_dir.clone())?,
            jwks_cache: Arc::new(RwLock::new(HashMap::new())),
            routing_index: ws::RoutingIndex::new(
                db_pool.clone(),
                config.clone(),
            ),
        };

        MIGRATOR.run(db_pool.as_ref()).await?;
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
