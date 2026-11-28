use runelink_types::{
    host::Host,
    user::{NewUser, User, UserRef},
    ws::{FederationWsReply, FederationWsRequest},
};

use super::federation;
use crate::{
    auth::Session,
    config::Locality,
    error::{ApiError, ApiResult},
    queries,
    state::AppState,
};

/// Create a new user on this host, without an account. Signup creates users
/// with accounts.
pub async fn create(
    state: &AppState,
    session: &Session,
    new_user: &NewUser,
) -> ApiResult<User> {
    if state.config.locality(&new_user.host).is_remote() {
        return Err(ApiError::BadRequest(
            "Users can only be created for this host".into(),
        ));
    }
    // Stored under this host's own name, however the request spelled it.
    let new_user = NewUser {
        host: state.config.public_host().clone(),
        ..new_user.clone()
    };
    state
        .actors
        .create_user(new_user, None, session.authorization())
        .await
}

/// List all users (public).
pub async fn get_all(
    state: &AppState,
    target_host: Option<&Host>,
) -> ApiResult<Vec<User>> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            let users = queries::users::get_all(&state.db_pool).await?;
            Ok(users)
        }
        Locality::Remote(host) => {
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
}

/// Find a user by UserRef (public).
pub async fn get_by_ref(
    state: &AppState,
    user_ref: UserRef,
) -> ApiResult<User> {
    if state.config.locality(&user_ref.host).is_local() {
        let user =
            queries::users::get_by_ref(&*state.db_pool, user_ref).await?;
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
    session: &Session,
    user_ref: &UserRef,
) -> ApiResult<()> {
    if state.config.locality(&user_ref.host).is_remote() {
        return Err(ApiError::BadRequest(
            "Can only delete users from their home server".into(),
        ));
    }
    state
        .actors
        .delete_home_user(user_ref.clone(), session.authorization())
        .await
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
    if state.config.locality(&session_user_ref.host).is_local() {
        return Err(ApiError::BadRequest(
            "Cannot delete local users via federation".into(),
        ));
    }

    let expected_home_server_url =
        session_user_ref.host.api_url(state.config.secure);
    let federation_claims = session.federation.as_ref().ok_or_else(|| {
        ApiError::AuthError("Federation claims required".into())
    })?;

    if federation_claims.iss != expected_home_server_url {
        return Err(ApiError::AuthError(
            "Only the home server can delete a user".into(),
        ));
    }

    state
        .actors
        .delete_remote_user(user_ref.clone(), session.authorization())
        .await
}

/// Get all hosts associated with a user (public).
pub async fn get_associated_hosts(
    state: &AppState,
    user_ref: UserRef,
    target_host: Option<&Host>,
) -> ApiResult<Vec<Host>> {
    match state.config.target_locality(target_host) {
        Locality::Local => {
            let hosts =
                queries::users::get_associated_hosts(&state.db_pool, &user_ref)
                    .await?;
            Ok(hosts)
        }
        Locality::Remote(host) => {
            let reply = federation::request(
                state,
                host,
                None,
                FederationWsRequest::UsersGetAssociatedHosts { user_ref },
            )
            .await?;
            let FederationWsReply::UsersGetAssociatedHosts(hosts) = reply
            else {
                return Err(ApiError::Internal(format!(
                    "Unexpected federation reply from {host} for users.get_associated_hosts"
                )));
            };
            Ok(hosts)
        }
    }
}

/// Auth requirements for user operations.
pub mod auth {
    use super::*;
    use crate::auth::Requirement as Req;

    pub fn create() -> Req {
        Req::HostAdmin.client_only()
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
