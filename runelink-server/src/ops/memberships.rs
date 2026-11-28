use runelink_types::{
    server::{
        FullServerMembership, NewServerMembership, ServerMember,
        ServerMembership, ServerRef, ServerRole,
    },
    user::{User, UserRef},
    ws::{FederationWsReply, FederationWsRequest},
};

use super::{federation, users};
use crate::{
    admission,
    auth::Session,
    config::Locality,
    error::{ApiError, ApiResult},
    queries,
    state::AppState,
};

/// Create a new membership for a user in a server.
pub async fn upsert(
    state: &AppState,
    session: &mut Session,
    new_membership: &NewServerMembership,
    remote_user: Option<&User>,
) -> ApiResult<FullServerMembership> {
    // If this membership is for a remote server, proxy via federation and cache locally.
    if let Locality::Remote(host) =
        state.config.locality(&new_membership.server_ref.host)
    {
        // Home Server should only create memberships for its own users.
        if state
            .config
            .locality(&new_membership.user_ref.host)
            .is_remote()
        {
            return Err(ApiError::BadRequest(
                "User host in membership does not match local host".into(),
            ));
        }
        let user =
            users::get_by_ref(state, new_membership.user_ref.clone(), None)
                .await?;
        let session_user_ref = session.user_ref.clone().ok_or_else(|| {
            ApiError::AuthError(
                "User reference required for remote memberships".into(),
            )
        })?;
        let reply = federation::request(
            state,
            host,
            Some(session_user_ref),
            FederationWsRequest::MembershipsUpsert {
                new_membership: new_membership.clone().as_full(user),
            },
        )
        .await?;
        let FederationWsReply::MembershipsUpsert(membership) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for memberships.upsert"
            )));
        };
        // The peer's update for this change, which arrived before the reply,
        // is what caches it here.
        return Ok(membership);
    }

    // Fetching a remote user waits on the network, so it stays out of the actor.
    let remote_user = if state
        .config
        .locality(&new_membership.user_ref.host)
        .is_remote()
    {
        let remote_user = match remote_user {
            Some(user) => user.clone(),
            // Queued after the lookup, so later requests from the same
            // connection wait for it rather than overtaking the membership.
            None => {
                admission::hold(users::get_by_ref(
                    state,
                    new_membership.user_ref.clone(),
                    None,
                ))
                .await?
            }
        };
        if remote_user.as_ref() != new_membership.user_ref {
            return Err(ApiError::BadRequest(
                "User payload does not match membership user_ref".into(),
            ));
        }
        Some(remote_user)
    } else {
        None
    };
    state
        .actors
        .upsert_membership(
            new_membership.clone(),
            remote_user,
            session.authorization(),
        )
        .await
}

/// Get all members of a server (public).
pub async fn get_members_by_server(
    state: &AppState,
    server_ref: &ServerRef,
) -> ApiResult<Vec<ServerMember>> {
    match state.config.locality(&server_ref.host) {
        Locality::Local => {
            let members = queries::memberships::get_members_by_server(
                &state.db_pool,
                server_ref.id,
            )
            .await?;
            Ok(members)
        }
        Locality::Remote(host) => {
            // Fetch from remote host (public endpoint, no auth needed)
            let reply = federation::request(
                state,
                host,
                None,
                FederationWsRequest::MembershipsGetMembersByServer {
                    server_id: server_ref.id,
                },
            )
            .await?;
            let FederationWsReply::MembershipsGetMembersByServer(members) =
                reply
            else {
                return Err(ApiError::Internal(format!(
                    "Unexpected federation reply from {host} for memberships.get_members_by_server"
                )));
            };
            Ok(members)
        }
    }
}

