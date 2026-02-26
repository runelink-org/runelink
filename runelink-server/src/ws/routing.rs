#![allow(dead_code)]

use std::sync::Arc;

use runelink_types::UserRef;
use uuid::Uuid;

use crate::{db::DbPool, error::ApiResult, queries};

#[derive(Clone, Debug)]
pub struct RoutingIndex {
    db_pool: Arc<DbPool>,
}

impl RoutingIndex {
    pub fn new(db_pool: Arc<DbPool>) -> Self {
        Self { db_pool }
    }
}

impl RoutingIndex {
    pub async fn hosts_for_server(
        &self,
        server_id: Uuid,
    ) -> ApiResult<Vec<String>> {
        let users = queries::memberships::get_local_user_refs_by_server(
            self.db_pool.as_ref(),
            server_id,
        )
        .await?;
        let hosts = users
            .into_iter()
            .map(|user| user.host)
            .collect::<std::collections::BTreeSet<_>>();
        Ok(hosts.into_iter().collect())
    }

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
