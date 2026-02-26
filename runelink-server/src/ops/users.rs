use runelink_client::util::get_api_url;
use runelink_types::{
    user::{NewUser, User, UserRef},
    ws::{
        ClientWsUpdate, FederationWsReply, FederationWsRequest,
        FederationWsUpdate,
    },
};

use crate::{
    auth::Session,
    error::{ApiError, ApiResult},
    queries,
    state::AppState,
};
use super::federation;

/// Create a new user.
pub async fn create(
    state: &AppState,
    _session: &Session,
    new_user: &NewUser,
) -> ApiResult<User> {
    let user = queries::users::insert(&state.db_pool, new_user).await?;
    let _ = state
        .client_ws_manager
        .broadcast_update(ClientWsUpdate::UserUpserted(user.clone()))
        .await;
    Ok(user)
}

/// List all users (public).
pub async fn get_all(
    state: &AppState,
    target_host: Option<&str>,
) -> ApiResult<Vec<User>> {
    if !state.config.is_remote_host(target_host) {
        let users = queries::users::get_all(&state.db_pool).await?;
        Ok(users)
    } else {
        let host = target_host.unwrap();
        let reply = federation::request(
            state,
            host,
            None,
            FederationWsRequest::UsersGetAll,
        )
        .await?;
        let FederationWsReply::UsersGetAll(users) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for users.get_all"
            )));
        };
        Ok(users)
    }
}

/// Find a user by UserRef (public).
pub async fn get_by_ref(
    state: &AppState,
    user_ref: UserRef,
    _target_host: Option<&str>,
) -> ApiResult<User> {
    if !state.config.is_remote_host(Some(&user_ref.host)) {
        let user = queries::users::get_by_ref(&state.db_pool, user_ref).await?;
        Ok(user)
    } else {
        let host = user_ref.host.clone();
        let reply = federation::request(
            state,
            &host,
            None,
            FederationWsRequest::UsersGetByRef { user_ref },
        )
        .await?;
        let FederationWsReply::UsersGetByRef(user) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for users.get_by_ref"
            )));
        };
        Ok(user)
    }
}

/// Delete a user from their home server.
pub async fn delete_home_user(
    state: &AppState,
    _session: &Session,
    user_ref: &UserRef,
) -> ApiResult<()> {
    let user =
        queries::users::get_by_ref(&state.db_pool, user_ref.clone()).await?;
    if user.host != state.config.local_host() {
        return Err(ApiError::BadRequest(
            "Can only delete users from their home server".into(),
        ));
    }

    let foreign_hosts = queries::memberships::get_remote_server_hosts_for_user(
        &state.db_pool,
        user_ref.clone(),
    )
    .await?;

    queries::users::delete(&state.db_pool, user_ref.clone()).await?;
    let _ = state
        .client_ws_manager
        .broadcast_update(ClientWsUpdate::UserDeleted {
            user_ref: user_ref.clone(),
        })
        .await;
    let _ = state
        .federation_ws_manager
        .send_update_to_hosts(
            foreign_hosts,
            FederationWsUpdate::RemoteUserDeleted {
                user_ref: user_ref.clone(),
            },
        )
        .await;
    Ok(())
}

/// Delete a remote user record from a foreign server.
pub async fn delete_remote_user_record(
    state: &AppState,
    session: &Session,
    user_ref: &UserRef,
) -> ApiResult<()> {
    let session_user_ref = session.user_ref.clone().ok_or_else(|| {
        ApiError::AuthError(
            "User reference required for federated user deletion".into(),
        )
    })?;
    if session_user_ref.name != user_ref.name
        || session_user_ref.host != user_ref.host
    {
        return Err(ApiError::BadRequest(
            "User identity in path does not match user reference in token"
                .into(),
        ));
    }
    if session_user_ref.host == state.config.local_host() {
        return Err(ApiError::BadRequest(
            "Cannot delete local users via federation".into(),
        ));
    }

    let expected_home_server_url = get_api_url(&session_user_ref.host);
    let federation_claims = session.federation.as_ref().ok_or_else(|| {
        ApiError::AuthError("Federation claims required".into())
    })?;

    if federation_claims.iss != expected_home_server_url {
        return Err(ApiError::AuthError(
            "Only the home server can delete a user".into(),
        ));
    }

    queries::users::delete(&state.db_pool, user_ref.clone()).await?;
    let _ = state
        .client_ws_manager
        .broadcast_update(ClientWsUpdate::UserDeleted {
            user_ref: user_ref.clone(),
        })
        .await;
    Ok(())
}

/// Get all hosts associated with a user (public).
pub async fn get_associated_hosts(
    state: &AppState,
    user_ref: UserRef,
    target_host: Option<&str>,
) -> ApiResult<Vec<String>> {
    if !state.config.is_remote_host(target_host) {
        let hosts =
            queries::users::get_associated_hosts(&state.db_pool, &user_ref)
                .await?;
        Ok(hosts)
    } else {
        let host = target_host.unwrap();
        let reply = federation::request(
            state,
            host,
            None,
            FederationWsRequest::UsersGetAssociatedHosts { user_ref },
        )
        .await?;
        let FederationWsReply::UsersGetAssociatedHosts(hosts) = reply else {
            return Err(ApiError::Internal(format!(
                "Unexpected federation reply from {host} for users.get_associated_hosts"
            )));
        };
        Ok(hosts)
    }
}

/// Auth requirements for user operations.
pub mod auth {
    use super::*;
    use crate::auth::Requirement as Req;

    pub fn create() -> Req {
        Req::Client
    }

    pub fn delete(user_ref: UserRef) -> Req {
        Req::User(user_ref).or_admin().client_only()
    }

    pub mod federated {
        use super::*;

        pub fn delete(user_ref: UserRef) -> Req {
            Req::FederatedUser(user_ref).federated_only()
        }
    }
}
