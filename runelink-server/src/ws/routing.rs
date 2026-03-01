use std::sync::Arc;

use runelink_types::user::UserRef;
use uuid::Uuid;

use crate::{config::ServerConfig, db::DbPool, error::ApiResult, queries};

#[derive(Clone, Debug)]
pub struct RoutingIndex {
    db_pool: Arc<DbPool>,
    server_config: Arc<ServerConfig>,
}

impl RoutingIndex {
    pub fn new(db_pool: Arc<DbPool>, server_config: Arc<ServerConfig>) -> Self {
        Self {
            db_pool,
            server_config,
        }
    }
}

impl RoutingIndex {
    /// Get the hosts for a server (excluding the local host).
    pub async fn hosts_for_server(
        &self,
        server_id: Uuid,
    ) -> ApiResult<Vec<String>> {
        let users = queries::memberships::get_local_user_refs_by_server(
            self.db_pool.as_ref(),
            server_id,
        )
        .await?;
        let local_host = self.server_config.local_host();
        let hosts = users
            .into_iter()
            .map(|user| user.host)
            .filter(|host| host != &local_host)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        Ok(hosts)
    }

    /// Get the users for a local server.
    pub async fn users_for_local_server(
        &self,
        server_id: Uuid,
    ) -> ApiResult<Vec<UserRef>> {
        queries::memberships::get_local_user_refs_by_server(
            self.db_pool.as_ref(),
            server_id,
        )
        .await
    }

    /// Get the users for a remote server.
    pub async fn users_for_remote_server(
        &self,
        server_id: Uuid,
    ) -> ApiResult<Vec<UserRef>> {
        queries::memberships::get_remote_user_refs_by_server(
            self.db_pool.as_ref(),
            server_id,
        )
        .await
    }
}
