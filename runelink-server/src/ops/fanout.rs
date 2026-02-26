use runelink_types::{
    user::UserRef,
    ws::{ClientWsUpdate, FederationWsUpdate},
};
use uuid::Uuid;

use crate::{error::ApiResult, state::AppState};

#[derive(Clone, Debug, Default)]
pub struct ServerFanoutTargets {
    pub local_users: Vec<UserRef>,
    pub remote_hosts: Vec<String>,
}

/// Resolve the targets for a server update.
pub async fn resolve_server_targets(
    state: &AppState,
    server_id: Uuid,
) -> ApiResult<ServerFanoutTargets> {
    let local_users = state
        .routing_index
        .users_for_local_server(server_id)
        .await?;
    let remote_hosts = state.routing_index.hosts_for_server(server_id).await?;
    Ok(ServerFanoutTargets {
        local_users,
        remote_hosts,
    })
}

/// Fanout a server update to the given targets (best effort).
pub async fn fanout_update(
    state: &AppState,
    targets: ServerFanoutTargets,
    client_update: ClientWsUpdate,
    federation_update: FederationWsUpdate,
) {
    for user_ref in &targets.local_users {
        let _ = state
            .client_ws_manager
            .send_update_to_user(user_ref, client_update.clone())
            .await;
    }
    let _ = state
        .federation_ws_manager
        .send_update_to_hosts(targets.remote_hosts, federation_update)
        .await;
}
