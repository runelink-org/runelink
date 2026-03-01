use runelink_types::{
    server::{
        FullServerMembership, NewServerMembership, ServerMember,
        ServerMembership,
    },
    user::UserRef,
    ws::{
        ClientWsUpdate, FederationWsReply, FederationWsRequest,
        FederationWsUpdate,
    },
};
use uuid::Uuid;

use super::federation;
use crate::{
    auth::Session,
    error::{ApiError, ApiResult},
    ops::fanout,
    queries,
    state::AppState,
};

/// Create a new membership for a user in a server.
pub async fn create(
    state: &AppState,
    session: &mut Session,
    new_membership: &NewServerMembership,
) -> ApiResult<FullServerMembership> {
    // If this membership is for a remote server, proxy via federation and cache locally.
    if state
        .config
        .is_remote_host(Some(&new_membership.server_host))
    {
        // Home Server should only create memberships for its own users.
        let user_host = new_membership.user_ref.host.clone();
        if state.config.is_remote_host(Some(&user_host)) {
            return Err(ApiError::BadRequest(
                "User host in membership does not match local host".into(),
            ));
        }
        let host = new_membership.server_host.as_str();
        let reply = federation::request(
            state,
            host,
            Some(new_membership.user_ref.clone()),
            FederationWsRequest::MembershipsCreate {
                server_id: new_membership.server_id,
                new_membership: new_membership.clone(),
            },
        )
        .await?;
        let FederationWsReply::MembershipsCreate(membership) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for memberships.create"
            )));
        };
        let user = membership.user.clone();
        // Cache the remote server and membership locally
        queries::servers::upsert_remote(&state.db_pool, &membership.server)
            .await?;
        let cached_membership = queries::memberships::insert_remote(
            &state.db_pool,
            &membership.into(),
        )
        .await?;
        // synced_at comes from cached membership
        return Ok(cached_membership.as_full(user));
    }

    // Ensure remote user exists locally before creating membership
    if new_membership.user_ref.host != state.config.local_host() {
        let user = session.lookup_user(state).await?;
        if user.is_none() {
            let host = new_membership.user_ref.host.as_str();
            let reply = federation::request(
                state,
                host,
                None,
                FederationWsRequest::UsersGetByRef {
                    user_ref: new_membership.user_ref.clone(),
                },
            )
            .await?;
            let FederationWsReply::UsersGetByRef(user) = reply else {
                return Err(ApiError::Internal(format!(
                    "Unexpected federation reply from {host} for users.get_by_ref"
                )));
            };
            queries::users::upsert_remote(&state.db_pool, &user).await?;
        }
    }

    // Create the membership
    let member =
        queries::memberships::insert_local(&state.db_pool, new_membership)
            .await?;
    let membership = queries::memberships::get_local_by_user_and_server(
        state,
        new_membership.server_id,
        new_membership.user_ref.clone(),
    )
    .await?;
    let full_membership = FullServerMembership {
        server: membership.server,
        user: member.user,
        role: membership.role,
        joined_at: membership.joined_at,
        updated_at: membership.updated_at,
        synced_at: membership.synced_at,
    };
    fanout::fanout_update(
        state,
        fanout::resolve_server_targets(state, new_membership.server_id).await?,
        ClientWsUpdate::MembershipUpserted(full_membership.clone()),
        FederationWsUpdate::MembershipUpserted(full_membership.clone()),
    )
    .await;
    Ok(full_membership)
}

/// Get all members of a server (public).
pub async fn get_members_by_server(
    state: &AppState,
    server_id: Uuid,
    target_host: Option<&str>,
) -> ApiResult<Vec<ServerMember>> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let members = queries::memberships::get_members_by_server(
            &state.db_pool,
            server_id,
        )
        .await?;
        Ok(members)
    } else {
        // Fetch from remote host (public endpoint, no auth needed)
        let host = target_host.unwrap();
        let reply = federation::request(
            state,
            host,
            None,
            FederationWsRequest::MembershipsGetMembersByServer { server_id },
        )
        .await?;
        let FederationWsReply::MembershipsGetMembersByServer(members) = reply
        else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for memberships.get_members_by_server"
            )));
        };
        Ok(members)
    }
}

