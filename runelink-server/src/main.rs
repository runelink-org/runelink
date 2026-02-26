use config::ServerConfig;
use sqlx::migrate::Migrator;
use state::AppState;
use tokio::{net::TcpListener, sync::RwLock};

use std::{collections::HashMap, sync::Arc};

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
mod ws;

// Embed all sql migrations in binary
static MIGRATOR: Migrator = sqlx::migrate!();

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // Initialize logger - reads RUST_LOG environment variable
    // Examples: RUST_LOG=info, RUST_LOG=debug, RUST_LOG=runelink_server=debug
    // Defaults to info level if RUST_LOG is not set
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();

    let config = ServerConfig::from_env()?;
    let db_pool = Arc::new(db::get_pool(&config).await?);

    let app_state = AppState {
        config: Arc::new(config.clone()),
        db_pool: db_pool.clone(),
        http_client: reqwest::Client::new(),
        client_ws_manager: ws::ClientWsManager::new(),
        federation_ws_manager: ws::FederationWsManager::new(),
        key_manager: KeyManager::load_or_generate(config.key_dir.clone())?,
        jwks_cache: Arc::new(RwLock::new(HashMap::new())),
        routing_index: ws::RoutingIndex::new(db_pool),
    };

    MIGRATOR.run(app_state.db_pool.as_ref()).await?;
    log::info!("Migrations are up to date.");

    let app = api::router().with_state(app_state);

    let ip_addr = format!("0.0.0.0:{}", config.port);
    let listener = TcpListener::bind(&ip_addr).await?;

    log::info!("Starting server on {ip_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
