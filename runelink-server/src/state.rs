use std::sync::Arc;

use crate::{
    actors::Actors, config::ServerConfig, db::DbPool, key_manager::KeyManager,
    ws,
};

pub type JwksCache = std::collections::HashMap<
    runelink_types::Host,
    crate::jwks_resolver::CachedJwks,
>;

#[derive(Clone, Debug)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub db_pool: Arc<DbPool>,
    pub http_client: reqwest::Client,
    pub client_ws_manager: ws::ClientWsManager,
    pub federation_ws_manager: ws::FederationWsManager,
    pub key_manager: KeyManager,
    pub jwks_cache: Arc<tokio::sync::RwLock<JwksCache>>,
    pub actors: Actors,
}