/// Get a specific server member (public).
pub async fn get_member_by_user_and_server(
    state: &AppState,
    server_ref: &ServerRef,
    user_ref: UserRef,
) -> ApiResult<ServerMember> {
    match state.config.locality(&server_ref.host) {
        Locality::Local => {
            let member =
                queries::memberships::get_local_member_by_user_and_server(
                    &*state.db_pool,
                    server_ref.id,
                    user_ref,
                )
                .await?;
            Ok(member)
        }
        Locality::Remote(host) => {
            // Fetch from remote host (public endpoint, no auth needed)
            let reply = federation::request(
                state,
                host,
                None,
                FederationWsRequest::MembershipsGetByUserAndServer {
                    server_id: server_ref.id,
                    user_ref,
                },
            )
            .await?;
            let FederationWsReply::MembershipsGetByUserAndServer(member) =
                reply
            else {
                return Err(ApiError::Internal(format!(
                    "Unexpected federation reply from {host} for memberships.get_by_user_and_server"
                )));
            };
            Ok(member)
        }
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
    server_ref: &ServerRef,
    user_ref: UserRef,
) -> ApiResult<()> {
    let session_user_ref = session.user_ref.clone().ok_or_else(|| {
        ApiError::AuthError("User reference required for leaving server".into())
    })?;

    match state.config.locality(&server_ref.host) {
        Locality::Local => {
            state
                .actors
                .delete_membership(
                    server_ref.id,
                    user_ref,
                    session.authorization(),
                )
                .await
        }
        Locality::Remote(host) => {
            let reply = federation::request(
                state,
                host,
                Some(session_user_ref),
                FederationWsRequest::MembershipsDelete {
                    server_id: server_ref.id,
                    user_ref: user_ref.clone(),
                },
            )
            .await?;
            let FederationWsReply::MembershipsDelete = reply else {
                return Err(ApiError::Internal(format!(
                    "Unexpected federation reply from {host} for memberships.delete"
                )));
            };
            // The peer's update for this change removes it from the cache.
            Ok(())
        }
    }
}

/// Auth requirements for membership operations.
pub mod auth {
    use super::*;
    use crate::auth::Requirement as Req;
    use crate::or;

    /// Users may join a server themselves as a member. Anything else (adding
    /// another user or granting the admin role) needs a server admin.
    fn upsert_base(
        new_membership_user: Req,
        server_ref: ServerRef,
        role: ServerRole,
    ) -> Req {
        // TODO: create an invite system. Servers should also be public or
        // private.
        match role {
            ServerRole::Member => {
                or!(new_membership_user, Req::ServerAdmin(server_ref))
            }
            ServerRole::Admin => Req::ServerAdmin(server_ref),
        }
    }

    pub fn create(new_membership: &NewServerMembership) -> Req {
        upsert_base(
            Req::User(new_membership.user_ref.clone()),
            new_membership.server_ref.clone(),
            new_membership.role,
        )
        .or_admin()
        .client_only()
    }

    pub fn delete(server_ref: &ServerRef, user_ref: UserRef) -> Req {
        or!(Req::User(user_ref), Req::ServerAdmin(server_ref.clone()))
            .or_admin()
            .client_only()
    }

    pub mod federated {
        use super::*;

        pub fn create(
            server_ref: ServerRef,
            user_ref: UserRef,
            role: ServerRole,
        ) -> Req {
            // Not `Req::User`, which needs the user to exist locally
            upsert_base(Req::FederatedUser(user_ref), server_ref, role)
                .federated_only()
        }

        pub fn delete(server_ref: ServerRef, user_ref: UserRef) -> Req {
            or!(Req::FederatedUser(user_ref), Req::ServerAdmin(server_ref))
                .federated_only()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        use runelink_types::server::ServerId;

        fn user() -> UserRef {
            UserRef::new("alice".into(), "example.com").unwrap()
        }

        fn server_ref() -> ServerRef {
            ServerRef::new(user().host, ServerId::new())
        }

        #[test]
        fn members_may_join_themselves() {
            let server_ref = server_ref();
            let req = upsert_base(
                Req::User(user()),
                server_ref.clone(),
                ServerRole::Member,
            );
            let Req::Or(reqs) = req else {
                panic!("expected an or requirement, got {req:?}");
            };
            assert!(matches!(
                reqs.as_slice(),
                [Req::User(u), Req::ServerAdmin(s)]
                    if *u == user() && *s == server_ref
            ));
        }

        #[test]
        fn admin_role_requires_server_admin() {
            let server_ref = server_ref();
            let req = upsert_base(
                Req::User(user()),
                server_ref.clone(),
                ServerRole::Admin,
            );
            assert!(matches!(req, Req::ServerAdmin(s) if s == server_ref));
        }
    }
}
