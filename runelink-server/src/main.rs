use std::{collections::HashMap, sync::Arc};

use axum::serve::ListenerExt;
use clap::Parser;
use sqlx::migrate::Migrator;
use tokio::{net::TcpListener, sync::RwLock, task::JoinSet};

use crate::{
    actors::{ActorCtx, Actors},
    config::ServerConfig,
    key_manager::KeyManager,
    peer_policy::PeerPolicy,
    state::AppState,
};

mod actors;
mod admission;
mod api;
mod auth;
mod auth_service;
mod bearer_auth;
mod capabilities;
mod channel_tree;
mod config;
mod db;
mod error;
mod ids;
mod jwks_resolver;
mod key_manager;
mod ops;
mod peer_policy;
mod presence;
mod queries;
mod state;
mod ws;

// Embed all sql migrations in binary
static MIGRATOR: Migrator = sqlx::migrate!();

#[derive(Parser)]
#[command(version)]
struct Cli {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Cli::parse();

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
            "Cluster mode inferred from config: starting {} hosts",
            server_configs.len()
        );
    } else {
        log::info!("Starting a single host");
    }

    let mut join_set = JoinSet::new();

    for config in server_configs {
        let config = Arc::new(config);
        let db_pool = Arc::new(db::get_pool(&config).await?);

        let (presence, presence_events) = presence::Presence::new();
        let client_ws_manager = ws::ClientWsManager::new(presence.clone());
        let federation_ws_manager = ws::FederationWsManager::new();
        let peer_policy =
            PeerPolicy::for_public_host(config.public_host()).await;
        if peer_policy == PeerPolicy::AnyAddress {
            log::info!(
                "Host {} is private, so it federates with private peers too",
                config.public_host()
            );
        }
        let actors = Actors::new(ActorCtx::new(
            db_pool.clone(),
            config.clone(),
            client_ws_manager.clone(),
            federation_ws_manager.clone(),
        ));
        let app_state = AppState {
            config: config.clone(),
            db_pool: db_pool.clone(),
            // Keys are fetched from an issuer's exact API URL, never from
            // where it redirects or through a proxy, and only at addresses
            // the peer policy allows. A stalled peer can't hold a request.
            http_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .dns_resolver(peer_policy)
                .timeout(std::time::Duration::from_secs(10))
                .build()?,
            peer_policy,
            client_ws_manager,
            federation_ws_manager,
            key_manager: KeyManager::load_or_generate(config.key_dir.clone())?,
            jwks_cache: Arc::new(RwLock::new(HashMap::new())),
            actors,
            presence,
        };
        app_state.federation_ws_manager.set_state(app_state.clone());
        tokio::spawn(presence::run(app_state.clone(), presence_events));

        MIGRATOR.run(db_pool.as_ref()).await?;
        log::info!("Migrations are up to date for {}.", config.public_host());

        let app = api::router().with_state(app_state);

        let bind_addr = config.bind_addr();
        // Nagle's algorithm can delay a small write while earlier data is
        // unacknowledged, which stalls back-to-back WebSocket frames.
        let listener =
            TcpListener::bind(&bind_addr).await?.tap_io(|tcp_stream| {
                if let Err(error) = tcp_stream.set_nodelay(true) {
                    log::warn!("Failed setting TCP_NODELAY: {error}");
                }
            });
        let host = config.public_host().clone();

        log::info!("Starting host {host} on {bind_addr}");
        join_set.spawn(async move {
            axum::serve(listener, app)
                .await
                .map_err(|e| format!("host {host} exited with error: {e}"))
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