/// Get a specific server member (public).
pub async fn get_member_by_user_and_server(
    state: &AppState,
    server_id: Uuid,
    user_ref: UserRef,
    target_host: Option<&str>,
) -> ApiResult<ServerMember> {
    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let member = queries::memberships::get_local_member_by_user_and_server(
            &state.db_pool,
            server_id,
            user_ref,
        )
        .await?;
        Ok(member)
    } else {
        // Fetch from remote host (public endpoint, no auth needed)
        let host = target_host.unwrap();
        let reply = federation::request(
            state,
            host,
            None,
            FederationWsRequest::MembershipsGetByUserAndServer {
                server_id,
                user_ref,
            },
        )
        .await?;
        let FederationWsReply::MembershipsGetByUserAndServer(member) = reply
        else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for memberships.get_by_user_and_server"
            )));
        };
        Ok(member)
    }
}

/// Get all server memberships for a user (public).
pub async fn get_by_user(
    state: &AppState,
    user_ref: UserRef,
) -> ApiResult<Vec<ServerMembership>> {
    let memberships =
        queries::memberships::get_by_user(state, user_ref).await?;
    Ok(memberships)
}

/// Delete a server membership.
pub async fn delete(
    state: &AppState,
    session: &mut Session,
    server_id: Uuid,
    user_ref: UserRef,
    target_host: Option<&str>,
) -> ApiResult<()> {
    let session_user_ref = session.user_ref.clone().ok_or_else(|| {
        ApiError::AuthError("User reference required for leaving server".into())
    })?;
    if session_user_ref != user_ref {
        return Err(ApiError::BadRequest(
            "User identity in path does not match authenticated user".into(),
        ));
    }

    // Handle local case
    if !state.config.is_remote_host(target_host) {
        let mut targets =
            fanout::resolve_server_targets(state, server_id).await?;
        if !targets.local_users.contains(&user_ref) {
            targets.local_users.push(user_ref.clone());
        }
        if user_ref.host != state.config.local_host() {
            targets.remote_hosts.push(user_ref.host.clone());
        }
        // Verify the membership exists
        queries::memberships::get_local_member_by_user_and_server(
            &state.db_pool,
            server_id,
            user_ref.clone(),
        )
        .await?;
        queries::memberships::delete_local(&state.db_pool, server_id, user_ref)
            .await?;
        fanout::fanout_update(
            state,
            targets,
            ClientWsUpdate::MembershipDeleted {
                server_id,
                user_ref: session_user_ref.clone(),
            },
            FederationWsUpdate::MembershipDeleted {
                server_id,
                user_ref: session_user_ref,
            },
        )
        .await;
        Ok(())
    } else {
        // Delete on remote host using federation
        let host = target_host.unwrap();
        let reply = federation::request(
            state,
            host,
            Some(user_ref.clone()),
            FederationWsRequest::MembershipsDelete {
                server_id,
                user_ref: user_ref.clone(),
            },
        )
        .await?;
        let FederationWsReply::MembershipsDelete = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for memberships.delete"
            )));
        };
        // Also delete from local cache if it exists
        let _ = queries::memberships::delete_remote(
            &state.db_pool,
            server_id,
            user_ref,
        )
        .await;
        Ok(())
    }
}

/// Auth requirements for membership operations.
pub mod auth {
    use super::*;
    use crate::auth::Requirement as Req;
    use crate::or;

    pub fn create(_server_id: Uuid) -> Req {
        // TODO: make this admin only and create an invite system
        // Servers should also be public or private
        Req::Always.or_admin().client_only()
    }

    pub fn delete(server_id: Uuid, user_ref: UserRef) -> Req {
        or!(Req::User(user_ref), Req::ServerAdmin(server_id))
            .or_admin()
            .client_only()
    }

    pub mod federated {
        use super::*;

        pub fn create(_server_id: Uuid, user_ref: UserRef) -> Req {
            Req::FederatedUser(user_ref).federated_only()
        }

        pub fn delete(_server_id: Uuid, user_ref: UserRef) -> Req {
            Req::FederatedUser(user_ref).federated_only()
        }
    }
}
